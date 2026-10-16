// The sound mixer for the stream and recordings (Settings → Streaming → Sound, and the
// Sound button in the top bar while live or recording). Each channel is an audio input with
// its own level and mute; the engine (stream.rs) mixes them, and level and mute changes go
// out live without a restart. A channel added or removed is used from the next start.
//
// Saved in settings.stream.mix: [{ id, deviceId, label, gainDb, muted, follow }]. The first
// channel follows the input chosen under Settings → Audio (follow: true) unless changed.
// The meters are measured here (Web Audio), only while a mixer is on screen.
(function () {
  const T = window.__TAURI__;
  const invoke = T && T.core && T.core.invoke;
  const MIN_DB = -30, MAX_DB = 10;
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
  const uid = () => 'mix-' + Math.random().toString(36).slice(2, 8);

  const streamCfg = () => (typeof settings === 'object' && settings && settings.stream) || {};
  // The channels as saved, or the one that follows Settings → Audio (muted if the old
  // "Sound desk or microphone" switch was off).
  function channels() {
    const m = streamCfg().mix;
    if (Array.isArray(m) && m.length) return m.filter(c => c && c.id);
    return [{ id: 'main', follow: true, gainDb: 0, muted: (streamCfg().audio || {}).input === false }];
  }
  let saveTimer = null;
  function save(next) {
    const s = { ...streamCfg(), mix: next };
    settings.stream = s;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => { if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ stream: settings.stream }); }, 300);
    pushLive();
  }

  // Inputs on this computer (labels need the microphone permission Kairo already has).
  let devices = [];
  async function loadDevices() {
    try { devices = (await navigator.mediaDevices.enumerateDevices()).filter(d => d.kind === 'audioinput' && d.deviceId !== 'default' && d.deviceId !== 'communications'); } catch { devices = []; }
  }
  const audioSel = () => document.getElementById('audio-source-settings');
  // The device a channel uses now: its own, or the one chosen under Settings → Audio.
  function deviceOf(c) {
    if (!c.follow) return { deviceId: c.deviceId || '', label: c.label || '' };
    const sel = audioSel();
    const opt = sel && sel.selectedOptions && sel.selectedOptions[0];
    if (sel && sel.value && opt) return { deviceId: sel.value, label: opt.textContent.trim() };
    const d = devices[0];
    return { deviceId: d ? d.deviceId : '', label: d ? d.label.replace(/^Default - /, '') : '' };
  }
  const nameOf = (c) => deviceOf(c).label || 'No input';

  // What the engine gets: every channel that has an input, by its name.
  function forEngine() {
    return channels().map(c => ({ id: c.id, label: deviceOf(c).label, gainDb: Number(c.gainDb) || 0, muted: !!c.muted })).filter(c => c.label);
  }
  function pushLive() {
    const S = window.KairoStreaming;
    if (!invoke || !S || !(S.isLive() || S.isRecording())) return;
    invoke('stream_mix', { mix: forEngine() }).catch(() => {});
  }

  // ── Meters ──
  let ctx = null;
  const meters = new Map();   // deviceId → { stream, analyser, buf, level }
  let raf = 0;
  const mounted = new Set();  // mixer roots on screen
  async function meterFor(deviceId) {
    if (!deviceId || meters.has(deviceId)) return;
    meters.set(deviceId, { level: -Infinity });
    try {
      ctx = ctx || new (window.AudioContext || window.webkitAudioContext)();
      const stream = await navigator.mediaDevices.getUserMedia({ audio: { deviceId: { exact: deviceId }, echoCancellation: false, noiseSuppression: false, autoGainControl: false } });
      if (!meters.has(deviceId)) { stream.getTracks().forEach(t => t.stop()); return; }
      const src = ctx.createMediaStreamSource(stream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 1024;
      src.connect(analyser);
      meters.set(deviceId, { stream, analyser, buf: new Float32Array(analyser.fftSize), level: -Infinity });
    } catch { /* no permission or the device went: no meter */ }
  }
  function stopMeters() {
    meters.forEach(m => m.stream && m.stream.getTracks().forEach(t => t.stop()));
    meters.clear();
    cancelAnimationFrame(raf); raf = 0;
  }
  function tick() {
    raf = 0;
    // No mixer on screen (Settings closed, the popover gone): let go of the inputs.
    const shown = (r) => r.isConnected && r.getClientRects().length > 0;
    if (![...mounted].some(shown)) { mounted.clear(); stopMeters(); return; }
    meters.forEach(m => {
      if (!m.analyser) return;
      m.analyser.getFloatTimeDomainData(m.buf);
      let peak = 0;
      for (let i = 0; i < m.buf.length; i++) { const v = Math.abs(m.buf[i]); if (v > peak) peak = v; }
      const db = peak > 0 ? 20 * Math.log10(peak) : -Infinity;
      // Fast up, slow down, like a desk.
      m.level = db > m.level ? db : Math.max(db, m.level - 1.2);
    });
    document.querySelectorAll('.mx-ch').forEach(row => {
      const c = channels().find(x => x.id === row.dataset.id);
      const m = c && meters.get(deviceOf(c).deviceId);
      const db = !c || c.muted || !m ? -Infinity : m.level + (Number(c.gainDb) || 0);
      const pct = Number.isFinite(db) ? Math.max(0, Math.min(100, ((db + 60) / 60) * 100)) : 0;
      const bar = row.querySelector('.mx-meter i');
      if (bar) { bar.style.width = `${pct}%`; bar.classList.toggle('is-hot', db > -3); }
    });
    raf = requestAnimationFrame(tick);
  }

  // ── The mixer ──
  function render(root) {
    root.innerHTML = '';
    root.classList.add('mx');
    const list = channels();
    list.forEach((c, i) => {
      const row = el('div', 'mx-ch' + (c.muted ? ' is-muted' : ''));
      row.dataset.id = c.id;
      const top = el('div', 'mx-top');
      const pick = el('select', 'setting-input mx-pick');
      if (i === 0) pick.appendChild(new Option(`Same as Audio settings${c.follow ? ` · ${nameOf(c)}` : ''}`, '__follow', false, !!c.follow));
      devices.forEach(d => pick.appendChild(new Option(d.label || 'Input', d.deviceId, false, !c.follow && d.deviceId === c.deviceId)));
      if (!c.follow && c.deviceId && !devices.some(d => d.deviceId === c.deviceId)) pick.appendChild(new Option(`${c.label || 'Input'} (not connected)`, c.deviceId, false, true));
      pick.addEventListener('change', () => {
        const next = channels().map(x => x.id !== c.id ? x : (pick.value === '__follow'
          ? { ...x, follow: true, deviceId: '', label: '' }
          : { ...x, follow: false, deviceId: pick.value, label: (devices.find(d => d.deviceId === pick.value) || {}).label || '' }));
        save(next); renderAll();
      });
      const mute = el('button', 'mx-mute' + (c.muted ? ' is-on' : ''), 'Mute');
      mute.type = 'button';
      mute.setAttribute('aria-pressed', c.muted ? 'true' : 'false');
      mute.addEventListener('click', () => { save(channels().map(x => x.id === c.id ? { ...x, muted: !x.muted } : x)); renderAll(); });
      top.append(pick, mute);
      if (list.length > 1) {
        const rm = el('button', 'mx-remove', '✕'); rm.type = 'button'; rm.title = 'Remove this input'; rm.setAttribute('aria-label', 'Remove this input');
        rm.addEventListener('click', () => { save(channels().filter(x => x.id !== c.id)); renderAll(); });
        top.appendChild(rm);
      }
      const bottom = el('div', 'mx-bottom');
      const meter = el('div', 'mx-meter'); meter.appendChild(document.createElement('i'));
      const fader = el('input', 'mx-fader'); fader.type = 'range'; fader.min = String(MIN_DB); fader.max = String(MAX_DB); fader.step = '0.5';
      fader.value = String(Number(c.gainDb) || 0);
      fader.setAttribute('aria-label', `${nameOf(c)} level`);
      const db = el('span', 'mx-db');
      const showDb = (v) => { db.textContent = `${v > 0 ? '+' : ''}${Number(v).toFixed(1)} dB`; };
      showDb(Number(fader.value));
      fader.addEventListener('input', () => { showDb(Number(fader.value)); save(channels().map(x => x.id === c.id ? { ...x, gainDb: Number(fader.value) } : x)); });
      fader.addEventListener('dblclick', () => { fader.value = '0'; fader.dispatchEvent(new Event('input')); });
      bottom.append(meter, fader, db);
      row.append(top, bottom);
      root.appendChild(row);
      meterFor(deviceOf(c).deviceId);
    });
    const foot = el('div', 'mx-foot');
    const add = el('button', 'modal-btn secondary', '+ Add input'); add.type = 'button';
    add.disabled = list.length >= 6 || !devices.length;
    add.addEventListener('click', () => {
      const used = new Set(channels().map(x => deviceOf(x).deviceId));
      const d = devices.find(x => !used.has(x.deviceId)) || devices[0];
      if (!d) return;
      save([...channels(), { id: uid(), deviceId: d.deviceId, label: d.label, gainDb: 0, muted: false }]);
      renderAll();
    });
    foot.appendChild(add);
    const S = window.KairoStreaming;
    if (S && (S.isLive() || S.isRecording())) foot.appendChild(el('span', 'setting-hint', 'Levels and mutes change straight away. An input added or removed is used the next time you go live or record.'));
    root.appendChild(foot);
    mounted.add(root);
    if (!raf) raf = requestAnimationFrame(tick);
  }
  function renderAll() { [...mounted].forEach(r => { if (r.isConnected) render(r); else mounted.delete(r); }); }
  async function mount(root) { await loadDevices(); render(root); }

  // ── The top bar's Sound button, while live or recording ──
  let btn = null, pop = null;
  function closePop() { if (pop) { pop.remove(); pop = null; document.removeEventListener('mousedown', outside, true); document.removeEventListener('keydown', esc, true); } }
  const outside = (e) => { if (pop && !pop.contains(e.target) && e.target !== btn) closePop(); };
  const esc = (e) => { if (e.key === 'Escape') { e.stopPropagation(); closePop(); } };
  async function openPop() {
    closePop();
    pop = el('div', 'mx-pop');
    pop.setAttribute('role', 'dialog');
    pop.setAttribute('aria-label', 'Sound');
    pop.appendChild(el('div', 'mx-pop-title', 'Sound for the stream and recording'));
    const body = el('div');
    pop.appendChild(body);
    const r = btn.getBoundingClientRect();
    pop.style.top = `${r.bottom + 6}px`; pop.style.left = `${Math.max(8, Math.min(window.innerWidth - 388, r.right - 380))}px`;
    document.body.appendChild(pop);
    document.addEventListener('mousedown', outside, true);
    document.addEventListener('keydown', esc, true);
    await mount(body);
  }
  function renderButton() {
    const listen = document.getElementById('listen-btn');
    const S = window.KairoStreaming;
    if (!listen || !S) return;
    if (!btn) {
      btn = el('button', 'tb-stream-btn tb-sound-btn', 'Sound');
      btn.type = 'button';
      btn.title = 'Sound levels for the stream and recording';
      btn.setAttribute('aria-haspopup', 'dialog');
      btn.addEventListener('click', () => (pop ? closePop() : openPop()));
      listen.parentNode.insertBefore(btn, document.getElementById('record-btn') || document.getElementById('stream-btn') || listen);
    }
    const on = S.isLive() || S.isRecording();
    btn.hidden = !on;
    if (!on) closePop();
    const muted = channels().every(c => c.muted);
    btn.classList.toggle('is-trouble', on && muted);
    btn.textContent = on && muted ? 'Sound muted' : 'Sound';
  }

  const style = document.createElement('style');
  style.textContent = `
    .mx { display: flex; flex-direction: column; gap: 4px; }
    .mx-ch { padding: 10px 0; border-bottom: 1px solid var(--border); display: flex; flex-direction: column; gap: 8px; }
    .mx-top { display: flex; gap: 8px; align-items: center; }
    .mx-pick { flex: 1; min-width: 0; }
    .mx-mute { height: 30px; padding: 0 12px; border-radius: 7px; border: 1px solid var(--border-md, rgba(255,255,255,0.14)); background: transparent; color: var(--text); font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; }
    .mx-mute.is-on { background: #5a2414; border-color: #8f3d1f; color: #ffe9e0; }
    .mx-remove { width: 28px; height: 30px; border-radius: 7px; border: 1px solid var(--border); background: transparent; color: var(--text-2, var(--text)); cursor: pointer; }
    .mx-bottom { display: grid; grid-template-columns: minmax(60px, 1fr) minmax(100px, 2fr) 64px; gap: 10px; align-items: center; }
    .mx-meter { height: 8px; border-radius: 4px; background: rgba(255,255,255,0.07); overflow: hidden; }
    .mx-meter i { display: block; height: 100%; width: 0; background: #6f9a5b; transition: width 60ms linear; }
    .mx-meter i.is-hot { background: #b5532c; }
    .mx-ch.is-muted .mx-meter i { width: 0 !important; }
    .mx-ch.is-muted .mx-fader { opacity: .45; }
    .mx-fader { width: 100%; accent-color: var(--accent, #ff6a3d); }
    .mx-db { font-size: 12px; color: var(--text-2, var(--text)); font-variant-numeric: tabular-nums; text-align: right; }
    .mx-foot { display: flex; align-items: center; gap: 10px; padding-top: 8px; flex-wrap: wrap; }
    .mx-foot .setting-hint { margin: 0; }
    .mx-pop { position: fixed; z-index: 2000; width: 380px; max-width: calc(100vw - 16px); max-height: 70vh; overflow: auto; padding: 12px 14px; border-radius: 12px; border: 1px solid var(--border); background: var(--bg-elevated, #1b1b1f); box-shadow: 0 16px 40px rgba(0,0,0,.45); }
    .mx-pop-title { font-size: 12px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--text-dim, var(--text)); margin-bottom: 4px; }
    .tb-sound-btn[hidden] { display: none; }
  `;
  document.head.appendChild(style);

  window.KairoMixer = { mount, forEngine, channels, isMounted: (r) => mounted.has(r) };
  const hook = () => { if (window.KairoStreaming) window.KairoStreaming.onChange(renderButton); else setTimeout(hook, 500); };
  hook();
  setInterval(renderButton, 1500);
})();
