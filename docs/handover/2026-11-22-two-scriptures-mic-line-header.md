# Handover, 22 Nov 2026: two scriptures called at once, the "No sound" line, the header cut off

Three things from the owner's service on 22 Nov 2026 (Kairo 4.7.3 on the Mac). Nothing here is fixed yet.

---

## 1. Two scriptures called at once: send one, hold the other

### What happened

The preacher called two verses in one breath, then preached for a while before reading either:

> "Jonah chapter two verse eight, and also Second Corinthians two verse fourteen. I want to talk briefly on what are titled the glory of the church…"
> later: "Job chapter two, verse eight, they that observe lying vanities forsake their own mercy…"

He meant Jonah the second time ("Job" was a slip, ignore it here). He read Jonah 2:8 first.

### What Kairo does today

Each citation in a sentence goes straight to the screen, in the order said (`engine/citations.rs`, `references()`, the loop over `refs` → `decide(… Target::Screen)`). So the second one replaces the first a moment after it went up, before anyone read either.

Replayed on the engine (the `screens()` helper in `crates/kairo-detect/tests/audit_wrong_sends.rs`, 22 Nov):

| Speech | Sent to screen, in order |
|---|---|
| Both called in one sentence, then preaching, then Jonah 2:8 read | Jonah 2:8 → 2 Corinthians 2:14 → Jonah 2:8 |
| The same with a pause between the two citations | Jonah 2:8 → 2 Corinthians 2:14 → Jonah 2:8 |
| Both called, Jonah 2:8 read, then 2 Corinthians 2:14 read ("thanks be unto God, which always causeth us to triumph in Christ") | Jonah 2:8 → 2 Corinthians 2:14 → Jonah 2:8, and **2 Corinthians 2:14 never comes back** when it is read |

So the screen shows the wrong verse while he preaches, and the second verse is missing when he finally reads it.

### What we want

When two (or more) verses are called together, **send the first and hold the rest**:

1. The first verse called goes to the screen as now.
2. Each other verse goes to the Live Queue as a **held** card (the queued look range cards already use, `range-queued` in `app.js`), not to the screen.
3. A held verse goes up when:
   - its words are read (the verbatim / stream match already finds them; today that match is what brought Jonah 2:8 back), or
   - the operator clicks it or presses Next, or
   - the preacher calls it again on its own.
4. The verse on screen stays until one of those happens. Preaching in between changes nothing.
5. A held verse that is never read just stays in the Live Queue. It is never sent on a timer.

"Together" means: in the same sentence, or the next citation within a few seconds with no verse words read in between ("…two verse eight, and also…"). A citation after the first verse has been read is a normal new citation and goes up as now.

### Where to build it

