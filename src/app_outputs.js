// KAIRO — Outputs: per-output themes, physical screens, language packs, the Outputs
// master-detail pane and per-output layer visibility. Split out of app.js; loaded after
// app_theme_studio.js and sharing the same global scope.

// ── Per-output themes ─────────────────────────────────────────────────────
// Each destination renders with its own theme — the ProPresenter model where
// stage and audience screens show different designs from the same source. The
// assignment lives with the OUTPUT (in Settings), not with the theme.
// One card per output. The primary external display keeps its own card (its
// theme picker is injected inline like every other output); any *additional*
// screens a church drives — stage monitor, foyer — are appended inside that
// card as extra rows. PRIMARY_DISPLAY itself is declared near the top of
// the file now (see the comment there) — it has to exist before
// wireExternalDisplayStatus's IIFE runs at load.
// NDI/Syphon are deliberately NOT here — Output Looks generalized them from
// one fixed sender each into a LIST of independent named outputs (owner:
// "you should be able to create multiple Syphon or NDI outputs", matching
// ProPresenter's own Screen Configuration list). See ndiOutputs()/
// syphonOutputs()/renderNdiOutputs()/renderSyphonOutputs() below — same
// list-of-named-instances shape as extraDisplays(), just with their own
// settings arrays since an NDI/Syphon output also carries a network source
// name extraDisplays() has no equivalent of.
// ProPresenter's own integration is gone entirely (owner: "I don't think
// anyone who owns ProPresenter will rather let us use messages to send to
// their ProPresenter than just send themselves") — it picks Kairo up as a
// normal NDI/Syphon input instead. OBS keeps its single fixed WebSocket
// connection (not addable/multi-instance the way Displays/NDI/Syphon are).
const OUTPUT_DEFS = [
  { key: PRIMARY_DISPLAY, label: 'External Display' },
  { key: 'obs',           label: 'OBS' },
];

// Extra screens beyond the primary one. Stored in settings.
function extraDisplays() {
  const list = settings.extraDisplays;
  return Array.isArray(list) ? list : [];
}

// Every install has a Monitor among them — the basic one, off until it's
// given a screen, always there the way External Display is (owner,
// 2026-10-01: "setting has no monitor" — it was only under + Add Output, and
// the Monitor message bar stays hidden without one — and "They can add more,
// but there should be the basic output"). More come from + Add Output; the
// last one has no Remove, and loadSettings calls this once the saved
// settings are in, to put one back if none is left.
function seedMonitorOutput() {
  const list = extraDisplays();
  if (list.some(d => d.kind === 'monitor')) return;
  settings.extraDisplays = [...list, { id: genOutputId('monitor'), name: 'Monitor', kind: 'monitor' }];
  saveSettingsPatch({ extraDisplays: settings.extraDisplays });
}

// Every screen, primary first — used for theme assignment and broadcasting.
// The primary's own name is renameable too (settings.primaryDisplayName —
// see renderDisplayDetail), same as every other output.
function displayOutputs() {
  return [{ id: PRIMARY_DISPLAY, name: settings.primaryDisplayName || 'External Display' }, ...extraDisplays()];
}

// Same id-generation shape the "+ Add another display" button already uses
// (see add-display-btn's own handler) — app.js has no shared uid() helper
// of its own (that's a service.js-internal one, a separate closure/script).
function genOutputId(prefix) { return `${prefix}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`; }

// settings.ndiOutputs/syphonOutputs: [{id, name, sourceName, enabled}, …].
// One-time migration from the old singleton settings.ndiEnabled/syphonEnabled
// flags (the source name was never actually persisted pre-migration — it
// always reset to the HTML's own default each launch — so there's nothing
// real to carry forward for it beyond the enabled state).
function ndiOutputs() {
  if (!Array.isArray(settings.ndiOutputs)) {
    settings.ndiOutputs = [{ id: genOutputId('ndi'), name: 'NDI Output', sourceName: 'KAIRO Scripture', width: 1920, height: 1080, enabled: !!settings.ndiEnabled }];
    saveSettingsPatch({ ndiOutputs: settings.ndiOutputs });
  }
  return settings.ndiOutputs;
}
function syphonOutputs() {
  if (!Array.isArray(settings.syphonOutputs)) {
    settings.syphonOutputs = [{ id: genOutputId('syphon'), name: `${nativeSenderLabel()} Output`, sourceName: 'KAIRO Scripture', width: 1920, height: 1080, enabled: !!settings.syphonEnabled }];
    saveSettingsPatch({ syphonOutputs: settings.syphonOutputs });
  }
  return settings.syphonOutputs;
}

// Blackmagic DeckLink outputs (src-tauri decklink.rs): none until added.
// { id, name, device, deviceName, mode, modeName, width, height, fps, keying, enabled }
function decklinkOutputs() {
  if (!Array.isArray(settings.decklinkOutputs)) settings.decklinkOutputs = [];
  return settings.decklinkOutputs;
}
// The saved list for a native output kind ('ndi' | 'syphon' | 'decklink').
function nativeOutputsOf(kind) {
  return kind === 'ndi' ? ndiOutputs() : kind === 'decklink' ? decklinkOutputs() : syphonOutputs();
}

function allOutputKeys() {
  return [
    ...OUTPUT_DEFS.map(d => d.key),
    ...extraDisplays().map(d => d.id),
    ...ndiOutputs().map(d => d.id),
    ...syphonOutputs().map(d => d.id),
    ...decklinkOutputs().map(d => d.id),
  ];
}

// ── Physical screen assignment ───────────────────────────────────────────
// Which real, detected screen each display output (External Display, extra
// display rows — NOT the broadcast-only outputs like NDI/Syphon) has been
// deliberately pointed at, keyed by output id. Populated from the Window
// Management API (see refreshDisplayStatus) and chosen explicitly per
// output, the same way ProPresenter's Hardware tab has the operator pick a
// device/resolution per screen rather than guessing from whatever's first
// in the OS's list. Used both to open a display window on the right
// physical screen (openDisplayOutput) and to shape the Live preview panel
// when it's monitoring that output (applyLivePreviewAspect).
let cachedScreens = [];

function outputScreenMap() {
  return (settings.outputScreens && typeof settings.outputScreens === 'object') ? settings.outputScreens : {};
}

function setOutputScreen(outputId, screen) {
  const map = { ...outputScreenMap() };
  if (screen) map[outputId] = screen; else delete map[outputId];
  if (screen && noScreenNoticeFor === outputId) noScreenNoticeFor = null;
  settings.outputScreens = map;
  saveSettingsPatch({ outputScreens: map });
  // A display is "on" when it has a screen — the monitor shows only outputs
  // that are on, and Main's box takes its screen's shape.
  if (typeof refreshMonitor === 'function') refreshMonitor();
}

// Which screens are connected, as one comparable string.
function screensSignature() {
  return cachedScreens.map(s => `${s.index}:${s.width}x${s.height}@${s.left},${s.top}${s.isPrimary ? 'p' : ''}`).join('|');
}

function populateScreenOptions(sel, outputId) {
  sel.dataset.screensSig = screensSignature();
  const current = outputScreenMap()[outputId];
  const prevValue = sel.value;
  sel.innerHTML = '';
  const noneOpt = document.createElement('option');
  noneOpt.value = '';
  noneOpt.textContent = 'None — don’t output here';
  sel.appendChild(noneOpt);
  cachedScreens.forEach(s => {
    const o = document.createElement('option');
    o.value = String(s.index);
    o.textContent = `${s.isPrimary ? 'This Mac’s screen' : 'Display ' + (s.index + 1)} — ${s.width}×${s.height}`;
    if (current && current.width === s.width && current.height === s.height &&
        current.left === s.left && current.top === s.top) o.selected = true;
    sel.appendChild(o);
  });
  // Keep whatever was selected a moment ago if it's still a valid option —
  // this runs on every 3s poll (see wireExternalDisplayOutput), so without
  // this the dropdown would silently reset selection out from under an
  // operator mid-interaction every time cachedScreens refreshes.
  if (prevValue && sel.querySelector(`option[value="${prevValue}"]`)) sel.value = prevValue;
}

// Picking a monitor here IS the action — no separate "enable" toggle, no
// "Open" button. Matches every other presentation app: select where it
// should go, and it goes there immediately. Selecting "None" closes
// whatever window was open for this output.
// Every "nothing happened" display-window bug this session traced back to
// a silent failure with zero error surface — a duck-typed cross-file
// guard failing quietly, a fallback nobody saw. This makes that class of
// failure visible in the debug log the moment it happens, instead of only
// once an operator reports "the display isn't working."
function logDisplayLifecycleFallback(where, detail) {
  // /api/debug-log only reads {event, data} off the body (see kairo-server routes.rs) —
  // anything else silently gets dropped, which would have been a fittingly
  // ironic way for this exact hardening to go silent itself.
  fetch(`${SERVER}/api/debug-log`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ event: 'display-lifecycle-fallback', data: { where, ...detail } }),
  }).catch(() => {});
}

function buildScreenSelect(outputId) {
  const sel = document.createElement('select');
  sel.className = 'setting-input output-screen-select';
  sel.dataset.outputId = outputId;
  populateScreenOptions(sel, outputId);
  sel.addEventListener('change', () => {
    // Real incident: `Number('')` evaluates to 0 in JS (not NaN) — the
    // "None — don't output here" option's own value IS '' (see
    // populateScreenOptions' noneOpt.value = ''), so selecting it silently
    // resolved to cachedScreens[0] (whatever monitor happens to be first
    // in the list — typically the operator's own main screen) instead of
    // "no screen at all". Owner: "display output is set to none, but it's
    // showing up on my machine." Explicit empty-string check first, rather
    // than trusting Number() to produce NaN for invalid input the way it
    // does for every OTHER non-numeric string.
    const s = sel.value === '' ? null : cachedScreens[Number(sel.value)];
    setOutputScreen(outputId, s ? { width: s.width, height: s.height, left: s.left, top: s.top } : null);
    const label = `kairo-${outputId}`;
    if (s) {
      if (typeof openDisplayOutput === 'function') openDisplayOutput({ id: outputId, name: outputId });
    } else if (typeof closeDisplayWindow === 'function') {
      closeDisplayWindow(label);
    } else {
      logDisplayLifecycleFallback('buildScreenSelect', { outputId, reason: 'closeDisplayWindow not a function' });
    }
    // The list's on/off switch and the panel's own header follow the pick.
    renderOutputsList();
    if (selectedOutputId === outputId) renderOutputsDetail();
  });
  return sel;
}

// The 3 s screen poll (wireExternalDisplayStatus) keeps an open detail panel
// current IN PLACE. It used to rebuild the whole panel every tick, and a
// rebuilt <select> takes its open menu with it: "None" in the Physical screen
// menu only took if it was picked before the next tick (owner, 2026-10-01: "I
// try to change it to none, it never works"). A screen list only changes when
// a display is plugged in or out, and a control under the operator's hand is
// left alone even then.
function refreshDisplayDetailInPlace() {
  const connected = cachedScreens.length > 1;
  document.getElementById('external-detail-dot')?.classList.toggle('connected', connected);
  const status = document.getElementById('external-detail-status');
  if (status?.lastChild?.nodeType === Node.TEXT_NODE) {
    status.lastChild.textContent = connected ? 'External display connected' : 'No external display connected';
  }
  const sig = screensSignature();
  document.querySelectorAll('select.output-screen-select').forEach(sel => {
    if (sel === document.activeElement || sel.dataset.screensSig === sig) return;
    populateScreenOptions(sel, sel.dataset.outputId);
  });
}

// Set when switching a display on found no external screen to open on — its
// detail panel says why instead of the switch just springing back.
let noScreenNoticeFor = null;

// Briefly numbers the given screen (Identify Displays, now wired per-row
// inside the Outputs master-detail's display detail panel — see
// renderExternalDisplayDetail/renderExtraDisplayDetail below — instead of
// one fixed button+dropdown pair, since there's no longer one static
// "the" screen picker to read from).
let identifyWindowsOpen = false;
async function identifyScreen(screenIdx) {
  if (identifyWindowsOpen) return;
  if (typeof openDisplayWindow !== 'function') {
    logDisplayLifecycleFallback('identifyScreen', { reason: 'openDisplayWindow not a function' });
    return;
  }
  const s = Number.isInteger(screenIdx) ? cachedScreens[screenIdx] : null;
  if (!s) { toast('Select a display first', 'error'); return; }
  identifyWindowsOpen = true;
  const label = 'kairo-identify-0';
  await openDisplayWindow(
    label,
    `/identify.html?n=${screenIdx + 1}&label=${encodeURIComponent(s.isPrimary ? 'This Mac’s screen' : `Display ${screenIdx + 1}`)}`,
    { width: s.width, height: s.height, x: s.left, y: s.top, fullscreen: false }
  );
  setTimeout(() => {
    if (typeof closeDisplayWindow === 'function') closeDisplayWindow(label);
    else logDisplayLifecycleFallback('identifyScreen/auto-close', { label, reason: 'closeDisplayWindow not a function' });
    identifyWindowsOpen = false;
  }, 3000);
}

