// KAIRO — Tauri hooks (auto-update, native menu), remappable hotkeys and the startup
// bootstrap. Split out of app.js; MUST load last (it boots the app once every other
// piece has defined its functions).

// ── Updates (Tauri only) ──────────────────────────────────────────────────
// The Rust side emits `update-available` after the background check on startup, or after "Check
// for updates" (Help menu, Settings → Help). An update shows once as a modal over the app; "Later"
// keeps it waiting in Settings → Help → Updates, with Update & Restart there. A banner above the
// toolbar used to show it, under macOS's window buttons and with the notes' markdown as text
// ("## KAIRO v4.2.1"; owner, 4.2.0 → 4.2.1).
(function initUpdater() {
  if (!window.__TAURI__) return; // not running inside Tauri shell
  const tauri = window.__TAURI__;
  let pending = null;     // { version, notes } of an update waiting to be installed
  let shownFor = null;    // the version the modal was already shown for
  let checking = false;   // a check asked for in Settings, so its answer is shown there

  const status = (text) => { const el = document.getElementById('upd-status'); if (el) el.textContent = text; };
  const installBtn = () => document.getElementById('upd-settings-install');
  const current = () => (typeof settings !== 'undefined' && settings?.appVersion) ? `v${settings.appVersion}` : 'this version';

  // The release notes' "What's new" items, markdown made plain (bold kept).
  function whatsNew(notes) {
    const lines = String(notes || '').split('\n').map(l => l.trim());
    const from = lines.findIndex(l => /^#+\s*what'?s new/i.test(l));
    const items = [];
    for (const l of lines.slice(from >= 0 ? from + 1 : 0)) {
      if (from >= 0 && /^#+\s/.test(l)) break;
      if (/^[-*]\s+/.test(l)) items.push(escapeHtml(l.replace(/^[-*]\s+/, '')).replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/[`_]/g, ''));
    }
    if (!items.length) {
      const line = lines.find(l => l && !l.startsWith('#') && !/^\*\*.*\*\*$/.test(l));
      if (line) items.push(escapeHtml(line.replace(/[*_`#]/g, '')));
    }
    return items.slice(0, 8);
  }

  async function install(btn) {
    for (const b of [btn, installBtn(), document.getElementById('upd-modal-install')]) {
      if (b) { b.textContent = 'Downloading…'; b.disabled = true; }
    }
    status(`Downloading Kairo ${pending?.version || ''}… Kairo restarts when it's in.`);
    try {
      await tauri.core.invoke('install_update');
    } catch (e) {
      toast('Update failed: ' + e, 'error');
      status(`The update couldn't be installed: ${e}`);
      for (const b of [installBtn(), document.getElementById('upd-modal-install')]) {
        if (b) { b.textContent = 'Update & Restart'; b.disabled = false; }
      }
    }
  }

  function showUpdateModal() {
    document.getElementById('update-modal')?.remove();
    const items = whatsNew(pending.notes);
    const modal = document.createElement('div');
    modal.id = 'update-modal';
    modal.className = 'modal';
    modal.innerHTML = `
      <div class="modal-overlay"></div>
      <div class="modal-content" style="width:480px;">
        <div class="modal-header">
          <h2>Kairo ${escapeHtml(pending.version || '')} is available</h2>
          <button class="modal-close-btn" id="upd-modal-close">
            <svg width="16" height="16" viewBox="0 0 16 16"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
            <span>Close</span>
          </button>
        </div>
        <div class="modal-body" style="display:block;padding:16px 20px;">
          <p class="setting-hint" style="margin:0 0 10px;">You have ${escapeHtml(current())}. Kairo restarts to finish the update.</p>
          ${items.length ? `<div class="setting-label" style="margin-bottom:6px;">What's new</div><ul class="upd-notes">${items.map(i => `<li>${i}</li>`).join('')}</ul>` : ''}
        </div>
        <div class="modal-footer">
          <button class="modal-btn secondary" id="upd-modal-later">Later</button>
          <button class="modal-btn primary" id="upd-modal-install">Update &amp; Restart</button>
        </div>
      </div>`;
    document.body.appendChild(modal);
    const close = () => modal.remove();
    modal.querySelector('.modal-overlay').addEventListener('click', close);
    modal.querySelector('#upd-modal-close').addEventListener('click', close);
    modal.querySelector('#upd-modal-later').addEventListener('click', close);
    modal.querySelector('#upd-modal-install').addEventListener('click', (e) => install(e.currentTarget));
  }

  tauri.event.listen('update-available', (event) => {
    const { version, notes } = event.payload || {};
    pending = { version, notes };
    status(`Kairo ${version} is available. You have ${current()}.`);
    installBtn()?.classList.remove('hidden');
    const nav = document.getElementById('settings-nav-version');
    if (nav && !nav.dataset.update) { nav.dataset.update = '1'; nav.title = `Kairo ${version} is available`; nav.classList.add('has-update'); }
    if (shownFor !== version || checking) { shownFor = version; showUpdateModal(); }
    checking = false;
  });

  // Answers a check asked for (Help menu or Settings): up to date, or why it couldn't check.
  tauri.event.listen('update-check-result', (event) => {
    const { upToDate, error } = event.payload || {};
    if (upToDate) { toast('KAIRO is up to date.', 'success'); status(`Kairo is up to date (${current()}).`); }
    else if (error) { toast('Could not check for updates: ' + error, 'error'); status(`Couldn't check for updates: ${error}`); }
    checking = false;
  });

  document.getElementById('upd-check-btn')?.addEventListener('click', () => {
    checking = true;
    status('Checking for updates…');
    tauri.core.invoke('check_updates').catch(e => { checking = false; status(`Couldn't check for updates: ${e}`); });
  });
  installBtn()?.addEventListener('click', (e) => install(e.currentTarget));
  status(`You have ${current()}. Kairo checks for updates when it starts.`);
})();

// ── Native app menu bridge (Tauri only) ───────────────────────────────────
// File > New Theme/Import…/Export Current Theme dispatch here as plain
// window events (see build_app_menu/on_menu_event in src-tauri/src/lib.rs)
// rather than Rust reimplementing any of those flows — every one of them
// already exists as a toolbar button in Theme Studio (file pickers,
// unsaved-state handling, toasts and all), so the menu just opens Theme
// Studio if it isn't already open and clicks that same button.
(function initNativeMenuBridge() {
  if (!window.__TAURI__) return;
  const clickWhenReady = (btnId) => {
    if (looksModal?.classList.contains('hidden')) openThemeStudio();
    document.getElementById(btnId)?.click();
  };

  window.__TAURI__.event.listen('menu-new-theme',     () => clickWhenReady('new-look-btn'));
  // File > Import used to go straight to clickWhenReady('import-look-btn')
  // — silently jumping into Theme Studio's own theme importer, with nothing
  // telling the operator that's what "Import" even meant. Import is almost
  // always about CONTENT (slides/a song), not a theme file, so this now
  // opens a small chooser instead of guessing — see #import-options-modal.
  window.__TAURI__.event.listen('menu-import',        () => window.KairoImport?.open());
  window.__TAURI__.event.listen('menu-export-theme',  () => clickWhenReady('export-look-btn'));
  // KAIRO > Settings… (Cmd+,) — same panel the toolbar gear icon opens.
  window.__TAURI__.event.listen('menu-settings',      () => { settingsModal?.classList.remove('hidden'); showFirstSettingsPane(); });
  // Edit > Find… (Cmd+F) — the global search overlay (global_search.js).
  window.__TAURI__.event.listen('menu-find',          () => window.KairoGlobalSearch?.open());

  // Controls menu — every item here is just a click on an existing
  // dashboard button (see src-tauri/src/lib.rs's controls_menu), no modal
  // to open first, unlike the Theme Studio items above.
  const clickDirect = (btnId) => document.getElementById(btnId)?.click();
  window.__TAURI__.event.listen('menu-toggle-listening', () => clickDirect('listen-btn'));
  // Director and Production menus: each item is a remote action ("menu-run-<id>"), the same
  // thing a Stream Deck button or an OSC message does.
  // (After this file has loaded: the action list is declared further down.)
  setTimeout(() => REMOTE_ACTIONS.forEach(a => window.__TAURI__.event.listen(`menu-run-${a.id}`, () => runRemoteAction(a.id))), 0);
  window.__TAURI__.event.listen('menu-range-next',       () => clickDirect('range-next-btn'));
  window.__TAURI__.event.listen('menu-range-end',        () => clickDirect('range-clear-btn'));
  window.__TAURI__.event.listen('menu-clear-slide',      () => clickDirect('clear-slide-layer-btn'));
  window.__TAURI__.event.listen('menu-clear-bible',      () => clickDirect('clear-bible-layer-btn'));
  window.__TAURI__.event.listen('menu-output-back',      () => clickDirect('output-back-btn'));
  window.__TAURI__.event.listen('menu-clear-media',      () => clickDirect('clear-media-layer-btn'));
  window.__TAURI__.event.listen('menu-clear-timer',      () => clickDirect('clear-timer-layer-btn'));
  window.__TAURI__.event.listen('menu-clear-all',        () => clickDirect('clear-all-layers-btn'));

  // File — a new playlist, and the service's exports (the toolbar's Export
  // menu, whose items do the work).
  window.__TAURI__.event.listen('menu-new-playlist', () => clickDirect('svc-add-menu-btn'));
  const exportItem = (kind) => document.querySelector(`#download-menu [data-download="${kind}"]`)?.click();
  window.__TAURI__.event.listen('menu-export-notes',      () => exportItem('notes'));
  window.__TAURI__.event.listen('menu-export-transcript', () => exportItem('transcript'));
  window.__TAURI__.event.listen('menu-export-verses',     () => exportItem('verses'));
  // View — the toolbar's tabs (Cmd+1…6), and what the Monitor shows.
  [['bible', 'bible-btn'], ['slides', 'slides-btn'], ['timer', 'timer-btn'],
   ['songs', 'songs-btn'], ['media', 'media-btn'], ['studio', 'looks-btn']]
    .forEach(([view, btnId]) => window.__TAURI__.event.listen(`menu-view-${view}`, () => clickDirect(btnId)));
  window.__TAURI__.event.listen('menu-monitor-grid', () => clickDirect('monitor-grid-toggle-btn'));
  // Window and Help — the Monitor window, and Settings opened where asked.
  const openSettingsAt = (pane) => { settingsModal?.classList.remove('hidden'); showSettingsPane(pane); };
  window.__TAURI__.event.listen('menu-monitor-window',  () => clickDirect('monitor-popout-btn'));
  window.__TAURI__.event.listen('menu-outputs',         () => openSettingsAt('outputs'));
  window.__TAURI__.event.listen('menu-getting-started', () => openOnboardingWizard());
  window.__TAURI__.event.listen('menu-shortcuts',       () => openSettingsAt('shortcuts'));
  window.__TAURI__.event.listen('menu-help',            () => openSettingsAt('help'));
})();

// ── Remappable in-app hotkeys ───────────────────────────────────────────
// Independent of the native menu's own accelerators (src-tauri/src/lib.rs)
// — those are fixed OS-level defaults and never change; this is a
// separate, operator-customizable layer for the same action set,
// persisted as a plain {actionId: comboString} map in settings.hotkeys
// via the existing saveSettingsPatch. Same actions the Controls menu
// drives, on purpose — one list, two independent ways to trigger it.
const HOTKEY_ACTIONS = [
  { id: 'toggle-listening', label: 'Start/Stop Listening', btnId: 'listen-btn',            default: 'cmd+l' },
  { id: 'find-verse',       label: 'Find a verse',         run: () => { const el = document.getElementById('scripture-search-input'); el?.focus(); el?.select(); }, default: '/' },
  { id: 'range-next',       label: 'Next',                 btnId: 'range-next-btn',        default: 'cmd+arrowright' },
  { id: 'output-back',      label: 'Back (undo last change)', btnId: 'output-back-btn',    default: 'cmd+arrowleft' },
  { id: 'range-end',        label: 'End Range',            btnId: 'range-clear-btn',       default: '' },
  { id: 'clear-bible',      label: 'Clear Bible',          btnId: 'clear-bible-layer-btn', default: 'cmd+k' },
  { id: 'clear-slide',      label: 'Clear Slide',          btnId: 'clear-slide-layer-btn', default: '' },
  { id: 'clear-media',      label: 'Clear Media',          btnId: 'clear-media-layer-btn', default: '' },
  { id: 'clear-timer',      label: 'Clear Timer',          btnId: 'clear-timer-layer-btn', default: '' },
  { id: 'clear-all',        label: 'Clear All',            btnId: 'clear-all-layers-btn',  default: 'cmd+shift+k' },
];

// OSC remote (kairo-server osc.rs sends {type:'remote', action}): the hotkey
// actions plus a few only a remote needs — slide stepping and explicit
// start/stop, so a button can't toggle listening the wrong way.
const REMOTE_ACTIONS = [
  ...HOTKEY_ACTIONS.filter(a => a.id !== 'find-verse'),
  { id: 'listen-start', label: 'Start Listening', run: () => { if (!isListening) document.getElementById('listen-btn')?.click(); } },
  { id: 'listen-stop',  label: 'Stop Listening',  run: () => { if (isListening) document.getElementById('listen-btn')?.click(); } },
  { id: 'slide-next',   label: 'Next slide',      run: () => window.KairoService?.advanceLiveSlide?.(1, { direct: true }) },
  { id: 'slide-prev',   label: 'Previous slide',  run: () => window.KairoService?.advanceLiveSlide?.(-1, { direct: true }) },
  { id: 'audio-play',     label: 'Audio: play',            run: () => window.KairoAudioLane?.play() },
  { id: 'audio-pause',    label: 'Audio: pause',           run: () => window.KairoAudioLane?.pause() },
  { id: 'audio-stop',     label: 'Audio: stop',            run: () => window.KairoAudioLane?.stop({ fade: false }) },
  { id: 'audio-next',     label: 'Audio: next track',      run: () => window.KairoAudioLane?.next() },
  { id: 'audio-prev',     label: 'Audio: previous track',  run: () => window.KairoAudioLane?.prev() },
  { id: 'overlays-off',    label: 'Overlays: turn all off', run: () => { fetch(`${SERVER}/api/overlays/clear`, { method: 'POST' }).catch(() => {}); } },
  { id: 'audio-fade-out', label: 'Audio: fade out and stop', run: () => window.KairoAudioLane?.fadeOut() },
  // Director mode (video_director.js). Each does nothing unless Director mode is on.
  { id: 'director-send',          label: 'Director: Send Preview to the screen', run: () => directorDo('send') },
  { id: 'director-fade-to-black', label: 'Director: Fade to black',              run: () => directorDo('fadeToBlack') },
  { id: 'director-undo',          label: 'Director: Undo',                       run: () => directorDo('undo') },
  { id: 'stream-start',           label: 'Streaming: go live',                   run: () => window.KairoStreaming?.start() },
  { id: 'stream-stop',            label: 'Streaming: stop',                      run: () => window.KairoStreaming?.stop(false) },
  { id: 'report-problem',          label: 'Help: report a problem',               run: () => window.KairoReport?.open() },
  { id: 'record-start',           label: 'Recording: start',                     run: () => window.KairoStreaming?.startRecording() },
  { id: 'record-stop',            label: 'Recording: stop',                      run: () => window.KairoStreaming?.stopRecording(false) },
  { id: 'auto-send-on',           label: 'Auto-send verses: on',                 run: () => setAutoSend(true) },
  { id: 'auto-send-off',          label: 'Auto-send verses: off',                run: () => setAutoSend(false) },
  ...Array.from({ length: 9 }, (_, i) => ({ id: `director-layout-${i + 1}`, label: `Director: Layout ${i + 1} to Preview`, run: () => directorDo('loadLayout', i + 1) })),
  ...Array.from({ length: 9 }, (_, i) => ({ id: `director-input-${i + 1}`,  label: `Director: Input ${i + 1} to the screen`, run: () => directorDo('sendInput', i + 1) })),
  ...Array.from({ length: 9 }, (_, i) => ({ id: `macro-${i + 1}`,           label: `Run macro ${i + 1}`,                    run: () => window.KairoMacros?.runIndex(i + 1) })),
];
function directorDo(fn, ...args) {
  const d = window.KairoVideoDirector;
  // A Director action from a macro, Stream Deck or a menu while Director mode is off does
  // nothing: say so, instead of a macro silently skipping its camera cut.
  if (!d || !isDirectorOn()) { window.kairoProblem?.('That needs Director mode, which is off.', 'director-off'); return false; }
  if (typeof d[fn] === 'function') return d[fn](...args);
  return false;
}
function setAutoSend(on) {
  settings.autoSend = !!on;
  saveSettingsPatch({ autoSend: settings.autoSend });
  const box = document.getElementById('auto-send-settings'); if (box) box.checked = settings.autoSend;
  if (typeof updateAutoDeployBadge === 'function') updateAutoDeployBadge();
}

function runRemoteAction(id) {
  const action = REMOTE_ACTIONS.find(a => a.id === id);
  if (!action) return;
  if (action.run) action.run();
  else document.getElementById(action.btnId)?.click();
}
window.runRemoteAction = runRemoteAction;

async function renderOscSettings() {
  const enabled = document.getElementById('osc-enabled');
  const port = document.getElementById('osc-port');
  const status = document.getElementById('osc-status');
  const list = document.getElementById('osc-addresses');
  if (!enabled || !port) return;
  enabled.checked = !!settings.oscEnabled;
  port.value = settings.oscPort || 8765;
  port.disabled = !enabled.checked;
  if (list && !list.childElementCount) {
    REMOTE_ACTIONS.forEach(a => {
      const row = document.createElement('div');
      row.className = 'dl-row';
      row.innerHTML = `<code class="osc-address">/kairo/${escapeHtml(a.id)}</code><span class="dl-name">${escapeHtml(a.label)}</span>`;
      list.appendChild(row);
    });
  }
  if (!status) return;
  if (!enabled.checked) { status.textContent = 'Off'; status.classList.remove('osc-error'); return; }
  try {
    const s = await (await fetch(`${SERVER}/api/osc/status`)).json();
    status.textContent = s.error || (s.listening ? `Listening on UDP ${s.port}${s.last ? ` · last: ${s.last}` : ''}` : 'Starting…');
    status.classList.toggle('osc-error', !!s.error);
  } catch { status.textContent = ''; }
}

document.getElementById('osc-enabled')?.addEventListener('change', async (e) => {
  settings.oscEnabled = e.target.checked;
  await saveSettingsPatch({ oscEnabled: e.target.checked });
  renderOscSettings();
  setTimeout(renderOscSettings, 2500); // the server picks it up within 2 s
});
document.getElementById('osc-port')?.addEventListener('change', async (e) => {
  const p = Math.round(Number(e.target.value));
  if (!(p >= 1024 && p <= 65535)) { e.target.value = settings.oscPort || 8765; return; }
  settings.oscPort = p;
  await saveSettingsPatch({ oscPort: p });
  setTimeout(renderOscSettings, 2500);
});
document.querySelector('.settings-nav-item[data-pane="shortcuts"]')?.addEventListener('click', () => renderOscSettings());

function hotkeyFor(actionId) {
  const action = HOTKEY_ACTIONS.find(a => a.id === actionId);
  const stored = settings.hotkeys?.[actionId];
  return stored !== undefined ? stored : (action?.default || '');
}

// Normalized as ctrl+alt+shift+cmd+<key>, always that modifier order, key
// lowercased — capture and lookup both go through this so they can never
// silently disagree on formatting.
function comboFromEvent(e) {
  const parts = [];
  if (e.ctrlKey) parts.push('ctrl');
  if (e.altKey) parts.push('alt');
  if (e.shiftKey) parts.push('shift');
  if (e.metaKey) parts.push('cmd');
  const key = e.key.toLowerCase();
  if (!['control', 'alt', 'shift', 'meta'].includes(key)) parts.push(key);
  return parts.join('+');
}

const COMBO_SYMBOLS = { cmd: '⌘', shift: '⇧', alt: '⌥', ctrl: '⌃', arrowright: '→', arrowleft: '←', arrowup: '↑', arrowdown: '↓' };

let capturingHotkeyId = null;

// One list, a row per action: its name, its keys as keycaps, and Change /
// Clear — the keys where the eye looks for them (2026-10-12; they were small
// grey text under each name, one tall card per action).
function renderHotkeysList() {
  const host = document.getElementById('hotkeys-list');
  if (!host) return;
  host.className = 'dl-list hotkeys-list';
  host.innerHTML = '';
  HOTKEY_ACTIONS.forEach(action => {
    const combo = hotkeyFor(action.id);
    const row = document.createElement('div');
    row.className = 'dl-row';
    const keys = combo
      ? combo.split('+').map(p => `<kbd>${escapeHtml(COMBO_SYMBOLS[p] || p.toUpperCase())}</kbd>`).join('')
      : '<span class="hotkey-unset">Not set</span>';
    row.innerHTML = `<span class="dl-name">${escapeHtml(action.label)}</span><span class="hotkey-keys">${keys}</span>`;
    const changeBtn = document.createElement('button');
    changeBtn.className = 'modal-btn secondary dl-btn';
    changeBtn.textContent = combo ? 'Change' : 'Set';
    changeBtn.addEventListener('click', () => startHotkeyCapture(action.id, changeBtn));
    row.appendChild(changeBtn);
    const clearBtn = document.createElement('button');
    clearBtn.className = 'modal-btn secondary dl-btn';
    clearBtn.textContent = 'Clear';
    clearBtn.style.visibility = combo ? '' : 'hidden'; // keeps the columns lined up
    clearBtn.addEventListener('click', () => saveHotkey(action.id, ''));
    row.appendChild(clearBtn);
    host.appendChild(row);
  });
}

function startHotkeyCapture(actionId, btn) {
  if (capturingHotkeyId) return; // one capture at a time
  capturingHotkeyId = actionId;
  const original = btn.textContent;
  btn.textContent = 'Press keys… (Esc cancels)';
  const onKey = (e) => {
    e.preventDefault(); e.stopPropagation();
    if (e.key === 'Escape') { finish(); return; }
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return; // a bare modifier isn't a combo yet
    saveHotkey(actionId, comboFromEvent(e));
    finish();
  };
  function finish() {
    document.removeEventListener('keydown', onKey, true);
    capturingHotkeyId = null;
    renderHotkeysList();
  }
  // Captured on the way down (capture:true), ahead of anything else on
  // the page, so recording a shortcut never also fires whatever it's
  // about to be bound to.
  document.addEventListener('keydown', onKey, true);
}

async function saveHotkey(actionId, combo) {
  settings.hotkeys = { ...(settings.hotkeys || {}), [actionId]: combo };
  await saveSettingsPatch({ hotkeys: settings.hotkeys });
  renderHotkeysList();
}

// Global dispatcher. Skips text inputs (typing "l" shouldn't toggle
// listening) and gets out of the way entirely while a new combo is being
// recorded — startHotkeyCapture's own capture-phase listener already
// claims the keydown first in that case, but the guard here is cheap
// insurance against ever double-firing on the same keystroke.
document.addEventListener('keydown', (e) => {
  if (capturingHotkeyId) return;
  const t = e.target;
  if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
  const combo = comboFromEvent(e);
  if (!combo) return;
  const action = HOTKEY_ACTIONS.find(a => hotkeyFor(a.id) === combo);
  if (!action) return;
  e.preventDefault();
  if (action.run) action.run();
  else document.getElementById(action.btnId)?.click();
});

renderHotkeysList();
renderOscSettings();

// ═══════════════════════════════════════════════════════════════════════════
// TRIGGERS — dispatch into the Timer tab (the server's Triggers (kairo-server library.rs), service.js)
//   The actual UI lives in service.js as a top-bar tab (segment cards,
//   see showTimerLibrary/onTimerAction) — this just forwards the two
//   built-in trigger types' broadcasts there, same bridge pattern as
//   'media-status'/'media-folder-changed' above.
// ═══════════════════════════════════════════════════════════════════════════
registerActionHandler('stage-timer',   (msg) => window.KairoService?.onTimerAction?.(msg));
registerActionHandler('clock-message', (msg) => window.KairoService?.onClockAction?.(msg));

// Boot-time wiring that reaches into the outputs code (outputLayerMap, ndiOutputs, refreshDisplayStatus, ...)
// lives here, not in app.js: those are defined in app_outputs.js, which loads after app.js.

// ── Native NDI bridge ────────────────────────────────────────────────────
// Whenever the live preview verse changes, push it to the native Rust NDI
// sender so the broadcast frame stays in sync. Coalesce rapid changes via
// requestAnimationFrame so we don't issue redundant invokes during multi-
// step UI updates (e.g. clear → new verse fires two mutations in one tick).
(function wireNdiBridge() {
  const tauriInvoke = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
  // Output Looks — Media/Timer layer parity for these two native, non-
  // webview senders (see ndi.rs/syphon.rs's own render_frame). pushMedia is
  // called directly from renderMediaPreview below (the one authoritative
  // call site whenever the media layer changes). pushTimer is called from
  // service.js's onTimerAction — a separate script/closure with no direct
  // access to tauriInvoke/outputLayerMap — via this exposed hook, the one
  // place this module needs to be called INTO rather than calling out,
  // unlike every other service.js/app.js interaction (which flows through
  // window.KairoService in the opposite direction). Both start as no-ops so
  // a call before this IIFE runs (or when Tauri isn't available at all)
  // never throws.
  // Output Looks: multiple independent named instances per kind now, not
  // one fixed sender each (owner: "you should be able to create multiple
  // Syphon or NDI outputs" — see ndiOutputs()/syphonOutputs()/
  // renderNativeOutputs() above). Last-known value of each layer lets
  // pushToOne catch a freshly-started output up immediately, and lets
  // pushMedia/pushTimer skip a disabled instance without losing track of
  // what to send once it's re-enabled.
  window.KairoNativeOutputs = { pushMedia() {}, pushTimer() {}, pushToOne() {} };
  if (!tauriInvoke) return;
  let lastVerse = '', lastRef = '', lastMediaDataUri = null, lastTimerText = '';
  function enabledNativeOutputs(kind) {
    return (kind === 'ndi' ? ndiOutputs() : syphonOutputs()).filter(o => o.enabled);
  }
  window.KairoNativeOutputs.pushMedia = function pushMedia(dataUri) {
    // `dataUri`: a `data:image/…;base64,…` string, or falsy to clear.
    // Anything else (a video src, a plain http(s)/blob URL) is silently
    // skipped — real video playback for these two senders is Output Looks'
    // own Phase 2, deliberately not bundled into this change (see the plan).
    const m = typeof dataUri === 'string' && /^data:image\/\w+;base64,(.*)$/.exec(dataUri);
    lastMediaDataUri = m ? dataUri : null;
    const b64 = m ? m[1] : null;
    const layers = outputLayerMap();
    for (const kind of ['ndi', 'syphon']) {
      for (const o of enabledNativeOutputs(kind)) {
        if (layers[o.id]?.media === false) continue;
        tauriInvoke(`${kind}_update_media`, { id: o.id, imageBase64: b64 }).catch(() => {});
      }
    }
  };
  window.KairoNativeOutputs.pushTimer = function pushTimer(text) {
    lastTimerText = text || '';
    const layers = outputLayerMap();
    for (const kind of ['ndi', 'syphon']) {
      for (const o of enabledNativeOutputs(kind)) {
        if (layers[o.id]?.timer === false) continue;
        tauriInvoke(`${kind}_update_timer`, { id: o.id, text: lastTimerText }).catch(() => {});
      }
    }
  };
  window.KairoNativeOutputs.pushToOne = function pushToOne(kind, id) {
    if (kind === 'decklink') return; // DeckLink only ever plays captured frames
    const want = outputLayerMap()[id] || { slide: true, media: true, timer: true };
    if (want.slide !== false) tauriInvoke(`${kind}_update`, { id, verse: lastVerse, reference: lastRef }).catch(() => {});
    if (want.media !== false) {
      const m = typeof lastMediaDataUri === 'string' && /^data:image\/\w+;base64,(.*)$/.exec(lastMediaDataUri);
      tauriInvoke(`${kind}_update_media`, { id, imageBase64: m ? m[1] : null }).catch(() => {});
    }
    if (want.timer !== false) tauriInvoke(`${kind}_update_timer`, { id, text: lastTimerText }).catch(() => {});
  };
  if (!previewVerseText) return;
  let scheduled = false;
  let lastSent = '';
  function pushToOutputs() {
    scheduled = false;
    const verse = (previewVerseText.textContent || '').trim();
    const ref   = (previewVerseRef?.textContent || '').trim();
    const blank = !verse || verse === 'Nothing on display';
    const sig   = blank ? '' : (ref + '' + verse);
    if (sig === lastSent) return;
    lastSent = sig;
    lastVerse = blank ? '' : verse;
    lastRef   = blank ? '' : ref;
    const layers = outputLayerMap();
    // Each output's _update is a no-op if that specific instance isn't
    // running, so we only need to skip ones Output Looks has disabled
    // Slide for — not check "is it actually broadcasting" here too.
    for (const kind of ['ndi', 'syphon']) {
      for (const o of enabledNativeOutputs(kind)) {
        if (layers[o.id]?.slide === false) continue;
        tauriInvoke(`${kind}_update`, { id: o.id, verse: lastVerse, reference: lastRef }).catch(() => {});
      }
    }
  }
  function schedule() {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(pushToOutputs);
  }
  new MutationObserver(schedule).observe(previewVerseText, { characterData: true, childList: true, subtree: true });
  if (previewVerseRef) {
    new MutationObserver(schedule).observe(previewVerseRef, { characterData: true, childList: true, subtree: true });
  }
})();

// ── External Display: picking a monitor in the Physical Screen select
// (buildScreenSelect) is the real action; the header toggle (see
// buildOutputRowToggle's 'display' case) is just a convenience mirror of
// that same assign/None choice, not a separate control. This IIFE just
// keeps the header status + the dropdown's own option list
// live while Settings is open: header status reports whether a physical
// external display is actually plugged in right now (not window-open
// state — you can have a window open on your own laptop screen with
// nothing external connected at all). Reuses the same list_monitors Tauri
// command refreshDisplayStatus() already calls (real OS-level monitor
// enumeration, not the Window Management API WebKit doesn't support).
(function wireExternalDisplayStatus() {
  // Re-queried fresh on every refresh() call below, NOT cached once here —
  // the Outputs master-detail redesign rebuilds #outputs-list (and
  // #outputs-detail, when External Display is selected) via innerHTML='',
  // so a reference captured once at script-load time (when the whole
  // Outputs pane hasn't even been rendered once yet) goes stale/null the
  // instant either panel first renders. Real bug this caused: the whole
  // rest of refresh() below silently stopped running every single tick
  // after the FIRST real render, because it also called the now-deleted
  // upsertPrimaryMonitorPicker() (removed in that same redesign, this
  // being the one call site that got missed) — an uncaught exception in
  // an async function with no caller awaiting it just dies silently, so
  // this whole poller had been a no-op since that redesign shipped.
  let autoResumeDone = false;

  // Shares refreshDisplayStatus()'s own list_monitors call (and its
  // cachedScreens result) rather than making a second, separate one here.
  async function refresh() {
    // Auto-reopen whatever monitor was previously selected, once, the
    // first time this runs after launch — a picker that only opens a
    // window on its OWN change event means every app restart (or a
    // display-server crash mid-dev-session, which happened repeatedly
    // while debugging this exact feature) silently loses the output
    // window with no way back short of manually re-touching the
    // dropdown, even though the assignment itself was never lost.
    // Matches how every other presentation app restores its output on
    // launch instead of requiring the operator to re-pick it.
    //
    // Gated on settingsLoaded, not just "has refresh() run yet" — this
    // IIFE's own refresh() call below fires SYNCHRONOUSLY at script load,
    // well before loadSettings() (which only runs inside ws.onopen, after
    // auth + the WebSocket connects) has populated `settings` at all. The
    // old code marked itself "done" on that very first, always-empty
    // attempt — outputScreenMap()[PRIMARY_DISPLAY] read from settings={},
    // so primaryScreen was always undefined, the real auto-reopen never
    // fired, and the one-shot flag then permanently blocked every later
    // retry (including the 3s poll below, by which point settings HAD
    // loaded) — the exact real incident: "I have to reset the output
    // every time [the app restarts]." Now the attempt itself is what gets
    // consumed, not just the opportunity to try.
    if (!autoResumeDone && settingsLoaded) {
      autoResumeDone = true;
      const primaryScreen = outputScreenMap()[PRIMARY_DISPLAY];
      if (primaryScreen && typeof openDisplayOutput === 'function') {
        openDisplayOutput({ id: PRIMARY_DISPLAY, name: PRIMARY_DISPLAY });
      }
      extraDisplays().forEach(d => {
        if (outputScreenMap()[d.id] && typeof openDisplayOutput === 'function') {
          openDisplayOutput(d);
        }
      });
    }
    await refreshDisplayStatus();
    if (settingsLoaded && autoResumeDone && typeof reconcileDisplayWindows === 'function') reconcileDisplayWindows().catch(() => {});
    const connected = cachedScreens.length > 1;
    // List-row dot/text (renderOutputsList, always present once the
    // Outputs pane has rendered at least once).
    const headerDot = document.getElementById('external-header-dot');
    const headerTxt = document.getElementById('external-header-status');
    if (headerDot) headerDot.className = 'bs-dot' + (connected ? ' connected' : '');
    const waiting = typeof displaysWaiting !== 'undefined' && displaysWaiting.size > 0;
    const own = typeof displayStatusFor === 'function' ? displayStatusFor(PRIMARY_DISPLAY) : null;
    if (headerTxt) headerTxt.textContent = waiting ? 'An output is waiting for its display. If it is AirPlay, set it to Extend, not Mirror.' : own || (connected ? 'External display connected' : 'No external display connected');
    // The open detail panel's dot, text and screen list — updated in place,
    // never rebuilt under the operator (see refreshDisplayDetailInPlace).
    refreshDisplayDetailInPlace();
  }
  refresh();
  // Real incident this fixes: a display plugged in AFTER the app was
  // already running never got picked up, because nothing re-queried
  // list_monitors once the settings panel's initial render had already
  // happened. Polling here keeps both this status line and the dropdown's
  // option list genuinely live while Settings is open.
  setInterval(refresh, 3000);
})();

// ── Startup bootstrap ───────────────────────────────────────────────────────
// Runs behind the branded #bootstrap-overlay before the operator touches the
// app:
//   1. Requests microphone permission up front (so the OS prompt appears at
//      launch, not mid-service when they hit Start).
//   2. Downloads required resources — the offline speech model — when the
//      offline engine is selected and it isn't present yet, with progress.
// Always resolves (and always removes the overlay) so a slow/failed step can
// never trap the operator on the loading screen.
//
// This briefly went through a "remove the overlay, it's redundant with the
// native splash.html window" pass — wrong call, corrected right after. Owner:
// "I wanted to retain the full large one with the brand mark and logo." The
// native window (since removed entirely, see src-tauri/src/lib.rs) was a
// small 440x300 fixed-size window, not this large full-page treatment — they
// were never really the same screen. This overlay is the app's one splash now.
async function bootstrapStartup() {
  const overlay = document.getElementById('bootstrap-overlay');
  const startedAt = Date.now();
  const finish = () => {
    if (overlay) { overlay.classList.add('done'); setTimeout(() => overlay.remove(), 500); }
    // How long the splash stayed up, so a slow start can be read from the log.
    try { window.kairoLog && window.kairoLog('startup-splash', { ms: Date.now() - startedAt }); } catch {}
  };
  // Hard safety valve — never keep the overlay up longer than 90s.
  const safety = setTimeout(finish, 90000);
  // Minimum time the brand lockup stays on screen. Without this the overlay
  // could disappear well under a second after launch (mic permission already
  // granted, deepgram engine needs no offline-model download) — too fast to
  // actually register the brand mark/wordmark that's the whole point of this
  // screen. Real startup work below still happens at its own pace; this only
  // holds the screen on a little longer, never makes it wait longer than it
  // already would. Owner: "lets keep the delay to 5s" (down from 8.5s).
  const MIN_DISPLAY_MS = 5000;

  try {
    // 1) Microphone permission
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: true });
      s.getTracks().forEach(t => t.stop());
    } catch { /* denied or unavailable — app still works; user can grant later */ }

    // 2) Required resources — offline model when the offline engine is chosen
    let engine = 'deepgram';
    try {
      const s = await fetchServerSettings();
      engine = (s.speechEngine || 'deepgram').toLowerCase();
    } catch {}

    if (engine === 'offline' || engine === 'browser') {
      try {
        // The model the spoken language runs on: English, or the other-
        // languages one (server's /api/offline/status reports which).
        const needed = (await (await fetch(`${SERVER}/api/offline/status`)).json()).needed === 'multi' ? 'multi' : 'en';
        const st = await (await fetch(`${SERVER}/api/offline/status?model=${needed}`)).json();
        if (!st.installed) {
          fetch(`${SERVER}/api/offline/install?model=${needed}`, { method: 'POST' }).catch(() => {});
          for (let i = 0; i < 600; i++) {          // up to ~10 min — the offline (sherpa-onnx) model is a sizable download
            await new Promise(r => setTimeout(r, 1000));
            const s2 = await (await fetch(`${SERVER}/api/offline/status?model=${needed}`)).json().catch(() => ({}));
            if (s2.installed) break;
            // The download stopped without installing (no internet, file
            // missing): Start says what's needed — no point holding the splash
            // for the rest of the ten minutes.
            if (i >= 3 && s2.installing === false) break;
          }
        }
      } catch { /* model status unavailable — proceed; Start will surface any real error */ }
    }

    const elapsed = Date.now() - startedAt;
    if (elapsed < MIN_DISPLAY_MS) await new Promise(r => setTimeout(r, MIN_DISPLAY_MS - elapsed));
  } finally {
    clearTimeout(safety);
    finish();
  }
}

