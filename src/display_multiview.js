// KAIRO — the multiview display mode (display.html, mode 'multiview').
//
// Video director mode can show its input grid full screen on one chosen
// display. This draws that grid, read-only: every enabled input as a tile with
// its name and the on-air / Preview frame, fed by what the director publishes
// (kairo-video-sources, kairo-video-director in localStorage). Clicks here do
// nothing; the director screen drives it. The window holds one capture per
// camera, through its own hub, only while this mode is on: switching the display
// back to its normal mode (or ending Director mode) releases every device.
'use strict';
(function () {
  let host = null, grid = null, devices = [], onStorage = null, onDevice = null, token = 0;

  const read = (k, d) => { try { const v = JSON.parse(localStorage.getItem(k) || 'null'); return v == null ? d : v; } catch { return d; } };

  async function refreshDevices() {
    try { devices = (await navigator.mediaDevices.enumerateDevices()).filter(d => d.kind === 'videoinput'); } catch { devices = []; }
  }

  async function start(el, loadScript) {
    if (grid) return;
    const my = ++token;
    host = el;
    for (const f of ['video_sources.js', 'video_hub.js', 'video_input_grid.js']) await loadScript(f);
    if (my !== token) return;
    await refreshDevices();
    if (my !== token) return;
    const V = window.KairoVideoSources;
    const sources = () => V.normalizeSources(read('kairo-video-sources', [])).filter(s => s.enabled !== false);
    grid = window.KairoVideoGrid.create({
      getSources: sources,
      getDevices: () => devices,
      getState: () => { const d = read('kairo-video-director', {}); return { programId: d.programId || null, mixed: !!d.mixed, hasOutputs: !!d.hasOutputs, previewId: d.previewId || null }; },
      onProgram() {}, onPreview() {},
      layout: 'fill', readOnly: true,
      profile: sources().length <= 4 ? 'mid' : 'thumb',
    });
    host.replaceChildren(grid.el);
    grid.render();
    onStorage = (e) => { if (e.key === 'kairo-video-sources' || e.key === 'kairo-video-director') grid && grid.render(); };
    window.addEventListener('storage', onStorage);
    onDevice = async () => { await refreshDevices(); grid && grid.render(); };
    navigator.mediaDevices && navigator.mediaDevices.addEventListener && navigator.mediaDevices.addEventListener('devicechange', onDevice);
  }

  function stop() {
    token++;
    if (onStorage) { window.removeEventListener('storage', onStorage); onStorage = null; }
    if (onDevice) { navigator.mediaDevices.removeEventListener && navigator.mediaDevices.removeEventListener('devicechange', onDevice); onDevice = null; }
    if (grid) { grid.destroy(); grid = null; }
    if (window.KairoVideoHub) window.KairoVideoHub.releaseAll();
    if (host) host.replaceChildren();
    host = null;
  }

  window.KairoDisplayMultiview = { start, stop };
})();
