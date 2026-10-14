# Kairo architecture

A map of the codebase for new contributors: how the pieces fit, where to start, and what to know
before changing things. The code comments hold the detail. Many of them record the real incident
behind a rule ("live test, 2026-09-30…"), so read the comment above a guard before you change it.

## What Kairo does

Kairo listens to a live service, transcribes the preacher, works out which scripture is being cited
or quoted, and puts it on the church's screens within a second or two. Around that sits a
presentation system: slides, songs, timers, media, themes, and a Monitor for whoever is on stage.
Detection is the hard part and the reason the project exists; the rest makes it usable in a real
service.

## Design principles

These decide most disagreements about detection behaviour.

1. **Seconds-to-screen is the metric.** Judge a detection change by how quickly the right verse
   appears after the preacher's words, not by search speed in the abstract.
2. **A wrong verse on screen is worse than a missed one.** When Kairo is unsure it offers the verse
   in **Possible Matches** (the `suggestions` target in code) instead of sending it.
3. **The quoted text is ground truth.** A spoken citation is a strong hint, not absolute authority:
   the preacher can misspeak a number while reading the right words (`server/detection_scoring.js`).
4. **Fixes are general speech rules, never tuned to one sermon.** The eval fixtures are examples.
5. **One calibrated behaviour.** No user-facing detection tuning knobs.
6. **It works offline.** Deepgram is the most accurate engine, but the offline engine and every
   detection layer run on the machine with no network.

## The big picture

```
 Microphone ── 16 kHz PCM frames (AudioWorklet → worker → WebSocket) ──┐
                                                                       ▼
┌──────────────────────────── Node server  (server/server.js) ────────────────────────────┐
│                                                                                          │
│  STT engine            transcript segments        detection              scoring         │
│  ├ Deepgram (cloud)    interim + final      ──►   direct citation  ──►   one score per   │
│  └ sherpa-onnx         (words arrive              verbatim quote         candidate;      │
│    (offline, in-proc)   as they are said)         4-gram anchors         viewer, or      │
│                                                   fingerprints           Possible        │
│                                                   semantic / paraphrase  Matches, or     │
│                                  ▲ lookups                               dropped         │
│                         ┌────────┴─────────┐                                             │
│                         │ detection worker │  worker thread: Bible index, IDF,           │
│                         │ (worker_threads) │  anchors, embeddings, reranker              │
│                         └──────────────────┘                                             │
└───────┬────────────────────────────────────────────────────────────┬─────────────────────┘
        │ WebSocket broadcasts + HTTP /api                           │ obs-websocket
        ▼                                                            ▼
  Operator window (index.html)      Display windows (display.html)      OBS text source
        │                           one per output, plus Monitor
        │ Tauri `invoke`
        ▼
  NDI / Syphon / Spout senders (Rust)

  Tauri shell (src-tauri, Rust): creates the windows, picks the port, makes the auth token,
  starts the Node server as a sidecar, owns the menu and the auto-updater.
```

Three things to hold on to:

- **The server is the brain; every window is a view.** The operator window, each display window and
  the Monitor all repaint from the same WebSocket broadcasts. Nothing on a projector is rendered
  from local state.
- **The UI is plain HTML and JavaScript** served by the Node server (`express.static` over `src/`).
  There is no bundler, framework or build step for the frontend.
- **Detection runs in two threads.** The main thread owns session state and decisions; a
  `worker_thread` owns the Bible indexes and the heavy searches, so a slow search never stalls the
  audio or the UI.

## How it starts

`src-tauri/src/lib.rs` is the entry point of a release build:

1. Picks a port (7777 if free, else a free loopback port) and generates a 32-byte random auth token.
2. Starts the bundled Node binary (`src-tauri/bin/node-<target>`) running `server/server.js`, passing
   configuration in environment variables: `KAIRO_SERVER_PORT`, `KAIRO_AUTH_TOKEN`,
   `KAIRO_APP_DATA_DIR` (writable settings, models, logs), `KAIRO_DB_DIR` (the Bible data) and
   `KAIRO_RESOURCE_DIR`.
