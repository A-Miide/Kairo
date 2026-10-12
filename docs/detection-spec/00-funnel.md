# 00 — The funnel (`handleTranscriptSegment`)

Source: `server/server.js` `handleTranscriptSegment` (~lines 3700–4056).
Every speech engine (Deepgram, sherpa-onnx offline, browser speech, eval
replays) calls this with `(transcript, isFinal, confidence, speechFinal,
words?, continuesLive?)`.

## Every segment (interim and final)

1. Broadcast `{type:'transcript', text, isFinal, confidence, words?}` to the UI
   (original text, accents intact).
2. Fold accents for detection (`João` → `joao`); the worker folds its Bible
   text the same way.
3. Finals only: `wordsHeard += word count` — the monotonic "how much has been
   said" counter (used instead of wall-clock for correction windows).
4. Named-person recency: scan words right-to-left; the most recent biblical
   person name (Set lookup, lowercase, letters only) becomes
   `recentlyMentionedName` with a timestamp. Most recent wins outright.

## Interim segment

Interims are growing prefixes of the current utterance. sherpa-onnx's
interims are append-only (verified: 13 consecutive partials never revised a
word); Deepgram's mostly so. A future batch-style engine must declare its own
stability flag rather than inherit "offline == stable".

In order:
1. `streamNewWords(text, false)` — feed new words to the streaming 4-gram
   anchor matcher (word-count watermark diff).
2. Spoken "next verse" trigger.
3. Range speech tracking (`noteRangeSpeech`), then range auto-advance on the
   last two words of the current verse, and on a "next verse" prefix.
4. If the worker is ready:
   - `processForReferences(text, interim)` — explicit citations (interim
     gating rules in `01-references.md`).
   - `checkCitationSiblings` — sibling-book citations.
   - If no reference and the reading context (book+chapter) is valid and the
     text doesn't announce a different book: resolve a partial reference
     ("verse N") with **bare numbers disabled on interims** (incident: an
     enumerated sermon outline's "number two?" caught as a verse).
     - If the interim looks incomplete: a number at the very end that could
       still grow ("verse twenty…") → wait; any other → must be seen again
       ≥ `INTERIM_STABLE_WINDOW_MS` apart before acting.
     - Resolved: reposition an active range to it (`jumpRangeTo`), send as
       `context-citation` (chapter was spoken) or `direct-partial`
       (`DIRECT_PARTIAL_SCORE`, target viewer, through the continuity gate —
       not bypassing it). If it actually went out: refresh the reading
       context (bug #25: chains of bare "verse N" must extend the context,
       not spend down the original citation's time) and clear any pending
       chapter-only citation.
   - Still nothing: if ≥ 6 words and `INTERIM_VERBATIM_MS` since the last:
     verbatim search. If ≥ 6 words and `FINGERPRINT_INTERVAL_MS` since the
     last: fingerprint search. Then the paraphrase detector (rate-limited
     inside).

## Final segment

In order:
1. Log `[Transcript] "<text>"`; append to the 90-second transcript buffer
   (text, time, word count); update the reading pace factor.
2. `streamNewWords(text, true, continuesLive)`.
3. Take the set of verse keys already sent from this utterance's interims
   (so the final doesn't re-send them), then clear it.
4. "Next verse" trigger; mark a verse end if one was just finished; spoken
   translation command ("switch to the NIV").
5. Topic accumulator (words → topic library).
6. Range: speech tracking, advance on last words, on "next verse" prefix, and
   by content (the next verse's words being read).
7. `processForReferences(text, final, sentFromInterim, continuesLive)`, then
   sibling citations; if no reference, offer named passages ("the prodigal
   son") as suggestions.
8. If no reference and the reading context is valid:
   - Not if the text announces a different book (compared with the previous
     final).
   - "verse" ending the previous final + this one opening with a number,
     within `PREV_FINAL_JOIN_WINDOW_MS` → resolve as "verse N" across the
     split, **but only if the verse shares words with what's being said**
     (incidents: a stale 2 Corinthians 3 context; "start from verse" | "one"
     then 66:2 quoted sent Isaiah 66:1).
   - Range ("verses 3 to 5") → set the range queue; single verse → reposition
     an active range if inside it (bug #27: used to destroy the range),
     otherwise the range is cleared only if the verse actually reached the
     screen (a demoted stray number must not destroy the range).
   - Confirmed → refresh context, clear pending chapter-only.
9. Still nothing: bare verse number continuation ("…sixteen" when 15 is up),
   then the verse-end number jump.
10. Mark the buffer entry `hasRef`.
11. Lexical + semantic passes:
    - A range is on screen → skip the sentence buffer; run verbatim, the
      fingerprint (if its interval has passed) and the paraphrase detector on
      this final now (end-of-verse transitions must feel immediate).
    - Otherwise → the sentence buffer: `speechFinal` flushes it, else append;
      a flush (or its timeout) runs verbatim + fingerprint + paraphrase on the
      flushed text. No flush and ≥ 8 words → verbatim on this final alone.
12. `speechFinal` → rescan the last ~60 words of the buffer (verbatim with
    `rescan`, fingerprint) if different from what was just searched. (The
    whole 90 s buffer cost 200–500 ms and delayed direct lookups.)

## How this maps to the Rust design

- Steps that only update state (word counter, named-person recency, buffer,
  pace, topic words, range speech) → `SessionState::observe(event)`.
- Steps that produce candidates (references, partials, bare numbers, verse-end
  jumps, siblings, named passages, verbatim, fingerprint, anchors, paraphrase,
  voting) → detectors emitting `Evidence`.
- Interim gates (bare numbers off, growable numbers wait, stability window,
  word-count minimums, per-path intervals) → declared per detector, enforced
  by the pipeline.
- "Sent from interim" suppression, the continuity gate, twin resolution →
  the scorer.
- Effects of a decision (range set / reposition / clear-only-if-sent, context
  refresh, chapter-only cleared) → `Decision` outcomes applied in one place,
  so every path gets them consistently instead of each call site
  re-implementing them.
- Commands (next verse, translation change) → `Command`s, not detections.
