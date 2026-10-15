// Going live (src-tauri stream.rs): the Go live button in the top bar and on Settings →
// Streaming, and the live status both show.
//
// Each output being streamed gets a hidden capture window (output_capture.js,
// `kairo-cap-stream-<output>`, display.html?stream=1): Kairo's graphics for that output,
// see-through where the camera goes. The app's FFmpeg puts the Director's camera under it,
// adds the audio input chosen under Settings → Audio, encodes once and sends to every
// destination of that output. The capture window itself tells the engine which camera is
// on air and where the layout's video zone is.
(function () {
  const T = window.__TAURI__;
  const invoke = T && T.core && T.core.invoke;
  const SIZES = { '720p': [1280, 720], '1080p': [1920, 1080] };
  const status = {};          // output → the engine's status for it
  const runs = {};            // output → the newest run seen (a Go live after a Stop is a new run)
  let captureOk = true;       // can this computer capture the outputs' graphics?
  let live = false;
  let listeners = [];
  let topBtn = null;

  const readJson = (k, f) => { try { const v = JSON.parse(localStorage.getItem(k) || 'null'); return v == null ? f : v; } catch { return f; } };
  const primary = () => (typeof PRIMARY_DISPLAY !== 'undefined' ? PRIMARY_DISPLAY : 'display-1');
  const cfg = () => (typeof settings === 'object' && settings && settings.stream) || {};
  const ready = (d) => d && d.enabled !== false && /^rtmps?:\/\/[^/\s:]+/i.test(String(d.url || '').trim());

  function notify() { listeners.forEach(fn => { try { fn(); } catch {} }); renderTopButton(); }

  // The audio input chosen under Settings → Audio, by its name (FFmpeg finds devices by name).
  async function audioLabel() {
    const sel = document.getElementById('audio-source-settings');
    const opt = sel && sel.selectedOptions && sel.selectedOptions[0];
    if (sel && sel.value && opt) return opt.textContent.trim();
    try {
      const devs = await navigator.mediaDevices.enumerateDevices();
      const d = devs.find(x => x.kind === 'audioinput' && x.deviceId === 'default') || devs.find(x => x.kind === 'audioinput');
      return d ? d.label.replace(/^Default - /, '') : '';
    } catch { return ''; }
  }

  async function start() {
    if (!invoke) { alertMsg('Streaming needs the Kairo app; it does not work in a browser.'); return; }
    const c = cfg();
    const dests = (c.destinations || []).filter(ready);
    if (!dests.length) { alertMsg('Add a destination and switch it on under Settings → Streaming first.'); return; }
    const noKey = dests.filter(d => !String(d.key || '').trim());
    if (noKey.length && !(await ask(`${noKey.map(d => d.name || d.url).join(', ')} ${noKey.length > 1 ? 'have' : 'has'} no stream key. Go live anyway?`, 'Go live'))) return;
    const [w, h] = SIZES[(c.video || {}).resolution] || SIZES['1080p'];
    const fps = Number((c.video || {}).fps) || 30;
    // Cameras belong to Director mode; placeholders (no device yet) can't be opened.
    const director = !!(typeof settings === 'object' && settings && settings.videoDirector);
    const cameras = director ? readJson('kairo-video-sources', [])
      .filter(s => s && s.enabled !== false && (s.deviceId || s.label) && s.label)
      .map(s => ({ id: s.id, label: s.label })) : [];
    // Every video zone the screen themes have: the stream prepares the camera for each.
    const zones = director && typeof looks !== 'undefined' && Array.isArray(looks)
      ? looks.filter(l => l && l.groupId === 'grp-screen').flatMap(l => (l.layers || []).filter(x => x.type === 'videozone' && x.visible !== false && x.pos).map(x => ({ x: x.pos.x, y: x.pos.y, w: x.pos.w, h: x.pos.h })))
      : [];
    const audioInput = await audioLabel();
    const liveMap = readJson('kairo-video-live', {});
    const layers = readJson('kairo-output-layers', {});
    const byOutput = new Map();
    dests.forEach(d => { const o = d.output || primary(); if (!byOutput.has(o)) byOutput.set(o, []); byOutput.get(o).push(d); });
    const pipelines = [...byOutput].map(([output, ds]) => ({
      output, width: w, height: h, fps,
      videoKbps: Number((c.video || {}).bitrate) || 6000,
      audioKbps: Number((c.audio || {}).bitrate) || 160,
      destinations: ds.map(d => ({ id: d.id, name: d.name || '', url: String(d.url).trim(), key: String(d.key || '').trim() })),
      cameras,
      program: (layers[output] && layers[output].video && liveMap[output]) || null,
      zone: null,
      zones,
      audioInput: (c.audio || {}).input === false ? null : (audioInput || null),
    }));
    // The pictures first, so the stream starts with Kairo's graphics on it.
    captureOk = !!(window.KairoCapture && await window.KairoCapture.isSupported());
    if (!captureOk && !(await ask('This computer can\'t capture Kairo\'s graphics for the stream yet, so it would carry the camera and sound only. Go live anyway?', 'Go live'))) return;
    for (const p of pipelines) {
      try { await window.KairoCapture?.open(`stream-${p.output}`, w, h, { output: p.output, stream: true }); } catch {}
    }
    try {
      await invoke('stream_start', { pipelines });
      live = true;
      // What the engine says now (it may already have reported, or failed).
      const now = await invoke('stream_status').catch(() => []);
      (now || []).forEach(take);
      pipelines.forEach(p => { if (!status[p.output]) status[p.output] = { output: p.output, state: 'starting', destinations: p.destinations.map(d => ({ id: d.id, name: d.name, state: 'connecting' })) }; });
      if (!captureOk) watchProgramWithoutCapture();
    } catch (e) {
      alertMsg(String(e && e.message || e));
      pipelines.forEach(p => window.KairoCapture?.close(`stream-${p.output}`));
    }
    notify();
  }

  async function stop(confirmFirst = true) {
    if (confirmFirst && !(await ask('Stop the live stream on every destination?', 'Stop stream', true))) return;
    try { await invoke?.('stream_stop'); } catch {}
    Object.keys(status).forEach(o => window.KairoCapture?.close(`stream-${o}`));
    Object.keys(status).forEach(o => delete status[o]);
    live = false;
    notify();
  }

  function ask(message, label, danger) {
    if (typeof confirmDialog === 'function') return confirmDialog(message, { title: 'Streaming', confirmLabel: label, danger: !!danger });
    return Promise.resolve(window.confirm(message));
  }
  function alertMsg(m) {
    if (typeof showToast === 'function') { showToast(m, 'error'); return; }
    window.alert(m);
  }

  // What every destination is doing, by its id.
  function destStatus(id) {
    for (const s of Object.values(status)) {
      const d = (s.destinations || []).find(x => x.id === id);
      if (d) return { ...d, pipeline: s.state };
    }
    return null;
  }
  function overall() {
    const all = Object.values(status);
    if (!all.length) return { state: 'off' };
    const states = all.map(s => s.state);
    const up = Math.max(0, ...all.map(s => s.uptimeSecs || 0));
    const state = states.includes('live') ? 'live' : states.includes('retrying') ? 'retrying' : states.every(s => s === 'failed') ? 'failed' : 'starting';
    const warnings = all.flatMap(s => s.warnings || []);
    if (!captureOk) warnings.push('Kairo\'s graphics are not in the stream on this computer: camera and sound only.');
    return { state, uptime: up, warnings, errors: all.map(s => s.error).filter(Boolean) };
  }
  const clock = (secs) => { const h = Math.floor(secs / 3600), m = Math.floor(secs / 60) % 60, s = secs % 60; return (h ? `${h}:` : '') + `${String(m).padStart(h ? 2 : 1, '0')}:${String(s).padStart(2, '0')}`; };

  // ── The top-bar button ──
  function renderTopButton() {
    const listen = document.getElementById('listen-btn');
    if (!listen) return;
    const anyDest = ((cfg().destinations) || []).some(ready);
    if (!topBtn) {
      topBtn = document.createElement('button');
      topBtn.id = 'stream-btn';
      topBtn.type = 'button';
      topBtn.className = 'tb-stream-btn';
      topBtn.addEventListener('click', () => (live ? stop() : start()));
      listen.parentNode.insertBefore(topBtn, listen);
    }
    topBtn.hidden = !anyDest && !live;
    const o = overall();
    topBtn.classList.toggle('is-live', live && o.state === 'live');
    topBtn.classList.toggle('is-trouble', live && (o.state === 'retrying' || o.state === 'failed'));
    topBtn.textContent = !live ? 'Go live'
      : o.state === 'live' ? `● LIVE ${clock(o.uptime || 0)}`
      : o.state === 'retrying' ? '● Reconnecting…' : o.state === 'failed' ? '● Stream failed' : '● Starting…';
    topBtn.title = live ? 'Click to stop the stream' : 'Start streaming to the destinations under Settings → Streaming';
  }

  // One status from the engine; an older run's (a stream stopped before this Go live) is ignored.
  function take(s) {
    if (!s || !s.output) return;
    if ((runs[s.output] || 0) > (s.run || 0)) return;
    runs[s.output] = s.run || 0;
    if (s.state === 'stopped') delete status[s.output]; else status[s.output] = s;
  }
  // Without a capture window, the main window tells the engine which camera is on air.
  let watching = false;
  function watchProgramWithoutCapture() {
    if (watching) return;
    watching = true;
    const send = () => {
      const liveMap = readJson('kairo-video-live', {}), layers = readJson('kairo-output-layers', {});
      Object.keys(status).forEach(o => invoke('stream_program', { output: o, program: (layers[o] && layers[o].video && liveMap[o]) || null, zone: null }).catch(() => {}));
    };
    window.addEventListener('storage', (e) => { if (e.key === 'kairo-video-live' || e.key === 'kairo-output-layers') send(); });
    setInterval(send, 2000);
  }

  if (T && T.event && T.event.listen) {
    T.event.listen('stream-status', (e) => {
      take(e && e.payload);
      live = Object.keys(status).length > 0;
      notify();
    });
    // A reload of this window while streaming: pick the state up again.
    invoke('stream_status').then(list => {
      (list || []).forEach(take);
      live = Object.keys(status).length > 0;
      notify();
    }).catch(() => {});
  }

  const style = document.createElement('style');
  style.textContent = `
    .tb-stream-btn { height: 30px; padding: 0 12px; border-radius: 999px; border: 1px solid var(--border-md, rgba(255,255,255,0.14)); background: transparent; color: var(--text); font: inherit; font-size: 12.5px; font-weight: 600; cursor: pointer; margin-right: 8px; white-space: nowrap; }
    .tb-stream-btn:hover { border-color: var(--accent, #ff6a3d); }
    .tb-stream-btn[hidden] { display: none; }
    .tb-stream-btn.is-live { border-color: #b5532c; background: #5a2414; color: #ffe9e0; font-variant-numeric: tabular-nums; }
    .tb-stream-btn.is-trouble { border-color: #b5832c; background: #4a3512; color: #ffeccc; }
  `;
  document.head.appendChild(style);

  window.KairoStreaming = {
    start, stop, isLive: () => live, overall, destStatus, available: !!invoke,
    onChange(fn) { listeners.push(fn); },
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', renderTopButton); else renderTopButton();
  // Destinations are added or switched on in Settings: show the button then.
  setInterval(renderTopButton, 1000);
})();
