// Overlays drawn on top of everything: a logo, a "Live" bug, a lower-third
// picture or video. Shared by the output screens (display.html) and the
// Monitor preview, so what you set up is what the screens show.
//
// render() keeps one element per overlay and only touches what changed, so a
// looping video is not restarted every time another overlay is adjusted.
(function () {
  function place(el, o) {
    el.style.left = `${o.x}%`;
    el.style.top = `${o.y}%`;
    el.style.width = `${o.w}%`;
    el.style.opacity = String(o.opacity);
  }

  function make(o) {
    const el = document.createElement(o.kind === 'video' ? 'video' : 'img');
    el.className = 'kairo-overlay';
    el.dataset.overlayId = o.id;
    if (o.kind === 'video') {
      el.autoplay = true; el.loop = true; el.muted = true; el.playsInline = true;
    }
    el.draggable = false;
    el.src = typeof authedMediaUrl === 'function' ? authedMediaUrl(o.url) : o.url;
    el.dataset.src = o.url;
    return el;
  }

  // items: [{ id, kind, url, x, y, w, opacity, live }]
  function render(container, items) {
    if (!container) return;
    const live = (items || []).filter(o => o && o.live);
    const keep = new Set(live.map(o => o.id));
    Array.from(container.children).forEach(el => { if (!keep.has(el.dataset.overlayId)) el.remove(); });
    live.forEach((o) => {
      let el = container.querySelector(`[data-overlay-id="${CSS.escape(o.id)}"]`);
      if (!el) { el = make(o); container.appendChild(el); }
      place(el, o);
      container.appendChild(el); // later in the list = on top
    });
    container.classList.toggle('empty', !live.length);
  }

  async function load() {
    try {
      const url = typeof authedMediaUrl === 'function' ? authedMediaUrl('/api/overlays') : '/api/overlays';
      const r = await fetch(url);
      return (await r.json()).items || [];
    } catch { return []; }
  }

  window.KairoOverlays = { render, load };
})();