function outputThemeMap() {
  const map = settings.outputThemes && typeof settings.outputThemes === 'object'
    ? { ...settings.outputThemes } : {};
  // Carry the pre-multi-display assignment onto the first screen.
  if (map.external && !map['display-1']) map['display-1'] = map.external;
  // Any output without a valid choice falls back to the first theme.
  for (const key of allOutputKeys()) {
    if (!map[key] || !looks.some(l => l.id === map[key])) map[key] = looks[0]?.id;
  }
  return map;
}

// ── Language ──────────────────────────────────────────────────────────────
// Spoken language drives transcription; scripture language selects which
// text a detected/displayed verse shows in (the server's applyScriptureLanguage,
// a real lookup against databases/bibles/packs/*.json — sourced 2026-09-13
// from public-domain/CC-BY-SA editions, real text, not placeholders).
//
// Important scope note, same honesty precedent as the old "Coming soon"
// this replaced: LIVE AUDIO DETECTION still only runs against the English
// KJV corpus — these packs change what a resolved reference DISPLAYS as,
// not what language the app listens for. A second language's own
// detection index (anchor trie, IDF map, embeddings) is real infrastructure
// that doesn't exist yet. `hasPack: true` means real bundled verse text;
// it does not mean "detects speech in this language."
const LANG_PACKS = [
  { code: 'en', name: 'English',    translations: 'KJV · NIV · NLT · ESV · NASB · NKJV' },
  { code: 'es', name: 'Spanish',    translations: 'Reina-Valera 1909 (public domain)' },
  { code: 'pt', name: 'Portuguese', translations: 'Almeida Atualizada 1911 (GPL)' },
  { code: 'fr', name: 'French',     translations: 'Louis Segond 1910 (public domain)' },
  { code: 'de', name: 'German',     translations: 'Luther 1545 (public domain)' },
  { code: 'yo', name: 'Yoruba',     translations: 'Bíbélì Mímọ́ (Biblica, CC BY-SA 4.0)' },
  { code: 'ig', name: 'Igbo',       translations: 'Baịbụl Nsọ (Biblica, CC BY-SA 4.0)' },
  { code: 'ha', name: 'Hausa',      translations: 'Littafi Mai Tsarki (Biblica, CC BY-SA 4.0)' },
  { code: 'sw', name: 'Swahili',    translations: 'New Testament only (public domain)' },
];

// Every Bible above comes with the app (databases/bibles/packs) — there is
// nothing to install, so the menu offers them all (2026-10-12: the old
// per-language "Install" buttons only flipped a setting, and read as nine
// downloads). Display only: live detection follows the Spoken language.
function renderLangPacks() {
  const bibleSel = document.getElementById('bible-language');
  if (bibleSel) {
    const want = settings.bibleLanguage || 'en';
    bibleSel.innerHTML = '';
    LANG_PACKS.forEach(p => {
      const o = document.createElement('option');
      o.value = p.code;
      o.textContent = p.code === 'en' ? 'English' : `${p.name} — ${p.translations.replace(/\s*\(.*\)$/, '')}`;
      if (p.code === want) o.selected = true;
      bibleSel.appendChild(o);
    });
  }
  const credits = document.getElementById('scripture-credits');
  if (credits) {
    const licensed = LANG_PACKS.filter(p => /CC BY|GPL/.test(p.translations));
    credits.textContent = 'Sources: ' + licensed.map(p => `${p.name} ${p.translations}`).join(' · ') + ' · the others public domain.';
  }
}

document.getElementById('stt-language')?.addEventListener('change', (e) => {
  settings.sttLanguage = e.target.value;
  saveSettingsPatch({ sttLanguage: e.target.value });
});
document.getElementById('custom-keyterms')?.addEventListener('change', (e) => {
  settings.customKeyterms = e.target.value;
  saveSettingsPatch({ customKeyterms: e.target.value });
});
document.getElementById('bible-language')?.addEventListener('change', (e) => {
  settings.bibleLanguage = e.target.value;
  saveSettingsPatch({ bibleLanguage: e.target.value });
});

// ── Outputs master-detail ────────────────────────────────────────────────
// Owner: "I like how ProPresenter does it better" (referencing its Screen
// Configuration panel — one list of every output, a detail panel for
// whichever's selected) — replaces the old per-type stacked cards
// entirely. One flat list (owner confirmed: no Audience/Stage-style
// grouping needed) of every configured output — External Display + extra
// displays, every NDI output, every Syphon output, OBS — click one to see/
// edit its full settings on the right. A single "+ Add Output" button asks
// which kind (Display/NDI/Syphon — OBS isn't offered: it's one fixed
// WebSocket connection, not an addable/multi-instance output the way the
// other three are) rather than the old one-"+Add"-button-per-type layout.
let selectedOutputId = null;

function allConfiguredOutputs() {
  const list = [];
  for (const d of displayOutputs()) {
    list.push({ id: d.id, type: 'display', name: d.name, primary: d.id === PRIMARY_DISPLAY, monitor: d.kind === 'monitor' });
  }
  for (const o of ndiOutputs())    list.push({ id: o.id, type: 'ndi', name: o.name, raw: o });
  for (const o of syphonOutputs()) list.push({ id: o.id, type: 'syphon', name: o.name, raw: o });
  for (const o of decklinkOutputs()) list.push({ id: o.id, type: 'decklink', name: o.name, raw: o });
  list.push({ id: 'obs', type: 'obs', name: settings.obsName || 'OBS WebSocket' });
  return list;
}

// Same "same-machine, zero-copy, bundled-with-Kairo" output concept, just a
// different native protocol per OS — Syphon on macOS, Spout on Windows (no
// equivalent shipped for Linux yet). Kept as one internal type ('syphon',
// matching the syphon_* Tauri commands and settings.syphonOutputs — renaming
// either would be a real migration for zero benefit, since an operator only
// ever sees this label, never the internal key) with a platform-aware
// display name, using the same synchronous pre-paint platform-* class
// index.html's own inline script sets on <html> for the traffic-light-inset
// fix, rather than adding a new Tauri command just to ask the OS.
// Every NDI / Syphon start and stop goes through these, so the output's
// hidden capture window (output_capture.js — its real theme on air) opens
// and closes with it.
async function nativeStart(invokeFn, kind, args) {
  if (kind === 'decklink') {
    // The card decides the frame size (its video format); capture to match.
    const o = decklinkOutputs().find(x => x.id === args.id) || {};
    if (o.mode == null) throw new Error('Pick a card and a video format first.');
    const size = await invokeFn('decklink_start', { id: o.id, device: o.device | 0, mode: o.mode, keying: !!o.keying, fps: o.fps || 30 });
    window.KairoCapture?.open(o.id, size.width, size.height);
    return;
  }
  await invokeFn(`${kind}_start`, args);
  window.KairoCapture?.open(args.id, args.width, args.height);
}
async function nativeStop(invokeFn, kind, id) {
  window.KairoCapture?.close(id);
  await invokeFn(`${kind}_stop`, { id });
}

function nativeSenderLabel() {
  return document.documentElement.classList.contains('platform-win32') ? 'Spout' : 'Syphon';
}
function outputTypeLabel(type) {
  return type === 'display' ? 'Display' : type === 'ndi' ? 'NDI' : type === 'syphon' ? nativeSenderLabel() : type === 'decklink' ? 'DeckLink' : 'OBS';
}

// Entry point — called from loadSettings and from every add/remove/rename
// action below. Replaces the old renderOutputThemePickers/
// renderOutputLayerPickers/renderDisplayOutputs/renderNdiOutputs/
// renderSyphonOutputs — one function per concern, all folded into the two
// halves of this one list+detail pair now.
function renderOutputsPane() {
  if (!selectedOutputId) selectedOutputId = PRIMARY_DISPLAY;
  renderOutputsList(); // also renders the Output Control matrix — see its own comment
  renderOutputsDetail();
  refreshMonitor(); // keep the Monitor in step with the configured outputs
  wireOutputsAddMenu();
  autoResumeNativeOutputs();
}

// Any NDI/Syphon output saved as enabled starts itself once, on the first
// real render after settings load — matches how the old per-type card
// rendering used to auto-resume every enabled row, and how External
// Display's own auto-reopen already works (wireExternalDisplayStatus).
// One-shot: renderOutputsPane/renderOutputsList re-run often (every
// add/remove/rename), which must NOT re-trigger a start() on an output
// the operator may have deliberately stopped since launch.
let nativeOutputsAutoResumeDone = false;
function autoResumeNativeOutputs() {
  if (nativeOutputsAutoResumeDone) return;
  nativeOutputsAutoResumeDone = true;
  const invokeFn = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
  if (!invokeFn) return;
  for (const kind of ['ndi', 'syphon', 'decklink']) {
    for (const o of nativeOutputsOf(kind)) {
      if (!o.enabled) continue;
      nativeStart(invokeFn, kind, {
        id: o.id, sourceName: o.sourceName || 'KAIRO Scripture',
        width: o.width || 1920, height: o.height || 1080,
      })
        .then(() => { window.KairoNativeOutputs?.pushToOne?.(kind, o.id); renderOutputsList(); if (selectedOutputId === o.id) renderOutputsDetail(); })
        .catch(err => console.warn(`[${kind}] auto-resume failed:`, err));
    }
  }
}

function renderOutputsList() {
  const host = document.getElementById('outputs-list');
  if (!host) return;
  const outputs = allConfiguredOutputs();
  if (selectedOutputId !== AUDIO_OUTPUT_ID && !outputs.some(o => o.id === selectedOutputId)) selectedOutputId = outputs[0]?.id || null;
  host.innerHTML = '';
  outputs.forEach(o => {
    // A plain <div> (not <button>) — a real on/off <input> lives inside
    // each row (see below), and a checkbox/label nested inside a <button>
    // is invalid HTML that behaves inconsistently across browsers. Keyboard/
    // click selection still works via tabindex + a click handler on the row.
    const row = document.createElement('div');
    row.className = 'outputs-list-item' + (o.id === selectedOutputId ? ' active' : '');
    row.dataset.outputId = o.id;
    row.tabIndex = 0;
    row.setAttribute('role', 'button');

    const dot = document.createElement('span');
    dot.className = 'bs-dot';
    // Stable ids for the two outputs that already have their own live
    // status-polling logic elsewhere (wireExternalDisplayStatus,
    // pollOBSStatus) — everything else (extra displays, NDI/Syphon
    // instances) gets a plain per-row dot with no separate poller,
    // updated directly by that row's own detail-panel actions instead.
    if (o.id === PRIMARY_DISPLAY) dot.id = 'external-header-dot';
    else if (o.type === 'obs')    dot.id = 'obs-header-dot';
    else if (o.type === 'ndi' || o.type === 'syphon' || o.type === 'decklink') dot.classList.toggle('connected', !!o.raw?.enabled);

    const info = document.createElement('span');
    info.className = 'outputs-list-item-info';
    const nameEl = document.createElement('span');
    nameEl.className = 'outputs-list-item-name';
    nameEl.textContent = o.name;
    const typeEl = document.createElement('span');
    typeEl.className = 'outputs-list-item-type';
    typeEl.textContent = o.monitor ? 'Monitor' : outputTypeLabel(o.type);
    info.appendChild(nameEl);
    // The kind, only when the name doesn't already say it ("Monitor" over
    // MONITOR read twice; "Stage screen" still gets "Display").
    if (!String(o.name || '').toLowerCase().includes(typeEl.textContent.toLowerCase())) info.appendChild(typeEl);
    // A per-row Bible/Slide/Media/Timer summary used to render here too —
    // owner (2026-09-21): redundant now that the Monitor grid
    // (renderMonitorGrid, above) is the dedicated place for exactly this
    // at-a-glance view; duplicating it in this compact list row was just
    // clutter. Removed here; buildOutputLayersSummary itself stays, still
    // used by the Monitor grid.

    row.appendChild(dot);
    row.appendChild(info);

    // On/off indicator, right in the list — owner: "each output should
    // have an indicator to turn it on or off. All displays" — every type
    // including Display now gets one (buildOutputRowToggle's own 'display'
    // case mirrors the Physical Screen picker's assign/None toggle).
    row.appendChild(buildOutputRowToggle(o));

    row.addEventListener('click', () => {
      selectedOutputId = o.id;
      renderOutputsList();
      renderOutputsDetail();
    });
    row.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); row.click(); }
    });
    host.appendChild(row);
  });
  host.appendChild(buildAudioOutputRow());
  // Folded in here (not called separately at each of this function's many
  // call sites) so the Output Control matrix — which lists the exact same
  // outputs this list does — can never drift out of sync with an output
  // being added/removed/renamed just because some call site forgot it.
  renderOutputControlMatrix();
}

