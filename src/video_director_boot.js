// KAIRO — Video director mode: the switch, and nothing else.
//
// This is the only video file the main window always loads. With the mode off
// it adds one small top-bar button and a menu listener, and makes sure no
// display window is told to open a camera. Turning the mode on loads the rest
// (video_sources.js, video_hub.js, sources_panel.js, video_director.js and
// the stylesheet); turning it off unloads the UI and releases every capture.
// The setting persists as settings.videoDirector.
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
        for (const f of FILES) await loadScript(f);
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
              <p class="setting-hint">A screen for cameras and inputs: Preview and Output, saved Layouts, and the playlist and media bin. Scripture and lower-thirds draw over the video. Turn it off and the normal screen comes back.</p>
            </div>
            <label class="output-toggle"><input type="checkbox" id="dm-toggle" aria-labelledby="dm-name"><span class="output-toggle-track"></span></label>
          </div>
          <p class="dm-armed" id="dm-armed" role="status" hidden>Video is live. Switch off again to go to no video and turn off.</p>
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
    let list = Array.isArray(settings.videoSources) ? settings.videoSources.filter(x => x && typeof x === 'object') : [];
    if (on) {
      sim.ids().forEach((id, i) => {
        if (list.some(x => x.deviceId === id)) return;
        list.push({ id: 'vid-sim' + (i + 1), name: 'Sim ' + (i + 1), deviceId: id, label: 'Simulated camera ' + (i + 1), width: 1920, height: 1080, fps: 30, enabled: true });
      });
    } else {
      if (!list.some(isSim)) return;
      list = list.filter(x => !isSim(x));
    }
    settings.videoSources = list;
    const live = settings.videoLive && typeof settings.videoLive === 'object' ? { ...settings.videoLive } : {};
    for (const k of Object.keys(live)) if (!list.some(x => x.id === live[k])) live[k] = null;
    settings.videoLive = live;
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ videoSources: list, videoLive: live });
    try { localStorage.setItem('kairo-video-sources', JSON.stringify(list)); } catch {}
  }

  function paint(on, armed) {
    if (dmToggle) dmToggle.checked = !!on;
    if (dmArmed) dmArmed.hidden = !armed;
    btn.classList.toggle('active', on);
    btn.classList.toggle('vd-armed', !!armed);
    btn.setAttribute('aria-pressed', String(on));
    const label = armed ? 'Video is live. Choose again to go to no video and turn off video director mode'
      : on ? 'Video director mode is on. Choose to turn it off' : 'Video director mode';
    btn.title = label;
    btn.setAttribute('aria-label', label);
  }
  paint(false, false);

  function refreshOutputsUi() {
    try { if (typeof renderOutputsList === 'function') renderOutputsList(); } catch {}
  }
  function save(on) {
    settings.videoDirector = on;
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ videoDirector: on });
  }

  async function enable(persist) {
    if (busy) return;
    busy = true;
    try {
      if (persist) save(true);
      paint(true, false);
      await ensureLoaded();
      await window.KairoVideoDirector.start();
      refreshOutputsUi();
    } catch (err) {
      console.warn('[Video] director mode could not start:', err.message);
      save(false);
      paint(false, false);
    } finally { busy = false; }
  }

  function disable() {
    const dir = window.KairoVideoDirector;
    // Taking live video off air is deliberate: the first choice arms it.
    if (dir && dir.isLive() && !btn.classList.contains('vd-armed')) {
      paint(true, true);
      clearTimeout(armedTimer);
      armedTimer = setTimeout(() => paint(!!settings.videoDirector, false), 5000);
      return;
    }
    clearTimeout(armedTimer);
    save(false);
    paint(false, false);
    if (dir) dir.stop();
    refreshOutputsUi();
  }

  function toggle() { return settings.videoDirector ? disable() : enable(true); }
  btn.addEventListener('click', toggle);
  // The switch does what the button and the View menu item do. A refused
  // first click (video is live) leaves it on and shows the line under it.
  if (dmToggle) dmToggle.addEventListener('change', () => { toggle(); dmToggle.checked = !!settings.videoDirector || btn.classList.contains('vd-armed'); });
  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen('menu-video-director', toggle);
  }

  // Called by loadSettings (app.js) once the saved settings are in.
  window.initVideoSources = async function () {
    // Simulated cameras: keep the four test inputs in step with the flag.
    try { if (window.KairoSimCameras && (window.KairoSimCameras.isOn() || (settings.videoSources || []).some(x => x && /^sim-cam-/.test(x.deviceId || '')))) syncSimSources(window.KairoSimCameras.isOn()); } catch {}
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
    if (settings.videoDirector) { await enable(false); return; }
    // Mode off: no screen frame may be left around an output (a crash could have left one).
    try {
      if (settings.outputFrames && Object.keys(settings.outputFrames).length) { settings.outputFrames = {}; saveSettingsPatch({ outputFrames: {} }); if (typeof applyOutputFrames === 'function') applyOutputFrames(); }
    } catch {}
    // Mode off: no display window may be left holding a camera for a source
    // chosen in an earlier session.
    try { if (localStorage.getItem('kairo-video-live') !== '{}') localStorage.setItem('kairo-video-live', '{}'); } catch {}
  };
})();
