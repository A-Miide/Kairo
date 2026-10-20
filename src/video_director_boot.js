// KAIRO — Video director mode: the switch, and nothing else.
//
// This is the only video file the main window always loads. With the mode off
// it adds one small top-bar button and a menu listener, and makes sure no
// display window is told to open a camera. Turning the mode on loads the rest
// (video_sources.js, video_hub.js, sources_panel.js, video_director.js and
// the stylesheet); turning it off unloads the UI and releases every capture.
// The setting persists as settings.videoDirector; everything else asks isDirectorOn() and
// listens with onDirectorChange() (settings_store.js).
'use strict';
(function () {
  const FILES = ['video_sources.js', 'video_hub.js', 'sources_panel.js', 'video_input_grid.js', 'video_director_layout.js', 'video_dock.js', 'video_looks.js', 'video_director.js'];
  let loading = null;
  let armedTimer = null;
  let busy = false;

  function loadScript(src) {
    return new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = src; s.onload = resolve; s.onerror = () => reject(new Error('could not load ' + src));
      document.head.appendChild(s);
    });
  }
  function ensureLoaded() {
    if (!loading) {
      loading = (async () => {
        if (!document.getElementById('video-director-css')) {
          const l = document.createElement('link');
          l.id = 'video-director-css'; l.rel = 'stylesheet'; l.href = 'video_director.css';
          document.head.appendChild(l);
        }
        // video_looks.js is loaded with the page (the Layout picker under Outputs reads it); not twice.
        for (const f of FILES) { if (f === 'video_looks.js' && window.KairoVideoLooks) continue; await loadScript(f); }
      })().catch((err) => { loading = null; throw err; });
    }
    return loading;
  }

  // The one discreet control, left of the Settings gear.
  const btn = document.createElement('button');
  btn.id = 'video-director-btn';
  btn.className = 'tb-studio-btn tb-icon-only';
  btn.type = 'button';
  btn.setAttribute('aria-pressed', 'false');
  btn.innerHTML = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg>';
  const settingsBtn = document.getElementById('settings-btn');
  if (settingsBtn) settingsBtn.before(btn);

  // Settings → Director mode: the same switch, as a card with one row.
  const style = document.createElement('style');
  style.textContent = `
    .dm-card { border: 1px solid var(--border); border-radius: var(--radius-lg); background: rgba(255,255,255,0.015); }
    .dm-row[hidden] { display: none; }
    .dm-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 16px; }
    .dm-name { font-size: 13px; font-weight: 600; color: var(--text); }
    .dm-text .setting-hint { margin: 3px 0 0; }
    .dm-armed { margin: 0; padding: 0 16px 12px; font-size: 11.5px; color: var(--orange); }
    .dm-armed[hidden] { display: none; }`;
  document.head.appendChild(style);
  const nav = document.getElementById('settings-nav');
  const panes = document.getElementById('settings-panes');
  let dmToggle = null, dmArmed = null;
  if (nav && panes) {
    const navBtn = document.createElement('button');
    navBtn.className = 'settings-nav-item';
    navBtn.dataset.pane = 'director';
    navBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg><span>Director mode</span>';
    const outputsBtn = nav.querySelector('[data-pane="outputs"]');
    outputsBtn ? outputsBtn.after(navBtn) : nav.appendChild(navBtn);
    const pane = document.createElement('section');
    pane.className = 'settings-pane';
    pane.dataset.pane = 'director';
    pane.innerHTML = `
      <div class="settings-section">
        <div class="settings-section-label">Director mode</div>
        <div class="dm-card">
          <div class="dm-row">
            <div class="dm-text">
              <div class="dm-name" id="dm-name">Video director mode</div>
              <p class="setting-hint">Cameras and inputs on your outputs, with a screen for directing them: Preview and Output, saved Layouts, and the playlist and media bin. Scripture and lower-thirds draw over the video. This switch turns video on or off; the video button in the top bar only shows or hides the Director screen, and the cameras stay on air.</p>
            </div>
            <label class="output-toggle"><input type="checkbox" id="dm-toggle" aria-labelledby="dm-name"><span class="output-toggle-track"></span></label>
          </div>
          <p class="dm-armed" id="dm-armed" role="status" hidden></p>
          <div class="dm-row">
            <div class="dm-text">
              <label class="dm-name" for="dm-default-layout">Layout</label>
              <p class="setting-hint">What the outputs with video show in Director mode: every verse, slide and media item follows it. The same as the Layout sent from the Director screen: change either and the other follows.</p>
            </div>
            <select class="setting-input" id="dm-default-layout"></select>
          </div>
          <div class="dm-row" id="dm-sim-row" hidden>
            <div class="dm-text">
              <div class="dm-name" id="dm-sim-name">Simulate 4 cameras at 30 fps</div>
              <p class="setting-hint">Developer. Adds four test inputs that need no camera, for measuring. Reloads the window.</p>
            </div>
            <label class="output-toggle"><input type="checkbox" id="dm-sim" aria-labelledby="dm-sim-name"><span class="output-toggle-track"></span></label>
          </div>
        </div>
      </div>`;
    panes.appendChild(pane);
    dmToggle = pane.querySelector('#dm-toggle');
    dmArmed = pane.querySelector('#dm-armed');
    // Default Layout: the saved Layouts, or None. Never chosen: None (video_director.js defaultLayout).
    const defSel = pane.querySelector('#dm-default-layout');
    const fillDefault = () => {
      if (!defSel || defSel === document.activeElement) return;
      // Read straight from settings: video_looks.js only loads with Director mode, and before the first
      // start there are no saved Layouts yet (the Lower third is made then).
      const list = (Array.isArray(settings.directorLooks) ? settings.directorLooks : []).filter(l => l && typeof l.id === 'string' && l.name);
      defSel.replaceChildren();
      const opt = (v, t) => { const o = document.createElement('option'); o.value = v; o.textContent = t; defSel.appendChild(o); };
      opt('', 'None: the outputs keep their own themes');
      const lower = list.find(l => l.themeId === 'screen-lower-third');
      if (!lower) opt('@lower-third', 'Lower third');
      list.forEach(l => opt(l.id, l.name));
      const cur = settings.directorDefaultLayout;
      const notChosen = lower ? lower.id : '@lower-third';
      defSel.value = cur === null || cur === undefined ? '' : (cur && list.some(l => l.id === cur) ? cur : notChosen);
    };
    if (defSel) {
      defSel.addEventListener('change', () => {
        // '@lower-third': the Lower third, made on the first start (video_director.js defaultLayout).
        const v = defSel.value === '' ? null : defSel.value;
        settings.directorDefaultLayout = v;
        saveSettingsPatch({ directorDefaultLayout: v });
        // One Layout: with Director mode on it goes on the outputs and Preview at once.
        window.KairoVideoDirector?.useLayout?.(v);
      });
      document.querySelector('.settings-nav-item[data-pane="director"]')?.addEventListener('click', fillDefault);
      if (typeof onSettingsChange === 'function') onSettingsChange(() => fillDefault());
      fillDefault();
    }
    // The developer row shows only in a dev setup: localStorage 'kairo-dev' = '1', or while simulating.
    const sim = window.KairoSimCameras;
    let dev = false;
    try { dev = localStorage.getItem('kairo-dev') === '1'; } catch {}
    const row = pane.querySelector('#dm-sim-row');
    if (sim && row && (dev || sim.isOn())) {
      row.hidden = false;
      const box = pane.querySelector('#dm-sim');
      box.checked = sim.isOn();
      box.addEventListener('change', () => { syncSimSources(box.checked); sim.set(box.checked); setTimeout(() => location.reload(), 400); });
    }
  }

  // Four simulated inputs (1080p30) are added when simulating and removed when it ends.
  // Nothing is touched while the flag was never used.
  function syncSimSources(on) {
    const sim = window.KairoSimCameras;
    if (!sim) return;
    const isSim = (x) => /^sim-cam-/.test(x.deviceId || '');
    let list = KairoCameras.list();
    if (on) {
      sim.ids().forEach((id, i) => {
        if (list.some(x => x.deviceId === id)) return;
        list.push({ id: 'vid-sim' + (i + 1), name: 'Sim ' + (i + 1), deviceId: id, label: 'Simulated camera ' + (i + 1), width: 1920, height: 1080, fps: 30, enabled: true });
      });
    } else {
      if (!list.some(isSim)) return;
      list = list.filter(x => !isSim(x));
    }
    const live = settings.videoLive && typeof settings.videoLive === 'object' ? { ...settings.videoLive } : {};
    for (const k of Object.keys(live)) if (!list.some(x => x.id === live[k])) live[k] = null;
    KairoCameras.save(list, live);
  }

  // armed: false, or the sentence saying what the next choice will do (shown under the switch and as the button's label).
  // The button shows whether the Director screen is up; the Settings switch whether video is on.
  const viewShown = () => !!(window.KairoVideoDirector && window.KairoVideoDirector.isShown && window.KairoVideoDirector.isShown());
  function paint(on, armed) {
    if (dmToggle) dmToggle.checked = !!on;
    if (dmArmed) { dmArmed.hidden = !armed; if (armed) dmArmed.textContent = armed; }
    on = on && (viewShown() || busy);
    btn.classList.toggle('active', on);
    btn.classList.toggle('vd-armed', !!armed);
    btn.setAttribute('aria-pressed', String(on));
    const label = armed || (on ? 'Video director mode is on. Choose to turn it off' : 'Video director mode');
    btn.title = label;
    btn.setAttribute('aria-label', label);
  }
  paint(false, false);

  // A stream or recording is built at Go live / Record with the cameras of that moment
  // (streaming_ui.js pipelineBase): switching the mode does not change what it carries.
  function capturing() {
    const s = window.KairoStreaming;
    const live = !!(s && s.isLive && s.isLive()), rec = !!(s && s.isRecording && s.isRecording());
    if (!live && !rec) return '';
    return live && rec ? 'The stream and the recording keep their cameras until they are restarted.'
      : live ? 'The stream keeps its cameras until it is restarted.' : 'The recording keeps its cameras until it is restarted.';
  }
  // The first choice only arms (says what will happen); a second within 5 seconds does it.
  let armedFor = null;   // 'on' | 'off' while armed
  function arm(dir, text) {
    armedFor = dir;
    paint(dir === 'off', text);
    clearTimeout(armedTimer);
    armedTimer = setTimeout(() => { armedFor = null; paint(isDirectorOn(), false); }, 5000);
  }
  function disarm() { clearTimeout(armedTimer); armedFor = null; }

  function refreshOutputsUi() {
    try { if (typeof renderOutputsList === 'function') renderOutputsList(); } catch {}
  }
  function save(on) {
    settings.videoDirector = on;
    saveSettingsPatch({ videoDirector: on });
    emitDirectorChange('flag');
  }

  async function enable(persist) {
    if (busy) return;
    // Turned on by a choice while streaming or recording: say first that it won't reach the stream.
    const cap = persist ? capturing() : '';
    if (cap && armedFor !== 'on') { arm('on', `${cap} Choose again to turn video director mode on.`); return; }
    disarm();
    busy = true;
    try {
      if (persist) save(true);
      if (settings.directorView === false) { settings.directorView = true; saveSettingsPatch({ directorView: true }); }
      paint(true, false);
      await ensureLoaded();
      await window.KairoVideoDirector.start();
      refreshOutputsUi();
      emitDirectorChange('started');
    } catch (err) {
      console.warn('[Video] director mode could not start:', err.message);
      try { window.KairoVideoDirector && window.KairoVideoDirector.stop(); } catch {}   // never leave it half started
      save(false);
      paint(false, false);
    } finally {
      busy = false;
      // Switched off while it was starting: do it now.
      if (wantOff) { wantOff = false; disable(); }
    }
  }

  let wantOff = false;
  function disable() {
    if (busy) { wantOff = true; return; }   // a start is still running: off comes right after it
    const dir = window.KairoVideoDirector;
    // Taking live video off air is deliberate, and so is changing the mode under a stream or
    // recording: the first choice arms it and says what will happen.
    const videoLive = !!(dir && dir.isLive());
    const cap = capturing();
    if ((videoLive || cap) && armedFor !== 'off') {
      const parts = [];
      if (videoLive) parts.push('Video is live.');
      if (cap) parts.push(cap);
      parts.push(videoLive ? 'Choose again to go to no video and turn off video director mode.' : 'Choose again to turn video director mode off.');
      arm('off', parts.join(' '));
      return;
    }
    disarm();
    save(false);
    paint(false, false);
    // The Director screen may be hidden (and, after a restart, not even loaded): video still comes off air.
    if (dir) dir.offAir(); else ensureLoaded().then(() => window.KairoVideoDirector.offAir()).catch(() => {});
    refreshOutputsUi();
    emitDirectorChange('stopped');
  }

  function toggle() { return isDirectorOn() ? disable() : enable(true); }
  // The top bar's video button only shows or hides the Director screen (owner, 2026-10-17):
  // hiding it leaves the cameras on air and the outputs as they are. With video off it turns
  // video on. Turning video off is the switch in Settings -> Director mode (or the View menu).
  async function toggleView() {
    if (busy) return;
    if (!isDirectorOn()) return enable(true);
    const dir = window.KairoVideoDirector;
    if (viewShown()) {
      dir.stop({ keepVideo: true });
      settings.directorView = false; saveSettingsPatch({ directorView: false });
      paint(true, false);
      refreshOutputsUi();
      return;
    }
    settings.directorView = true; saveSettingsPatch({ directorView: true });
    busy = true;
    try { await ensureLoaded(); await window.KairoVideoDirector.start(); } finally { busy = false; }
    paint(true, false);
    refreshOutputsUi();
  }
  btn.addEventListener('click', toggleView);
  // The switch does what the button and the View menu item do. A first click that only
  // armed (video is live, or a stream or recording is running) leaves it as it was and shows the line under it.
  if (dmToggle) dmToggle.addEventListener('change', () => { toggle(); dmToggle.checked = armedFor === 'off' || (armedFor !== 'on' && isDirectorOn()); });
  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen('menu-video-director', toggle);
  }

  // ── What a Layout changed on the outputs during a Director session ──
  // Kept in settings (not only in memory), so a crash or a quit with a Layout applied can't
  // leave the normal screen with, say, the Bible layer off: it is put back when the mode
  // ends, or at the next start with the mode off. Only what a Layout (or Director turning
  // video on by itself) changed is listed, each with the value it had before (`was`) and the
  // value Director set (`set`). Putting back touches a layer or overlay only while it still
  // shows what Director set: one the operator changed by hand afterwards is theirs and stays.
  // Shape: { layers: { outputId: { key: { was, set } } }, overlays: { overlayId: { was, set } } }.
  const readBaseline = () => {
    const b = settings.directorBaseline;
    return b && typeof b === 'object' ? b : null;
  };
  function writeBaseline(b) {
    settings.directorBaseline = b;
    saveSettingsPatch({ directorBaseline: b });
  }
  const isEntry = (e) => !!e && typeof e === 'object' && typeof e.was === 'boolean' && typeof e.set === 'boolean';
  window.KairoDirectorBaseline = {
    // changes: { layers: [[outputId, key, was, set]], overlays: [[overlayId, was, set]] }.
    // The first `was` wins (the state from before any Layout); `set` follows the latest change.
    record(changes) {
      const L = (changes && changes.layers) || [], O = (changes && changes.overlays) || [];
      if (!L.length && !O.length) return;
      const b = readBaseline() || {};
      const layers = { ...(b.layers && typeof b.layers === 'object' ? b.layers : {}) };
      const overlays = { ...(b.overlays && typeof b.overlays === 'object' ? b.overlays : {}) };
      for (const [id, k, was, set] of L) {
        const ly = { ...(layers[id] && typeof layers[id] === 'object' ? layers[id] : {}) };
        ly[k] = { was: isEntry(ly[k]) ? ly[k].was : !!was, set: !!set };
        layers[id] = ly;
      }
      for (const [id, was, set] of O) overlays[id] = { was: isEntry(overlays[id]) ? overlays[id].was : !!was, set: !!set };
      writeBaseline({ layers, overlays });
    },
    async restore() {
      const b = readBaseline();
      if (!b) return;
      // Cleared first: a session started while the overlays load records a list of its own.
      writeBaseline(null);
      if (b.layers && typeof b.layers === 'object' && typeof outputLayerMap === 'function' && typeof setOutputLayer === 'function') {
        const now = outputLayerMap();
        for (const [id, ly] of Object.entries(b.layers)) {
          if (!ly || typeof ly !== 'object' || !now[id]) continue;
          for (const [k, e] of Object.entries(ly)) if (isEntry(e) && now[id][k] === e.set && e.set !== e.was) setOutputLayer(id, k, e.was);
        }
      }
      const ov = b.overlays && typeof b.overlays === 'object' ? Object.entries(b.overlays).filter(([, e]) => isEntry(e) && e.set !== e.was) : [];
      if (ov.length && window.KairoOverlays && window.KairoOverlays.load) {
        const items = await window.KairoOverlays.load();
        const live = new Map(items.map(it => [it.id, !!it.live]));
        // A refused change is reported by the app's request watcher (problems.js).
        for (const [id, e] of ov) if (live.get(id) === e.set) fetch(`${SERVER}/api/overlays/${id}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ live: e.was }) });
      }
    },
  };

  // Called by loadSettings (app.js) once the saved settings are in.
  window.initVideoSources = async function () {
    // Simulated cameras: keep the four test inputs in step with the flag.
    try { if (window.KairoSimCameras && (window.KairoSimCameras.isOn() || KairoCameras.list().some(x => /^sim-cam-/.test(x.deviceId || '')))) syncSimSources(window.KairoSimCameras.isOn()); } catch {}
    // An earlier build swapped an output's own theme for a screen theme: put the saved normal themes back once.
    try {
      const saved = settings.directorScreenOrig;
      if (saved && typeof saved === 'object' && Object.keys(saved).length && typeof outputThemeMap === 'function') {
        const patch = {};
        for (const [id, t] of Object.entries(saved)) if (t) patch[id] = t;
        if (Object.keys(patch).length) { settings.outputThemes = { ...outputThemeMap(), ...patch }; saveSettingsPatch({ outputThemes: settings.outputThemes }); if (typeof applyOutputThemes === 'function') applyOutputThemes(); }
        settings.directorScreenOrig = {}; saveSettingsPatch({ directorScreenOrig: {} });
      }
    } catch {}
    // Video on with the Director screen hidden last time: the normal screen, cameras as they were.
    if (isDirectorOn() && settings.directorView === false) { paint(true, false); return; }
    if (isDirectorOn()) { await enable(false); return; }
    // Mode off: a Layout's layer and overlay changes left by a crash go back as they were.
    window.KairoDirectorBaseline.restore().catch((err) => console.warn('[Video] could not restore the outputs from before a Layout:', err.message));
    // Mode off: no screen frame may be left around an output (a crash could have left one).
    try {
      if (settings.outputFrames && Object.keys(settings.outputFrames).length) { settings.outputFrames = {}; saveSettingsPatch({ outputFrames: {} }); if (typeof applyOutputFrames === 'function') applyOutputFrames(); }
    } catch {}
    // Mode off: no display window may be left holding a camera for a source
    // chosen in an earlier session.
    try { if (localStorage.getItem('kairo-video-live') !== '{}') localStorage.setItem('kairo-video-live', '{}'); } catch {}
  };
})();