3. Polls the server's `/health`, then points the main window at `http://127.0.0.1:<port>`.

The server binds to loopback only. Every HTTP request and WebSocket upgrade must carry the token
(`Authorization: Bearer …` or `?token=`), except `/health` and static assets. The frontend fetches
the token from Tauri on boot (`src/client_common.js`) and a patched `fetch` adds it to every call.

A **dev run** is different: `npm run dev` starts `npm run server` first (port 7777, no token, so auth
is off), and the Rust side does not spawn a second server. You can also run `npm run server` alone
and open `http://localhost:7777` in a browser, which is enough for most UI work. Capturing the
microphone and opening real display windows needs the Tauri app.

Where things live at runtime: in a release build, `settings.json`, `server.log`, `segments/`,
`triggers/`, `songs/`, `media/` and the downloaded `models/` sit in the app-data directory
(`com.kairo.scripture`); in dev the repo's `databases/` folder plays that role.

## Repository map

| Path | What is in it |
|---|---|
| `server/` | The Node server and detection engine (about 23k lines including tests). |
| `src/` | The frontend: operator window, display renderer, Theme Studio, service planner. |
| `src-tauri/` | The Rust shell: windows, menu, updater, NDI / Syphon / Spout senders, packaging config. |
| `databases/bibles/` | `map.json` (31,008 verses with their metadata), `kjv.json`, `packs/` (public-domain and open scripture-language packs), `translations/`. The embedding model, reranker and verse vectors are not in git; installers fetch them (see Data and models). |
| `scripts/` | Asset and data generators (backgrounds, hymn imports), release helpers, packaging. |
| `eval/` | The accuracy harness and its fixtures (see Testing). |
| `docs/` | This file, and a landing page. |

### The server (`server/`)

| File | Role |
|---|---|
| `server.js` | HTTP and WebSocket API, session state, the transcript pipeline, detection orchestration, outputs. About 7.9k lines. Its `// ── Section ──` banner comments work as a table of contents: `grep -n '^// ── ' server/server.js`. |
| `detection_worker.js` | The worker thread. Builds the indexes at startup, then answers requests such as `directLookup`, `verbatimSearch`, `fingerprintSearch`, `streamText`, `semanticSearch`, `rerank`. |
| `reference_parser.js` | Turns speech into citations: number words, book-name healing for speech-to-text errors ("Fake John" is 1 John), ranges, corrections ("3:17, sorry, 16"), and the book/chapter context that resolves a bare "verse 14". |
| `detection_scoring.js` | The scoring model (below). Pure functions, no I/O. |
| `detection_constants.js` | Thresholds shared by the server, the worker and the scoring model. |
| `translation_commands.js` | The grammar for spoken "switch to the NIV" commands. Pure; `server.js` applies a match. |
| `chapter_resolver.js`, `citation_voting.js`, `citation_i18n.js` | A verse inside a chapter that was named without one; votes across extra Deepgram streams; citations in Spanish, Portuguese, French and German. |
| `semantic_engine.js`, `reranker_engine.js`, `paraphrase.js`, `named_passages.js` | Meaning-based matching, the cross-encoder that re-ranks it, which stretches of speech to search, and "the Lord's Prayer"-style references. |
| `sherpa_engine.js`, `sherpa_installer.js` | The offline speech engine and its model downloads. English runs on one model; every other spoken language runs on a multilingual one, chosen by the Spoken language setting (`speechModelFor`). |
| `stt_ensemble.js` | Extra time-offset Deepgram streams that rescue citations the main stream mis-hears. |
| `segments.js`, `triggers.js` | Service segments (Preservice, Worship, …) and the timer and clock actions they drive. |
| `sermon_notes.js` | Extractive sermon notes from a session, always the preacher's own words. |
| `media.js`, `songs.js`, `slide_import.js`, `theme_import.js`, `translate.js`, `mt_engine.js` | The media library, song library, slide and theme importers, and translation of non-scripture slide text. |

