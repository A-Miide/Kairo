// KAIRO — developer tools for the video inputs. Off unless switched on; nothing here runs
// for a normal user (the script returns at once).
//
//  1. Simulated cameras: localStorage 'kairo-sim-cameras' = '1' (or open the page with
//     ?simcams=1) adds four "Simulated camera" devices to enumerateDevices and answers
//     getUserMedia for them with canvas streams at the asked size and rate (30 fps by
//     default). Real devices, and the microphone, are untouched. Loaded in the operator
//     window and in every display window, so each opens its own simulated capture exactly
//     as it would a real camera.
//  2. KairoVideoStats: frames shown and dropped by every <video> in this window and its
//     same-origin frames, for measuring (scripts/measure-video.md).
'use strict';
(function () {
  const FLAG = 'kairo-sim-cameras';
  const COUNT = 4;
  try {
    if (/[?&]simcams=1\b/.test(location.search)) localStorage.setItem(FLAG, '1');
    if (/[?&]simcams=0\b/.test(location.search)) localStorage.removeItem(FLAG);
  } catch {}
  const isOn = () => { try { return localStorage.getItem(FLAG) === '1'; } catch { return false; } };

  // ── Stats: always available, costs nothing until called. ──
  function videosIn(win, out, depth) {
    try {
      win.document.querySelectorAll('video').forEach(v => out.push({ v, where: win === window ? 'main' : (win.location.search.replace(/^\?/, '').slice(0, 40) || 'frame') }));
      if (depth < 3) win.document.querySelectorAll('iframe').forEach(f => { try { videosIn(f.contentWindow, out, depth + 1); } catch {} });
    } catch {}
    return out;
  }
  function snapshot() {
    return videosIn(window, [], 0).filter(x => x.v.srcObject).map((x, i) => {
      const q = x.v.getVideoPlaybackQuality ? x.v.getVideoPlaybackQuality() : { totalVideoFrames: 0, droppedVideoFrames: 0 };
      const track = x.v.srcObject.getVideoTracks && x.v.srcObject.getVideoTracks()[0];
      return { key: i, where: x.where, w: x.v.videoWidth, h: x.v.videoHeight, shown: q.totalVideoFrames, dropped: q.droppedVideoFrames, label: track ? track.label : '' };
    });
  }
  // Watch for `seconds`: per video, frames per second shown and frames dropped.
  async function run(seconds = 10) {
    const a = snapshot();
    await new Promise(r => setTimeout(r, seconds * 1000));
    const b = snapshot();
    return b.map((y, i) => {
      const x = a[i] || y;
      return { where: y.where, label: y.label, size: y.w + 'x' + y.h, fps: Math.round((y.shown - x.shown) / seconds * 10) / 10, dropped: y.dropped - x.dropped };
    });
  }
  window.KairoVideoStats = { snapshot, run };

  // ── Simulated cameras. ──
  const hues = [210, 28, 140, 300];
  const timers = new Set();
  function makeStream(index, constraints) {
    const v = constraints && typeof constraints.video === 'object' ? constraints.video : {};
    const num = (c, d) => (c == null ? d : typeof c === 'number' ? c : (c.ideal || c.exact || c.max || d));
    const width = Math.min(3840, Math.max(160, Math.round(num(v.width, 1280))));
    const height = Math.min(2160, Math.max(90, Math.round(num(v.height, 720))));
    const fps = Math.min(60, Math.max(1, Math.round(num(v.frameRate, 30))));
    const canvas = document.createElement('canvas');
    canvas.width = width; canvas.height = height;
    const ctx = canvas.getContext('2d', { alpha: false });
    const hue = hues[(index - 1) % hues.length];
    let n = 0;
    const draw = () => {
      n++;
      const t = n / fps;
      ctx.fillStyle = `hsl(${hue} 45% 22%)`;
      ctx.fillRect(0, 0, width, height);
      // A bar sweeping across, so every frame differs and a stall is visible.
      const x = ((t * 0.35) % 1) * (width + width * 0.2) - width * 0.2;
      ctx.fillStyle = `hsl(${hue} 70% 60%)`;
      ctx.fillRect(x, 0, width * 0.2, height);
      ctx.fillStyle = '#fff';
      ctx.font = `700 ${Math.round(height * 0.5)}px system-ui, sans-serif`;
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillText(String(index), width / 2, height * 0.5);
      ctx.font = `500 ${Math.round(height * 0.06)}px ui-monospace, monospace`;
      ctx.textAlign = 'left'; ctx.textBaseline = 'top';
      ctx.fillText(`SIM ${index}  ${width}x${height}@${fps}  f${n}`, height * 0.04, height * 0.04);
    };
    draw();
    const stream = canvas.captureStream(fps);
    const timer = setInterval(draw, Math.round(1000 / fps));
    timers.add(timer);
    const track = stream.getVideoTracks()[0];
    Object.defineProperty(track, 'label', { value: 'Simulated camera ' + index });
    const stop = track.stop.bind(track);
    track.stop = () => { clearInterval(timer); timers.delete(timer); stop(); };
    return stream;
  }
  const simIndex = (id) => { const m = /^sim-cam-(\d+)$/.exec(id || ''); return m ? Number(m[1]) : 0; };

  if (isOn() && navigator.mediaDevices) {
    const md = navigator.mediaDevices;
    const realEnum = md.enumerateDevices.bind(md);
    const realGet = md.getUserMedia.bind(md);
    md.enumerateDevices = async () => {
      let list = [];
      try { list = await realEnum(); } catch {}
      const sims = [];
      for (let i = 1; i <= COUNT; i++) sims.push({ deviceId: 'sim-cam-' + i, kind: 'videoinput', label: 'Simulated camera ' + i, groupId: 'sim', toJSON() { return this; } });
      return [...list, ...sims];
    };
    md.getUserMedia = async (constraints) => {
      const v = constraints && constraints.video;
      const id = v && typeof v === 'object' && v.deviceId ? (v.deviceId.exact || v.deviceId.ideal || v.deviceId) : '';
      const index = simIndex(typeof id === 'string' ? id : '');
      if (index) return makeStream(index, constraints);
      // The permission probe in the Sources panel (video: true) must not fail without a camera.
      if (v === true && !constraints.audio) {
        try { return await realGet(constraints); } catch { return makeStream(1, { video: { width: 320, height: 180, frameRate: 5 } }); }
      }
      return realGet(constraints);
    };
  }
  window.KairoSimCameras = {
    isOn,
    ids: () => Array.from({ length: COUNT }, (_, i) => 'sim-cam-' + (i + 1)),
    set(on) { try { on ? localStorage.setItem(FLAG, '1') : localStorage.removeItem(FLAG); } catch {} },
    count: COUNT,
  };
})();