// The on/off switch embedded directly in each list row (see its own call
// site's comment above). Reuses the exact enable/disable logic each detail
// panel's own toggle/Start-Stop button already has — one real
// implementation per output type, called from two places, not duplicated.
// Whether an output is actually ON right now — the same per-type check
// buildOutputRowToggle's own `input.checked` logic below already
// determines, factored out so other callers (renderMonitorGrid) don't
// re-derive it a second way.
function isOutputEnabled(o) {
  if (o.type === 'obs') return settings.obsEnabled === true;
  if (o.type === 'display') return !!outputScreenMap()[o.id];
  return !!o.raw?.enabled; // ndi/syphon
}

function buildOutputRowToggle(o) {
  const label = document.createElement('label');
  label.className = 'output-toggle';
  label.title = `Enable this ${outputTypeLabel(o.type)} output`;
  const input = document.createElement('input');
  input.type = 'checkbox';
  const track = document.createElement('span');
  track.className = 'output-toggle-track';
  label.appendChild(input); label.appendChild(track);
  // Never let the toggle's own click also select the row underneath it —
  // both fire on the same event otherwise (label click bubbles to row).
  label.addEventListener('click', (e) => e.stopPropagation());

  if (o.type === 'obs') {
    input.checked = isOutputEnabled(o);
    input.addEventListener('change', () => {
      settings.obsEnabled = input.checked;
      saveSettingsPatch({ obsEnabled: input.checked });
      renderOutputsList();
      if (selectedOutputId === o.id) renderOutputsDetail();
    });
    return label;
  }

  if (o.type === 'display') {
    // "On" mirrors picking a real screen in the Physical Screen select;
    // "off" mirrors picking "None — don't output here". Reuses the exact
    // assign/open/close sequence buildScreenSelect's own change handler
    // already does, rather than a second parallel implementation.
    input.checked = isOutputEnabled(o);
    input.addEventListener('change', () => {
      if (input.checked) {
        // The screen it was given before, else the first EXTERNAL screen.
        // With nothing plugged in it used to take cachedScreens[0] — this
        // Mac's own screen — and open there fullscreen and always on top,
        // over Kairo itself (owner, 2026-10-01: "it defaults to my mac
        // display because nothing is connected"). This Mac's screen is
        // still there to pick, deliberately, under Physical screen.
        const s = outputScreenMap()[o.id] || cachedScreens.find(x => !x.isPrimary);
        if (!s) {
          input.checked = false;
          noScreenNoticeFor = o.id;
          selectedOutputId = o.id;
          renderOutputsList();
          renderOutputsDetail();
          return;
        }
        setOutputScreen(o.id, { width: s.width, height: s.height, left: s.left, top: s.top });
        if (typeof openDisplayOutput === 'function') openDisplayOutput({ id: o.id, name: o.id });
      } else {
        setOutputScreen(o.id, null);
        const label2 = `kairo-${o.id}`;
        if (typeof closeDisplayWindow === 'function') closeDisplayWindow(label2);
        else logDisplayLifecycleFallback('buildOutputRowToggle(display)', { outputId: o.id, reason: 'closeDisplayWindow not a function' });
      }
      renderOutputsList();
      if (selectedOutputId === o.id) renderOutputsDetail();
    });
    return label;
  }

  // ndi/syphon
  const kind = o.type;
  input.checked = isOutputEnabled(o);
  input.addEventListener('change', async () => {
    const invokeFn = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
    if (!invokeFn) { input.checked = false; toast(`${outputTypeLabel(kind)} not available (run inside the app)`, 'error'); return; }
    if (input.checked) {
      try {
        await nativeStart(invokeFn, kind, {
          id: o.id, sourceName: o.raw.sourceName || 'KAIRO Scripture',
          width: o.raw.width || 1920, height: o.raw.height || 1080,
        });
        updateNativeOutput(kind, o.id, { enabled: true, lastError: null });
        window.KairoNativeOutputs?.pushToOne?.(kind, o.id);
      } catch (e) {
        input.checked = false;
        // toast() is a permanent no-op in this app (by design) — the native
        // side's actual failure reason (e.g. "NDI runtime not found...") has
        // to reach the user some other way, or the toggle just silently
        // reverts with zero explanation. Surfaced in the detail panel's own
        // status text instead (see renderNativeOutputDetail) — still calling
        // toast() too in case that ever changes, but not relying on it.
        const msg = String(e && e.message || e || 'unknown error').replace(/^Error:\s*/, '');
        updateNativeOutput(kind, o.id, { enabled: false, lastError: msg });
        if (kind === 'ndi' && /runtime not found/i.test(msg)) selectedOutputId = o.id; // show the download offer
        toast(`${outputTypeLabel(kind)} failed: ` + msg, 'error');
      }
    } else {
      try { await nativeStop(invokeFn, kind, o.id); } catch {}
      updateNativeOutput(kind, o.id, { enabled: false });
    }
    renderOutputsList();
    if (selectedOutputId === o.id) renderOutputsDetail();
  });
  return label;
}

// ── Audio Output ──────────────────────────────────────────────────────────
// Sound from video on the Media layer plays through whichever screen shows it,
// which for an HDMI display means the HDMI cable. This entry sends it to a
// different device instead — a speaker, a mixer, this computer's own speakers.
// The choice is applied in display.html via setSinkId (see /api/service/
// audio-output); an engine that can't route sound says so here.
const AUDIO_OUTPUT_ID = 'audio-out';

function mediaOutputRoutable() {
  return typeof HTMLMediaElement !== 'undefined' && typeof HTMLMediaElement.prototype.setSinkId === 'function';
}
function savedMediaOutput() {
  try { return localStorage.getItem(MEDIA_OUTPUT_KEY) || ''; } catch { return ''; }
}
function setMediaOutput(deviceId) {
  try { localStorage.setItem(MEDIA_OUTPUT_KEY, deviceId || ''); } catch {}
  fetch(`${SERVER}/api/service/audio-output`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ deviceId: deviceId || null }),
  }).catch(() => {});
}
const MEDIA_VOLUME_KEY = 'kairo-media-volume';
function savedMediaVolume() {
  try { const v = parseInt(localStorage.getItem(MEDIA_VOLUME_KEY) || '', 10); return v >= 0 && v <= 100 ? v : 100; } catch { return 100; }
}
function setMediaVolume(pct) {
  try { localStorage.setItem(MEDIA_VOLUME_KEY, String(pct)); } catch {}
  const bar = document.getElementById('media-volume');
  if (bar) bar.value = String(pct); // the transport bar's slider follows
  fetch(`${SERVER}/api/service/media-control`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'volume', value: pct / 100 }),
  }).catch(() => {});
}
async function mediaOutputDevices() {
  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    return devices.filter(d => d.kind === 'audiooutput' && d.deviceId);
  } catch { return []; }
}

function buildAudioOutputRow() {
  const row = document.createElement('div');
  row.className = 'outputs-list-item' + (selectedOutputId === AUDIO_OUTPUT_ID ? ' active' : '');
  row.dataset.outputId = AUDIO_OUTPUT_ID;
  row.tabIndex = 0;
  row.setAttribute('role', 'button');
  const dot = document.createElement('span');
  dot.className = 'bs-dot';
  dot.classList.toggle('connected', !!savedMediaOutput());
  const info = document.createElement('span');
  info.className = 'outputs-list-item-info';
  const name = document.createElement('span');
  name.className = 'outputs-list-item-name';
  name.textContent = 'Audio Output';
  const type = document.createElement('span');
  type.className = 'outputs-list-item-type';
  type.textContent = savedMediaOutput() ? 'Chosen device' : 'System default';
  info.append(name, type);
  row.append(dot, info);
  row.addEventListener('click', () => { selectedOutputId = AUDIO_OUTPUT_ID; renderOutputsList(); renderOutputsDetail(); });
  row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); row.click(); } });
  return row;
}

function renderAudioOutputDetail(host) {
  host.appendChild(detailTitle('Audio Output', null, null));
  const intro = document.createElement('p');
  intro.className = 'setting-hint';
  intro.textContent = 'Sound from video on the Media layer plays from this computer, through its sound output \u2014 or through the device you pick here: a speaker, a mixer, an HDMI display. Screens show the picture only.';
  host.appendChild(intro);

  const group = detailGroup('Play media sound on');
  const row = document.createElement('div');
  row.className = 'audio-out-row';
  const sel = document.createElement('select');
  sel.className = 'setting-input';
  const refresh = document.createElement('button');
  refresh.type = 'button';
  refresh.className = 'modal-btn secondary audio-refresh-btn';
  refresh.title = 'Refresh device list';
  refresh.textContent = '\u21BB';
  row.append(sel, refresh);
  group.appendChild(row);
  const note = document.createElement('p');
  note.className = 'setting-hint';
  group.appendChild(note);
  host.appendChild(group);

  const volGroup = detailGroup('Volume');
  const volRow = document.createElement('div');
  volRow.className = 'audio-out-row';
  const vol = document.createElement('input');
  vol.type = 'range'; vol.min = '0'; vol.max = '100'; vol.step = '1';
  vol.value = String(savedMediaVolume());
  vol.className = 'audio-out-volume';
  vol.setAttribute('aria-label', 'Media volume');
  const volVal = document.createElement('span');
  volVal.className = 'audio-out-volume-val';
  volVal.textContent = `${vol.value}%`;
  vol.addEventListener('input', () => { volVal.textContent = `${vol.value}%`; setMediaVolume(Number(vol.value)); });
  volRow.append(vol, volVal);
  volGroup.appendChild(volRow);
  const volHint = document.createElement('p');
  volHint.className = 'setting-hint';
  volHint.textContent = 'How loud video on the Media layer plays. The same level as the slider on the media controls.';
  volGroup.appendChild(volHint);
  host.appendChild(volGroup);

  const testBtn = document.createElement('button');
  testBtn.type = 'button';
  testBtn.className = 'modal-btn secondary';
  testBtn.textContent = 'Play test sound';
  host.appendChild(testBtn);

  const fill = async () => {
    const saved = savedMediaOutput();
    const devices = await mediaOutputDevices();
    sel.innerHTML = '';
    const dflt = document.createElement('option');
    dflt.value = ''; dflt.textContent = 'System default';
    sel.appendChild(dflt);
    devices.forEach(d => {
      const o = document.createElement('option');
      o.value = d.deviceId;
      o.textContent = d.label || `Output ${d.deviceId.slice(0, 6)}`;
      if (d.deviceId === saved) o.selected = true;
      sel.appendChild(o);
    });
    const routable = mediaOutputRoutable();
    sel.disabled = !routable;
    testBtn.disabled = !routable;
    note.textContent = routable
      ? 'Applies to the video that is playing now and to every video you send after it.'
      : 'This computer\u2019s display engine can\u2019t pick a sound device. Set the output in your system\u2019s Sound settings instead.';
  };
  sel.addEventListener('change', () => { setMediaOutput(sel.value); window.applyMediaSink?.(); renderOutputsList(); });
  refresh.addEventListener('click', fill);
  testBtn.addEventListener('click', async () => {
    try {
      const ctx = new (window.AudioContext || window.webkitAudioContext)();
      const dest = ctx.createMediaStreamDestination();
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      gain.gain.value = 0.15;
      osc.frequency.value = 440;
      osc.connect(gain).connect(dest);
      const el = new Audio();
      el.srcObject = dest.stream;
      if (sel.value && el.setSinkId) await el.setSinkId(sel.value);
      osc.start();
      await el.play();
      setTimeout(() => { osc.stop(); el.pause(); ctx.close(); }, 900);
    } catch (err) {
      note.textContent = `Couldn\u2019t play to that device: ${String(err && err.message || err)}`;
    }
  });
  fill();
}

function renderOutputsDetail() {
  const host = document.getElementById('outputs-detail');
  if (!host) return;
  host.innerHTML = '';
  if (selectedOutputId === AUDIO_OUTPUT_ID) { renderAudioOutputDetail(host); return; }
  const o = allConfiguredOutputs().find(x => x.id === selectedOutputId);
  if (!o) { host.textContent = 'Select an output.'; return; }
  if (o.type === 'display') renderDisplayDetail(host, o);
  else if (o.type === 'obs') renderObsDetail(host, o);
  else if (o.type === 'decklink') renderDeckLinkDetail(host, o);
  else renderNativeOutputDetail(host, o); // ndi/syphon
}

