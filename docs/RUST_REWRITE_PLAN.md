# Kairo — Rust rewrite plan

Branch: `rust-rewrite`. Owner decisions recorded 2026-10-11.

## Goals

1. Replace the Node sidecar (`server/`, ~16k lines) with Rust inside the Tauri
   process. One native binary, no bundled Node runtime.
2. Rewrite every detection path cleanly while keeping every behaviour the
   current code learned from real services (see "Parity" below).
3. Add what Pewbeam does that Kairo doesn't (see "Pewbeam gains").
4. Smaller app, while bundling everything scripture detection depends on.

## Non-goals and fixed constraints

- **The UI does not change.** `src/` keeps working untouched. The Rust backend
  serves the same HTTP routes, WebSocket messages, auth token, and Tauri
  commands/events the UI uses today.
- **Deepgram stays** the live speech engine, including the time-offset extra
  streams used for citation voting.
- **The offline model stays**: sherpa-onnx with NVIDIA Nemotron Speech
  Streaming 0.6B (int8), and the multilingual Nemotron 3.5 variant.
- **One funnel.** Deepgram, the offline engine, browser speech, and test
  replays each have a thin adapter that produces the same `TranscriptEvent`.
  Everything after the adapter is shared, so every improvement benefits every
  engine. The vocabulary list (church terms, scripture terms, Bible names,
  learned keyterms) is defined once and fed to both engines: Deepgram as
  `keyterm`, sherpa-onnx as hotwords (to verify: transducer hotword biasing
  with the Nemotron model).
- **No text taken from other apps.** Bible text comes from sources the owner
  holds rights to or from public-domain/openly licensed sources.

## The contract the UI depends on

- 70 HTTP routes in `server/server.js` (`grep -nE "^\s*app\.(get|post|put|delete)" server/server.js`).
- WebSocket on the same port: server→client messages (`detection`,
  `transcript`, `connection-state`, `range-*`, `layer-state`, `look-update`,
  `output-transition`, `clear-layer`, `timer-slide`, `media*`, `obs-status`,
  `translation-changed`, `reading-mode`, `topic-library`, `worker-*`,
  `listening-error`, `monitor-message`, `audio-output`, `shutdown`), client→server
  messages, and the PCM audio stream from `src/audio_sender_worker.js`.
- Tauri commands already in `src-tauri/src/lib.rs` (`get_server_port`,
  `get_server_token`, `get_server_config`, `list_monitors`, NDI/Syphon/Spout,
  fonts, updater) and the `__TAURI__.event.listen` events.
- `settings.json` keys and the app-data directory layout.

Phase 1 writes this contract out as JSON schemas plus recorded request/response
pairs, and the Rust server is tested against them.

## Target architecture

```
src-tauri/                      the Tauri app (exists): windows, outputs, menu
crates/
  kairo-bible/                  corpus, book names, verse counts, translations,
                                language packs, named passages
  kairo-detect/                 pure detection — no IO, no clock reads
    reference/                  spoken-reference parser (+ i18n citations)
    lexical/                    verbatim, fingerprint, streaming 4-gram anchors
    semantic/                   query building (shingles, narrative anchors)
    context/                    sermon context, chapter resolver, ranges,
                                reading mode, topic accumulator
    scoring/                    evidence ledger → one decision
  kairo-semantic/               EmbeddingGemma (ort + tokenizers), int8 vector
                                index, Matryoshka prefilter, reranker,
                                translation-fusion index builder
  kairo-stt/                    adapters → TranscriptEvent: Deepgram (+ extra
                                streams), sherpa-onnx, web speech, replay;
                                shared vocabulary provider
  kairo-server/                 axum: the 70 routes + WebSocket, same port/token
  kairo-integrations/           ProPresenter, OBS, triggers, segments, songs,
                                media, slide/theme import, export, translation
```

Disambiguation is one stage of the scorer, used by every path: the **twin
engine** (verses sharing ≥2 distinctive 4-grams, derived from the corpus;
resolved by citation, then the words unique to each twin, then the passage in
play; decisions held per passage) and **sibling books** (1/2 Corinthians etc.).
Verses with identical wording (Mark 9:46/48, Revelation 3:13/22) are exact
twins and are resolved the same way.

Before a path is ported, its behaviour is written down in
`docs/detection-spec/<path>.md` from its code, comments and tests (every
threshold, every incident it encodes). The Rust module and its tests are built
from that spec, so nuances carry over by design rather than by luck.

The core design rule: `kairo-detect` is a deterministic state machine.
`TranscriptEvent`s go in with timestamps (no `Date.now()` inside), and
`Decision`s come out. Every detection path is a detector that adds evidence
to the ledger; only the scorer decides what reaches the screen. That is what
makes replays exact and lets new paths be added without conflicting with
existing ones.

## Pewbeam gains

