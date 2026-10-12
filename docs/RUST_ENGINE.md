# The detection engine (`kairo-detect::engine`)

What replaces `server.js`'s detection half and `detection_worker.js`. The
behaviour is specified in `docs/detection-spec/`; this is the shape.

## One state machine, no clock, no I/O

```
Engine::handle(Event) -> Output { effects: Vec<Effect>, requests: Vec<Request> }
```

- **Events**: a transcript segment (text, interim/final, end of utterance,
  word timings, continues-live, which stream), an operator action (send,
  clear, back, range next/jump/clear, settings change, a non-scripture slide
  going live), a timer firing, or the result of a request.
- **Every event carries its time.** Nothing inside reads a clock, so a replay
  of recorded events gives the same decisions every time (the Node engine
  varied on ~8% of decisions between two runs of the same input).
- **Effects** are what the server turns into the UI's existing messages: a
  verse to the screen or Possible Matches (with method, score, corrected-from),
  range state, reading mode, topic library, translation changed.
- **Requests** are the slow, model-backed work (meaning search, reranking,
  comparing verses against speech). The runtime runs them on a worker pool and
  feeds the answers back as events, stamped with when they arrived. Detection
  never blocks on a model, and tests can answer instantly or with simulated
  latency.
- **Timers** (a chapter-said-twice hold expiring, the sentence buffer timing
  out) are requested as effects and come back as events.

## Read-only knowledge, shared

`Bible` (built once, `Arc`): the corpus and its shape, the reference parser,
the `Lexicon` (word indexes, weights, fingerprints, anchors, twins), confusable
chapters (derived from twins), the person-name index, display translations.

## Session state, grouped by what it's about

Instead of ~100 module-level variables, each concern owns its state and the
rules that read it:

| Part | Holds |
|---|---|
| `Transcript` | 90 s of finals, words-heard counter, the previous final for joins, the open interim, the sentence buffer, interim stability and sent-from-interim sets |
| `Screen` | the verse up and when, verses shown this book, last detected / sent, recent detections, a correction remembered, the last citation (for corrections), a live non-scripture slide |
| `Reading` | the reading context (book, chapter, pace) and reading mode |
| `Range` | queue, all verses, current, the verse it left, speech since it moved |
| `Evidence` | the corroboration ledger, matches waiting for corroboration, recent stream anchors, cross-check rejections, suggestion de-duplication |
| `Sermon` | recent citations, the topic library and word counts, the person just named |
| `Pending` | a chapter waiting for its verse, a chapter said twice, the citation check, twin decisions, the paraphrase repeat |
| `Stream` | the streaming matcher |

## Detectors and the decider

Each path is a detector: it reads the transcript and the state it needs and
produces `Candidate { verses, method, evidence, intent, caps }`. Detectors
never send anything. The `Decider` applies the ordered rules of
`detection-spec/01-decision.md` to every candidate, and only the decider
produces screen effects and updates `Screen`, `Range` and `Evidence`. That
single point is what lets new paths be added without new interactions:
everything a path needs to respect is in the decider.

## Measuring

- `parity/golden/*.json` inputs replay through the engine with their recorded
  timing; its decisions are compared with Node's stable ones.
- Ground-truth recall and wrong sends per sermon (development sermons while
  building; the held-out pair only at the end).
- The written paraphrase cases and negatives (`server/eval/paraphrase_cases.json`).