// Small local helpers shared by every detail-panel builder below — keeps
// each builder itself readable (just the fields that actually differ per
// type) instead of repeating this boilerplate four times.
function detailGroup(labelText) {
  const group = document.createElement('div');
  group.className = 'setting-group';
  const lbl = document.createElement('label');
  lbl.className = 'setting-label';
  lbl.textContent = labelText;
  group.appendChild(lbl);
  return group;
}

// Label beside the control instead of above it — owner: "to save space,
// can we not use the same theme picker from bible mode for outputs?" —
// a compact one-line row instead of detailGroup's stacked label+control,
// used for Theme specifically since the detail panel is already tight on
// vertical room with everything else an output needs.
function detailInlineGroup(labelText, controlEl) {
  const row = document.createElement('div');
  row.className = 'outputs-detail-inline-row';
  const lbl = document.createElement('label');
  lbl.className = 'setting-label';
  lbl.textContent = labelText;
  row.appendChild(lbl);
  row.appendChild(controlEl);
  return row;
}
// Owner: "on or off toggle should be on the far right of each output
// header. include indicator for each connection in the header as well.
// All displays" — title (left) → connection status (middle) → on/off
// toggle (far right, flex layout naturally pushes it there since title
// has flex:1), for every output type uniformly.
function detailTitle(text, statusEl, toggleEl) {
  const wrap = document.createElement('div');
  wrap.className = 'outputs-detail-header';
  const h = document.createElement('h3');
  h.className = 'outputs-detail-title';
  h.textContent = text;
  wrap.appendChild(h);
  if (statusEl) wrap.appendChild(statusEl);
  if (toggleEl) wrap.appendChild(toggleEl);
  return wrap;
}

// The theme picker for an output: a dropdown whose rows carry a tiny preview
// of each theme (the same thumbnail the Slides/Bible theme picker and Theme
// Studio draw), which a native <select> can't show.
function buildThemeSelect(outputId) {
  return buildThemeDropdown({
    value: () => {
      const id = outputThemeMap()[outputId];
      return (looks.find(l => l.id === id) || looks[0])?.id;
    },
    onPick: (id) => {
      settings.outputThemes = { ...outputThemeMap(), [outputId]: id };
      saveSettingsPatch({ outputThemes: settings.outputThemes });
      applyOutputThemes();
    },
  });
}

// Shared thumbnail theme dropdown (grouped like every other theme picker).
// `value()` returns the current theme id; `onPick(id)` applies a choice;
// `defaultLabel` adds a first row meaning "no theme of its own" (id '').
// The returned element's refresh() repaints after the value changes outside.
function buildThemeDropdown({ value, onPick, defaultLabel }) {
  const wrap = document.createElement('div');
  wrap.className = 'theme-pick';
  const trigger = document.createElement('button');
  trigger.type = 'button';
  trigger.className = 'setting-input theme-pick-trigger';
  trigger.setAttribute('aria-haspopup', 'listbox');
  trigger.setAttribute('aria-expanded', 'false');
  const menu = document.createElement('div');
  menu.className = 'theme-pick-menu';
  menu.setAttribute('role', 'listbox');
  menu.hidden = true;
  wrap.appendChild(trigger);
  wrap.appendChild(menu);

  const thumbFor = (look) => {
    const t = document.createElement('div');
    t.className = 'ts-theme-thumb';
    if (look) window.KairoService?.renderLookThumbnail(t, look);
    else t.classList.add('theme-pick-thumb-none');
    return t;
  };
  const currentLook = () => looks.find(l => l.id === value()) || null;
  const paintTrigger = () => {
    const look = currentLook();
    trigger.innerHTML = '';
    if (look || defaultLabel) {
      trigger.appendChild(thumbFor(look));
      const name = document.createElement('span');
      name.className = 'theme-pick-name';
      name.textContent = look ? look.name : defaultLabel;
      trigger.appendChild(name);
    }
    const chev = document.createElement('span');
    chev.className = 'theme-pick-chev';
    chev.innerHTML = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>';
    chev.setAttribute('aria-hidden', 'true');
    trigger.appendChild(chev);
  };
  const close = () => {
    menu.hidden = true;
    trigger.setAttribute('aria-expanded', 'false');
    document.removeEventListener('mousedown', onOutside, true);
    document.removeEventListener('keydown', onKey, true);
    document.removeEventListener('scroll', onScroll, true);
  };
  const onOutside = (e) => { if (!wrap.contains(e.target)) close(); };
  const onKey = (e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); trigger.focus(); } };
  const onScroll = (e) => { if (!menu.contains(e.target)) close(); };
  const pick = (id) => { onPick(id); paintTrigger(); close(); };
  // Fixed to the viewport so a dialog or scrolling panel can't clip it;
  // opens upward when there's more room above.
  const placeMenu = () => {
    const r = trigger.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 12, above = r.top - 12;
    const up = below < 220 && above > below;
    menu.style.position = 'fixed';
    menu.style.left = r.left + 'px';
    menu.style.width = r.width + 'px';
    menu.style.right = 'auto';
    menu.style.maxHeight = Math.max(160, Math.min(360, up ? above : below)) + 'px';
    menu.style.top = up ? 'auto' : (r.bottom + 4) + 'px';
    menu.style.bottom = up ? (window.innerHeight - r.top + 4) + 'px' : 'auto';
  };
  const open = () => {
    menu.innerHTML = '';
    const current = currentLook();
    const buildRow = (l) => {
      const row = document.createElement('button');
      row.type = 'button';
      row.setAttribute('role', 'option');
      row.className = 'theme-pick-item' + (current && l.id === current.id ? ' active' : '');
      row.appendChild(thumbFor(l));
      const name = document.createElement('span');
      name.textContent = l.name;
      row.appendChild(name);
      row.addEventListener('click', () => pick(l.id));
      return row;
    };
    if (defaultLabel) {
      const row = document.createElement('button');
      row.type = 'button';
      row.setAttribute('role', 'option');
      row.className = 'theme-pick-item' + (current ? '' : ' active');
      row.appendChild(thumbFor(null));
      const name = document.createElement('span');
      name.textContent = defaultLabel;
      row.appendChild(name);
      row.addEventListener('click', () => pick(''));
      menu.appendChild(row);
    }
    menu.appendChild(window.KairoService.buildThemeTree(looks, buildRow, current && current.id));
    menu.hidden = false;
    placeMenu();
    trigger.setAttribute('aria-expanded', 'true');
    menu.querySelector('.active')?.scrollIntoView({ block: 'nearest' });
    document.addEventListener('mousedown', onOutside, true);
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('scroll', onScroll, true);
  };
  trigger.addEventListener('click', () => (menu.hidden ? open() : close()));
  paintTrigger();
  wrap.refresh = paintTrigger;
  return wrap;
}

function renderDisplayDetail(host, o) {
  // Connection indicator in the header (owner: "include indicator for
  // each connection in the header as well. All displays"). Primary gets
  // the real live dot wireExternalDisplayStatus's refresh() re-renders
  // this whole panel to update (see its own comment) — 'external-detail-*'
  // ids, distinct from the list row's 'external-header-*' ones since both
  // exist in the DOM at once. Extra displays don't have their own live
  // poller (only the primary's list_monitors call is polled) — reflects
  // "is a screen currently assigned" instead, which is exactly as
  // accurate and needs no separate polling loop.
  const statusEl = document.createElement('span');
  statusEl.className = 'native-output-status';
  if (o.primary) {
    const dot = document.createElement('span');
    dot.className = 'bs-dot';
    dot.id = 'external-detail-dot';
    dot.classList.toggle('connected', cachedScreens.length > 1);
    statusEl.id = 'external-detail-status';
    statusEl.textContent = cachedScreens.length > 1 ? 'External display connected' : 'No external display connected';
    statusEl.prepend(dot);
  } else {
    const assigned = !!outputScreenMap()[o.id];
    statusEl.textContent = assigned ? 'Assigned' : 'Not assigned';
    statusEl.classList.toggle('is-active', assigned);
  }
  host.appendChild(detailTitle(o.name, statusEl, buildOutputRowToggle(o)));

  if (noScreenNoticeFor === o.id && !outputScreenMap()[o.id]) {
    const note = document.createElement('p');
    note.className = 'setting-hint outputs-detail-notice';
    note.setAttribute('role', 'status');
    note.textContent = 'No external display is connected, so nothing was opened. Plug one in and switch this on again — or pick This Mac’s screen under Physical screen to see the output here.';
    host.appendChild(note);
  }

  // Renameable — the primary display included (owner: "users should be
  // able to rename each output"), stored separately from extras since it
  // isn't itself an entry in the extraDisplays array.
  const nameGroup = detailGroup('Name');
  const nameInput = document.createElement('input');
  nameInput.type = 'text';
  nameInput.className = 'setting-input';
  nameInput.value = o.name;
  nameInput.addEventListener('change', () => {
    const newName = nameInput.value.trim() || o.name;
    if (o.primary) {
      settings.primaryDisplayName = newName;
      saveSettingsPatch({ primaryDisplayName: newName });
    } else {
      const list = extraDisplays().map(x => x.id === o.id ? { ...x, name: newName } : x);
      settings.extraDisplays = list;
      saveSettingsPatch({ extraDisplays: list });
    }
    renderOutputsList();
  });
  nameGroup.appendChild(nameInput);
  host.appendChild(nameGroup);

  const screenGroup = detailGroup('Physical screen');
  const screenSel = buildScreenSelect(o.id);
  screenGroup.appendChild(screenSel);
  host.appendChild(screenGroup);

  const identifyBtn = document.createElement('button');
  identifyBtn.type = 'button';
  identifyBtn.className = 'modal-btn secondary';
  identifyBtn.textContent = 'Identify Display';
  identifyBtn.style.marginBottom = '4px';
  identifyBtn.addEventListener('click', () => identifyScreen(screenSel.value === '' ? null : Number(screenSel.value)));
  host.appendChild(identifyBtn);

  if (o.monitor) {
    // Its own layout, words only — no theme or key to choose.
    const about = document.createElement('p');
    about.className = 'setting-hint';
    about.textContent = 'For whoever is up front: what’s on screen now with the next slide right under it, the time, the segment countdown, and any message you send from Monitor message under the preview. A single scripture called out shows on its own.';
    host.appendChild(about);
  } else {
    host.appendChild(detailInlineGroup('Theme', buildThemeSelect(o.id)));
    host.appendChild(detailInlineGroup('Keying', buildKeyingSelect(o.id)));
  }
  appendOutputLayersSection(host, o.id);

  // The last Monitor is the basic one (see seedMonitorOutput): it stays.
  if (!o.primary && !(o.monitor && monitorOutputs().length <= 1)) {
    const del = document.createElement('button');
    del.type = 'button';
    del.className = 'modal-btn secondary display-output-del';
    del.textContent = o.monitor ? 'Remove this monitor' : 'Remove this display';
    del.addEventListener('click', () => {
      settings.extraDisplays = extraDisplays().filter(x => x.id !== o.id);
      const screens = { ...outputScreenMap() }; delete screens[o.id];
      settings.outputScreens = screens;
      const themes = { ...outputThemeMap() }; delete themes[o.id];
      const layers = { ...outputLayerMap() }; delete layers[o.id];
      settings.outputThemes = themes; settings.outputLayers = layers;
      saveSettingsPatch({ extraDisplays: settings.extraDisplays, outputScreens: screens, outputThemes: themes, outputLayers: layers });
      selectedOutputId = PRIMARY_DISPLAY;
      applyOutputThemes(); applyOutputLayers(); applyOutputModes();
      renderOutputsList(); renderOutputsDetail();
    });
    host.appendChild(del);
  }
}

