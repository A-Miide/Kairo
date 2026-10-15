// The audio lane: sound that plays on its own, apart from slides, scripture and
// video. Background music, a walk-in song, a stinger. It keeps playing while the
// slide changes, fades in and out, loops, steps through a playlist, and lowers
// ("ducks") under a video that has sound.
//
// Nothing runs until a track is played: no audio element, no timers.
// The level is  volume x fade x duck  and is written to the element in one place
// (applyLevel). A short timer runs only while a fade or duck is moving.
(function () {
  const VOL_KEY = 'kairo-audio-lane-volume';
  const DEVICE_KEY = 'kairo-audio-lane-device';
  const LOOP_KEY = 'kairo-audio-lane-loop';
  const DUCK_KEY = 'kairo-audio-lane-duck';
  const FADE_KEY = 'kairo-audio-lane-fade';
  const AUTO_KEY = 'kairo-audio-lane-autostart';
  const LAST_KEY = 'kairo-audio-lane-last';
  const DUCK_LEVEL = 0.25;     // how low music goes under a video
  const DUCK_MS = 400;         // how fast it goes down and back up
  const TICK_MS = 40;

  const store = {
    get(k, d) { try { const v = localStorage.getItem(k); return v === null ? d : v; } catch { return d; } },
    set(k, v) { try { localStorage.setItem(k, String(v)); } catch {} },
  };

  const lane = {
    tracks: [],            // [{ name, label, url }] in the order they play
    index: -1,             // the track loaded now
    el: null,
    volume: clamp01(Number(store.get(VOL_KEY, 80)) / 100),
    loop: ['off', 'one', 'all'].includes(store.get(LOOP_KEY, 'off')) ? store.get(LOOP_KEY, 'off') : 'off',
    duckOn: store.get(DUCK_KEY, '1') === '1',
    autostart: store.get(AUTO_KEY, '0') === '1',
    fadeSec: Math.min(10, Math.max(0, Number(store.get(FADE_KEY, 2)) || 0)),
    fade: 1,               // 0..1, the fade's current level
    duck: 1,               // 1 = normal, DUCK_LEVEL = lowered
    _fade: null,           // { from, to, t0, ms, then }
    _duck: null,
    _timer: null,
    _listeners: new Set(),
  };

  function clamp01(n) { return Math.min(1, Math.max(0, Number.isFinite(n) ? n : 0)); }
  function emit() { lane._listeners.forEach(fn => { try { fn(lane.state()); } catch {} }); }

  function applyLevel() {
    if (lane.el) lane.el.volume = clamp01(lane.volume * lane.fade * lane.duck);
  }

  // One timer for both ramps; it stops itself when nothing is moving.
  function tick() {
    const now = performance.now();
    let moving = false;
    for (const key of ['_fade', '_duck']) {
      const r = lane[key];
      if (!r) continue;
      const p = r.ms <= 0 ? 1 : Math.min(1, (now - r.t0) / r.ms);
      const v = r.from + (r.to - r.from) * p;
      if (key === '_fade') lane.fade = v; else lane.duck = v;
      if (p >= 1) { lane[key] = null; if (r.then) r.then(); } else moving = true;
    }
    applyLevel();
    if (!moving && lane._timer) { clearInterval(lane._timer); lane._timer = null; }
    emit();
  }
  function ramp(key, to, ms, then) {
    const from = key === '_fade' ? lane.fade : lane.duck;
    lane[key] = { from, to, t0: performance.now(), ms, then };
    if (!lane._timer) lane._timer = setInterval(tick, TICK_MS);
    tick();
  }

  function element() {
    if (lane.el) return lane.el;
    const el = new Audio();
    el.preload = 'auto';
    el.addEventListener('ended', onEnded);
    el.addEventListener('play', emit);
    el.addEventListener('pause', emit);
    el.addEventListener('timeupdate', emit);
    el.addEventListener('error', () => { lane.error = 'That file couldn’t be played.'; emit(); });
    lane.el = el;
    applySink();
    applyLevel();
    return el;
  }

  function onEnded() {
    if (lane.loop === 'one') { lane.el.currentTime = 0; lane.el.play().catch(() => {}); return; }
    const last = lane.index >= lane.tracks.length - 1;
    if (last && lane.loop !== 'all') { emit(); return; }
    lane.play(last ? 0 : lane.index + 1, { fade: false });
  }

  function applySink() {
    const el = lane.el;
    if (!el || typeof el.setSinkId !== 'function') return;
    const id = store.get(DEVICE_KEY, '') || store.get('kairo-media-output-device', '');
    el.setSinkId(id || '').catch(() => {});
  }

  const urlOf = (u) => (typeof authedMediaUrl === 'function' ? authedMediaUrl(u) : u);

  lane.state = function () {
    const el = lane.el;
    const t = lane.tracks[lane.index];
    return {
      track: t ? t.label : '',
      index: lane.index,
      count: lane.tracks.length,
      playing: !!(el && !el.paused && !el.ended),
      loaded: !!t,
      time: el ? el.currentTime : 0,
      duration: el && Number.isFinite(el.duration) ? el.duration : 0,
      volume: Math.round(lane.volume * 100),
      loop: lane.loop,
      duckOn: lane.duckOn,
      ducked: lane.duck < 1,
      fadeSec: lane.fadeSec,
      autostart: lane.autostart,
      error: lane.error || '',
    };
  };
  lane.onChange = (fn) => { lane._listeners.add(fn); return () => lane._listeners.delete(fn); };

  lane.refresh = async function () {
    try {
      const r = await fetch(`${SERVER}/api/audio/tracks`, { headers: typeof authHeaders === 'function' ? authHeaders() : undefined });
      const d = await r.json();
      const playing = lane.tracks[lane.index]?.name;
      lane.tracks = (d.items || []).map(i => ({ name: i.name, label: i.label, url: i.url }));
      lane.index = playing ? lane.tracks.findIndex(t => t.name === playing) : -1;
    } catch {}
    emit();
    return lane.tracks;
  };

  // Play track i (or the loaded one when i is omitted). Fades in unless told not to.
  lane.play = function (i, opts = {}) {
    lane.error = '';
    if (typeof i === 'number') {
      const t = lane.tracks[i];
      if (!t) return;
      const el = element();
      lane.index = i;
      el.src = urlOf(t.url);
      store.set(LAST_KEY, t.name);
      el.currentTime = 0;
    } else if (!lane.tracks[lane.index]) {
      if (!lane.tracks.length) return;
      return lane.play(0, opts);
    }
    const el = element();
    const useFade = opts.fade !== false && lane.fadeSec > 0;
    lane.fade = useFade ? 0 : 1;
    lane._fade = null;
    applyLevel();
    el.play().catch((e) => { lane.error = String(e && e.message || e); emit(); });
    if (useFade) ramp('_fade', 1, lane.fadeSec * 1000);
    emit();
  };
  lane.pause = function () {
    if (!lane.el || lane.el.paused) return;
    const done = () => { lane.el.pause(); lane.fade = 1; applyLevel(); emit(); };
    if (lane.fadeSec > 0) ramp('_fade', 0, lane.fadeSec * 1000, done); else done();
  };
  lane.toggle = function () { lane.state().playing ? lane.pause() : lane.play(); };
  lane.stop = function (opts = {}) {
    if (!lane.el) return;
    const done = () => { lane.el.pause(); lane.el.currentTime = 0; lane.fade = 1; applyLevel(); emit(); };
    if (opts.fade !== false && lane.fadeSec > 0 && !lane.el.paused) ramp('_fade', 0, lane.fadeSec * 1000, done); else done();
  };
  lane.fadeOut = function () { lane.stop({ fade: true }); };
  lane.next = function () { if (lane.tracks.length) lane.play((lane.index + 1) % lane.tracks.length); };
  lane.prev = function () {
    if (!lane.tracks.length) return;
    if (lane.el && lane.el.currentTime > 3) { lane.el.currentTime = 0; return; }
    lane.play((lane.index - 1 + lane.tracks.length) % lane.tracks.length);
  };
  lane.seek = function (sec) { if (lane.el && Number.isFinite(sec)) lane.el.currentTime = Math.max(0, sec); };

  lane.setVolume = function (pct) {
    lane.volume = clamp01(pct / 100);
    store.set(VOL_KEY, Math.round(lane.volume * 100));
    applyLevel(); emit();
  };
  lane.setLoop = function (mode) {
    if (!['off', 'one', 'all'].includes(mode)) return;
    lane.loop = mode; store.set(LOOP_KEY, mode); emit();
  };
  lane.setFade = function (sec) {
    lane.fadeSec = Math.min(10, Math.max(0, Number(sec) || 0));
    store.set(FADE_KEY, lane.fadeSec); emit();
  };
  lane.setDuckOn = function (on) {
    lane.duckOn = !!on; store.set(DUCK_KEY, on ? '1' : '0');
    if (!on) ramp('_duck', 1, DUCK_MS);
    emit();
  };
  lane.setAutostart = function (on) { lane.autostart = !!on; store.set(AUTO_KEY, on ? '1' : '0'); emit(); };

  // On launch: load the library, and start the last track when asked to.
  lane.init = async function () {
    await lane.refresh();
    if (!lane.autostart || !lane.tracks.length) return;
    const last = store.get(LAST_KEY, '');
    const i = Math.max(0, lane.tracks.findIndex(t => t.name === last));
    lane.play(i);
  };
  lane.setDevice = function (id) { store.set(DEVICE_KEY, id || ''); applySink(); emit(); };
  lane.device = () => store.get(DEVICE_KEY, '');
  lane.applySink = applySink; // the Media output picker changed

  // A video with sound is playing: music lowers, then returns.
  lane.duckFor = function (videoPlaying) {
    if (!lane.duckOn || !lane.el) return;
    const to = videoPlaying ? DUCK_LEVEL : 1;
    if (lane.duck === to && !lane._duck) return;
    if (lane._duck && lane._duck.to === to) return;
    ramp('_duck', to, DUCK_MS);
  };

  lane.upload = async function (files) {
    const out = { added: 0, failed: [] };
    for (const f of Array.from(files || [])) {
      try {
        const buf = new Uint8Array(await f.arrayBuffer());
        let bin = ''; for (let k = 0; k < buf.length; k += 0x8000) bin += String.fromCharCode.apply(null, buf.subarray(k, k + 0x8000));
        const r = await fetch(`${SERVER}/api/audio/upload`, {
          method: 'POST',
          headers: Object.assign({ 'Content-Type': 'application/json' }, typeof authHeaders === 'function' ? authHeaders() : {}),
          body: JSON.stringify({ filename: f.name, dataBase64: btoa(bin) }),
        });
        const d = await r.json();
        if (d.ok) out.added++; else out.failed.push(`${f.name}: ${d.error || 'failed'}`);
      } catch (e) { out.failed.push(`${f.name}: ${e.message || e}`); }
    }
    await lane.refresh();
    return out;
  };
  lane.remove = async function (name) {
    const wasCurrent = lane.tracks[lane.index]?.name === name;
    if (wasCurrent) lane.stop({ fade: false });
    try { await fetch(`${SERVER}/api/audio/${encodeURIComponent(name)}`, { method: 'DELETE', headers: typeof authHeaders === 'function' ? authHeaders() : undefined }); } catch {}
    await lane.refresh();
  };

  window.KairoAudioLane = lane;
})();