- **Detector** (`crates/kairo-detect/src/engine/citations.rs`, `references()`): when `refs` has more than one verse citation (and the interim path's "a genuine second citation goes up" rule, `docs/detection-spec/02-citations.md`, section Sending), send `refs[0]` and keep the others as held. There is no "held for later" target today: `Target` is `Screen | Suggestions | Drop` (`engine/types.rs`). Either add a `Hold` reason such as `Hold::CalledWithAnother` and send them to Suggestions, or keep a small "called, not yet read" list in engine state, like the soft range (`set_soft_range`) keeps a passage's next verses.
- **Promotion when read:** the same check that holds a range's next verse until its words are heard (`Hold::RangeNotReadOnYet`, `engine/ranges.rs`) is the model. A held verse whose words are read is sent; it should count as confirmed, not need a fresh citation.
- **The last row above:** find out why 2 Corinthians 2:14 read aloud after Jonah 2:8 is not sent at all. Likely the "already sent" or "quote behind the reading" rules (`Hold::QuoteBehindReading`) treat it as old because it was cited earlier. Fix this whichever way the hold is built.
- **App** (`src/app.js`, `showInViewerNow` and the Live Queue): show the held verse as a queued card under the one on screen, in the order called. One click sends it (Live Queue single-click rule, owner 19 Nov).
- **Spec:** add the rule to `docs/detection-spec/02-citations.md`.

### How to test

- Add the three cases above to a test in `crates/kairo-detect/tests/` (copy `screens()` from `audit_wrong_sends.rs`). Expected: Jonah 2:8 only, until "they that observe lying vanities…", then Jonah 2:8; 2 Corinthians 2:14 only when "thanks be unto God… triumph in Christ" is read.
- Must not break: a range ("John 3:16 to 18"), two citations where the second is the correction of the first ("John 3:16, sorry, 3:17"), and a citation said while another verse is being read.
- Then replay a real church log with the `replay-log` tool (`kairo-eval`) and check nothing else changed.

### Open questions for the owner

- Hold the second always, or only when the preacher doesn't start reading straight away? (Proposed: always; reading it sends it at once anyway.)
- Three or more called together: same rule, all but the first held, in order?

---

## 2. "No sound from the input": remove the line

The line under Live Status (PR #27, shipped in 4.7.1) showed during the service together with the "NO AUDIO!" pill. The pill already says the same thing.

**Owner's decision (22 Nov):** "I don't think we need the no sound input." Remove the silent message; **keep the clipping warning** ("Input too loud: it is clipping").

To do:
- `src/input_health.js`: drop the `silent` state and its message (keep the rules and tests for clipping).
- `src/app.js` `showInputHealth`: only `clipping` shows the line.
- `src/input_health.test.js`: remove the silence tests, keep the clipping ones.
- Check it in the browser pane as in PR #27: fake clipping still shows and clears; fake silence shows only the "NO AUDIO!" pill.

Not checked: whether the input was really silent at that moment. The words count (55 in 2:26) suggests speech was coming through around then. If the pill also fired with speech arriving, look at `client:audio-pipeline` and `server:audio` (peakDb, zeroFrames) in that day's `debug.log` for the minute of the screenshot.

---

## 3. The header controls are cut off

### What the screenshot shows

The main window's top bar (Bible, Slides, Timer… tabs, Start Listening, Settings) is gone. Only a red sliver of the Start Listening button shows under the Mac menu bar, and the window buttons (red, yellow, green) sit on top of Live Status. The page has scrolled up by the height of the top bar; the window itself is in the right place.

### Likely cause

This is the old "top bar disappears" bug coming back through another path. In WebKit, `element.scrollIntoView()` can scroll the whole page (body and html), even with `overflow: hidden`, which pushes `.top-bar` off the top. See the comment in `src/service.js` (`focusInStack`, "Confirmed root cause of the top-bar-disappearing bug") and `src/app.js` (Settings search, "scrollIntoView() can shift the whole page"). Those two places were fixed by scrolling their own container instead.

Calls still left on `origin/main` that can do it:
- `src/app.js` `showListeningProblem`: `note.scrollIntoView({ block: 'nearest' })`. It runs during listening when there is a listening problem, the most likely one here given the warnings on screen.
- `src/app.js` scripture search suggestions: `scriptureSuggest?.children[i]?.scrollIntoView(…)` (arrow keys in the search box).
- `src/app.js` `offline-summary-group` and the settings field `field?.scrollIntoView(…)`.
- `src/service.js` `first.scrollIntoView({ block: 'center', … })`.

A second way in: below 1180 × 700 points the page is allowed to scroll on purpose (`styles.css`, `@media (max-width: 1179px), (max-height: 699px) { html.kairo-app { overflow: auto; } }`). A window that small can be scrolled up by the wheel or trackpad, and nothing brings it back.

### Fix direction

- Replace each remaining `scrollIntoView` with scrolling its own container (as `focusInStack` does).
- Add one central guard so no future call can do it: when the window is at least 1180 × 700, keep the page at the top (`document.scrollingElement.scrollTop = 0` on `scroll`). Prefer this guard over hand-fixing each call.
- Get the operator back without restarting: the same guard fixes it the moment anything scrolls.

### How to check

In the browser pane at 1440 × 900: start listening with a forced listening problem, and press the arrow keys in the scripture search with suggestions open. The top bar must stay put (`document.scrollingElement.scrollTop` stays 0). Then on the Mac app itself.