function renderObsDetail(host, o) {
  // Connection indicator in the header — dot + text as separate sibling
  // elements (NOT text set directly on the wrapper, which would wipe the
  // dot via .textContent replacing all children — a real bug this fixes).
  // Same ids updateOBSHeaderStatus already knows to write to.
  const statusEl = document.createElement('span');
  statusEl.className = 'native-output-status';
  const dot = document.createElement('span');
  dot.className = 'bs-dot';
  dot.id = 'obs-detail-dot';
  const statusText = document.createElement('span');
  statusText.id = 'obs-detail-status-text';
  statusText.textContent = 'Checking…';
  statusEl.appendChild(dot);
  statusEl.appendChild(statusText);
  host.appendChild(detailTitle(o.name, statusEl, buildOutputRowToggle(o)));
  pollOBSStatus(); // one immediate check so this fresh dot isn't stuck on "Checking…" until the next 5s tick

  const nameGroup = detailGroup('Name');
  const nameInput = document.createElement('input');
  nameInput.type = 'text';
  nameInput.className = 'setting-input';
  nameInput.value = o.name;
  nameInput.addEventListener('change', () => {
    settings.obsName = nameInput.value.trim() || o.name;
    saveSettingsPatch({ obsName: settings.obsName });
    renderOutputsList();
  });
  nameGroup.appendChild(nameInput);
  host.appendChild(nameGroup);

  const urlGroup = detailGroup('URL');
  const urlInput = document.createElement('input');
  urlInput.type = 'text';
  urlInput.className = 'setting-input';
  urlInput.placeholder = 'ws://localhost:4455';
  urlInput.value = settings.obsUrl || '';
  urlInput.addEventListener('change', () => { settings.obsUrl = urlInput.value || 'ws://localhost:4455'; saveSettingsPatch({ obsUrl: settings.obsUrl }); });
  urlGroup.appendChild(urlInput);
  host.appendChild(urlGroup);

  const passGroup = detailGroup('Password');
  const passInput = document.createElement('input');
  passInput.type = 'password';
  passInput.className = 'setting-input';
  passInput.placeholder = 'Leave blank if no password';
  passInput.value = settings.obsPassword || '';
  passInput.addEventListener('change', () => { settings.obsPassword = passInput.value || ''; saveSettingsPatch({ obsPassword: settings.obsPassword }); });
  passGroup.appendChild(passInput);
  host.appendChild(passGroup);

  const srcGroup = detailGroup('Text Source Name');
  const srcInput = document.createElement('input');
  srcInput.type = 'text';
  srcInput.className = 'setting-input';
  srcInput.placeholder = 'Scripture';
  srcInput.value = settings.obsTextSource || '';
  srcInput.addEventListener('change', () => { settings.obsTextSource = srcInput.value || 'Scripture'; saveSettingsPatch({ obsTextSource: settings.obsTextSource }); });
  srcGroup.appendChild(srcInput);
  host.appendChild(srcGroup);

  const testRow = document.createElement('div');
  testRow.style.cssText = 'display:flex;align-items:center;gap:8px;';
  const testBtn = document.createElement('button');
  testBtn.type = 'button';
  testBtn.className = 'modal-btn secondary';
  testBtn.textContent = 'Test';
  const testStatusEl = document.createElement('span');
  testStatusEl.style.cssText = 'font-size:11px;color:var(--text-2);';
  testBtn.addEventListener('click', () => testObsConnection(testStatusEl, testBtn));
  testRow.appendChild(testBtn); testRow.appendChild(testStatusEl);
  host.appendChild(testRow);

  const hint = document.createElement('p');
  hint.className = 'setting-hint';
  hint.style.marginTop = '8px';
  hint.textContent = 'For full Slide/Media/Timer parity in OBS, add a Syphon (same machine) or NDI (network) source instead — see the NDI/Syphon outputs above.';
  host.appendChild(hint);
}

// NDI/Syphon detail panel — name, network source name, resolution,
// enable toggle, theme, layers, remove. Owner: "have an add button at the
// top, so the user can define what kind of output they want with screen
// resolution" — width/height live here, editable any time, not just at
// creation (the "add" flow just seeds sensible 1920x1080 defaults).
// NDI with no runtime on this computer: Kairo fetches NDI's own free runtime
// from NDI (kairo-server ndi_runtime.rs — signature-checked, no installer on a
// Mac), then turns the output on. One click; on Windows, one permission prompt.
function buildNdiRuntimeOffer(o, invokeFn) {
  const box = document.createElement('div');
  box.className = 'ndi-offer';
  const lead = document.createElement('p');
  lead.innerHTML = '<strong>NDI needs a one-time download.</strong> Kairo fetches NDI’s free runtime straight from NDI (about 5 MB) — nothing else to install.';
  const fine = document.createElement('p');
  fine.className = 'setting-hint';
  fine.innerHTML = 'NDI® is a registered trademark of Vizrt NDI AB. Downloading accepts NDI’s licence terms (<a href="https://ndi.video" target="_blank" rel="noopener">ndi.video</a>).';
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'modal-btn primary';
  btn.textContent = 'Download NDI and turn on';
  const note = document.createElement('span');
  note.className = 'setting-hint ndi-offer-note';
  box.append(lead, fine, btn, note);

  fetch(`${SERVER}/api/ndi/runtime`).then(r => r.json()).then(st => {
    if (!st.available) {
      btn.hidden = true;
      lead.innerHTML = '<strong>NDI isn’t installed.</strong> Install the free NDI runtime from <a href="https://ndi.video/tools/" target="_blank" rel="noopener">ndi.video</a>, then turn this output on.';
    } else if (st.platform === 'windows') {
      note.textContent = 'Windows will ask for permission once.';
    }
  }).catch(() => {});

  btn.addEventListener('click', async () => {
    btn.disabled = true;
    btn.textContent = 'Downloading…';
    note.textContent = '';
    try {
      await fetch(`${SERVER}/api/ndi/runtime/install`, { method: 'POST' });
      let st;
      for (let i = 0; i < 600; i++) {
        await new Promise(r => setTimeout(r, 500));
        st = await (await fetch(`${SERVER}/api/ndi/runtime`)).json();
        if (st.percent != null && st.running) btn.textContent = `Downloading… ${st.percent}%`;
        if (!st.running) break;
      }
      if (st?.error) throw new Error(st.error);
      btn.textContent = 'Turning on…';
      await nativeStart(invokeFn, 'ndi', { id: o.id, sourceName: o.raw.sourceName || 'KAIRO Scripture', width: o.raw.width || 1920, height: o.raw.height || 1080 });
      updateNativeOutput('ndi', o.id, { enabled: true, lastError: null });
      window.KairoNativeOutputs?.pushToOne?.('ndi', o.id);
    } catch (e) {
      btn.disabled = false;
      btn.textContent = 'Try again';
      note.textContent = String(e?.message || e).replace(/^Error:\s*/, '');
    }
    renderOutputsList();
    if (selectedOutputId === o.id) renderOutputsDetail();
  });
  return box;
}

function renderNativeOutputDetail(host, o) {
  const kind = o.type; // 'ndi' | 'syphon'
  const raw = o.raw;
  const invokeFn = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;

  const statusEl = document.createElement('span');
  statusEl.className = 'native-output-status';
  statusEl.classList.toggle('is-active', !!raw.enabled);
  const statusDot = document.createElement('span');
  statusDot.className = 'bs-dot' + (raw.enabled ? ' connected' : '');
  const statusText = document.createElement('span');
  statusText.textContent = raw.enabled ? 'Broadcasting' : (raw.lastError || 'Off');
  if (!raw.enabled && raw.lastError) {
    statusText.classList.add('output-status-error');
    // Long failure messages ("NDI runtime not found...") don't fit on the
    // same line as the title+toggle — wrap them onto their own full-width
    // line instead of overflowing past the panel's edge (see .has-error).
    statusEl.classList.add('has-error');
  }
  statusEl.appendChild(statusDot);
  statusEl.appendChild(statusText);
  // On/off toggle now lives in the header (owner: "toggle should be on
  // the far right of each output header") — the body's old separate
  // Start/Stop button is gone, buildOutputRowToggle's ndi/syphon case is
  // the one real implementation, shared with the list row's own toggle.
  host.appendChild(detailTitle(o.name, statusEl, buildOutputRowToggle(o)));
  if (kind === 'ndi' && !raw.enabled && /runtime not found/i.test(raw.lastError || '')) {
    host.appendChild(buildNdiRuntimeOffer(o, invokeFn));
  }

  const nameGroup = detailGroup('Label (Settings only)');
  const nameInput = document.createElement('input');
  nameInput.type = 'text'; nameInput.className = 'setting-input'; nameInput.value = raw.name || o.name;
  nameInput.addEventListener('change', () => {
    updateNativeOutput(kind, o.id, { name: nameInput.value.trim() || raw.name });
    renderOutputsList();
  });
  nameGroup.appendChild(nameInput);
  host.appendChild(nameGroup);

  const srcGroup = detailGroup('Network Source Name');
  const srcInput = document.createElement('input');
  srcInput.type = 'text'; srcInput.className = 'setting-input'; srcInput.value = raw.sourceName || 'KAIRO Scripture';
  srcGroup.appendChild(srcInput);
  host.appendChild(srcGroup);

  const resGroup = detailGroup('Resolution');
  const resRow = document.createElement('div');
  resRow.style.cssText = 'display:flex;align-items:center;gap:6px;';
  const wInput = document.createElement('input');
  wInput.type = 'number'; wInput.className = 'setting-input'; wInput.min = '160'; wInput.max = '7680';
  wInput.value = raw.width || 1920; wInput.style.width = '90px';
  const xEl = document.createElement('span'); xEl.textContent = '×'; xEl.style.color = 'var(--text-3)';
  const hInput = document.createElement('input');
  hInput.type = 'number'; hInput.className = 'setting-input'; hInput.min = '90'; hInput.max = '4320';
  hInput.value = raw.height || 1080; hInput.style.width = '90px';
  resRow.appendChild(wInput); resRow.appendChild(xEl); resRow.appendChild(hInput);
  resGroup.appendChild(resRow);
  host.appendChild(resGroup);

  // Restart-with-new-settings debounce — the network name AND resolution
  // are only read at start() time (native texture/frame allocation), so
  // changing either while broadcasting means stop+start, same as before.
  let debounce = null;
  function scheduleRestart() {
    updateNativeOutput(kind, o.id, {
      sourceName: srcInput.value.trim() || 'KAIRO Scripture',
      width: Math.max(160, Number(wInput.value) || 1920),
      height: Math.max(90, Number(hInput.value) || 1080),
    });
    if (!raw.enabled) return;
    clearTimeout(debounce);
    debounce = setTimeout(async () => {
      if (!invokeFn) return;
      try { await nativeStop(invokeFn, kind, o.id); } catch {}
      try {
        await nativeStart(invokeFn, kind, {
          id: o.id, sourceName: srcInput.value.trim() || 'KAIRO Scripture',
          width: Math.max(160, Number(wInput.value) || 1920), height: Math.max(90, Number(hInput.value) || 1080),
        });
      } catch {}
    }, 600);
  }
  srcInput.addEventListener('input', scheduleRestart);
  wInput.addEventListener('change', scheduleRestart);
  hInput.addEventListener('change', scheduleRestart);

  host.appendChild(detailInlineGroup('Theme', buildThemeSelect(o.id)));

  appendOutputLayersSection(host, o.id);


  const del = document.createElement('button');
  del.type = 'button';
  del.className = 'modal-btn secondary display-output-del';
  del.style.marginTop = '4px';
  del.textContent = `Remove this ${outputTypeLabel(kind)} output`;
  del.addEventListener('click', async () => {
    if (invokeFn && raw.enabled) { try { await nativeStop(invokeFn, kind, o.id); } catch {} }
    const list = nativeOutputsOf(kind).filter(x => x.id !== o.id);
    settings[`${kind}Outputs`] = list;
    const themes = { ...outputThemeMap() }; delete themes[o.id];
    const layers = { ...outputLayerMap() }; delete layers[o.id];
    settings.outputThemes = themes; settings.outputLayers = layers;
    saveSettingsPatch({ [`${kind}Outputs`]: list, outputThemes: themes, outputLayers: layers });
    selectedOutputId = PRIMARY_DISPLAY;
    applyOutputThemes(); applyOutputLayers();
    renderOutputsList(); renderOutputsDetail();
  });
  host.appendChild(del);
}

// A Blackmagic DeckLink output: card, video format, keying, then the same
// theme/layers as any output. Frames are the output's captured theme
// (capture.rs), played at the format's own rate by decklink.rs.
let decklinkCache = null;
async function decklinkCards(refresh) {
  if (decklinkCache && !refresh) return decklinkCache;
  const invokeFn = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
  try { decklinkCache = invokeFn ? await invokeFn('decklink_list') : { ok: false, error: 'DeckLink works in the Kairo app.' }; }
  catch (e) { decklinkCache = { ok: false, error: String(e?.message || e) }; }
  return decklinkCache;
}

