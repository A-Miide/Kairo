# Handover — tour, settings and sermon slides (6 Nov 2026)

Three local branches, none pushed or merged. Each needs a pull request the owner merges.

| Branch | Where | Builds on | Commits |
|---|---|---|---|
| `feat/tour-easyworship` | `kairo-tour` worktree | `main` | 23 |
| `feat/sermon-slides` | `kairo-tour` worktree | `feat/tour-easyworship` | 24 more |
| `feat/references-in-spanish` | `Kairo` folder | `main` | 3 |

Merge order: `feat/tour-easyworship`, then `feat/sermon-slides` (it carries the first branch's commits). The language branch is independent.

## feat/tour-easyworship — first-run tour and settings
- Tour: language panel on step 1 (required), microphone before the engine choice, every input on a step grouped in one titled panel, Next/Back fixed in place, one Skip.
- Settings: Language page as a four-step chain ("Bible version", "Slide translation" with a hover helper, tracks the Audio choice); Downloads page above Help; X close buttons; brand orange-red, white and green only (no yellow anywhere).
- Live views: "Alerts to monitors" (monitors only; picker only with 2+ monitors); outputs grid keeps the preview's size and scrolls; Live Queue chapter list capped at 7 rows; preview pitch black when empty, LIVE only when something is up; big green free-offer badges (Deepgram, API.Bible).

## feat/sermon-slides — sermon outline to designed, editable deck
- Import: a Word/text outline (numbered points with scriptures) offers **Make sermon slides**. One **Style** button and pop-out (look, then layout, then background) and a four-colour strip, shared with the editor (`styleButton` in `src/sermon_slides_ui.js`).
- 11 layouts, 18 looks, 3 backgrounds (`src/sermon_slides.js`): Left, Centred, Lower third, Split, Editorial, Statement, Card, Quote, Badge card, Worthy, Light. Looks include the owner's palette-card ladders and four sampled from reference slides (Red Card, Royal Blue, Night Worship, Lime Black). Fine grain (`backgrounds/grain*.svg`) and vignette sit on the background only.
- Every word is an ordinary text layer, edited in place with styling kept. "In the deck" lists everything in the document, to choose what becomes a slide.
- Shared parts: any layer other slides also have offers **Just this slide / This and later slides / All slides**; "all" also becomes the deck's style. A look/layout change asks before restyling slides edited by hand (their words are kept).
- Thumbnails tag **Cut off** when text runs past its box (decorative ghost words and quote marks are exempt).
- Editor-wide: one colour picker for every swatch (`src/color_picker.js`: shade square, hue, hex/RGB, eyedropper, recents); shape Style controls (Each corner, Stroke, Shadow, Liquid glass) drawn by the shared helper in `src/layer_geometry.js`; layer **lock** (padlock, right-click, ⇧⌘L); theme text binds to scripture by layer name (Verse, Reference, Translation, Countdown); the Bible bar's Theme button used for every theme picker; the Media Bin resizes on its own.
- Help (Settings → Help) covers all of the above.
- Tests: `node --test src/sermon_slides.test.js src/layer_geometry.test.js` (24 pass).

## feat/references-in-spanish — detection in es/pt/fr/de
- French Bible rebuilt from eBible.org's public-domain Louis Segond 1910 (`scripts/fr-pack-from-ebible.py`).
- Replay: wrong sends es 2.5%, pt 4.1%, fr 2.9%, de 7.6%; English unchanged. Four German sermons added with answer keys. Full write-up: `Kairo/docs/handover/` (the language agent's note).

## Open items
- Owner to confirm four possible real quotes for the answer keys: Colossians 3:3, Romans 10:3, Ephesians 1:14, Psalm 38:4.
- API.Bible key in the installed app (needed for read-aloud verses in es/pt/de); German numbers caught mid-word ("3 zu 3", "32.6").
- Not checked in the installed app: the eyedropper (shows only where the system supports it).
- Generated abstract backgrounds were rejected and removed; images bundled in the Media Bin are on hold.

## Test copy
The owner's test copy runs the `Kairo` debug server on localhost:7793 with `KAIRO_UI_DIR` pointed at `kairo-tour/src` (UI changes need only a reload). Start it from outside `~/Downloads` (see memory note on the Downloads hang). Playlists live in each browser's own storage.
