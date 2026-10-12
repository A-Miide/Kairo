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