function renderDeckLinkDetail(host, o) {
  const raw = o.raw;
  const invokeFn = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
  const statusEl = document.createElement('span');
  statusEl.className = 'native-output-status';
  const dot = document.createElement('span');
  dot.className = 'bs-dot' + (raw.enabled ? ' connected' : '');
  const statusText = document.createElement('span');
  statusText.textContent = raw.enabled ? `On air · ${raw.modeName || ''}` : (raw.lastError || 'Off');
  if (!raw.enabled && raw.lastError) { statusText.classList.add('output-status-error'); statusEl.classList.add('has-error'); }
  statusEl.append(dot, statusText);
  host.appendChild(detailTitle(o.name, statusEl, buildOutputRowToggle(o)));

  const cardsBox = document.createElement('div');
  host.appendChild(cardsBox);
  cardsBox.textContent = 'Looking for DeckLink cards…';

  // Changing card/format/keying while on air restarts the output.
  const restartIfLive = async () => {
    const cur = decklinkOutputs().find(x => x.id === o.id);
    if (!cur?.enabled || !invokeFn) return;
    try { await nativeStop(invokeFn, 'decklink', o.id); } catch {}
    try { await nativeStart(invokeFn, 'decklink', { id: o.id }); updateNativeOutput('decklink', o.id, { lastError: null }); }
    catch (e) { updateNativeOutput('decklink', o.id, { enabled: false, lastError: String(e?.message || e) }); }
    renderOutputsList(); renderOutputsDetail();
  };

  decklinkCards().then((info) => {
    cardsBox.innerHTML = '';
    if (!info?.ok || !(info.devices || []).length) {
      const p = document.createElement('p');
      p.className = 'setting-hint';
      p.innerHTML = info?.ok
        ? 'No DeckLink card found. Connect one (and install <a href="https://www.blackmagicdesign.com/support/family/capture-and-playback" target="_blank" rel="noopener">Blackmagic Desktop Video</a>), then '
        : `${escapeHtml(info?.error || 'DeckLink is unavailable.')} Install <a href="https://www.blackmagicdesign.com/support/family/capture-and-playback" target="_blank" rel="noopener">Blackmagic Desktop Video</a>, then `;
      const again = document.createElement('button');
      again.type = 'button'; again.className = 'modal-btn secondary dl-btn'; again.textContent = 'look again';
      again.addEventListener('click', () => { decklinkCache = null; renderOutputsDetail(); });
      p.appendChild(again);
      cardsBox.appendChild(p);
      return;
    }
    const devices = info.devices;
    const dev = devices.find(d => d.index === raw.device) || devices[0];

    const cardGroup = detailGroup('Card');
    const cardSel = document.createElement('select');
    cardSel.className = 'setting-input';
    devices.forEach(d => { const op = document.createElement('option'); op.value = d.index; op.textContent = d.name; cardSel.appendChild(op); });
    cardSel.value = String(dev.index);
    cardGroup.appendChild(cardSel);
    cardsBox.appendChild(cardGroup);

    const modeGroup = detailGroup('Video format');
    const modeSel = document.createElement('select');
    modeSel.className = 'setting-input';
    const fillModes = (d) => {
      modeSel.innerHTML = '';
      const none = document.createElement('option'); none.value = ''; none.textContent = 'Choose a format…'; modeSel.appendChild(none);
      (d.modes || []).forEach(m => { const op = document.createElement('option'); op.value = m.id; op.textContent = `${m.name} (${m.width}×${m.height})`; modeSel.appendChild(op); });
      const cur = decklinkOutputs().find(x => x.id === o.id);
      modeSel.value = cur && cur.device === d.index && cur.mode != null ? String(cur.mode) : '';
    };
    fillModes(dev);
    modeGroup.appendChild(modeSel);
    const modeHint = document.createElement('p');
    modeHint.className = 'setting-hint';
    modeHint.textContent = 'Match your switcher or router (e.g. 1080p59.94, 1080i50).';
    modeGroup.appendChild(modeHint);
    cardsBox.appendChild(modeGroup);

    const keyGroup = document.createElement('div');
    keyGroup.className = 'setting-group setting-checks';
    const keyLabel = document.createElement('label');
    keyLabel.className = 'bs-check-label';
    const keyBox = document.createElement('input');
    keyBox.type = 'checkbox';
    keyBox.checked = !!raw.keying;
    const keyText = document.createElement('span');
    keyText.innerHTML = '<strong>Fill and key</strong><span class="setting-hint">The card puts out the graphics and a separate key signal, so a vision mixer can lay lower thirds over camera. Set the mixer’s key to pre-multiplied. Give this output a theme with a transparent background.</span>';
    keyLabel.append(keyBox, keyText);
    keyGroup.appendChild(keyLabel);
    const setKeyAvailable = (d) => {
      keyGroup.hidden = !d.keying;
      if (!d.keying && keyBox.checked) { keyBox.checked = false; updateNativeOutput('decklink', o.id, { keying: false }); }
    };
    setKeyAvailable(dev);
    cardsBox.appendChild(keyGroup);

    const saveMode = (d) => {
      const m = (d.modes || []).find(x => String(x.id) === modeSel.value);
      updateNativeOutput('decklink', o.id, m
        ? { device: d.index, deviceName: d.name, mode: m.id, modeName: m.name, width: m.width, height: m.height, fps: m.scale / m.duration }
        : { device: d.index, deviceName: d.name, mode: null, modeName: null });
    };
    cardSel.addEventListener('change', () => {
      const d = devices.find(x => String(x.index) === cardSel.value) || devices[0];
      fillModes(d); setKeyAvailable(d); saveMode(d); restartIfLive();
    });
    modeSel.addEventListener('change', () => {
      const d = devices.find(x => String(x.index) === cardSel.value) || devices[0];
      saveMode(d); restartIfLive();
    });
    keyBox.addEventListener('change', () => { updateNativeOutput('decklink', o.id, { keying: keyBox.checked }); restartIfLive(); });
  });

  host.appendChild(detailInlineGroup('Theme', buildThemeSelect(o.id)));
  appendOutputLayersSection(host, o.id);

  const del = document.createElement('button');
  del.type = 'button';
  del.className = 'modal-btn secondary display-output-del';
  del.style.marginTop = '4px';
  del.textContent = 'Remove this DeckLink output';
  del.addEventListener('click', async () => {
    if (invokeFn && raw.enabled) { try { await nativeStop(invokeFn, 'decklink', o.id); } catch {} }
    const list = decklinkOutputs().filter(x => x.id !== o.id);
    const themes = { ...outputThemeMap() }; delete themes[o.id];
    const layers = { ...outputLayerMap() }; delete layers[o.id];
    settings.decklinkOutputs = list; settings.outputThemes = themes; settings.outputLayers = layers;
    saveSettingsPatch({ decklinkOutputs: list, outputThemes: themes, outputLayers: layers });
    selectedOutputId = PRIMARY_DISPLAY;
    applyOutputThemes(); applyOutputLayers();
    renderOutputsList(); renderOutputsDetail();
  });
  host.appendChild(del);
}

function updateNativeOutput(kind, id, patch) {
  const list = nativeOutputsOf(kind).map(x => x.id === id ? { ...x, ...patch } : x);
  settings[`${kind}Outputs`] = list;
  saveSettingsPatch({ [`${kind}Outputs`]: list });
  if ('enabled' in patch && typeof refreshMonitor === 'function') refreshMonitor();
}

// "+ Add Output" — a small type-chooser popover (mirrors the existing
// .svc-popover pattern used elsewhere, e.g. the Add-to-playlist menu)
// instead of one separate "+Add" button per output type.
function wireOutputsAddMenu() {
  const btn = document.getElementById('outputs-add-btn');
  const menu = document.getElementById('outputs-add-menu');
  if (!btn || !menu || btn.dataset.wired) return;
  btn.dataset.wired = '1';
  // Same-machine sender option: label/hint follow the OS (Syphon on macOS,
  // Spout on Windows); neither exists on Linux yet, so the option itself is
  // hidden there rather than offering something that can't work.
  const syphonBtn = document.getElementById('add-output-syphon-btn');
  if (syphonBtn) {
    const isWin = document.documentElement.classList.contains('platform-win32');
    const isMac = document.documentElement.classList.contains('platform-darwin');
    if (!isWin && !isMac) {
      syphonBtn.remove();
    } else {
      const label = nativeSenderLabel();
      syphonBtn.firstChild.textContent = label;
      syphonBtn.title = `${label} is ${isWin ? 'Windows' : 'macOS'}-only`;
      const hint = syphonBtn.querySelector('.svc-popover-item-hint');
      if (hint) hint.textContent = isWin ? 'Windows only' : 'macOS only';
    }
  }
  btn.addEventListener('click', () => {
    if (!menu.classList.contains('hidden')) { menu.classList.add('hidden'); return; }
    const r = btn.getBoundingClientRect();
    menu.style.top = `${r.bottom + 4}px`;
    menu.style.left = `${r.left}px`;
    menu.classList.remove('hidden');
  });
  document.addEventListener('click', (e) => {
    if (!menu.classList.contains('hidden') && !menu.contains(e.target) && !btn.contains(e.target)) {
      menu.classList.add('hidden');
    }
  });
  menu.querySelectorAll('[data-add-type]').forEach(item => {
    item.addEventListener('click', () => {
      const type = item.dataset.addType;
      menu.classList.add('hidden');
      if (type === 'display') {
        const list = extraDisplays();
        const next = [...list, { id: `display-${Date.now().toString(36)}`, name: `Display ${list.filter(d => d.kind !== 'monitor').length + 2}` }];
        settings.extraDisplays = next;
        saveSettingsPatch({ extraDisplays: next });
        selectedOutputId = next[next.length - 1].id;
        applyOutputThemes();
      } else if (type === 'monitor') {
        const list = extraDisplays();
        const n = list.filter(d => d.kind === 'monitor').length;
        const next = [...list, { id: genOutputId('monitor'), name: n ? `Monitor ${n + 1}` : 'Monitor', kind: 'monitor' }];
        settings.extraDisplays = next;
        saveSettingsPatch({ extraDisplays: next });
        selectedOutputId = next[next.length - 1].id;
        applyOutputModes();
      } else if (type === 'decklink') {
        const list = decklinkOutputs();
        const next = [...list, { id: genOutputId('decklink'), name: list.length ? `DeckLink ${list.length + 1}` : 'DeckLink', device: 0, mode: null, keying: false, enabled: false }];
        settings.decklinkOutputs = next;
        saveSettingsPatch({ decklinkOutputs: next });
        selectedOutputId = next[next.length - 1].id;
      } else {
        const outputsFn = type === 'ndi' ? ndiOutputs : syphonOutputs;
        const list = outputsFn();
        const next = [...list, { id: genOutputId(type), name: `${outputTypeLabel(type)} Output ${list.length + 1}`, sourceName: 'KAIRO Scripture', width: 1920, height: 1080, enabled: false }];
        settings[`${type}Outputs`] = next;
        saveSettingsPatch({ [`${type}Outputs`]: next });
        selectedOutputId = next[next.length - 1].id;
      }
      renderOutputsList();
      renderOutputsDetail();
    });
  });
}

// Report whether a second screen is actually attached, so the operator knows
// whether "Open" will land on a projector or just stack on this monitor.
// Also refreshes cachedScreens from the Window Management API when
// available, which the per-output screen pickers (upsertPrimaryScreenPicker,
// buildScreenSelect) read synchronously once this resolves — resolution
// itself is no longer captured here; each output's screen is chosen
// explicitly instead (see outputScreenMap).
// Populates cachedScreens (read by populateScreenOptions/buildScreenSelect
// for the monitor pickers). Real incident: this used to bail out entirely
// via an early `if (!hint) return` guarding a status-text element that got
// deleted from the External Display card during its redesign to the
// monitor-picker layout — cachedScreens silently never populated again,
// no error anywhere, the picker just always showed "None" regardless of
// how many real displays were connected. Removed the dependency outright
// rather than re-adding UI the new minimal design doesn't need — the
// header status dot/text and the picker's own option list already say
// everything this used to render into a separate hint line.
async function refreshDisplayStatus() {
  // Real OS-level enumeration first — the Window Management API
  // (getScreenDetails/isExtended) this used to rely on exclusively is
  // Chromium-only and WebKit has never implemented it, so it silently
  // never detected anything in the packaged macOS app (see list_monitors
  // in src-tauri/src/lib.rs for the full story). Only fall back to the
  // web APIs when not running inside Tauri at all (e.g. testing app.js
  // directly in a plain browser tab).
  const tauriInvoke = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
  let detected = null;
  if (tauriInvoke) {
    try {
      const screens = await tauriInvoke('list_monitors');
      if (Array.isArray(screens) && screens.length) {
        cachedScreens = screens;
        detected = screens.length;
      }
    } catch (err) { console.warn('[KAIRO] list_monitors failed:', err); }
  }
  if (detected == null) {
    try {
      if (window.getScreenDetails) {
        const d = await window.getScreenDetails();
        const screens = d.screens || [];
        cachedScreens = screens.map((s, i) => ({
          index: i, width: s.width, height: s.height, left: s.left, top: s.top, isPrimary: !!s.isPrimary,
        }));
        detected = screens.length;
      } else if (typeof window.screen?.isExtended === 'boolean') {
        detected = window.screen.isExtended ? 2 : 1;
      }
    } catch { /* permission denied — leave cachedScreens as last known */ }
  }
}

