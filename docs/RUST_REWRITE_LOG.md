# Rust rewrite — decision log

Newest first. Every decision the owner didn't make directly is recorded here
with the reason and, where it applies, the measurement behind it.

## 2026-10-11

### Owner decisions (from chat)
- Full rewrite of the Node server in Rust; the UI stays exactly as it is.
- Deepgram stays as the live engine; the offline Nemotron model stays.
- Deepgram and the offline engine share one pipeline, so every improvement
  helps both.
- Everything that serves scripture detection is bundled (no accuracy
  downloads). Detection includes the offline speech model (it keeps detection
  running when the internet or Deepgram credit drops).
- KJV is the anchor of the merged verse index, paired with modern English.
- No bolted-on code: Pewbeam's ideas become first-class parts of one design.
- Full autonomy to make anything significantly better.

### Reference parser (kairo-detect::reference) — decisions
- **Rebuilt as normalization → book matching → a citation grammar**, each a
  module with its rules listed at the top. Every limit comes from the corpus
  (chapters per book, a chapter's last verse, the most verses any chapter has).
- **"N to the end"** now resolves to the chapter's real last verse (the Node
  engine substituted 176, Psalm 119's length).
- **Mishearing lists removed.** "proverbate", "naha", "philippine", "profit
  chapter", … are covered by one rule: in a citation-shaped slot, a word that
  is spelled within one or two letters of a book name, or sounds the same
  (consonant skeleton of 4+ sounds, at most one sound apart), is that book.
  Spelling matches rank above sound-only matches.
- **Sound-alike books are computed**, not listed: same opening sound, similar
  consonant skeleton, used only when the heard book lacks the chapter.
- **Dropped:** "someone 25" → Psalm 125 (one recognition slip, special-cased),
  and the "digit digit zero digit" rewrite (it could turn "Psalm one one zero
  one…" into Psalm 11). Both to be re-checked against the dev sermons.
- **Fixed an inherited bug:** "forty thirteen" composed to 53. A tens word now
  only takes a units word.
- **Kept as linguistic vocabulary** (not incident lists): book names that are
  everyday words or people's names, words that introduce a book ("turn to"),
  filler before a bare verse number, everyday words one letter from a book
  name. The "ambiguous books" list exists in two forms (Node's, and a
  principled one: every everyday-word book and every person-named book); the
  dev sermons decide which ships.
- **Tests:** the Node suite's cases ported as behaviour tests (30), plus a
  whole-Bible generator — 4,000 random verses × 13 spoken forms (~44,000
  citations), all reading back exactly.

### Executive decisions
- **Work happens on branch `rust-rewrite`.** Nothing is pushed or merged
  without the owner.
- **Nothing is copied from Pewbeam.** Its NLT and its verse vectors are
  licensed or derived content; only ideas are taken.
- **Parity is measured, not assumed.** `parity/record_golden.js` records what
  the Node engine decides on every fixture, twice, so the Rust engine can be
  checked against it (decisions that differ between two Node runs are marked
  unstable).
- **The semantic layer will be rebuilt on measurements.** Found: today's
  embeddings use `last_hidden_state` mean pooling with no task prompts, which
  skips EmbeddingGemma's trained projection (`sentence_embedding` output) and
  its query/document prompts. The rebuild benchmarks, in order: correct model
  usage → other embedding models → a modern reranker → fine-tuning on
  scripture → translation merging. Each step ships only if it beats the
  previous one on held-out sermon quotes.
- **8 GB RAM is the target machine** (this dev Mac is an M1 with 8 GB). Model
  choices are judged on accuracy, CPU latency, memory and size together, since
  the embedder, reranker and offline speech model can all be loaded at once.

## The engine, end to end (2026-10-12)

The whole funnel now runs in Rust (`crates/kairo-detect/src/engine`): spoken
citations, ranges, spoken triggers, the stream / verbatim / fingerprint paths,
twins, miscitation correction and the citation check, all through one decider.
Meaning-based paths (paraphrase, scoped meaning search) are next.

### How it's measured
- **`replay`** (`cargo run --release -p kairo-detect --bin replay`) feeds the
  recorded sermons through the engine and scores them the way
  `server/eval/run_eval.js` does (found within 30 s, wrong sends, misses
  offered in Possible Matches, suggestions per hour), side by side with what
  Node decided on the same input. Dev sermons only, unless `--held-out`.
- **Real timing.** The engine is clock-free, so sermons replay at their real
  audio times. Node's eval could only run compressed (a 60-minute sermon in
  ~4 minutes), which hid every behaviour that depends on seconds: how often
  the fingerprint path runs, how long a pause is. Replaying at real speed
  surfaced several problems Node's numbers never showed (below).
- **`--trace a..b`** prints every input and every decision with its reasons
  (score parts, guards, twin checks) for a stretch of a sermon.
- **Word matchers vs Node** (`tests/lexical_vs_node.rs`, ignored): same verse
  on 99.8% of 7,334 real texts for verbatim, 98.6% for fingerprint. The
  differences come from Node reading `map.json`, whose KJV text still holds
  the translators' margin notes (John 1:5 carries "{comprehended: or, did not
  admit, or, receive}") and lacks 92 verses; Rust reads the cleaned KJV.