### The frontend (`src/`)

| File | Role |
|---|---|
| `index.html`, `app.js`, `app_startup.js` | The operator window: Bible, Slides, Timer, Songs, Media, Theme Studio. `app.js` holds the WebSocket client and the message handlers. |
| `display.html` | The output renderer. One page, loaded as `display.html?output=<id>` by every display window and by the Monitor's live copies. It draws the layers (media, slide, Bible, timer) from the server's broadcasts. |
| `app_outputs.js` | Settings → Outputs: displays, Monitor outputs, NDI, Syphon, OBS, per-output theme, keying and layers. |
| `service.js` | The service planner: playlists, slides, songs, timer segments, the Media Bin. |
| `app_theme_studio.js` | Theme Studio, a layer-based design tool for themes and per-slide edits. |
| `layer_geometry.js`, `motion_graphics.js`, `gradients.js`, `color_utils.js` | Shared by the editor and the renderer so a preview matches the output. |
| `lyrics_follow.js`, `sermon_follow.js`, `hymns.js` | Speech-driven slide advance for songs and sermon slides, and the bundled hymn bank. |
| `monitor.html`, `identify.html` | The Monitor popped out into its own window, and the "which screen is this" overlay. |

## The live pipeline

1. **Audio.** The operator window captures the microphone (`audio_capture_worklet.js`,
   `audio_sender_worker.js`) and sends 16 kHz PCM frames over a binary WebSocket. `ingestAudio()` in
   `server.js` hands them to whichever engine is running.