// Open a window for one screen. It identifies itself via ?output= so it renders
// with that screen's assigned theme. The explicitly-assigned physical screen
// (outputScreenMap) always wins over the old d.screen/window.screen guesses,
// which only apply when nothing's been assigned yet.
function openDisplayOutput(d) {
  const url = `/display.html?output=${encodeURIComponent(d.id)}`;
  const assigned = outputScreenMap()[d.id];
  const scr = assigned || d.screen || {};
  const w = scr.width  || window.screen.width;
  const h = scr.height || window.screen.height;
  const x = scr.left   != null ? scr.left : window.screen.width;
  const y = scr.top    != null ? scr.top  : 0;
  if (typeof openDisplayWindow === 'function') {
    const label = `kairo-${d.id}`;
    openDisplayWindow(label, url, { width: w, height: h, x, y, fullscreen: true });
    verifyDisplayWindowOpened(label);
  } else {
    // This exact fallback — silently opening the system browser instead
    // of a real positioned output window — was the actual root cause
    // behind an entire session of "nothing sends to the display" reports
    // (openDisplayWindow existed but wasn't attached to `window` yet).
    // The underlying bug is fixed, but the fallback itself still needs to
    // never be silent again if it's ever hit for some other reason.
    logDisplayLifecycleFallback('openDisplayOutput', { outputId: d.id, reason: 'openDisplayWindow not a function — falling back to window.open()' });
    window.open(url, `kairo-${d.id}`, `width=${w},height=${h},left=${x},top=${y}`);
  }
}

// A targeted assertion on exactly the operation that broke 5 times this
// session, not a general health-check subsystem: give the window a
// second to actually come up, then confirm it did. Every prior bug here
// left an operator finding out mid-service, from an empty screen, that
// nothing had actually opened — this surfaces it immediately instead.
async function verifyDisplayWindowOpened(label) {
  const getWin = window.__TAURI__?.webviewWindow?.WebviewWindow;
  if (!getWin) return; // not running inside Tauri (plain dev preview) — nothing to verify
  await new Promise(r => setTimeout(r, 1000));
  try {
    const win = await getWin.getByLabel(label);
    const visible = win ? await win.isVisible().catch(() => null) : null;
    if (!win || visible === false) {
      logDisplayLifecycleFallback('verifyDisplayWindowOpened', { label, exists: !!win, visible });
      toast('Display window may not have opened — check the monitor picker in Settings', 'error');
    }
  } catch (err) {
    logDisplayLifecycleFallback('verifyDisplayWindowOpened threw', { label, error: String(err) });
  }
}

// ── Monitor outputs, and Text only ─────────────────────────────────────────
// A Monitor is its own output, added from the Outputs list (owner, 2026-10-01:
// "a confidence monitor ... Caters to the minister up front. just like
// propresenter stage output" — "stage should be an output option, in output",
// "call it Monitor, not stage"): a screen showing what's on screen now with
// the next slide right under it, the time, the segment countdown and messages
// from Monitor message under the preview — words only (display.html's
// confidence view). Kept with the extra displays (kind: 'monitor'): a screen
// and a window like any display.
// Text only (Output Control): a display shows just its theme's words — no
// background, shapes, pictures or media.
function isMonitorOutput(id) { return extraDisplays().some(d => d.id === id && d.kind === 'monitor'); }
function monitorOutputs() { return extraDisplays().filter(d => d.kind === 'monitor'); }
function outputTextOnlyMap() {
  const src = settings.outputTextOnly && typeof settings.outputTextOnly === 'object' ? settings.outputTextOnly : {};
  const map = {};
  for (const d of displayOutputs()) if (d.kind !== 'monitor') map[d.id] = src[d.id] === true;
  return map;
}
function setOutputTextOnly(outputId, on) {
  settings.outputTextOnly = { ...outputTextOnlyMap(), [outputId]: !!on };
  saveSettingsPatch({ outputTextOnly: settings.outputTextOnly });
  applyOutputModes();
  renderOutputsList();   // the Output Control matrix too
  if (selectedOutputId === outputId) {
    const cb = document.querySelector('.output-layer-checks input[data-text-only]');
    if (cb && cb.checked !== !!on) cb.checked = !!on;
    const hint = document.querySelector('.outputs-layers-hint');
    if (hint) hint.textContent = buildOutputLayersHint(outputId).textContent;
  }
}
// What each display window renders (display.html's outputMode).
function outputModeMap() {
  const textOnly = outputTextOnlyMap();
  const map = {};
  for (const d of displayOutputs()) map[d.id] = d.kind === 'monitor' ? 'confidence' : textOnly[d.id] ? 'text' : 'theme';
  return map;
}
async function applyOutputModes() {
  const map = outputModeMap();
  localStorage.setItem('kairo-output-modes', JSON.stringify(map));
  refreshMonitorMessageBar();
  try {
    await fetch(`${SERVER}/api/look/apply`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ modes: map }),
    });
  } catch (err) {
    console.warn('[Look] per-output mode broadcast failed:', err.message);
  }
}

// ── Monitor message — a line from the booth to the Monitor outputs ─────────
// Under the preview while there is a Monitor output.
function refreshMonitorMessageBar() {
  document.getElementById('monitor-message-bar')?.classList.toggle('hidden', !monitorOutputs().length);
}
function showMonitorMessageState(text) {
  const bar = document.getElementById('monitor-message-bar');
  if (!bar) return;
  const live = !!String(text || '').trim();
  bar.classList.toggle('is-live', live);
  const clear = document.getElementById('monitor-message-clear');
  if (clear) clear.disabled = !live;
  const send = document.getElementById('monitor-message-send');
  if (send) {
    send.title = live ? 'Update the message on the Monitor' : 'Show on the Monitor';
    send.setAttribute('aria-label', send.title);
  }
  // What's up on the Monitor stays in view (and editable) — unless being typed over.
  const input = document.getElementById('monitor-message-input');
  if (input && document.activeElement !== input) input.value = live ? String(text).trim() : '';
}
async function sendMonitorMessage(text) {
  try {
    await fetch(`${SERVER}/api/monitor-message`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text }),
    });
  } catch (err) {
    console.warn('[Monitor] message failed:', err.message);
  }
}
(function wireMonitorMessageBar() {
  const input = document.getElementById('monitor-message-input');
  const send = document.getElementById('monitor-message-send');
  const clear = document.getElementById('monitor-message-clear');
  if (!input || !send || !clear) return;
  const show = () => { const t = input.value.trim(); if (t) sendMonitorMessage(t); };
  send.addEventListener('click', show);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); show(); }
  });
  clear.addEventListener('click', () => { sendMonitorMessage(''); input.value = ''; });
})();

// ── Keying — how a display output's background keys out for a mixer ───────
// Each display window's own choice; the theme never carries it, so one theme
// serves the projector and the stream overlay alike. None shows the theme's
// background; Transparent makes the window see-through (real alpha); Chroma
// key shows pure green and Luma key black where the background would be, for
// OBS or vMix to key out from a plain window capture. NDI and Syphon need
// none: they already send the content on real alpha.
const KEYING_OPTIONS = [
  ['none', "None — the theme's background"],
  ['alpha', 'Transparent'],
  ['chroma', 'Chroma key (green)'],
  ['luma', 'Luma key (black)'],
];
function outputKeyingMap() {
  const src = settings.outputKeying && typeof settings.outputKeying === 'object' ? settings.outputKeying : {};
  const map = {};
  for (const d of displayOutputs()) map[d.id] = KEYING_OPTIONS.some(([k]) => k === src[d.id]) ? src[d.id] : 'none';
  return map;
}
// Same dual delivery as the output layers: localStorage for the windows on
// this machine, and a look-update broadcast.
async function applyOutputKeying() {
  const map = outputKeyingMap();
  localStorage.setItem('kairo-output-keying', JSON.stringify(map));
  try {
    await fetch(`${SERVER}/api/look/apply`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ keying: map }),
    });
  } catch (err) {
    console.warn('[Look] per-output keying broadcast failed:', err.message);
  }
}
function buildKeyingSelect(outputId) {
  const sel = document.createElement('select');
  sel.className = 'setting-input';
  sel.title = 'Transparent needs a capture that keeps alpha; Chroma or Luma key works with a plain window capture in OBS or vMix.';
  const current = outputKeyingMap()[outputId];
  KEYING_OPTIONS.forEach(([value, label]) => sel.add(new Option(label, value, false, value === current)));
  sel.addEventListener('change', () => {
    settings.outputKeying = { ...outputKeyingMap(), [outputId]: sel.value };
    saveSettingsPatch({ outputKeying: settings.outputKeying });
    applyOutputKeying();
  });
  return sel;
}

async function applyOutputThemes() {
  const map = outputThemeMap();
  const themes = {};
  for (const [key, id] of Object.entries(map)) {
    const look = looks.find(l => l.id === id);
    if (look) themes[key] = look;
  }
  // Same-origin display windows pick this up via the storage event.
  localStorage.setItem('kairo-output-themes', JSON.stringify(themes));
  localStorage.setItem('kairo-active-look-ts', Date.now().toString());
  // Keep the legacy single-look key in sync so any display window opened
  // without an ?output= tag still shows the primary screen's theme.
  const primary = themes[displayOutputs()[0]?.id] || themes.external;
  if (primary) localStorage.setItem('kairo-active-look', JSON.stringify(primary));

  try {
    await fetch(`${SERVER}/api/look/apply`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ themes, look: primary || null }),
    });
  } catch (err) {
    console.warn('[Look] per-output broadcast failed:', err.message);
  }

  // Whatever's already showing in the in-app preview only carries its own
  // look when the sent item had one — anything using the output-default
  // fallback (renderPreviewScreen's primaryOutputLook()) is now stale the
  // instant the assignment changes. Re-paint it immediately instead of
  // leaving the operator staring at yesterday's theme until the next send.
  repaintPreviewWithOutputLook();
}

// ── Output Looks — per-output Bible/Slide/Media/Timer visibility ───────────
// "a user can send only media content to a specific screen or only timer"
// (owner, referencing ProPresenter's own Edit Looks panel). Mirrors
// outputThemeMap/applyOutputThemes/renderOutputThemePickers exactly — same
// settings-object shape family, same localStorage+storage-event delivery to
// display.html, same /api/look/apply broadcast, same per-card UI injection.
// Any output missing from settings.outputLayers (or missing one of the four
// keys) defaults to ALL FOUR true — a fresh install, or any output nobody
// has touched yet, behaves exactly like today, zero config needed.
//
// Bible vs. Slide split (owner: "the output that are just rendered under
// each display... should be bible, slide, media" — a plain Slides-tab
// slide and a Bible verse used to share one "slide" layer/checkbox, so
// there was no way to show scripture on an output while hiding
// announcement slides, or vice versa). No new plumbing needed for the
// distinction — display.html already receives real book/chapter/verse
// fields on actual scripture (see attachBibleTranslations' own
// `!v.book || !v.chapter || !v.verse` check, the same signal reused here)
// and never on a plain slide, so classification is free. Bible listed
// first/takes precedence over Slide when a message could ever be read as
// either — owner: "by their order, bible supersedes because it's the top."
// Single source of truth for the four layer keys/labels — was written out
// as an identical array literal in four separate places (buildOutputLayers
// Summary/Hint/ChecksRow, plus the Output Control matrix), and the
// all-true fallback object was a fourth near-duplicate literal on top of
// that. One name for each, used everywhere, so a fifth layer added later
// can't be added to three of the four sites and forgotten in the fourth.
// Owner's own priority order — Bible is the flagship feature and always
// takes precedence, Slide (which Songs also renders through) sits right
// under it, then Timer, then Media last: "I can clear timer and not
// necessarily want to clear my media." Order here drives every checkbox
// row, the Output Control matrix's row order, and the layers summary —
// one list, so all three can't drift out of sync with each other again.
// Video input (live camera / capture card, see video_sources.js) is the fifth
// layer, drawn UNDER the rest so scripture and slides sit over the picture. It
// is the one layer that defaults OFF: nothing changes until it is turned on.
const OUTPUT_LAYER_KEYS = [['bible', 'Bible'], ['slide', 'Slide'], ['timer', 'Timer'], ['media', 'Media'], ['video', 'Video input']];
const DEFAULT_OUTPUT_LAYERS = { bible: true, slide: true, media: true, timer: true, video: false };

