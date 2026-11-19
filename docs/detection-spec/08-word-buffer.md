# 08 — The word buffer and the live citation context

Source: `kairo-detect::engine::words` (`WordBuffer`), read by `citations.rs`,
`paths.rs`, `twins.rs`, `decide.rs`, `correction.rs`. Cases:
`eval/segmentation/` and `kairo-detect/tests/segmentation.rs`.

## Why

A recogniser cuts speech into segments wherever it likes, and a lagging one
(the offline engine under load skips audio and restarts) cuts it badly. On
2026-10-26 the interim "Psalm sixty seven it" was followed by the final
"says let the people praise thee": the reference was never in a final, and
Psalm 67 never resolved. The same words split another way found 67:3.

Detectors used to reason per segment: the previous final joined to this one
(`prev_final`, 12 carried words for the offline engine), two finals back
(`join_two_finals_back`), "the finals of the last N seconds plus the
interim" rebuilt by hand in a dozen places. Every one of them is a guess at
where the speaker's sentence really was. **The rule now: a detector reads
the words, not the segments.** Where a segment boundary falls must not change
what is found.

## The buffer

Every word heard, in order:

| field    | meaning |
|----------|---------|
| `text`   | the word as the recogniser wrote it (punctuation kept) |
| `at`     | when it was **first** heard: its first interim, else its final |
| `status` | `Settled` (a final locked it), `Live` (in the interim still growing), `Unconfirmed` (heard in an interim that no final re-heard) |

Positions are absolute (word *n* of the service keeps its number as old words
are dropped), so a detector can remember *where* something was said and ask
for "every word since". Kept 90 s / 600 words.

### Updating it

- **Interim** — replaces the live words. A word at the same position keeps
  its `at` even if the recogniser revised its spelling ("got" → "God"):
  revisions apply in place.
- **Final** — settles live words:
  1. it begins where the live words begin (the usual case), or
  2. it begins at a later live word (the recogniser dropped the first live
     words: "O God let" | "let all the people"), or
  3. its words are mostly the live words reworded ("some sixty seven" →
     "Psalm 67") — treated as (1);
  4. otherwise it re-hears none of them.

  Live words before where the final begins become **Unconfirmed**: still what
  was said, just never locked. In case 4 that is all of them — the Psalm 67
  log. Without `continues_live` the final is the whole utterance and live
  words past its end go (they were the same speech cut differently: "sixty
  seven verse three" → "67:3"); with it (the offline engine locking a few words
  at a time) the live words past the locked ones stay live.
- **Reconnect** — the live words go; settled and unconfirmed words stay.

`changed()` is the stretch the latest segment brought: the new live words, or
what the final settled.

### Reading it

- `since(t)` — everything heard since `t`, any status. Replaces "finals since
  `t`, then the interim" where `t` is an event (a verse went up, a citation).
- `recent(now, ms)` — the last `ms` of speech **and all of the utterance being
  said**, however long it has run: a window in seconds must not cut the start
  off a long sentence (cut, the twin check lost the words that kept Psalm 67:3
  and 67:5 from both going up).
- `newest()` — what's being said now: the live words, else the latest
  segment's. Replaces "the interim, else the last final".
- `last(n)` — the last `n` words heard.
- `before(pos, n, now, ms)` — at most `n` words before `pos`, each heard within
  `ms`. Replaces "the previous final" and "two finals back".
- `text(range)` — the words in a range of positions.

Settled-only readers (the 60-word verbatim rescan, pace) keep reading the
finals list for now: an unconfirmed word is good enough to name a chapter, but
not yet trusted as quoted scripture.

## The live citation context

A citation names a place in the Bible; it doesn't stop mattering when its
segment ends. The **live citation context** is the chapter-only citation most
recently heard (`ChapterPending`), now recorded with the **position** of its
first word instead of a copy of its segment's text:

- set from any segment that parses "Book chapter N" with no verse — interim
  or final — and from words an earlier interim heard that the final dropped;
- the words said after it are always `text(from..end)`: whatever segments
  carried them, unconfirmed words included, the live interim included;
- it lasts while the reading context still points at that chapter (as
  before), and is spent when a verse of the chapter reaches the screen.