2. **Speech to text.** Deepgram (Nova-3, with the church's vocabulary as key terms) streams back
   interim and final text. The offline engine (`sherpa_engine.js`) runs an NVIDIA streaming
   transducer through sherpa-onnx in the same process and emits text the same way. Both feed one
   function, `handleTranscriptSegment()`, so everything downstream is identical.
3. **Detection.** Each segment goes through several layers, some on every interim update (they act
   while the sentence is still being spoken) and some on finals:

   | Layer | Catches | Where |
   |---|---|---|
   | Direct citation | "John 3:16", "chapter 3, verse 16", corrections, bare "verse 14" in context | `reference_parser.js` → `processForReferences()` |
   | Verbatim | A verse read word for word, or a long part of one | worker `verbatimSearch` → `processVerbatim()` |
   | 4-gram anchors | A quote as it is spoken, word by word | worker `streamText` → `streamNewWords()` |
   | Fingerprints | A verse recognised from its most distinctive words | worker `fingerprintSearch` |
   | Semantic and paraphrase | The preacher's own words for a verse | worker `semanticSearch` and `rerank` → `maybeRunParaphrase()` |
   | Spoken navigation | "next verse", the last words of a verse, a bare number after a verse | `maybeHandleNextVerseTrigger()` and the range code |
   | Spoken translation change | "switch to the NIV", "let's read this in the King James" (a command, never a bare mention) | `translation_commands.js` → `maybeHandleTranslationCommand()` |

4. **Decision.** Every layer ends in `broadcastDetection()`. It scores the candidate as
   `B + D + A`: a per-method base certainty, a distance from the current reading position, and
   corroboration from other methods in a rolling window (`EvidenceLedger`). The score decides
   between the screen, Possible Matches and dropping it. A pile of guards sits around this for
   cases the numbers cannot see: dedup, "this verse was already replaced", a citation corrected by
   what is read next, twin verses (Matthew 6:33 and Luke 12:31 share wording), a misheard book.
5. **Output.** A verse that wins is translated for display, put on the screen and sent to OBS. The
   operator window shows it in the Live Queue.

State that spans segments is deliberate and lives in `server.js`: the book and chapter in play
(`referenceContext`), recent citations, the active range (`rangeQueue`) that auto-advances as the
preacher reads, a chapter named with no verse yet, and the screen history behind Back.

## Protocol

**HTTP:** about 70 routes under `/api`. They cover settings, search, ranges, service and output
control (`/api/service/*`), segments and triggers, media, songs, themes (`/api/look`), model
installers (`/api/offline`, `/api/semantic-model`, `/api/reranker-model`, `/api/translate-model`),
OBS, and exports.

**WebSocket, server to clients:** JSON messages with a `type`. The important ones:

| `type` | Meaning |
|---|---|
| `transcript` | Interim and final text, for the transcript log. |
| `detection` | A verse (or slide) for a `target`: `viewer` is the screen, `suggestions` is Possible Matches. |
| `range-verses`, `range-state`, `range-active` | A cited range and where the reading is in it. |
| `clear-layer`, `layer-state`, `timer-slide`, `media`, `monitor-message` | What each output layer shows. |
| `connection-state`, `listening-error`, `worker-ready` | Engine and worker status. |

**WebSocket, client to server:** binary audio frames, plus one text message (`media-status`, a display
reporting video playback).

**Main thread to worker:** `workerCall(type, payload, timeoutMs)` posts a typed request and awaits the
reply. The list of types is the `switch` in `detection_worker.js`.

A new connection gets a snapshot of the current state (`lastSlideBroadcast`, `lastTimerBroadcast`,
`lastRangeState`, …), so a display that reconnects mid-service shows the right thing straight away.

## Outputs

- **Display windows** are Tauri windows on a chosen screen, each loading `display.html?output=<id>`.
- **Monitor** is an output of kind `monitor`: a words-only confidence view for the stage (clock,
  segment countdown, the current and next slide, a single scripture on its own, and a message the
  operator types).
- **NDI, Syphon (macOS) and Spout (Windows)** senders live in Rust (`ndi.rs`, `syphon.rs`,
  `spout.rs`). The frontend pushes verse, media and timer updates through Tauri commands.
- **OBS** gets verse text over obs-websocket (`sendToOBS()` in `server.js`).
- **Layers.** Every output composes four layers: media, slide, Bible and timer, with the Bible above
  slides. Clear Bible brings back the slide underneath. Each output chooses which layers it shows,
  its theme, and keying.

## State and persistence

Worth knowing before you touch storage, because it is split:

| Where | What |
|---|---|
| Server files (app-data directory) | `settings.json`, service segments, triggers, songs, media library, downloaded models, `server.log`. |
| The webview's `localStorage` | **Themes** (`kairo-looks-v3`), **playlists** (`kairo-playlists-v1`), per-output theme, layer and mode choices, and editor preferences. |

Themes and playlists living in `localStorage` means they are per machine and can be lost if the
webview's data is cleared. Moving them to server-side files is a sensible project.

## Data and models

- `databases/bibles/map.json` is the detection corpus: the KJV plus the NLT's wording, with
  per-verse metadata. The worker indexes it at startup (about five seconds).
- **What a quote is matched against.** Every verse is matched in the KJV and in a list of other
  wordings (`altWordings` in `detection_worker.js`): the church's own translation when it has one,
  then the NLT and the NIV for every English service, because those are the modern versions
  preachers quote most whatever the church shows. A non-English service matches its language's
  Bible instead (`databases/bibles/packs/`). The worker builds the list at spawn time
  (`MATCH_ALSO` in `detection_worker.js`), and changing the church's translation restarts the worker.
  The NLT and the NIV are matched by phrase only: the word-by-word index holds just the KJV and the
  church's own wording, because modern four-word sequences fire on ordinary preaching. The
  near-duplicate table and each verse's identifying words also come from the KJV and the first other
  wording only.
- **Downloaded on demand, not in git:** the semantic model (`embeddinggemma-300m`), the
  cross-encoder reranker, the verse vectors built from them, the offline speech model, and the local
  translation models. Each has an installer module with the same shape (an NDJSON progress stream,
  disk-space check, resumable download) and a matching panel in Settings. A release build bundles
  the detection models; a fresh clone fetches them on first use.

## Testing and the eval harness

There is no single test command yet. The 34 test files run individually:

```bash
for f in server/*.test.js; do KAIRO_EVAL_MODE=1 node "$f" || echo "FAILED: $f"; done
for f in src/*.test.js;    do node "$f" || echo "FAILED: $f"; done
```

- Fourteen server tests use `node:test`; the rest are plain scripts. Any test that `require`s
  `server.js` must set `KAIRO_EVAL_MODE=1` (no auto-start, no log capture) and finish with
  `await server.stopDetectionWorker()` before exiting, or the process can abort while the worker is
  still loading a model.
- **`server/service_scenarios.test.js`** pushes scripted real-service situations through the whole
  pipeline and asserts what reaches the screen. Most come from live runs. This is the best place to
  add a regression test for a detection bug: write the preacher's words as they arrived and assert
  the verse that should (or should not) go up.
- **`eval/run_eval.js`** replays real sermon transcripts through the live code path and scores
  recall (answer-key verses found), precision (screen sends that were right) and how many missed
  verses were still offered in Possible Matches. Recent runs: about 86% of the answer-key verses
  found, about 99% of screen sends correct. The answer keys have gaps, so check a "wrong send"
  against the transcript before believing it. Results go to `eval-results/` (git-ignored).
- **`eval/ab_audio.js`** plays a real audio file through an engine in real time, for comparing
  the offline engine with Deepgram on identical audio.

## Conventions and sharp edges

- **Comments explain why.** Most non-obvious code carries the incident that produced it. Keep
  doing that, and read it before changing a guard.
- **`toast()` is a deliberate no-op.** Surface errors inline, next to the control that failed.
- **Big files.** `server.js` (about 7.9k lines), `app_theme_studio.js` (about 7.5k) and `service.js`
  (about 6.7k) are single closures with shared module state. Section banners are the map. Splitting
  them is welcome, but do it as its own change with the tests green before and after.
- **Versioning.** A release bumps the version in four places: `package.json`,
  `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`.
- **Wall-clock windows.** Detection rules use time windows (how long a chapter stays "in play",
  dedup windows). The eval harness compresses an hour of sermon into minutes, so a slow machine can
  move a verse across a window edge. Run it twice before blaming a change for a one-verse swing.

## Building and releasing

```bash
npm install && (cd server && npm install)   # dependencies
npm run dev                                 # Tauri dev window + the server on :7777
npm run build                               # release bundle (src-tauri/target/release/bundle)
```

Needs Node 22+, a Rust toolchain and Tauri's platform prerequisites. A few things to know:

- `src-tauri/tauri.conf.json` names the original maintainer's macOS signing identity. On another
  machine a release build needs its own identity there (or ad-hoc signing); dev runs do not sign.
- The auto-updater (`tauri-plugin-updater`) reads
  `https://github.com/miide-Adeyemi/Kairo/releases/latest/download/latest.json`. Its private signing
  key is not in the repository.
- `.github/workflows/build.yml` builds macOS, Windows and Linux when a `v*` tag is pushed. It was
  written for an earlier engine stack, so check it before relying on it.

## Where to start

| If you want to… | Open |
|---|---|
| Fix a mis-detected citation | `server/reference_parser.js`, then add a case to `reference_parser.test.js` |
| Fix a wrong or missing verse in a real situation | `server/service_scenarios.test.js` first (reproduce it), then the layer that decided |
| Improve the offline engine | `server/sherpa_engine.js` and `eval/ab_audio.js` |
| Add an output type | `src/app_outputs.js`, `src-tauri/src/` and the output handling in `display.html` |
| Change how a theme or slide is drawn | `src/layer_geometry.js` and `display.html` (the renderer), `src/app_theme_studio.js` (the editor) |
| Support another spoken language | `server/citation_i18n.js` and `databases/bibles/packs/` |
