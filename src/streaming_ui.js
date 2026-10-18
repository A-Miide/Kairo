// Going live (src-tauri stream.rs): the Go live button in the top bar and on Settings →
// Streaming, and the live status both show. Recording (the Record button) is the same
// engine writing an MP4 instead (lib.rs record_*), and starts and stops on its own.
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
  let preparing = false;   // Go live pressed, checks still running
  let listeners = [];
  let topBtn = null;

  const readJson = (k, f) => { try { const v = JSON.parse(localStorage.getItem(k) || 'null'); return v == null ? f : v; } catch { return f; } };
  const primary = () => (typeof PRIMARY_DISPLAY !== 'undefined' ? PRIMARY_DISPLAY : 'display-1');
  const cfg = () => (typeof settings === 'object' && settings && settings.stream) || {};
  const ready = (d) => d && d.enabled !== false && /^rtmps?:\/\/[^/\s:]+/i.test(String(d.url || '').trim());

  function notify() { listeners.forEach(fn => { try { fn(); } catch {} }); renderTopButton(); renderRecordButton(); }

  // The stream and a recording of the same output share its capture window: it closes when
  // neither uses it any more.
  const captureUsers = {};
  async function useCapture(output, who, w, h) {
    (captureUsers[output] = captureUsers[output] || new Set()).add(who);
    try { await window.KairoCapture?.open(`stream-${output}`, w, h, { output, stream: true }); }
    catch (err) {
      // No graphics window: the stream would carry the camera and sound only. Said in the warnings.
      captureOk = false;
      console.warn('[Stream] capture window failed:', err);
    }
  }
  function dropCapture(output, who) {
    const u = captureUsers[output];
    if (u) u.delete(who);
    if (!u || !u.size) { delete captureUsers[output]; window.KairoCapture?.close(`stream-${output}`); }
  }

  // The cameras the engine opens. Each carries its place among the devices of its name, from the
  // input's own device id: two cameras of one model share a name, and FFmpeg is only told names.
  async function engineCameras() {
    const sources = KairoCameras.list();
    let devices = [];
    try { devices = (await navigator.mediaDevices.enumerateDevices()).filter(d => d.kind === 'videoinput'); } catch {}
    const V = window.KairoVideoSources;
    return sources
      .filter(s => s && s.enabled !== false && (s.deviceId || s.label) && s.label)
      .map(s => {
        const cam = { id: s.id, label: s.label };
        try {
          const dev = V && V.resolveDevice(s, devices, sources);
          const same = devices.filter(d => (d.label || '').toLowerCase() === s.label.toLowerCase());
          const at = same.findIndex(d => d.deviceId === dev);
          if (dev && at >= 0) cam.slot = at;
        } catch {}
        return cam;
      });
  }
  // A camera added or changed while streaming or recording: the engine says which are not in the
  // picture yet (its status line in the panel). It never restarts a live stream for it.
  let camerasTimer = null;
  window.addEventListener('kairo:cameras', () => {
    clearTimeout(camerasTimer);
    camerasTimer = setTimeout(async () => {
      if (!invoke || !(live || recording) || !isDirectorOn()) return;
      try { await invoke('stream_cameras', { cameras: await engineCameras() }); } catch {}
    }, 400);
  });

  // What each pipeline is made of besides where it goes: size, cameras, zones, sound.
  async function pipelineBase(output, videoKbps) {
    const c = cfg();
    const [w, h] = SIZES[(c.video || {}).resolution] || SIZES['1080p'];
    const fps = Number((c.video || {}).fps) || 30;
    // Cameras belong to Director mode; placeholders (no device yet) can't be opened.
    const director = isDirectorOn();
    const cameras = director ? await engineCameras() : [];
    // Every video zone the screen themes have: the engine prepares the camera for each.
    const zones = director && typeof looks !== 'undefined' && Array.isArray(looks)
      ? looks.filter(l => l && l.groupId === 'grp-screen').flatMap(l => (l.layers || []).filter(x => x.type === 'videozone' && x.visible !== false && x.pos).map(x => ({ x: x.pos.x, y: x.pos.y, w: x.pos.w, h: x.pos.h })))
      : [];
    const liveMap = readJson('kairo-video-live', {});
    const layers = readJson('kairo-output-layers', {});
    const audioInput = (c.audio || {}).input === false ? null : ((await audioLabel()) || null);
    return {
      output, width: w, height: h, fps,
      videoKbps, audioKbps: Number((c.audio || {}).bitrate) || 160,
      destinations: [], cameras,
      program: (layers[output] && layers[output].video && liveMap[output]) || null,
      zone: null, zones, audioInput,
      // The sound mixer's channels (sound_mixer.js); the engine mixes them.
      audioMix: (window.KairoMixer && window.KairoMixer.forEngine()) || [],
    };
  }

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

  // A second click while Go live (or Record) is still starting is ignored: it used to start a
  // second FFmpeg sending to the same stream keys. (The engine also takes starts one at a time.)
  let startingStream = false, startingRecord = false;
  async function start() {
    if (startingStream || live) return;
    startingStream = true;
    try { await startNow(); } finally { startingStream = false; }
  }
  async function startNow() {
    if (!invoke) { alertMsg('Streaming needs the Kairo app; it does not work in a browser.'); return; }
    const c = cfg();
    const dests = (c.destinations || []).filter(ready);
    if (!dests.length) { alertMsg('Add a destination and switch it on under Settings → Streaming first.'); return; }
    const noKey = dests.filter(d => !String(d.key || '').trim());
    if (noKey.length && !(await ask(`${noKey.map(d => d.name || d.url).join(', ')} ${noKey.length > 1 ? 'have' : 'has'} no stream key. Go live anyway?`, 'Go live'))) return;
    // The camera, capture and FFmpeg checks take a few seconds: say so.
    preparing = true; renderTopButton();
    try { await startPrepared(c, dests); } finally { preparing = false; renderTopButton(); }
  }
  async function startPrepared(c, dests) {
    const byOutput = new Map();
    dests.forEach(d => { const o = d.output || primary(); if (!byOutput.has(o)) byOutput.set(o, []); byOutput.get(o).push(d); });
    const pipelines = [];
    for (const [output, ds] of byOutput) {
      const p = await pipelineBase(output, Number((c.video || {}).bitrate) || 6000);
      p.destinations = ds.map(d => ({ id: d.id, name: d.name || '', url: String(d.url).trim(), key: String(d.key || '').trim() }));
      pipelines.push(p);
    }
    // The pictures first, so the stream starts with Kairo's graphics on it.
    captureOk = !!(window.KairoCapture && await window.KairoCapture.isSupported());
    if (!captureOk && !(await ask('This computer can\'t capture Kairo\'s graphics for the stream yet, so it would carry the camera and sound only. Go live anyway?', 'Go live'))) return;
    for (const p of pipelines) await useCapture(p.output, 'stream', p.width, p.height);
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
      pipelines.forEach(p => dropCapture(p.output, 'stream'));
    }
    notify();
  }

  async function stop(confirmFirst = true) {
    if (confirmFirst && !(await ask('Stop the live stream on every destination?', 'Stop stream', true))) return;
    try { await invoke?.('stream_stop'); } catch (err) {
      // Not confirmed stopped: keep showing it live rather than claim it ended.
      alertMsg(`The stream may still be running: ${err && err.message ? err.message : err}`);
      return;
    }
    Object.keys(status).forEach(o => dropCapture(o, 'stream'));
    Object.keys(status).forEach(o => delete status[o]);
    live = false;
    notify();
    // A recording that was the stream's own picture goes on by itself (a new file).
    continueRecordingOnItsOwn();
  }

  function ask(message, label, danger) {
    if (typeof confirmDialog === 'function') return confirmDialog(message, { title: 'Streaming', confirmLabel: label, danger: !!danger });
    return Promise.resolve(window.confirm(message));
  }
  function alertMsg(m) {
    if (window.kairoProblem) { window.kairoProblem(m); return; }
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

  // ── Recording ──
  const recStatus = {};        // output → the recorder's status for it
  const recRuns = {};
  let recording = false;
  let lastFiles = [];          // the newest recording's files, for "Show in Finder"
  const rcfg = () => cfg().record || {};
  const stamp = () => { const d = new Date(), p = (n) => String(n).padStart(2, '0'); return `Kairo ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}`; };

  // `going` (a recording carrying on by itself) never asks: nobody is at the button mid-service.
  async function startRecording(going = false) {
    if (startingRecord || recording) return;
    startingRecord = true;
    try { await startRecordingNow(going); } finally { startingRecord = false; }
  }
  async function startRecordingNow(going) {
    if (!invoke) { alertMsg('Recording needs the Kairo app; it does not work in a browser.'); return; }
    if (recording) return;
    const r = rcfg();
    const known = typeof displayOutputs === 'function' ? displayOutputs().map(o => o.id) : [];
    const outputs = [known.includes(r.output) ? r.output : primary()];
    const pipelines = [];
    for (const o of outputs) pipelines.push(await pipelineBase(o, Number(r.bitrate) || 12000));
    captureOk = !!(window.KairoCapture && await window.KairoCapture.isSupported());
    if (!captureOk && !going && !(await ask('This computer can\'t capture Kairo\'s graphics yet, so the recording would have the camera and sound only. Record anyway?', 'Record'))) return;
    for (const p of pipelines) await useCapture(p.output, 'record', p.width, p.height);
    try {
      recWhy = '';
      lastFiles = await invoke('record_start', { pipelines, folder: r.folder || null, name: stamp() }) || [];
      recording = true;
      const now = await invoke('record_status').catch(() => []);
      (now || []).forEach(takeRec);
      pipelines.forEach(p => { if (!recStatus[p.output]) recStatus[p.output] = { output: p.output, state: 'starting', files: lastFiles }; });
      if (!captureOk) watchProgramWithoutCapture();
    } catch (e) {
      alertMsg(String(e && e.message || e));
      pipelines.forEach(p => dropCapture(p.output, 'record'));
    }
    notify();
  }
  async function stopRecording(confirmFirst = true) {
    if (confirmFirst && !(await ask('Stop recording? What has been recorded so far is kept.', 'Stop recording', true))) return;
    try { await invoke?.('record_stop'); } catch (err) {
      alertMsg(`The recording may still be running: ${err && err.message ? err.message : err}`);
      return;
    }
    Object.keys(recStatus).forEach(o => { lastFiles = recStatus[o].files && recStatus[o].files.length ? recStatus[o].files : lastFiles; dropCapture(o, 'record'); delete recStatus[o]; });
    recording = false;
    notify();
    // No pop-up: the Record panel shows it as "Last recording" with Show in Finder.
    if (lastFiles.length) { try { window.kairoLog && window.kairoLog('record-saved', { file: lastFiles[lastFiles.length - 1].split(/[\\/]/).pop(), parts: lastFiles.length }); } catch {} }
  }
  let recWhy = '';             // what the engine said when the recording last restarted
  let recFromStream = false;   // the recording is the stream's own picture (one encode), not its own pipeline
  // The stream ended while the recording was copying it: the recording starts again on its own (a new file),
  // so it never just stops.
  async function continueRecordingOnItsOwn() {
    if (!recording || !recFromStream) return;
    recFromStream = false;
    try { await invoke('record_stop'); } catch {}
    Object.keys(recStatus).forEach(o => { dropCapture(o, 'record'); delete recStatus[o]; });
    recording = false;
    notify();
    try { window.kairoLog && window.kairoLog('record-continued-alone', {}); } catch {}
    await startRecording(true);
  }
  function takeRec(s) {
    if (!s || !s.output) return;
    if ((recRuns[s.output] || 0) > (s.run || 0)) return;
    recRuns[s.output] = s.run || 0;
    if (s.state !== 'stopped') recFromStream = !!s.fromStream;
    // Kept in the log too: the restart's reason is otherwise gone once the status clears.
    if (s.state === 'retrying' || s.state === 'failed') {
      const why = String(s.error || '').slice(0, 200);
      if (why && why !== recWhy) { recWhy = why; try { window.kairoLog && window.kairoLog('record-trouble', { output: s.output, state: s.state, error: why, warnings: s.warnings || [] }); } catch {} }
    }
    if (s.files && s.files.length) lastFiles = s.files;
    if (s.state === 'stopped') delete recStatus[s.output]; else recStatus[s.output] = s;
  }
  function recOverall() {
    const all = Object.values(recStatus);
    if (!all.length) return { state: 'off', files: lastFiles, why: recWhy };
    const states = all.map(s => s.state);
    const state = states.includes('live') ? 'live' : states.includes('retrying') ? 'retrying' : states.every(s => s === 'failed') ? 'failed' : 'starting';
    const warnings = all.flatMap(s => s.warnings || []);
    if (!captureOk) warnings.push('Kairo\'s graphics are not in the recording on this computer: camera and sound only.');
    return { state, uptime: Math.max(0, ...all.map(s => s.uptimeSecs || 0)), warnings, errors: all.map(s => s.error).filter(Boolean), files: lastFiles, why: recWhy };
  }
  // The Record button: shown in Director mode, when switched on under Settings → Streaming,
  // or while recording.
  let recBtn = null;
  function renderRecordButton() {
    const listen = document.getElementById('listen-btn');
    if (!listen) return;
    if (!recBtn) {
      recBtn = document.createElement('button');
      recBtn.id = 'record-btn';
      recBtn.type = 'button';
      recBtn.className = 'tb-stream-btn tb-record-btn';
      recBtn.addEventListener('click', () => (recording ? stopRecording() : startRecording()));
      listen.parentNode.insertBefore(recBtn, document.getElementById('stream-btn') || listen);
    }
    const director = isDirectorOn();
    recBtn.hidden = !recording && !(director || rcfg().button === true);
    const o = recOverall();
    recBtn.classList.toggle('is-live', recording && o.state === 'live');
    recBtn.classList.toggle('is-trouble', recording && (o.state === 'retrying' || o.state === 'failed'));
    // The red dot is the button's own (CSS), so the words carry none.
    recBtn.textContent = !recording ? 'Record'
      : o.state === 'live' ? `REC ${clock(o.uptime || 0)}`
      : o.state === 'retrying' ? 'REC restarting…' : o.state === 'failed' ? 'Recording failed' : 'REC…';
    recBtn.title = recording ? 'Click to stop recording' : 'Record what the output shows to a video file';
  }
  const reveal = (path) => invoke?.('record_reveal', { path }).catch(e => alertMsg(String(e && e.message || e)));

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
    // Live but a destination isn't receiving it: still LIVE, marked as trouble.
    const destDown = Object.values(status).some(s => s.state === 'live' && (s.destinations || []).some(d => d.state !== 'live'));
    topBtn.classList.toggle('is-trouble', live && (o.state === 'retrying' || o.state === 'failed' || destDown));
    topBtn.disabled = preparing && !live;
    topBtn.textContent = preparing && !live ? '● Starting…' : !live ? 'Go live'
      : o.state === 'live' ? `● LIVE ${clock(o.uptime || 0)}`
      : o.state === 'retrying' ? '● Reconnecting…' : o.state === 'failed' ? '● Stream failed' : '● Starting…';
    topBtn.title = live ? 'Click to stop the stream' : 'Start streaming to the destinations under Settings → Streaming';
  }

  // One status from the engine; an older run's (a stream stopped before this Go live) is ignored.
  function take(s) {
    if (!s || !s.output) return;
    if ((runs[s.output] || 0) > (s.run || 0)) return;
    runs[s.output] = s.run || 0;
    // The stream a recording copies has ended (other outputs may still be streaming): it records on its own.
    if (s.state === 'stopped') { delete status[s.output]; if (recFromStream && recStatus[s.output]) continueRecordingOnItsOwn(); } else status[s.output] = s;
    health();
  }

  // While live, a destination reconnecting and every engine warning reach the
  // problem strip, so they're seen from the Director screen, not only in
  // Settings → Streaming. Raised once on the way into trouble, cleared on the way out.
  const raised = new Set();
  function health() {
    const now = new Set();
    for (const s of Object.values(status)) {
      if (s.state !== 'live') continue;
      for (const d of s.destinations || []) {
        if (d.state === 'live') continue;
        const key = `stream-dest-${d.id}`;
        now.add(key);
        if (!raised.has(key)) window.kairoProblem?.(`${d.name || 'A stream destination'} isn't receiving the stream: ${d.state === 'reconnecting' ? 'reconnecting' : 'connecting'}${d.error ? ` (${d.error})` : ''}.`, key);
      }
    }
    for (const w of overall().warnings || []) {
      const key = `stream-warn-${w}`;
      now.add(key);
      if (!raised.has(key)) window.kairoProblem?.(w, key);
    }
    for (const key of raised) if (!now.has(key)) window.kairoProblemCleared?.(key);
    raised.clear();
    now.forEach(k => raised.add(k));
  }
  // Without a capture window, the main window tells the engine which camera is on air.
  let watching = false;
  function watchProgramWithoutCapture() {
    if (watching) return;
    watching = true;
    const send = () => {
      const liveMap = readJson('kairo-video-live', {}), layers = readJson('kairo-output-layers', {});
      [...new Set([...Object.keys(status), ...Object.keys(recStatus)])].forEach(o => invoke('stream_program', { output: o, program: (layers[o] && layers[o].video && liveMap[o]) || null, zone: null }).catch(() => {}));
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
    T.event.listen('record-status', (e) => {
      takeRec(e && e.payload);
      recording = Object.keys(recStatus).length > 0;
      notify();
    });
    invoke('record_status').then(list => {
      (list || []).forEach(takeRec);
      recording = Object.keys(recStatus).length > 0;
      notify();
    }).catch(() => {});
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
    /* Record: a red dot, like every record button; while recording, a red button with the dot pulsing. */
    .tb-record-btn { display: inline-flex; align-items: center; gap: 7px; }
    .tb-record-btn::before { content: ''; width: 10px; height: 10px; border-radius: 50%; background: #ff3b30; flex: none; }
    .tb-record-btn:hover { border-color: #ff3b30; }
    .tb-record-btn.is-live { border-color: #ff3b30; background: #8a1d17; color: #fff; }
    .tb-record-btn.is-live::before { background: #fff; animation: kairo-rec-pulse 1.4s ease-in-out infinite; }
    .tb-record-btn.is-trouble::before { background: #ffcf66; }
    @keyframes kairo-rec-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.25; } }
    @media (prefers-reduced-motion: reduce) { .tb-record-btn.is-live::before { animation: none; } }
  `;
  document.head.appendChild(style);

  window.KairoStreaming = {
    start, stop, isLive: () => live, overall, destStatus, available: !!invoke,
    startRecording, stopRecording, isRecording: () => recording, recOverall, reveal,
    onChange(fn) { listeners.push(fn); },
  };
  const renderButtons = () => { renderTopButton(); renderRecordButton(); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', renderButtons); else renderButtons();
  // Destinations are added or switched on in Settings, Director mode turned on: show the buttons then.
  onSettingsChange(renderButtons);
  onDirectorChange(renderButtons);
})();
