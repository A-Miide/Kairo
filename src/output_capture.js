// Themed NDI / Syphon output: each running native output gets a hidden
// display window (`kairo-cap-<id>`, display.html?capture=1) rendering its
// theme exactly as a screen would; the app snapshots it on every change
// (src-tauri capture.rs) and the sender puts that on air instead of its
// own fixed lower-third drawing. Where capture isn't supported yet, nothing
// opens and the sender keeps drawing as before.
(function () {
  const T = window.__TAURI__;
  let supported = null;
  async function isSupported() {
    if (supported === null) {
      try { supported = !!(await T?.core?.invoke('output_capture_supported')); } catch { supported = false; }
    }
    return supported;
  }
  const labelFor = (id) => `kairo-cap-${id}`;
  // One open/close at a time per output (a start can arrive twice at launch —
  // auto-resume plus a toggle — and two overlapping replacements left none),
  // and the size each open window was made for.
  const queue = new Map();
  const sizes = new Map();
  const serial = (id, job) => {
    const next = (queue.get(id) || Promise.resolve()).then(job, job);
    queue.set(id, next.catch(() => {}));
    return next;
  };

  function close(id) { return serial(id, () => closeNow(id)); }
  async function closeNow(id) {
    const W = T?.webviewWindow?.WebviewWindow;
    if (!W) return;
    sizes.delete(id);
    try {
      const existing = await W.getByLabel(labelFor(id));
      if (existing) await existing.close();
      for (let i = 0; i < 30 && await W.getByLabel(labelFor(id)); i++) await new Promise(r => setTimeout(r, 100));
    } catch {}
  }

  function open(id, width, height) { return serial(id, () => openNow(id, width, height)); }
  async function openNow(id, width, height) {
    const W = T?.webviewWindow?.WebviewWindow;
    if (!W || !(await isSupported())) return;
    width = Math.max(16, Math.round(width || 1920));
    height = Math.max(16, Math.round(height || 1080));
    // Already open at this size: leave it be.
    if (sizes.get(id) === `${width}x${height}` && await W.getByLabel(labelFor(id))) return;
    await closeNow(id);
    sizes.set(id, `${width}x${height}`);
    // Points: the snapshot comes back at the screen's pixel density, so a
    // 1920-pixel output on a Retina Mac is a 960-point window.
    const dpr = window.devicePixelRatio || 1;
    const win = new W(labelFor(id), {
      url: `${location.origin}/display.html?output=${encodeURIComponent(id)}&capture=1&w=${width}&h=${height}&fs=0`,
      title: 'KAIRO Output Capture',
      width: Math.round(width / dpr),
      height: Math.round(height / dpr),
      // Off every screen: it renders and can be snapshotted, but nobody sees it.
      x: -32000, y: -32000,
      decorations: false, resizable: false, focus: false, skipTaskbar: true,
      shadow: false, alwaysOnBottom: true, visible: false,
      // Off-screen, WebKit would otherwise suspend the page (macOS 14+ honours this).
      backgroundThrottling: 'disabled',
    });
    win.once('tauri://error', (e) => console.warn('[capture] window failed:', e));
  }

  window.KairoCapture = { open, close, isSupported };
})();