// Video input goes to display windows and the Monitor only (Phase 1). NDI,
// Syphon/Spout and DeckLink render natively and don't carry it yet.
// Video input shows in the outputs UI only while Video director mode is on.
function visibleLayerKeys() {
  return settings.videoDirector ? OUTPUT_LAYER_KEYS : OUTPUT_LAYER_KEYS.filter(([k]) => k !== 'video');
}
const VIDEO_NATIVE_NOTE = 'Video input is not sent to NDI, Syphon or DeckLink outputs yet.';
function outputSupportsVideo(outputId) {
  return displayOutputs().some(d => d.id === outputId);
}

function outputLayerMap() {
  const src = settings.outputLayers && typeof settings.outputLayers === 'object' ? settings.outputLayers : {};
  const map = {};
  for (const key of [...allOutputKeys(), 'ndi', 'syphon']) {
    const entry = src[key] && typeof src[key] === 'object' ? src[key] : {};
    map[key] = {
      bible: entry.bible !== false,
      slide: entry.slide !== false,
      media: entry.media !== false,
      timer: entry.timer !== false,
      video: entry.video === true,
    };
  }
  return map;
}

// Compact "what's actually going out on this output" summary — Bible/Slide/
// Media/Timer, each dimmed+struck-through when that layer is off. Used
// both in the outputs list (one per row, so the full picture is visible
// without opening any detail panel), the monitor grid, and as a helper
// line at the top of the detail panel itself. A single shared builder so
// none of them can drift apart. `map` is an optional pre-computed
// outputLayerMap() result — every hot call site (the outputs list, the
// Output Control matrix, the monitor grid) now computes it once and
// passes it to every output's summary/hint instead of each one silently
// rebuilding the whole map again; a bare call still works standalone.
function buildOutputLayersSummary(outputId, map) {
  const want = (map || outputLayerMap())[outputId] || DEFAULT_OUTPUT_LAYERS;
  const el = document.createElement('span');
  el.className = 'outputs-layers-summary';
  el.title = 'What this output is currently showing';
  visibleLayerKeys().forEach(([key, label]) => {
    // Video input only shows up here once it is on, so every other output's
    // summary reads as it always did.
    if (key === 'video' && !(want.video && outputSupportsVideo(outputId))) return;
    const item = document.createElement('span');
    item.className = 'outputs-layers-summary-item' + (want[key] ? '' : ' is-off');
    item.textContent = label;
    el.appendChild(item);
  });
  return el;
}

// Plain-language version of the same summary, for the detail panel — owner:
// "include a helper text to let the user know what is going out to what
// display." Sits right under the Bible/Slide/Media/Timer checkboxes so the
// sentence updates the moment a checkbox does (caller re-renders this
// alongside buildLayerChecksRow, same element tree).
function buildOutputLayersHint(outputId, map) {
  const want = (map || outputLayerMap())[outputId] || DEFAULT_OUTPUT_LAYERS;
  // Words only (a Monitor, or Text only): media never goes out there.
  const wordsOnly = isMonitorOutput(outputId) || !!outputTextOnlyMap()[outputId];
  const noVideo = !settings.videoDirector || !outputSupportsVideo(outputId);
  const keys = visibleLayerKeys().filter(([k]) => !(wordsOnly && k === 'media') && !(noVideo && k === 'video'));
  const on  = keys.filter(([k]) => want[k]).map(([, l]) => l);
  // Video input is off by default and optional: it is not listed as hidden.
  const off = keys.filter(([k]) => !want[k] && k !== 'video').map(([, l]) => l);
  const hint = document.createElement('p');
  hint.className = 'setting-hint outputs-layers-hint';
  if (!off.length) hint.textContent = wordsOnly ? `Going out on this output: ${on.join(', ')}.` : `Everything is going out on this output: ${on.join(', ')}.`;
  else if (!on.length) hint.textContent = 'Nothing is going out on this output right now — every layer is hidden.';
  else hint.textContent = `Going out on this output: ${on.join(', ')}. Hidden: ${off.join(', ')}.`;
  if (wordsOnly && on.length) hint.textContent += isMonitorOutput(outputId) ? ' Words only.' : ' Text only — just the words: no background, shapes, pictures or media.';
  if (settings.videoDirector && !outputSupportsVideo(outputId)) hint.textContent += ` ${VIDEO_NATIVE_NOTE}`;
  return hint;
}

// Push the current per-output layer visibility to every display client —
// same dual delivery path as applyOutputThemes: localStorage (picked up
// instantly by every same-origin display.html window via its existing
// 'storage' listener) + a server broadcast (belt-and-suspenders, and the
// only path a non-webview output like NDI/Syphon can observe at all — see
// their own gating in wireNdiBridge below, which reads this same map
// directly rather than through a broadcast round-trip).
async function applyOutputLayers() {
  const map = outputLayerMap();
  localStorage.setItem('kairo-output-layers', JSON.stringify(map));
  localStorage.setItem('kairo-active-look-ts', Date.now().toString());
  try {
    await fetch(`${SERVER}/api/look/apply`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ layers: map }),
    });
  } catch (err) {
    console.warn('[Look] per-output layer broadcast failed:', err.message);
  }
}


// Builds one reusable Bible/Slide/Media/Timer checkbox row for a given
// output — used by every detail-panel renderer that has one (display/NDI/
// Syphon; OBS doesn't — see the Outputs master-detail redesign's own
// Non-goals).
// Single source of truth for "toggle one output's one layer" — was
// duplicated inline inside buildLayerChecksRow's own change handler; the
// Output Control matrix (renderOutputControlMatrix) needs the exact same
// save+apply+re-render sequence, so it's factored out here instead of a
// second near-copy of it.
function setOutputLayer(outputId, layerKey, value) {
  const current = outputLayerMap();
  current[outputId] = { ...current[outputId], [layerKey]: value };
  settings.outputLayers = current;
  saveSettingsPatch({ outputLayers: current });
  applyOutputLayers();
  if (layerKey === 'video' && typeof onVideoLayerChanged === 'function') onVideoLayerChanged();
  // Keep the list's own per-row summary (buildOutputLayersSummary) in sync
  // live — owner: "you cannot see all of the selected output per display"
  // was as much about staying current after a toggle as about being
  // visible at all. renderOutputsList also re-renders the Output Control
  // matrix itself (folded in there — see its own comment) so both stay
  // consistent from this one call.
  renderOutputsList();
  // Update the detail panel's plain-language hint in place, if it's open
  // on this same output — without rebuilding the checkboxes themselves
  // (that would interrupt whichever control the click is still bubbling
  // from).
  if (selectedOutputId === outputId) {
    const hint = document.querySelector('.outputs-layers-hint');
    if (hint) hint.textContent = buildOutputLayersHint(outputId).textContent;
    const cb = document.querySelector(`.output-layer-checks input[data-layer-key="${layerKey}"]`);
    if (cb && cb.checked !== value) cb.checked = value;
  }
}

function buildLayerChecksRow(outputId, map) {
  const row = document.createElement('div');
  row.className = 'output-layer-checks';
  const want = (map || outputLayerMap())[outputId];
  const monitor = isMonitorOutput(outputId);
  visibleLayerKeys().forEach(([layerKey, label]) => {
    const wrap = document.createElement('label');
    wrap.className = 'output-layer-check';
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = want[layerKey] !== false;
    cb.dataset.layerKey = layerKey;
    if (monitor && layerKey === 'media') { cb.checked = false; cb.disabled = true; wrap.title = 'A Monitor shows words only'; }
    else if (layerKey === 'video' && !outputSupportsVideo(outputId)) { cb.checked = false; cb.disabled = true; wrap.title = VIDEO_NATIVE_NOTE; }
    else cb.addEventListener('change', () => setOutputLayer(outputId, layerKey, cb.checked));
    wrap.appendChild(cb);
    wrap.appendChild(document.createTextNode(label));
    row.appendChild(wrap);
  });
  // Text only — display outputs (NDI/Syphon render natively and don't offer
  // it; a Monitor is always words only).
  if (displayOutputs().some(d => d.id === outputId)) {
    const wrap = document.createElement('label');
    wrap.className = 'output-layer-check';
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.dataset.textOnly = '1';
    if (monitor) { cb.checked = true; cb.disabled = true; wrap.title = 'A Monitor shows words only'; }
    else {
      cb.checked = !!outputTextOnlyMap()[outputId];
      wrap.title = 'Just the words — no background, shapes, pictures or media';
      cb.addEventListener('change', () => setOutputTextOnly(outputId, cb.checked));
    }
    wrap.appendChild(cb);
    wrap.appendChild(document.createTextNode('Text only'));
    row.appendChild(wrap);
  }
  return row;
}

// Was copy-pasted (identical 4 lines + identical comment) between
// renderDisplayDetail and renderNativeOutputDetail — one helper instead,
// called from both.
function appendOutputLayersSection(host, outputId, map) {
  const layerGroup = detailGroup('Show on this output');
  // Owner: "maybe create a separate section for users to customize their
  // output, don't cluster it with the main output control" — visually set
  // apart from Name/Physical Screen/Theme above, not just another row in
  // the same stack.
  layerGroup.classList.add('output-layers-section');
  layerGroup.appendChild(buildLayerChecksRow(outputId, map));
  layerGroup.appendChild(buildOutputLayersHint(outputId, map));
  host.appendChild(layerGroup);
}

// "create a new subsection called output control, this will have a
// vertical list of all things that can be sent to the display" (owner,
// referencing ProPresenter's own Looks matrix — layers as rows, outputs as
// columns). Same outputLayerMap()/setOutputLayer data every per-output
// detail panel already edits — this is just every output's layers laid
// out together instead of one at a time. OBS excluded (see
// buildLayerChecksRow's own comment — no per-layer config, single fixed
// text-only integration).
function renderOutputControlMatrix() {
  const host = document.getElementById('output-control-matrix');
  if (!host) return;
  const outputs = allConfiguredOutputs().filter(o => o.type !== 'obs');
  host.innerHTML = '';
  if (!outputs.length) {
    host.innerHTML = '<p class="setting-hint">No outputs configured yet.</p>';
    return;
  }
  // Computed once, not once per checkbox (was outputLayerMap()[o.id] inside
  // the innermost loop — N outputs × 4 layers worth of full-map rebuilds
  // for what should be a single read).
  const map = outputLayerMap();
  const table = document.createElement('table');
  table.className = 'ocm-table';
  const thead = document.createElement('thead');
  const headRow = document.createElement('tr');
  headRow.appendChild(document.createElement('th'));
  outputs.forEach(o => {
    const th = document.createElement('th');
    th.textContent = o.name;
    th.title = o.monitor ? 'Monitor' : outputTypeLabel(o.type);
    headRow.appendChild(th);
  });
  thead.appendChild(headRow);
  table.appendChild(thead);

  const tbody = document.createElement('tbody');
  visibleLayerKeys().forEach(([layerKey, label]) => {
    const tr = document.createElement('tr');
    const th = document.createElement('th');
    th.scope = 'row';
    th.textContent = label;
    tr.appendChild(th);
    outputs.forEach(o => {
      const td = document.createElement('td');
      const cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.checked = map[o.id]?.[layerKey] !== false;
      cb.title = `${label} on ${o.name}`;
      if (o.monitor && layerKey === 'media') { cb.checked = false; cb.disabled = true; cb.title = 'A Monitor shows words only'; }
      else if (layerKey === 'video' && !outputSupportsVideo(o.id)) { cb.checked = false; cb.disabled = true; cb.title = VIDEO_NATIVE_NOTE; }
      else cb.addEventListener('change', () => setOutputLayer(o.id, layerKey, cb.checked));
      td.appendChild(cb);
      tr.appendChild(td);
    });
    tbody.appendChild(tr);
  });
  // Text only: just the theme's words — display outputs (see setOutputTextOnly).
  const textOnly = outputTextOnlyMap();
  const tr = document.createElement('tr');
  const rowHead = document.createElement('th');
  rowHead.scope = 'row';
  rowHead.textContent = 'Text only';
  tr.appendChild(rowHead);
  outputs.forEach(o => {
    const td = document.createElement('td');
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    if (o.monitor) { cb.checked = true; cb.disabled = true; cb.title = 'A Monitor shows words only'; }
    else if (o.type !== 'display') { cb.disabled = true; cb.title = `Not available for ${outputTypeLabel(o.type)}`; }
    else {
      cb.checked = !!textOnly[o.id];
      cb.title = `Text only on ${o.name}`;
      cb.addEventListener('change', () => setOutputTextOnly(o.id, cb.checked));
    }
    td.appendChild(cb);
    tr.appendChild(td);
  });
  tbody.appendChild(tr);
  table.appendChild(tbody);
  host.appendChild(table);
}

// Persist a partial settings change without clobbering unrelated fields.
async function saveSettingsPatch(patch) {
  try {
    await fetch(`${SERVER}/api/settings`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    });
  } catch (err) {
    console.warn('[Settings] save failed:', err.message);
  }
}
