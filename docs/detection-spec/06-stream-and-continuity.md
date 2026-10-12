# 06 — Following a reading word by word, and chapter continuity

Source: `server/server.js` ~4596–5101 (`tryRangeAdvanceByRef`,
`tryRangeAdvanceByDetection`, `tryChapterAdvanceByDetection`,
`streamNewWords`, `processStreamText`, `rankStreamHits`, `settleStreamTie`).
Matching itself is `kairo-detect::lexical::stream`.

## Feeding words

Every word, interim and final, as soon as it's heard: a watermark over the
growing interim feeds only new words; it resets at each final — except when the
live text continues past the final (offline engine), where what was already fed
carries over. The offline engine's newest interim word may still be growing,
so it's fed once the next word shows it complete.

## Ranking hits

Confirmed alignments first; then longer matches; verses in the topic library;
verses near a citation of the last 5 minutes (same book, within 5 chapters,
+40 − 6·Δchapter, fading with age).

## Confirmed alignments (top 5 considered)

1. **Tie** — a different verse within 2 aligned words of the top (shared
   opening wording): if exactly one side's book was on screen in the last
   `TIE_BREAK_CONTINUITY_WINDOW_MS` and it isn't behind the verse on screen in
   its chapter → it wins; else, among the tied, exactly one with passage
   affinity ≥ 2 and pointed at (not behind the verse on screen) → it wins; else
   hold both back.
2. Miscitation correction gets first look (alternates: the other top hits).
3. Confirmed only via backward credit and not corroborated → offered; held for
   late corroboration.
4. **Cross-check** (not for forward reading in the chapter on screen): a
   verbatim search of the last 100 words picks a different verse with matched
   weight ≥ `STREAM_CROSSCHECK_MIN_IDF` → offered, marked disagreed, and that
   verse is refused again for `STREAM_CROSSCHECK_COOLDOWN_MS`.
5. **Thin on its own**: nothing points at the passage (no citation, range or
   chapter on screen behind it), fewer than 3 weighty words aligned, not
   corroborated → before holding it, its twin (if the speech separates them)
   or a verse the stream touched just now that explains clearly more of what
   was said goes up instead; otherwise held in Possible Matches for a second
   method (late corroboration re-decides it).
6. Sent as `stream`. If it went up with nothing pointing at it and isn't
   certain on its own words (weight < 18), the citation check runs over the
   other verses the stream touched (see `05-citation-check.md`).

## Unconfirmed anchors

A phrase unique to one verse (df 1) carrying ≥ 12 of weight: miscitation
correction first, else sent as `stream` at ≥ 0.90 (the scorer then grades it:
≥ 18 sends, 12–18 is offered). Shared phrases are not shown — the alignment
promotes the real one within a few words.

## Range advance by any method

A range is active and a single-verse candidate is exactly its next verse,
score ≥ 0.50 → advance. For text matches, the match must rest on a word only
the next verse says, heard since the range last moved — otherwise it is held
in Possible Matches (words both verses say are the current verse being read
just as much). A spoken reference ("verse two") names the verse and advances
directly.

## Chapter continuity

No range active, a verse on screen sent within 60 s, a single-verse candidate
in the same chapter, a different verse, score ≥ 0.50 (≥ 0.95 for semantic and
fingerprint, whose scores are noisier) → promoted to the screen.