| Pewbeam behaviour | Kairo today | Plan |
|---|---|---|
| Tauri + native backend, no bundled runtime | Tauri + Node sidecar | This rewrite |
| Fast path on interim transcripts | Yes | Port |
| Chapter-only / partial references | Yes | Port |
| Translation named in the reference | Yes | Port |
| Voice commands ("next verse") | Yes | Port |
| Range-end suppression | Partly (range code) | Port + explicit guard |
| Book-conflict suppression on interims | No | Add as a guard on interim evidence |
| Converged-utterance gating | Partly | Make explicit in the scorer |
| Direct reference suppresses meaning-based candidates | Partly | Make explicit in the scorer |
| Utterance shingles → semantic search | No | Add as a semantic query builder |
| Narrative anchors | Named passages + named-entity search | Port + merge |
| Semantic search time budget | No | Add (fallback cancelled after a budget) |
| Adjustable confidence threshold (incl. OSC) | No | Add setting + OSC address |
| Learned keyterms, saved between services | Fixed list + custom terms | Add; feeds Deepgram and sherpa |
| Translation-fusion verse vectors | KJV only | KJV + modern translation(s), chosen by benchmark |

## Bible data and the fused index

- The corpus is the complete KJV (31,100 verses in `kjv.json`). `map.json` is
  missing 92 verses with duplicate wording (e.g. Luke 13:5, Psalm 107:15/21/31,
  Revelation 3:13/22, Mark 9:46/48), so today those can't be shown even when
  cited directly.
- One cleaning step produces the text every path matches against: marginal
  notes (`{undefiled: or, perfect}`) removed, italic markers (`{are}`) kept as
  words, superscriptions (`[A Psalm of David.]`) and acrostic headers
  (`ALEPH.`) removed.
- KJV is the anchor (most English-speaking churches read it).
- It is fused with modern-English translation(s). Candidates are benchmarked
  with `eval/paraphrase_bench.js`; the winner ships.
- NLT in `map.json` is missing Psalms (0/2457) and ~70 other verses. It gets
  filled only from a licensed source once Tyndale's permission arrives.
- Note: `databases/bibles/translations/ATTRIBUTION.md` records NKJV/NIV/ESV/NASB
  as coming from another app's bundle path. Confirm the rights before shipping
  them to churches.

## Size

Bundled (detection depends on it): EmbeddingGemma q4, int8 verse vectors
(91 MB → ~23 MB), reranker, Bibles and fusion data, offline Nemotron model.
Optional downloads (not detection): live-translation models, non-English
language packs.

Removed: Node runtime (79–115 MB), `server/node_modules` (108 MB),
duplicate `kjv.json`, other platforms' native binaries.

Models live in a versioned directory so app updates don't re-download them
unless a model actually changes.

## Generalization rule (owner, 2026-10-11)

The Node engine was tuned incident by incident and leans on the verses in its
tests. The rewrite keeps each lesson as a general rule and drops the tuning:

- No logic special-cases a verse, chapter or book. Limits and lists come from
  the corpus (chapter/verse counts, a chapter's last verse, verses sharing
  wording) or from the structure of speech, not from incidents.
- Mishearings are handled by general mechanisms (sound-based book matching in
  a citation-shaped slot, number words heard as near-homophones), not by
  lists of specific slips. The only hand-kept vocabulary is linguistic (book
  names that are everyday English words, the words for "chapter"/"verse").
- Comments state the principle, not the incident or the verse.
- Tests are generated across the whole Bible (random verses in many spoken
  forms; ordinary sermon speech as negatives). Two of the five recorded
  sermons are held out and only used for the final accuracy check.

### Evaluation split (fixed 2026-10-11, before any results were inspected)

- Development: `enough-is-enough`, `oyedepo-kingdomlaw`,
  `shiloh2025-impartation`, and the three `live_runs` transcripts. Rules may be
  checked and adjusted against these.
- Held out: `oyedepo-meditation`, `shiloh2025-love-mystery` (one of each
  preacher with two recordings). Only scored at the end; their transcripts and
  per-case results are not inspected while building.

## Parity

Parity means no loss of accuracy, not identical decisions — identical would
carry the overfitting over. The golden logs show every place the two engines
differ; each difference is checked against ground truth and either fixed or
listed in `docs/RUST_REWRITE_DIFFS.md` as an intended change.

1. Record a golden log from the current Node engine for every fixture in
   `eval/fixtures/` and `eval/live_runs/`: every `detection`
   broadcast (target, verses, method), with a virtual clock so runs are
   repeatable.
2. The Rust engine replays the same inputs and must produce the same log.
   Each difference is either a bug (fix it) or a deliberate improvement
   (listed in `docs/RUST_REWRITE_DIFFS.md` with the reason).
3. The 84 cases in `service_scenarios.test.js` and the other test files are
   ported as Rust tests.
4. Ground-truth accuracy (`run_eval.js`) must not drop.

## Phases

| # | Phase | Done when |
|---|---|---|
| 0 | Golden recorder + contract capture | Golden logs and API/WS recordings committed |
| 1 | Workspace, `kairo-bible`, `kairo-semantic` (incl. fused int8 index) | Semantic search matches Node top-K; paraphrase bench ≥ today |
| 2 | `kairo-detect`: reference parser, lexical layers, scoring | Parser and scoring tests ported and passing |
| 3 | `kairo-detect`: context, ranges, chapter resolver, paraphrase, voting + Pewbeam gains | Golden replay parity |
| 4 | `kairo-stt`: Deepgram (+ ensemble), sherpa-onnx, shared vocabulary | Live and offline runs through one funnel |
| 5 | `kairo-server` + `kairo-integrations` | UI runs unchanged against Rust; contract tests pass |
| 6 | Packaging: drop Node, bundle models, update-size handling | Installers built on macOS/Windows/Linux in CI |
