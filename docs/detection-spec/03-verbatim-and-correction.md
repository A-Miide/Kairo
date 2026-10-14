# 03 — Verbatim quotes and miscitation correction

Source: `server/server.js` `processVerbatim` (~6013), `maybeCorrectMiscitation`
(~6178), `buildClauseList` (~5960). Matching itself is
`kairo-detect::lexical::verbatim`.

## What gets searched

- Clauses split at `. ! ? , ;` (6+ words each); a clause longer than 21 words
  is also cut into overlapping 14-word windows, step 7 (the offline engine
  writes few full stops, so quotes arrive inside long runs); the whole text
  last. One batched search, results cached 8 s (the same text is searched
  repeatedly: a final usually equals its last interim).
- `rescan` mode: the end-of-utterance sweep over the last ~60 words.

## Deciding

- **Screen candidates:** similarity ≥ 0.95, or matched weight ≥ 18 (certain
  despite partial coverage).
  - Rescan hit that the segment just spoken doesn't share words with, or that
    steps back to an earlier verse of the chapter on screen → offered only
    (the sweep must not resurrect a quote the reading has moved past).
  - Otherwise: try a miscitation correction first (below).
  - Partial quote (via weight, coverage < `CORROBORATING_QUOTE_COVERAGE`):
    if another verse the speech touched (stream anchors near it, other results)
    explains clearly more of what was said → offered only.
  - Score used: max(similarity, 0.90) when certain via weight.
  - Certain via weight but not corroborated by another method → offered only.
  - Similarity ≥ 0.92 feeds the sermon context and reading-mode tracker.
- **Below the bar:** the best result is offered (capped at Possible Matches;
  the scorer's 0.50 floor drops weak ones).

## Miscitation correction

A preacher who cites X but reads Y: Y replaces X on screen, marked corrected.
Only when ALL hold:

1. Auto-correct is on, and a citation was sent.
2. The cited verse is still what's on screen.
3. Within 35 words heard since the citation, and not within 3 s of it.
4. The cited book hasn't been independently re-confirmed since (another verse
   of it reached the screen after the citation).
5. Y isn't already what was last detected.
6. Y isn't the citation's twin (the citation decides between twins).
7. The text driving Y doesn't name Y's book aloud (that's a fresh citation
   still being parsed, not a misquote).
8. Strong: similarity ≥ 0.95, or matched weight ≥ 15.
9. No comparably strong alternate in another book (≥ 70% of Y's weight) —
   two verses sharing wording, not a correction.
10. Not a confusable passage pair (**Rust: derived from twins**, replacing the
    registry).
11. Not the same verse.
12. Y wasn't on screen before the citation (stale reading text in the buffer).
13. Not forward within the cited chapter by ≤ 5 verses (that's reading on);
    not within an established range's chapter.
14. Not when ≥ 70% of the words driving Y are also the cited verse's.
15. Not when everything said since the citation (from 1 s before it) fits the
    cited verse at least as well as Y (compared on words they don't share).

Effects: the corrected-away reference is remembered (a re-parse of it is then
blocked while the correction is up); the correction window is consumed; Y is
sent directly to outputs (bypassing de-duplication) and recorded as shown.

## Thresholds

| Name | Value |
|---|---|
| VERBATIM_AUTOSEND_MIN | 0.95 |
| VERBATIM_CERTAIN_IDF | 18 |
| CORRECTION_MIN_SIMILARITY | 0.95 |
| CORRECTION_MIN_IDF | 15 |
| CORRECTION_WINDOW_WORDS | 35 |
| CORRECTION_IMMUNITY_MS | 3 000 |
| MISCITATION_FORWARD_EXEMPT_VERSES | 5 |
| CLAUSE_WINDOW_WORDS / STEP | 14 / 7 |
| VERBATIM_CACHE_MS | 8 000 |
