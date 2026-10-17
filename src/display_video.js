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

  // Where a device stream comes from. By default this window opens the device
  // itself; a copy inside the operator window passes the parent's hub so the
  // camera is captured once.
  const directProvider = {
    async open(source) {
      const devices = await navigator.mediaDevices.enumerateDevices();
      const deviceId = V.resolveDevice(source, devices);
      if (!deviceId) { const e = new Error('device missing'); e.name = 'NotFoundError'; throw e; }
      // Never more than this window can show (a capture window's own size, else its screen's).
      const q = new URLSearchParams(location.search);
      const dpr = window.devicePixelRatio || 1;
      const cap = { width: Number(q.get('w')) || Math.round((window.screen.width || 1920) * dpr), height: Number(q.get('h')) || Math.round((window.screen.height || 1080) * dpr) };
      const stream = await navigator.mediaDevices.getUserMedia(V.buildConstraints(source, deviceId, cap));
      return { stream, deviceId, release: () => { try { stream.getTracks().forEach(t => t.stop()); } catch {} } };
    },
  };

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
    let spareM = null;             // a source held on the spare slot for a hand fade (T-bar)
    let manualGen = 0;
    let manualT = 0;

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
      const done = () => {
        stream && stream.release && stream.release();
        // The slot may have been taken again in the meantime (1, 2, 1 in quick succession): leave it alone then.
        if (slot === front) return;
        if (stream && stream.stream && slots[slot].srcObject && slots[slot].srcObject !== stream.stream) return;
        slots[slot].srcObject = null; slots[slot].style.opacity = '0';
      };
      if (ms > 0) setTimeout(done, ms + 50); else done();
    }

    const provider = opts.provider || directProvider;
    const open = (source) => provider.open(source);

    function waitForFrame(el) {
      return new Promise((resolve) => {
        let settled = false;
        const finish = () => { if (!settled) { settled = true; resolve(); } };
        if (el.readyState >= 2) { finish(); return; }   // the first frame is already up
        el.addEventListener('loadeddata', finish, { once: true });
        setTimeout(finish, 3000);   // a source with no frames yet still takes over
      });
    }

    // What this window really shows, for the operator window to compare with
    // what it put on air: the source wanted, the one showing, and why not.
    // Only a change is reported: a retry every few seconds isn't news.
    let lastReport = '';
    const report = (want, error) => {
      const r = { output: lastArgs && lastArgs.outputKey, want: want || null, showing: current ? current.sourceId : null, error: error || null };
      const sig = JSON.stringify(r);
      if (sig === lastReport) return;
      lastReport = sig;
      try { opts.onShowing && opts.onShowing(r); } catch {}
    };

    // The device went away (unplugged): drop it and keep trying.
    function watch(opened) {
      const track = opened.stream.getVideoTracks()[0];
      if (track) track.addEventListener('ended', () => {
        if (!current || current.stream !== opened.stream) return;
        const lost = current.sourceId;
        const slot = front;
        front = -1; current = null;
        fadeStyle(slots[slot], 200);
        slots[slot].style.opacity = '0';
        setActive(false);
        release(slot, { release: opened.release }, 200);
        report(lost, 'disconnected');
        retryTimer = setTimeout(() => sync(lastArgs), 2000);
      }, { once: true });
    }

    function cancelManual() {
      manualGen++;
      if (spareM) {
        spareM.opened && spareM.opened.release();
        slots[spareM.slot].srcObject = null;
        fadeStyle(slots[spareM.slot], 0);
        slots[spareM.slot].style.opacity = '0';
        spareM = null;
      }
    }

    // Fade by hand (the T-bar): `sourceId` comes in over what is showing as t
    // goes 0 → 1 (null = going to no video). Nothing is switched until the
    // director commits; the live map change that follows adopts the stream
    // already open here, so the camera is not opened twice.
    async function manual({ layers, sources, sourceId, t }) {
      if (!opts.enabled) return;
      manualT = t;
      if (!sourceId) {
        cancelManual();
        if (front >= 0) { fadeStyle(slots[front], 0); slots[front].style.opacity = String(1 - t); }
        return;
      }
      const want = (sources || []).find(x => x.id === sourceId);
      if (!want || !layers || !layers.video) return;
      if (t <= 0.001) {
        cancelManual();
        if (front >= 0) { fadeStyle(slots[front], 0); slots[front].style.opacity = '1'; }
        return;
      }
      if (current && current.sourceId === sourceId && !spareM) return;   // already showing
      if (front >= 0 && !spareM) { fadeStyle(slots[front], 0); slots[front].style.opacity = '1'; }
      if (!spareM || spareM.sourceId !== sourceId) {
        cancelManual();
        const token = manualGen;
        const slot = front === 0 ? 1 : 0;
        spareM = { slot, sourceId, opened: null, want };
        let opened;
        try { opened = await open(want); } catch { if (token === manualGen) spareM = null; return; }
        if (token !== manualGen || !spareM) { opened.release(); return; }
        const el = slots[slot];
        el.srcObject = opened.stream;
        try { await el.play(); } catch {}
        await waitForFrame(el);
        if (token !== manualGen || !spareM) { opened.release(); el.srcObject = null; return; }
        spareM.opened = opened;
      }
      if (spareM && spareM.opened) { const el = slots[spareM.slot]; fadeStyle(el, 0); el.style.opacity = String(manualT); }
    }

    // layers: this output's layer map; sources/live: settings.videoSources / videoLive.
    async function sync(args) {
      lastArgs = args;
      clearTimeout(retryTimer);
      if (!opts.enabled || !navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) return;
      const myGen = ++gen;
      const want = V.liveSourceFor(args.outputKey, args.layers, args.live, args.sources);

      if (!want) {   // nothing to show: fade out, then let go of the camera
        cancelManual();
        report(null, null);
        if (front >= 0) {
          const plan = V.switchPlan(opts.getTransition().animation, opts.getTransition().speed, true);
          const old = { slot: front, stream: current };
          fadeStyle(slots[old.slot], plan.ms);
          slots[old.slot].style.opacity = '0';
          front = -1; current = null;
          setActive(false);
          release(old.slot, old.stream, plan.ms);
        }
        return;
      }

      // The T-bar already has this source open on the spare slot: it takes over as it is.
      if (spareM && spareM.sourceId === want.id && spareM.opened) {
        const el = slots[spareM.slot];
        const old = front >= 0 ? { slot: front, stream: current } : null;
        const adopted = spareM; spareM = null; manualGen++;
        fadeStyle(el, 0); el.style.opacity = '1';
        if (old) { fadeStyle(slots[old.slot], 0); slots[old.slot].style.opacity = '0'; }
        front = adopted.slot;
        current = { stream: adopted.opened.stream, release: adopted.opened.release, sourceId: want.id, settings: [want.width, want.height, want.fps].join('x'), key: sourceKey(want, adopted.opened.deviceId) };
        setActive(true);
        if (old) release(old.slot, old.stream, 0);
        watch(adopted.opened);
        return;
      }
      if (spareM && spareM.sourceId !== want.id) cancelManual();

      if (current && front >= 0) {
        // Same source, same settings, stream still live: nothing to do.
        const track = current.stream.getVideoTracks()[0];
        if (current.sourceId === want.id && current.settings === [want.width, want.height, want.fps].join('x') && track && track.readyState === 'live') return;
      }

      let opened;
      try { opened = await open(want); } catch (err) {
        if (myGen !== gen) return;
        console.warn('[Video] could not open', want.name, err && err.name);
        report(want.id, (err && err.name) || 'Error');
        // Keep what is showing; try again shortly (a replug, or a camera another app released).
        retryTimer = setTimeout(() => sync(lastArgs), 3000);
        return;
      }
      if (myGen !== gen) { opened.release(); return; }

      const spare = front === 0 ? 1 : 0;
      const el = slots[spare];
      el.srcObject = opened.stream;
      try { await el.play(); } catch {}
      await waitForFrame(el);
      if (myGen !== gen) { opened.release(); el.srcObject = null; return; }

      const plan = V.switchPlan(opts.getTransition().animation, opts.getTransition().speed, front >= 0);
      const old = front >= 0 ? { slot: front, stream: current } : null;
      fadeStyle(el, plan.ms);
      el.style.opacity = '1';
      if (old) { fadeStyle(slots[old.slot], plan.ms); slots[old.slot].style.opacity = '0'; }
      front = spare;
      current = { stream: opened.stream, release: opened.release, sourceId: want.id, settings: [want.width, want.height, want.fps].join('x'), key: sourceKey(want, opened.deviceId) };
      setActive(true);
      if (old) release(old.slot, old.stream, plan.ms);
      report(want.id, null);

      watch(opened);
    }

    if (opts.enabled && !opts.provider && navigator.mediaDevices && navigator.mediaDevices.addEventListener) {
      navigator.mediaDevices.addEventListener('devicechange', () => { if (lastArgs) sync(lastArgs); });
    }

    // The page is going away: let go of the device now.
    function dispose() {
      gen++;
      cancelManual();
      clearTimeout(retryTimer);
      if (current && current.release) current.release();
      current = null; front = -1;
    }

    return { sync, manual, dispose, isActive: () => active };
  }

  window.KairoDisplayVideo = { create };
})();
