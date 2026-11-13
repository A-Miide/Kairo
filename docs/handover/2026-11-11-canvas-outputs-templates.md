# Handover — canvas on outputs, timer templates, drag and drop (11 Nov 2026)

Branch `feat/sermon-slides` (`kairo-tour` worktree). 13 commits are ahead of `origin/feat/sermon-slides`: nothing here is pushed, and no pull request exists. The owner merges pull requests. The branch is also several commits ahead of `main` (4.5.0): detector work, sermon slides exports, the song follower, Auto-Deploy per file, the Text view. All of it ships with one pull request, or the detector commits can be split off first (`fcca2bd`, `8870727`, `de138fb`, which are already on GitHub).

## What this session did (last three commits, plus one earlier today)

1. `f93e096` **Song follower gate.** A song advances by itself only when its own Auto-Deploy switch is on, and the Bible tab's master is on. A song with the switch off is not followed and shows no HUD. The title-found path was already gated by `item.auto`. Content lookup is fed no songs. Gate: `songFollowOn` and `startFollower` in `src/service.js`. The follower rewrite itself (`src/lyrics_follow.js`) is kept as it was, for further work.
2. `a508d7f` **Timer templates and bundled fonts.** Server: built-in templates (`BUILT_IN_TEMPLATES` in `library.rs`: Welcome and invite, Starting soon, Reach) refresh on start; saved templates (`/api/timer-styles`); apply to a segment; extend a running timer. `src/fonts/` holds the faces (open licences, `fonts/README.md`). `src/backgrounds/` holds the template pictures.
3. `b293063` **Outputs.** One rule everywhere: a design is laid out in a box of its own canvas shape, 1080 high, then scaled as one piece to the output (`canvasFitRect` in `src/layer_geometry.js`, `canvasRect`/`mountCanvas` in `src/display.html`). Per-output **Fit**: Contain (default) or Cover, in Settings → Outputs beside Theme and Keying (`outputFit` setting, `kairo-output-fit` in localStorage, `fit` in the `/api/look/apply` broadcast). Ten tests in `src/canvas_fit.test.js`.
4. `3bbb750` **App UI.** Template picker with "No template (plain timer)"; plain timers are the time, large and centred on black; Canvas eye switches transparency; canvas W/H and display presets in the shared editor; drag a Media Bin picture into a deck (red line marks the slot), reorder slides by dragging, reorder the Media Bin (order kept in `kairo-media-bin-order`); thumbnails and previews size text as the outputs do; the order-of-service strip (`service_plan.js`, `service_strip.js`).

Also fixed: a verse sent while only a countdown was up never reached the screen (`showVerse` in `display.html`).

## Decisions the owner made (keep)

- Dark palettes (blue, red, green), not cream. No faces, no wheat or Bible imagery. Pre-service words warm and welcoming; never "Come as you are".
- Templates belong to the single Preservice timer; any timer can borrow one. Other timers: no background unless given a design.
- Canvas: a design with no background is transparent and shows a checkerboard; the Canvas layer's eye is the switch; the padlock stays locked. The editor is one shared component for themes, slides and timers.
- Songs go up by themselves only when the song's Auto-Deploy switch is on.
- Do not turn the new song follower on by default.

## Open (needs the owner)

- ~~Highlands and Ember as built-ins~~ **Done 12 Nov** (`8fbed66`): both ship; Ember keeps its script face over HOME (owner's call); Reach stays.
- **Documents folder.** Typing a name for a new media folder creates `Documents/Kairo Media/<name>` (`create_folder`, `media.rs`). The agreed fix, not done: create it inside Kairo's data folder (`media/folders/<name>`), keep linked folders as references only.
- **Pull request.** One for everything, or the detector commits first.

## Not verified

- Real mouse drags (the browser pane can't start one; the same events were fired through the real handlers).
- A real second display, NDI, Syphon or a keyed output; the PowerPoint and ProPresenter exports still ignore canvas size.
- The running dev server is an old build (10 Nov). Rebuild and restart it so Reach shows in the dropdown and the `fit` broadcast reaches outputs. `cargo check -p kairo-server` passed; the Rust test suite was not re-run after the last template edits.
- The song follower with real audio. In a typed-lines test with the switch on, singing slide 2 did not move it; it jumped when slide 3 was sung.
- The automatic contrast check on template text: the Python tooling it used (Pillow) is not installed now. Highlands was judged by eye.

## Gotchas

- Dev server: start it from outside `~/Downloads`; one cargo build at a time (memory: `reference-dev-server-downloads-hang`).
- The browser pane caches scripts: fetch with `cache: 'reload'`, then reload.
- `src-tauri/target` grows to 10–20 GB per checkout; clearing `debug/` is safe (it rebuilds). `Kairo/` and `kairo-tour/` each build separately.
- Test data in the dev instance: segments Preservice, Prayer (canvas reset to 1920×1080), Worship, The Word, Testimony, Ministration; the deck "Sermon: Obedience" has 7 slides.
- `announcement_pack.json` is one long line: a diff shows thousands of changed lines even for a small edit.