### Executive decisions (each measured on the dev sermons, real timing)
- **Fingerprint never puts a verse up alone.** At a real cadence it runs every
  second, and short verses made of preaching words ("thank… praise… glorious
  name", 1 Chronicles 29:13) matched the preaching itself: 1 right, 11 wrong
  on screen. It now needs a second method or the chapter on screen; it still
  offers in Possible Matches. Recall unchanged.
- **Only new evidence counts as a repeat.** Re-searching a growing sentence
  found the same words again and the scorer counted each as another sighting.
  A sighting now counts only if it rests on words not counted before.
- **Guards judge the words that produced the match**, not just the newest
  segment ("does the verse on screen explain this at least as well?").
- **Twin decisions use only words that can tell verses apart.** "God" (from
  "God help me") and "there" switched Genesis 12:2 to its near-duplicate 46:3.
- **A pause is measured from the end of one segment to the start of the
  next.** Node measured final to final (8 s); this preacher's segments are ~8 s
  long, so "Hebrews chapter 3:12 to" | "15" was never joined and the range
  never set.
- **The sentence that ends an utterance is searched with its own words.**
  Node's flush on end-of-utterance left the last segment out of the sentence.

### Where it stands (dev sermons, real timing)
| | found | wrong sends | missed but offered | suggestions/hour |
|---|---|---|---|---|
| Rust | 184/212 (86.8%) | 3 of 298 | 18/28 | 78 |
| Node (its own recorded run) | 181/212 (85.4%) | 2 of 279 | 22/31 | 50 |

Two of Rust's three "wrong" sends are verses the preacher did read that the
ground truth doesn't list (1 Corinthians 9:24, Genesis 26:13). Rust doesn't
yet have the meaning paths (Node found 1 verse that way). Possible Matches is
busier than Node's; that's next to look at.

## The server, the speech engines, the app (2026-10-12)

The Node server is replaced by `crates/kairo-server` (axum), serving the same
contract the UI already speaks — same routes, same WebSocket messages, same
port and token — so the UI is unchanged. In release builds it runs inside the
app; `npm run dev` runs it as its own process (`npm run server`). Node is no
longer bundled (`npm run server:node` still runs the old server for
comparison).

### Ported
- Static UI, token auth, settings, the WebSocket (catch-up state for windows
  that reconnect, microphone audio intake), Back history, layers, looks,
  monitor message, Live Queue sends, search, lookups, ranges.
- **Deepgram** (keep-alive, stuck-connection watchdog, reconnect with the gap's
  audio replayed, unfinished words locked, Finalize on stop, refusals
  explained) and the **offline engine** (Nemotron through sherpa-onnx,
  locking, endpoints, shared and released model) — one funnel for both, one
  vocabulary for both. Tested end to end with streamed audio.
- The offline model installer (resumable, retried, checksum-verified).
- Songs, triggers (stage timer, clock), segments (with the pre-service loop,
  exported from `src/announcement_pack.js` by
  `scripts/export-announcement-pack.js`), media library (range-served files,
  watched folders), OBS (obs-websocket v5), translation (the Bible in the
  language; Ollama for other text), exports.
- Slide import (text, Word, PowerPoint, ProPresenter 6/7, bundles, playlists)
  and ProPresenter themes — own zip, protobuf and RTF readers.

### Executive decisions
- **Word and pasted text import the way they're written.** Most Word documents
  have no blank lines between paragraphs and imported as one slide. Now:
  section labels and headings start slides; Word line breaks (and their
  clipboard forms) stay lines; short lines group four to a slide; each prose
  paragraph is its own slide, split at sentence ends when long.
- **Sermon notes rebuilt on the engine's record of the service**: the main
  text printed, each scripture marked read / cited / alluded to, themes from
  the words that identify the scriptures used, people of the Bible named,
  notes for unpunctuated speech. No cloud model and no bundled language model
  (owner: no added costs, no bloat); lines must be almost entirely real words
  to be quoted.
- **No re-offer of a verse within 3 minutes; fingerprint and scoped meaning
  never offer alone; partial wording is offered only where the sermon's
  context points at it** (passage in play, person named, book just named,
  "the Bible says…"). Possible Matches: 78 → 20 per hour, noise 11.7 → 5.5
  per hour (Node: 50 and 11.7), the same verses found.
- **The meaning models stay as Node used them for now** (mean-pooled
  EmbeddingGemma, MiniLM cross-encoder) with Node's thresholds; the corrected
  model usage is calibrated (`kairo-eval --bin calibrate`) but not yet
  validated on the dev sermons, so it doesn't ship.

### Not in this build
- The extra time-offset Deepgram streams (citation voting).
- Downloading local translation models (Ollama and the bundled Bibles still
  translate); scripture language-pack downloads.
- The optional Ollama polish of sermon notes.

### Size
App about 400 MB on disk with the same features (655 MB today): no Node
runtime or packages, the verse index 23 MB instead of 91 MB of raw vectors.
Compressing the Bible text and tokenizer would bring it to about 330 MB.

### How to test
1. `npm run dev` on branch `rust-rewrite` (the first run compiles the server:
   a minute or two). Nothing else should be listening on port 7777.
2. Live: Deepgram and Offline (Settings → Speech Engine). The offline model
   downloads from Settings as before.
3. Detection, Possible Matches, ranges ("verses 3 to 5", "next verse"),
   search, Back, Live Queue, media, songs, segments/timers, OBS.
4. Import a Word document and paste from the clipboard.
5. Export → Sermon notes / transcript / verses.
6. Replays: `cargo run --release -p kairo-eval --bin replay` (dev sermons,
   real timing, with `--verbose`, `--noise`, `--trace a..b`).

## First live test (2026-10-12)

The owner's test service played the "identity in Christ" clip; three
recordings of it are in `parity/golden` and now have an answer key
(`server/eval/fixtures/2026-09-26-identity-in-christ-*.json`).

### Fixed
- **"John four verse four" with 1 John's words read** stayed John 4:4: the
  citation check switched only when the cross-encoder scored the sibling at
  0.5 or more (it scored 0.45). It now also switches when the cited verse
  plainly isn't what's said (cross-encoder ≤ 0.1) and the sibling's rare
  words and meaning both are; and when the cross-encoder is sure (≥ 0.9) the
  embedding needs only to agree in direction, not lead by 0.1.
- **A citation without its book** ("17 verse seven to eight", the book
  dropped by the recogniser) is kept for 30 s; a quote of a verse those
  numbers fit names the book, and it goes up as cited (Jeremiah 17:7–8; Psalm
  1:3, which shares "a tree planted by the", no longer goes up instead).
- **"Someone 25 verse one"** is "Psalm one twenty-five" — Node's parser rule,
  missing from the port.
- **The offline engine's short locked segments** ("I am he" / "and they fell
  backward") were too short for "the reading skipped ahead": that check now
  takes at least 10 words, so it sees what Deepgram's sentences show it.
- **Distinctive partial quotes are offered.** Rule 7b dropped every partial
  quote the sermon didn't point at; the answer keys list mostly cited verses,
  so real uncited quotes counted as noise. Read by hand, the dropped offers
  whose matched wording weighs ≥ 14.5 were real 12 times in 13 ("no respecter
  of persons", "our light affliction", "Christ our passover is sacrificed for
  us"), those below 3 in 16 ("in the name of Jesus Christ", "all the days of
  our life"). Checked, not tuned, on the held-out sermons: 4–5 right of 6.
  Paraphrases didn't separate (6 right of 9 at the same scores) and still need
  the sermon's support.
- **The bundled KJV was misnumbered** — the current app shows the same wrong
  text. Six verses were missing (Matthew 2:16, 22:1, 26:38; Mark 4:40, 7:11,
  8:8: every later verse in those chapters carried the next one's text — "let
  this cup pass from me" showed as Matthew 26:38) and four split (1 Samuel
  20:42, 1 Kings 22:43, 3 John 14, Revelation 12:17/13:1).
  `scripts/fix-kjv-versification.py` restores the standard 31,102 verses;
  wording checked against the KJV. The NLT (aligned by reference) now lines up
  at those verses; it has no text for the last verse of the six chapters. The
  verse index is rebuilt, and an index built for another numbering is refused.
- Double-clicking a transcript line no longer fills the Bible search.
- The server prints each decision (`[Detect] John 3:16 → screen (direct 0.93)`).

### Offline, tested like Deepgram
`kairo-eval --bin offline-transcribe <audio> <name>` runs a recording through
the offline engine exactly as a live session does (the transcriber is shared
with the app), faster than real time; `replay --offline <name>` scores it.
Enough is Enough: offline 20/25, Deepgram 20/25 (the one "wrong" offline send
is Mark 11:23, which was quoted — the key lacks it). Live, 15 minutes through
each engine: both sent Romans 8:28 and Mark 9:23, both right, nothing else.

### Still missed
Quotes and paraphrases the sermon gives no pointer to and whose wording isn't
distinctive ("I am that I am", "Thou shalt serve the Lord and he shall bless",
the everlasting mountains of Habakkuk 3:6); "In Romans, whosoever call upon
the name…" with no chapter (Node misses it too).

### Offline closer to Deepgram (2026-10-12)
- **The rolling locks stay** (4 live words, locks of 3+, never ending on a
  number — `server/sherpa_engine.js`'s reasons: locking at pauses let a line
  grow to 72 words and fire its citations 25 s late). The engine adapts to
  them instead: every "the newest speech" window now takes segments until it
  has enough words (`Transcript::recent_finals`) — the paraphrase context (4
  segments, ≥ 40 words), the speech a range or twin check reads (≥ 12) — so a
  Deepgram sentence and the offline engine's several short runs give the same
  text. The on-screen comparison keeps the newest segment only (reaching
  further back read the verse on screen: Isaiah 53:8 held back by 53:7).
  Deepgram unchanged (218/255); offline +1.
- **Beam search and the book names as hotwords** are written and measurable
  (`offline-transcribe --beam`, `--hotwords 1.5`) but off: sherpa-onnx 1.13.8,
  the newest release, has no beam search for NeMo streaming models (it exits);
  k2-fsa/sherpa-onnx#3895 (14 Sep) adds it. Measure on the next release.
- **The answer keys gained four quotes** that were said but missing (Haggai 2:9,
  Proverbs 22:7, 2 Timothy 1:7 in Impartation; Mark 11:23 in Enough is Enough),
  read against both transcriptions.
- Result on the two sermons with audio: offline 64/81 found, 4 wrong; Deepgram
  64/81, 0 wrong. Offline's wrong sends are the recogniser's: "the eye shall be
  filled" for "the earth" (Isaiah 11:9 for Habakkuk 2:14), "Psalm one zero four
  verse" dropped (a bare "twenty four" read as Proverbs 3:24), and Romans 10:13
  for Acts 2:21 (the words said were Romans'). Closing that needs a better
  transcript: hotwords when the release lands, or a second, non-streaming pass
  over each finished sentence (a separate download — not done).
