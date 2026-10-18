# KAIRO

> Live scripture detection for church services, with the presentation system around it.

KAIRO listens to the preacher, works out which scripture is being cited or quoted, and puts it on the church's screens within a second or two. When it is unsure it offers the verse in **Possible Matches** instead of sending it: a wrong verse on screen is worse than a missed one.

Around detection sits a full presentation system: slides, songs, timers, media, themes, a stage Monitor, multiple outputs (screens, NDI, Syphon, DeckLink), a Director mode for cameras, and streaming and recording.

Downloads: [the landing page](https://miide-adeyemi.github.io/Kairo/) or [Releases](https://github.com/miide-Adeyemi/Kairo/releases).

---

## Stack

| Layer | Tech |
|-------|------|
| Desktop shell | [Tauri v2](https://tauri.app) (Rust), macOS 12+, Windows, Linux (glibc 2.38+) |
| Frontend | Vanilla JS + HTML/CSS (`src/`) |
| Engine | Rust, inside the app (`src-tauri/crates/kairo-server`, `kairo-detect`, `kairo-bible`) |
| Speech-to-text | Deepgram (cloud) or sherpa-onnx (offline, downloaded on first use) |
| Meaning search | bge-small embedder + MiniLM reranker, ONNX (`kairo-semantic`) |
| Video | A bundled LGPL FFmpeg (`scripts/build-ffmpeg-*.sh`, see `docs/THIRD_PARTY.md`) |

The app has no Node runtime; Node is only used for the Tauri CLI and the JS tests.

---

## Development

### Prerequisites

- [Rust](https://rustup.rs) (stable) and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS
- [Node.js](https://nodejs.org) 22
- About 20 GB free disk: `src-tauri/target` grows to ~18 GB

### Run

```bash
npm install
KAIRO_SERVER_PORT=7788 npm run dev -- --config '{"build":{"devUrl":"http://localhost:7788"}}'
```

The first build compiles the whole engine (15-30 minutes); later rebuilds are incremental. Rust edits restart the dev build.

### Tests

```bash
node --test src/*.test.js
cargo test --release --manifest-path src-tauri/Cargo.toml -p kairo -p kairo-server -p kairo-detect --lib
```

Detection evaluations (sermon replay, scenarios, the meaning benchmark) are described in the latest `docs/HANDOVER_*.md`.

---

## Releasing

1. Bump the version in `package.json`, `package-lock.json` (2 places), `src-tauri/Cargo.toml` (2 places), `src-tauri/Cargo.lock` (the `kairo*` crates) and `src-tauri/tauri.conf.json`.
2. Tag from `main` and push the tag (`git tag v4.x.y && git push origin v4.x.y`). The **Build KAIRO** workflow tests, builds every platform and makes a **draft** release with the signed updater packages and `latest.json`.
3. Check the draft, then publish it **as the latest release** (`gh release edit v4.x.y --draft=false --latest`). The in-app updater and the landing page both read the release GitHub marks *Latest*; if another release (a model or data upload) holds that mark, nobody gets the update.

Model and data releases (`offline-model-*`, `bundle-data-*`, `bible-packs`) should be made as pre-releases so they never take the *Latest* mark.

---

## Docs

- `docs/RUST_ENGINE.md`: the engine
- `docs/HANDOVER_*.md`: current state, decisions and how to run the evaluations
- `docs/THIRD_PARTY.md`: bundled software and licences

## License

MIT
