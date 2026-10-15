// KAIRO — the Video Input layer of a display window (display.html).
//
// Each display window opens the camera itself (getUserMedia in its own
// webview) rather than receiving a shared stream: no peer connection to set
// up and tear down on every switch, and the window only holds a device while
// its Video layer is on and a source is live. Two <video> slots live in
// #video-layer; a switch opens the new source in the spare slot and cuts or
// fades to it once its first frame is up, so a slow camera never shows a
// black gap. The pure choices (which source, which device, cut or fade) come
// from video_sources.js.
'use strict';
(function () {
  const V = window.KairoVideoSources;

  // opts: { host, enabled, getTransition(): {animation, speed}, onActive(bool) }
  function create(opts) {
    const host = opts.host;
    const slots = [0, 1].map(() => {
      const el = document.createElement('video');
      el.muted = true; el.autoplay = true; el.playsInline = true;
      el.setAttribute('aria-hidden', 'true');
      el.style.opacity = '0';
      host.appendChild(el);
      return el;
    });
    let front = -1;                // slot showing now, or -1
    let current = null;            // { key, stream } of the front slot
    let gen = 0;                   // bumped by every sync; a stale open is dropped
    let lastArgs = null;
    let retryTimer = null;
    let active = false;

    const setActive = (on) => {
      if (on === active) return;
      active = on;
      try { opts.onActive && opts.onActive(on); } catch {}
    };
    const stopStream = (s) => { try { s && s.getTracks().forEach(t => t.stop()); } catch {} };
    const sourceKey = (s, deviceId) => [s.id, deviceId, s.width, s.height, s.fps].join('|');

    function fadeStyle(el, ms) {
      el.style.transition = ms > 0 ? `opacity ${ms}ms ease` : 'none';
    }

    function release(slot, stream, ms) {
      const done = () => { stopStream(stream); slots[slot].srcObject = null; slots[slot].style.opacity = '0'; };
      if (ms > 0) setTimeout(done, ms + 50); else done();
    }

    async function open(source) {
      const devices = await navigator.mediaDevices.enumerateDevices();
      const deviceId = V.resolveDevice(source, devices);
      if (!deviceId) { const e = new Error('device missing'); e.name = 'NotFoundError'; throw e; }
      const stream = await navigator.mediaDevices.getUserMedia(V.buildConstraints(source, deviceId));
      return { stream, deviceId };
    }

    function waitForFrame(el) {
      return new Promise((resolve) => {
        let settled = false;
        const finish = () => { if (!settled) { settled = true; resolve(); } };
        if (el.readyState >= 2) { finish(); return; }   // the first frame is already up
        el.addEventListener('loadeddata', finish, { once: true });
        setTimeout(finish, 3000);   // a source with no frames yet still takes over
      });
    }

    // layers: this output's layer map; sources/live: settings.videoSources / videoLive.
    async function sync(args) {
      lastArgs = args;
      clearTimeout(retryTimer);
      if (!opts.enabled || !navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) return;
      const myGen = ++gen;
      const want = V.liveSourceFor(args.outputKey, args.layers, args.live, args.sources);

      if (!want) {   // nothing to show: fade out, then let go of the camera
        if (front >= 0) {
          const plan = V.switchPlan(opts.getTransition().animation, opts.getTransition().speed, true);
          const old = { slot: front, stream: current && current.stream };
          fadeStyle(slots[old.slot], plan.ms);
          slots[old.slot].style.opacity = '0';
          front = -1; current = null;
          setActive(false);
          release(old.slot, old.stream, plan.ms);
        }
        return;
      }

      if (current && front >= 0) {
        // Same source, same settings, stream still live: nothing to do.
        const track = current.stream.getVideoTracks()[0];
        if (current.sourceId === want.id && current.settings === [want.width, want.height, want.fps].join('x') && track && track.readyState === 'live') return;
      }

      let opened;
      try { opened = await open(want); } catch (err) {
        if (myGen !== gen) return;
        console.warn('[Video] could not open', want.name, err && err.name);
        // Keep what is showing; try again shortly (a replug, or a camera another app released).
        retryTimer = setTimeout(() => sync(lastArgs), 3000);
        return;
      }
      if (myGen !== gen) { stopStream(opened.stream); return; }

      const spare = front === 0 ? 1 : 0;
      const el = slots[spare];
      el.srcObject = opened.stream;
      try { await el.play(); } catch {}
      await waitForFrame(el);
      if (myGen !== gen) { stopStream(opened.stream); el.srcObject = null; return; }

      const plan = V.switchPlan(opts.getTransition().animation, opts.getTransition().speed, front >= 0);
      const old = front >= 0 ? { slot: front, stream: current && current.stream } : null;
      fadeStyle(el, plan.ms);
      el.style.opacity = '1';
      if (old) { fadeStyle(slots[old.slot], plan.ms); slots[old.slot].style.opacity = '0'; }
      front = spare;
      current = { stream: opened.stream, sourceId: want.id, settings: [want.width, want.height, want.fps].join('x'), key: sourceKey(want, opened.deviceId) };
      setActive(true);
      if (old) release(old.slot, old.stream, plan.ms);

      // The device went away (unplugged): drop it and keep trying.
      const track = opened.stream.getVideoTracks()[0];
      if (track) track.addEventListener('ended', () => {
        if (!current || current.stream !== opened.stream) return;
        const slot = front;
        front = -1; current = null;
        fadeStyle(slots[slot], 200);
        slots[slot].style.opacity = '0';
        setActive(false);
        release(slot, opened.stream, 200);
        retryTimer = setTimeout(() => sync(lastArgs), 2000);
      }, { once: true });
    }

    if (opts.enabled && navigator.mediaDevices && navigator.mediaDevices.addEventListener) {
      navigator.mediaDevices.addEventListener('devicechange', () => { if (lastArgs) sync(lastArgs); });
    }

    return { sync, isActive: () => active };
  }

  window.KairoDisplayVideo = { create };
})();
