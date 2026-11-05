# Confidence scores, measured (2026-11-05)

Measured only: nothing about how scores are made or used was changed.

Every screen send in the recorded-sermon replays, grouped by the method and the score shown to
the operator (rounded to a whole percent, as the server sends it). "Wrong" is the replay's own
count of wrong sends: not a verse cited or read then, and not part of a range or passage in play.
Every other send counts as right.

How to reproduce (branch `feat/detection-owner-asks`):

```
replay --meaning bge-small.json --by-score
replay --offline --meaning bge-small.json --by-score enough-is-enough.coreml-cpu shiloh2025-impartation.coreml-cpu
```

## Deepgram transcripts (6 sermons: 233/265 found, 3 wrong of 323 sent)

| Method | Score shown | Sends | Right | Wrong | Measured accuracy |
|---|---|---|---|---|---|
| chapter-keyword | 82% | 6 | 6 | 0 | 100% |
| chapter-keyword | 88% | 1 | 1 | 0 | 100% |
| chapter-keyword | 97% | 1 | 1 | 0 | 100% |
| context-citation | 85% | 2 | 2 | 0 | 100% |
| context-citation | 100% | 1 | 1 | 0 | 100% |
| continuation | 100% | 1 | 1 | 0 | 100% |
| direct | 93% | 75 | 73 | 2 | 97.3% |
| direct | 98% | 2 | 2 | 0 | 100% |
| direct | 99% | 2 | 2 | 0 | 100% |
| direct | 100% | 114 | 113 | 1 | 99.1% |
| direct-partial | 81–100% | 12 | 12 | 0 | 100% |
| fingerprint | 100% | 4 | 4 | 0 | 100% |
| stream | 82% | 1 | 1 | 0 | 100% |
| stream | 90% | 33 | 33 | 0 | 100% |
| stream | 91–99% | 31 | 31 | 0 | 100% |
| stream | 100% | 31 | 31 | 0 | 100% |
| verbatim | 100% | 6 | 6 | 0 | 100% |

## Offline Core ML transcripts (2 sermons: 67/82 found, 3 wrong of 92 sent)

| Method | Score shown | Sends | Right | Wrong | Measured accuracy |
|---|---|---|---|---|---|
| direct | 93% | 14 | 14 | 0 | 100% |
| direct | 98–99% | 2 | 2 | 0 | 100% |
| direct | 100% | 24 | 21 | 3 | 87.5% |
| direct-partial | 90–100% | 4 | 4 | 0 | 100% |
| fingerprint | 100% | 1 | 1 | 0 | 100% |
| stream | 90% | 23 | 23 | 0 | 100% |
| stream | 91–99% | 5 | 5 | 0 | 100% |
| stream | 100% | 16 | 16 | 0 | 100% |
| verbatim | 100% | 3 | 3 | 0 | 100% |

## What it says

- The score shown does not track how likely a send is to be right. All six wrong sends were
  `direct` (a reference heard), and four of them showed 100%; a misheard reference scores as
  high as a clear one. Stream, verbatim, fingerprint and the lower-scored methods were never
  wrong here.
- Most methods show a fixed score (direct 93% or 100%, stream 90%), so the number tells the
  operator the method more than the certainty.
- The samples are small (6 wrong of 415 sends), so per-score accuracies below 100% rest on one
  to three sends each.
