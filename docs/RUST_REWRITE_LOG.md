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
