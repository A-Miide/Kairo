// KAIRO — one capture per physical camera inside the operator window.
//
// Thumbnails, the director's Preview, its Program copy and instant replay all draw from the
// same MediaStream, so a camera is decoded once here however many video
// elements show it. A device is opened at the lowest size that covers every
// holder ('thumb' for thumbnails, 'mid' for Preview/Program, 'replay' for instant replay) and released a
// moment after its last holder lets go. Nothing here runs a timer, a frame
// loop or a canvas: the video elements decode in the browser.
//
// Display windows (separate webviews) open their own capture at the source's
// own size; only same-origin frames in this window share through the hub
// (display_video.js takes it as its `provider`).
'use strict';
(function () {
  const PROFILES = {
    thumb: { rank: 1, width: 320, height: 180, fps: 10, maxFps: 15 },
    mid:   { rank: 2, width: 640, height: 360, fps: 15, maxFps: 15 },
    // Instant replay (instant_replay.js) records the camera on air at this size: it shares the
    // hub's capture rather than opening the device a second time.
    replay: { rank: 3, width: 1280, height: 720, fps: 30, maxFps: 30 },
  };
  const LINGER_MS = 1500;   // a re-render can take the device back without reopening it
  const entries = new Map();

  function constraints(deviceId, profile) {
    const p = PROFILES[profile];
    return {
      audio: false,
      video: { deviceId: { exact: deviceId }, width: { ideal: p.width }, height: { ideal: p.height }, frameRate: { ideal: p.fps, max: p.maxFps } },
    };
  }
  const stop = (s) => { try { s && s.getTracks().forEach(t => t.stop()); } catch {} };
  // A camera that can't meet the size or the frame-rate cap (a built-in camera that won't go
  // down to 15 fps: OverconstrainedError) is opened as it is rather than not at all — its tile
  // said "Unavailable" with access allowed (owner, 2026-10-17).
  async function open(id, profile) {
    try { return await navigator.mediaDevices.getUserMedia(constraints(id, profile)); }
    catch (err) {
      if (!err || (err.name !== 'OverconstrainedError' && err.name !== 'NotReadableError')) throw err;
      try { window.kairoLog?.('video-problem', { what: 'camera-constraints', name: err.name, constraint: err.constraint || null }); } catch {}
      return navigator.mediaDevices.getUserMedia({ audio: false, video: { deviceId: { exact: id } } });
    }
  }
  function needed(entry) {
    let best = null;
    for (const l of entry.leases) if (!best || PROFILES[l.profile].rank > PROFILES[best].rank) best = l.profile;
    return best;
  }

  // A holder only gets a stream captured at its own size or larger: a Preview
  // frame must never be handed (and then lose) a thumbnail-sized stream.
  function deliver(entry) {
    for (const l of entry.leases) {
      if (l.sent === entry.stream || PROFILES[l.profile].rank > PROFILES[entry.profile].rank) continue;
      l.sent = entry.stream;
      try { l.onStream(entry.stream); } catch {}
    }
  }

  async function settle(id, entry) {
    if (entry.busy) { entry.again = true; return; }
    entry.busy = true;
    try {
      do {
        entry.again = false;
        const need = needed(entry);
        if (!need) {
          stop(entry.stream);
          entries.delete(id);
          return;
        }
        if (!entry.stream || entry.profile !== need) {
          const next = await open(id, need);
          // releaseAll() (Director mode ended) while this was opening: close it at once, the light must go out.
          if (entry.dead || entries.get(id) !== entry) { stop(next); return; }
          const prev = entry.stream;
          entry.stream = next;
          entry.profile = need;
          deliver(entry);
          stop(prev);
        } else {
          deliver(entry);
        }
      } while (entry.again);
    } catch (err) {
      // A camera that won't open shows "Unavailable" on its tile; the reason goes to the debug log.
      try { window.kairoLog?.('video-problem', { what: 'camera-open', name: err && err.name, message: String(err && err.message || err).slice(0, 200) }); } catch {}
      const failed = [...entry.leases];
      entries.delete(id);
      stop(entry.stream);
      for (const l of failed) { try { l.onError(err); } catch {} }
    } finally {
      entry.busy = false;
    }
  }

  // Hold `deviceId` at `profile`. onStream(stream) fires when a stream is
  // ready and again if it is swapped; call the returned release() when done.
  function lease(deviceId, profile, onStream, onError) {
    let entry = entries.get(deviceId);
    if (!entry) { entry = { leases: new Set(), stream: null, profile: null, busy: false, again: false, timer: null }; entries.set(deviceId, entry); }
    clearTimeout(entry.timer);
    const l = { profile, onStream, onError: onError || (() => {}), sent: null };
    entry.leases.add(l);
    queueMicrotask(() => settle(deviceId, entry));   // never deliver before the caller has the lease
    let released = false;
    return () => {
      if (released) return;
      released = true;
      entry.leases.delete(l);
      clearTimeout(entry.timer);
      entry.timer = setTimeout(() => { if (entries.get(deviceId) === entry) settle(deviceId, entry); }, LINGER_MS);
    };
  }

  async function resolve(source) {
    const V = window.KairoVideoSources;
    const devices = await navigator.mediaDevices.enumerateDevices();
    return V.resolveDevice(source, devices);
  }

  window.KairoVideoHub = {
    lease,
    // For a same-origin display.html frame (display_video.js `provider`).
    async open(source) {
      const deviceId = await resolve(source);
      if (!deviceId) { const e = new Error('device missing'); e.name = 'NotFoundError'; throw e; }
      return new Promise((res, rej) => {
        let release = null;
        let done = false;
        release = lease(deviceId, 'mid', (stream) => {
          if (done) return;
          done = true;
          res({ stream, deviceId, release: () => release && release() });
        }, (err) => { if (!done) { done = true; rej(err); } });
      });
    },
    resolve,
    // Devices still held open, for diagnostics.
    openCount: () => entries.size,
    releaseAll() { for (const [, e] of entries) { e.dead = true; clearTimeout(e.timer); stop(e.stream); } entries.clear(); },
  };
})();
