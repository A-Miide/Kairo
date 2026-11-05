# 01 — From candidate to decision (`broadcastDetection`)

Source: `server/server.js` `broadcastDetection` (~7383–7782) and
`server/detection_scoring.js`. Every path (citation, partial, continuation,
verbatim, stream, fingerprint, semantic, paraphrase, named passage, voting)
ends here with `(verses, method, score, intended target, opts)`.

In Rust this is `kairo-detect::decide`: one `Decider` applying the rules
below in this order. Each rule is a named function with its principle as its
doc comment.

## Rules, in order

1. **Range advance.** If a range is active and the candidate is exactly the
   range's next expected verse (any method, score ≥ 0.50), advance the range
   and show it. A range "held" result caps the candidate at Possible Matches.
2. **Chapter continuity promotion.** A candidate that continues the chapter
   being read (see `tryChapterAdvanceByDetection`) is promoted to the screen —
   unless the caller capped it or the verbatim cross-check disagreed.
3. **Score.** finalScore = clamp(B + D + A) (`detection_scoring.js`):
   - B per method: direct 0.93, continuation 0.93, context-citation 0.85,
     chapter-keyword 0.82, direct-partial 0.75 (never alone), verbatim
     (similarity, floored to 0.90+ when matchedIdf ≥ 18, to 0.55–0.60 when
     10–18), stream (confirmed 0.90–0.97; unconfirmed unique phrase with
     idf ≥ 18 → 0.90; 12–18 → 0.55–0.60; else 0), fingerprint (≤ 0.75;
     0.85 if ≥ 95% coverage at high confidence), semantic (≤ 0.60; 0.85 at
     cosine ≥ 0.95), paraphrase (its own decision score), named passage 0.70.
   - D distance from the verse on screen, fading over 60 s: same chapter
     forward +0.15·(1−Δ/40); same chapter backward −0.20 only if that verse was
     already shown (an unshown earlier verse is a catch-up read, no penalty);
     same book −0.10; other book −0.15. Skipped entirely when B alone clears
     the screen bar (self-sufficient evidence) — except during a cited range
     for a non-citation candidate that collides with the range.
   - A corroboration in the last 20 s: +0.08 per extra independent method (max
     2), +0.05 per repeat (max 3), +0.06 when 2+ verses of the chapter were hit,
     +0.15 when a person named in the last minute appears in the candidate's
     chapter.
   - Hard caps (below the screen bar): a non-citation candidate colliding with
     an active range; a non-citation candidate in a chapter confusable with the
     one on screen. **Rust: "confusable" is derived — two chapters containing
     a twin pair — replacing the hand-kept Jeremiah 17 / Psalm 1 registry.**
   - Target: ≥ 0.80 screen, ≥ 0.50 Possible Matches, else drop. Semantic needs
     corroboration (2+ landed methods on the verse, read before recording this
     one) or very high confidence to reach the screen.
   - The ledger records every hit; only a hit that landed somewhere — and, for
     verbatim, covered at least `CORROBORATING_QUOTE_COVERAGE` of the verse —
     counts as a second opinion.
4. **Late corroboration.** A stream match held for want of corroboration is
   re-decided when another method's landed hit arrives on the same verse within
   `LATE_CORROBORATION_WINDOW_MS`.
5. **Caller caps.** `verbatimDisagreed` (the full-index cross-check picked
   another verse) and `capAtSuggestions` only ever demote screen → Possible
   Matches.
6. **Drop** ends here.
7. **Twin in play.** A text-matched suggestion whose twin is the passage in
   play isn't offered — the screen already shows what was said.
8. **Twin resolution** (text-matched screen candidates; see the twin engine):
   switch to the better-supported twin (re-decided with the same evidence), or
   hold both as Possible Matches.
9. **Text-match guards** (text-matched screen candidates only — a spoken
   citation names its verse): offer instead of send when
   - a song / announcement / non-scripture slide is live;
   - the verse was shown and replaced, and nothing said since points back to it;
   - a range just moved on from it and nothing since is more of it than its
     last words;
   - a chapter was just named with its verse still to come, and the candidate
     is elsewhere (unless a word-for-word quote ≥ 0.95);
   - the words being said fit the verse on screen at least as well (compared
     on the words the two verses don't share), unless a near-complete verbatim.
10. **Suggestion de-duplication.** A verse is offered at most once per 30 s.
11. **Citation guards.** A spoken citation is demoted to Possible Matches when
    it re-asserts a reference just corrected away while the correction is still
    on screen, or drags the display back to an already-shown earlier verse of
    the chapter on screen (a re-parse of text already handled).
12. **Screen de-duplication.** The same verse isn't re-sent within its window
    (shorter for spoken triggers), except: a different verse of the same book
    within the book window; a spoken citation replacing a weaker method's send
    of the same verse; a citation widening a range that started on the same
    verse.
13. **Effects** of a screen decision: translation and language applied,
    outputs updated, the verse recorded as shown for its book, the reading
    context moved to it (citations set it at their own call sites), the
    correction watermarks set (citations) or the stale-override note kept
    (text matches), the Bible layer brought above slides.

## Thresholds

| Name | Value | Meaning |
|---|---|---|
| VIEWER_MIN_SCORE | 0.80 | screen bar |
| suggestions floor | 0.50 | Possible Matches bar (decideTarget) |
| SUGGESTION_MIN_SCORE | 0.87 | caller-side bar for some suggestion paths |
| VERBATIM_AUTOSEND_MIN | 0.95 | verbatim's own screen bar |
| VERBATIM_CERTAIN_IDF | 18 | matched weight that's certain despite partial coverage |
| VERBATIM_MODERATE_IDF | 10 | matched weight worth offering |
| STREAM_IDF_FULL_CONFIDENCE | 12 | stream confirmation weight |
| LEDGER_WINDOW_MS | 20 000 | corroboration window |
| SAME_BOOK_WINDOW_MS | 60 000 | continuity window |
| SUGGESTION_DEDUP_MS | 30 000 | offer a verse once per |
| INTERIM_STABLE_WINDOW_MS | 3 000 | an interim citation must be seen twice this far apart |
| CORRECTION_WINDOW_WORDS | 35 | words after a citation in which a quote can correct it |
| CORRECTION_IMMUNITY_MS | 3 000 | no correction right after a send |
| DIRECT_PARTIAL_SCORE | 0.90 | score shown for a "verse N" send |
| RANGE_ADVANCE_MIN_SCORE | 0.50 | a range's next verse needs only this |
| CHAPTER_NAMED_HOLD_MS | 30 000 | a named chapter holds text matches elsewhere |
