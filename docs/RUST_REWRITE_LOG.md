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
