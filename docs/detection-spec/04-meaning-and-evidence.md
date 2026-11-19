# 04 — Meaning-based matching and the shared evidence tests

Sources: `server/server.js` `runFingerprintSearch` (~6449), the semantic and
paraphrase sections (~6576–6773), the evidence helpers (~6794–6987);
`server/paraphrase.js`.

## Fingerprint

- Text: recent buffer segments that held no citation + the current segment;
  clauses (4+ words) that contain no book name (citations own those), most
  recent first; the last 40 words as a fallback if they hold no book name.
- Suggestions only, never within 4 s of a citation; results ≥ 0.87, at most 3.
  ≥ 0.87 feeds the sermon context.
- **Scoped pass** (also semantic, below): restricted to chapters with real
  context — every chapter of a book named in the last ~25 words, the chapters a
  person named in the last minute appears in, and the chapter on screen ± 1
  (sent within 60 s). Qualifies at ≥ 0.60 with a ≥ 0.03 lead over the
  runner-up, and not already offered by the unscoped pass.

## Paraphrase (meaning)

- Every 6 words heard (interim included). Searched on windows of the last ~60
  words (`paraphraseWindows`): after the last quote signal ("the Bible says",
  "Jesus said", "Paul wrote", … — crowd work removed), the fused last ~26 words
  with crowd work and fillers removed, the last sentence, the last two, two
  sentences around a short aside, the last 24 and 14 words. At most 5.
- Per candidate verse: best cosine over windows, cross-encoder score against
  the KJV (`rr`) and against a modern wording (`rrModern`, the church's own,
  else NLT, else NIV), and shared identifying wording (`lex`, IDF sum over
  shared stems in KJV + first other wording).
- `decideParaphrase`: send (rr ≥ 0.99, cos ≥ 0.84, lex ≥ 15, lead ≥ 0.2); or
  sendable but tied with a verse saying the same thing, and the passage in play
  favours this one (affinity ≥ 2 vs < 2); offer (rr ≥ 0.98, cos ≥ 0.8, lex ≥ 10;
  in play: 0.95 / 0.78 / 8); offer by modern wording (rrModern ≥ 0.98, cos ≥
  0.75, lex ≥ 15, lead ≥ 0.2); no reranker (non-English): cos ≥ 0.82, lex ≥ 10.
- **Acted on only when the next window's decision repeats it.**
- Within 4 s of a citation: offer only; not offered at all when the verse is
  on screen or (just after a citation) in the cited chapter.
- **Thresholds are calibrated to the current embedding usage and reranker.**
  Any change of model, pooling, prompts or fusion requires recalibration on the
  development sermons and written cases (never the held-out sermons).

## Shared evidence tests (used across the decision rules)

- **Evidence words:** meaningful words (≥ 3 letters, not stop words), minus
  pronouns and helper verbs that occur in nearly every verse, lightly stemmed.
  Everyday preaching words (Jesus, Christ, Lord, God, name, holy, ghost, spirit,
  father, amen, praise, glory, hallelujah) never count as explaining a verse.
- **A fits speech at least as well as B:** count speech words only A has and
  only B has (shared words can't separate them). A holds if it has ≥ 1 own word
  heard and at least as many as B; or B has none heard and A has any word heard.
- **Explained better by another verse:** a rival verse whose evidence words
  heard number ≥ 3 and ≥ the candidate's + 2, with no tie for the best rival.
- **Speech favours a candidate over the range:** more of the candidate's words
  in what was just said than of any single range verse's.
- **A shown verse returns only on new evidence:** a verse already shown and
  replaced goes back up only if the words said since the current verse went up
  hold words only it has, more than words only the current verse has.
- **A range's left verse returns only on its own words:** for 8 s after the
  range moved on, the verse it left needs words of its own heard since the move
  — not its last three words, not words the new verse also has.
- **Passage affinity** (higher = more clearly the passage in play): range
  chapter 3; chapter on screen 2 (its next verse 2.5); reading context 2;
  reading-mode chapter 2; cited in the last 5 min: same chapter 2, same book 1;
  chapter of the person just named 1.