So "Psalm sixty seven it" | "says let the people praise thee" and
"Psalm sixty seven it says" | "let the people praise thee" and one long final
all give the chapter the same words to pick its verse from.

## Joining citations split across segments

A citation split by a pause ("…John 6" | "48-57", "Daniel" | "chapter what?
Chapter" | "six from verse three") is parsed over the words heard just before
this segment's words (`before(changed().start, 24, now, 8 s)`) joined with
them; only citations the join creates or extends count (one already in the
earlier words was handled with them). Words only an interim heard may hold a citation never acted on, so
they count as new — except a chapter-only citation that is already the live
citation context: found again by every later join, it isn't named afresh, and
the chapter goes on waiting and retrying on the newest words ("Psalm sixty
seven it" | "says let the people praise thee" | … | "thee then the earth shall
yield" → 67:6; `eval/segmentation/psalm-67-read-in-pieces.txt`). This is one rule for every engine,
replacing `prev_final`, the 12-word offline carry and the two-finals-back
join — and it sees interim words a final dropped.

## Migration

Each step is behind the replay (`cargo run --release -p kairo-eval --bin
replay`: no fewer found, no more wrong) and `cargo test --release -p
kairo-detect`.

| step | reader | status |
|------|--------|--------|
| 1 | the buffer, fed from every segment | done |
| 2 | chapter-only citation reads `text(from..)` | done |
| 3 | citation joins, "announces a different book", "verse" \| "N" across a split, reading through a chapter → `before(…)` | done |
| 4 | an ordinal before a book, twin separation, verse numbers said, the paraphrase window, recent range words → `recent`/`last`; corrections, "a citation pending elsewhere", "the verse on screen explains it better", "a shown verse returns" → `newest`/`since` | done |
| 5a | the end-of-utterance verbatim rescan → `last(60)`, checked against `brought()` (this segment's words and the unconfirmed words just before them) | done |
| 5 | settled-only readers: the sentence buffer, the citation check's reading finals and words before a citation, a range's heard words, the stream watermark | later: they need a rule for unconfirmed words |

Replay at each step (2026-10-26): Deepgram recordings 222/263 found, 3 wrong
(baseline 222/263, 3 wrong of 300 sent); offline recordings 62/82, 0 wrong
(baseline the same).

## Refrains (owner, 2026-10-26)

Verses of one chapter worded exactly alike — Psalm 67:3 and 67:5, Psalm
107:8, 15, 21 and 31 — can't be told apart by their words, and the twin check
used to offer both and send neither: a verse read word for word never reached
the screen. Now, when the chapter is in play (the chapter-only citation
waiting for its verse, the chapter being read, the sermon's passage, or a
verse of it on screen):

- the repeat the reading has reached goes up — the first after the verse of
  the chapter on screen, else the chapter's first; the same reading matched
  twice (its last words in a final of their own) stays on the one up until
  the refrain is heard again;
- the other repeats are offered;
- its next verses are held as for any verse up (the soft range);
- **the verse found next tells which repeat it was**: any path proposing the
  verse after another repeat (67:6 while 67:3 is up) puts that repeat up in
  place of the guess (67:5, marked as correcting 67:3), with its own next
  verses held. The verse after the one up (67:4) confirms it. This is
  `decide` step 0a, so every path's finding counts, not a separate search.

The same words in different chapters (Psalms 107:1, 118:1, 136:1) are
different passages: still offered, not guessed.

## Robustness cases

`eval/segmentation/*.txt` are `say --segments` scripts (a line starting "~ "
is an interim). The test `tests/segmentation.rs` feeds the same words split
several ways — one final, the live log's split, the reference only in an
interim, every final boundary moved word by word, and the offline engine's
own cut (all but the newest two words locked every 2–6 words, the rest a
final) — and requires the same verses on screen, and the same offers, from
each. Refrain cases: Psalm 67 (67:3 up, 67:5 offered, every split), Psalm 107
read through (107:8, then 15, then 21), the verse after it correcting the
guess, and the same line in other chapters staying a choice. A boundary that cuts a number in two ("sixty | seven") is a different
number heard and is left out.
