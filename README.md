# KAIRO

> Live scripture detection for church services, with the presentation system around it.

KAIRO listens to the preacher, works out which scripture is being cited or quoted, and puts it on the church's screens within a second or two. When it is unsure it offers the verse in **Possible Matches** instead of sending it: a wrong verse on screen is worse than a missed one.

Around detection sits a full presentation system: slides, songs, timers, media, themes, a stage Monitor, multiple outputs (screens, NDI, Syphon, DeckLink), a Director mode for cameras, and streaming and recording.

Downloads: [the landing page](https://a-miide.github.io/Kairo/) or [Releases](https://github.com/A-Miide/Kairo/releases).

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

Detection evaluations (sermon replay, scenarios, the meaning benchmark) live in `eval/`.

---

## Releasing

1. **Smoke-test the packaged app** on a Mac and on Windows (the church PC runs Windows). Run **Build KAIRO** by hand on `main` (Actions → Build KAIRO → Run workflow), install the app from the run's artifacts, quit any running Kairo, then:

   ```bash
   node scripts/smoke-test.mjs --app "/Applications/Kairo.app/Contents/MacOS/Kairo"
   ```

   ```bash
   node scripts/smoke-test.mjs --app "C:\Program Files\Kairo\Kairo.exe"
   ```

   It launches the app and checks that it starts, that each display output opens on its own screen, that a verse doesn't show where Output Control has Scripture off (and does once it is back on), and that a replayed transcript (`scripts/smoke-test.json`) auto-sends each verse into the Live Queue and onto the screen. Every line says PASS or FAIL; exit code 0 means all passed. Give `display-1` a screen first (or pass `--output <id>`), and leave Auto-send on. It uses the computer's own settings, so the screens show the test verses: not during a service. Output Control is put back afterwards.

   How it gets in: the app's server still needs its token on every request. The script starts the app with `KAIRO_SERVER_PORT` and `KAIRO_SERVER_TOKEN` (43 random characters it makes for this run, never printed or saved; the app ignores a token under 32 characters) and reads back what the windows report they show (`GET /api/state-reports`, token required). Only something that can already start the app can set them. `--url http://127.0.0.1:<port>` runs the same checks against a dev server.
2. Bump the version in `package.json`, `package-lock.json` (2 places), `src-tauri/Cargo.toml` (2 places), `src-tauri/Cargo.lock` (the `kairo*` crates) and `src-tauri/tauri.conf.json`.
3. Tag from `main` and push the tag (`git tag v4.x.y && git push origin v4.x.y`). The **Build KAIRO** workflow tests, builds every platform and makes a **draft** release with the signed updater packages and `latest.json`.
4. Check the draft, then publish it **as the latest release** (`gh release edit v4.x.y --draft=false --latest`). The in-app updater and the landing page both read the release GitHub marks *Latest*; if another release (a model or data upload) holds that mark, nobody gets the update.

Model and data releases (`offline-model-*`, `bundle-data-*`, `bible-packs`) should be made as pre-releases so they never take the *Latest* mark.

---

## Docs

- `docs/RUST_ENGINE.md`: the engine
- `docs/THIRD_PARTY.md`: bundled software and licences

## License

KAIRO's source code is licensed under the GNU General Public License, version 3 (see [LICENSE](LICENSE)). Copyright (C) 2026 Miide.

Third-party software and data keep their own licences: see [docs/THIRD_PARTY.md](docs/THIRD_PARTY.md) and, for Bible text, the `ATTRIBUTION.md` files in `databases/bibles/packs/` and `databases/bibles/translations/`. The GPL covers KAIRO's code, not those texts.