// Init — load the auth token from Tauri FIRST so all subsequent fetch / WS
// traffic carries the bearer header. Static assets and /health are exempt on
// the server side, so the page itself loads even before this resolves — but
// bootstrapStartup() and connectWS() both call authenticated endpoints, so
// they must genuinely wait rather than just being kicked off alongside it
// (that gap was the actual bug: bootstrapStartup() used to fire in parallel,
// so its own settings fetch silently 401'd on a cold Tauri launch before the
// IPC round-trip finished, and if the token never arrived at all, every
// later action — including "Start Listening" — kept failing all session).
renderLooksList();
wireRangeSliders();
(async () => {
  await loadAuthToken();
  connectWS();
  bootstrapStartup();
  // Tell Rust it's safe to reveal the main window now — our CSS is applied
  // and there's a real frame painted behind it. Rust used to show the window
  // right after dispatching navigate(), which raced the new page's own load
  // and could flash WebKit's default white background before this script (and
  // our dark styles) ever ran.
  //
  // This used to wait on a double requestAnimationFrame instead of the
  // setTimeout below — reads as more "correct" (wait for an actual paint,
  // not just a fixed delay), but it deadlocked every single launch: the
  // window is created with `visible: false`, and WebKit never runs a
  // compositing/paint pass for a surface that was never made visible — so
  // rAF's callback had nothing to synchronize against and simply never
  // fired. Confirmed via a boot trace: execution reached this exact point
  // every time, then nothing — the app only ever appeared via Rust's 12s
  // "frontend never signalled ready" fallback. A short timer doesn't depend
  // on compositing/visibility at all, so it can't deadlock the same way;
  // ~50ms is plenty for the DOM/CSSOM to settle after bootstrapStartup()
  // returns. No-ops harmlessly outside Tauri (plain-browser dev).
  setTimeout(() => {
    const inv = window.__TAURI__?.core?.invoke || window.__TAURI__?.invoke;
    inv?.('signal_main_ready').catch(() => {});
  }, 50);
})();


// ── Audio lane strip (the small player under the Monitor; see audio_lane.js) ──
(function wireAudioLaneStrip() {
  const lane = window.KairoAudioLane;
  const strip = document.getElementById('audio-lane-strip');
  if (!lane || !strip) return;
  const $ = (id) => document.getElementById(id);
  const fmt = (s) => { s = Math.max(0, Math.floor(s || 0)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
  lane.onChange((st) => {
    strip.classList.toggle('hidden', !st.loaded);
    $('al-title').textContent = st.track + (st.ducked ? ' (lowered)' : '');
    $('al-play').innerHTML = st.playing ? '&#9208;' : '&#9654;';
    $('al-time').textContent = `${fmt(st.time)} / ${fmt(st.duration)}`;
    const v = $('al-volume'); if (v && document.activeElement !== v) v.value = String(st.volume);
  });
  $('al-play').addEventListener('click', () => lane.toggle());
  $('al-prev').addEventListener('click', () => lane.prev());
  $('al-next').addEventListener('click', () => lane.next());
  $('al-fade').addEventListener('click', () => lane.fadeOut());
  $('al-volume').addEventListener('input', (e) => lane.setVolume(Number(e.target.value)));
  lane.init();
})();
