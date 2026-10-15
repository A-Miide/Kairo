// KAIRO — Video director mode: the dock, one bar along the bottom.
//
// A secondary shelf under the four main sections: filter chips for Slides
// (only slides), Video, Images and, when the app has them, Overlays. Video and Images reuse the Media Bin's
// own cards (moved here while the mode is on, so clicking one does exactly
// what it does there); Slides sends through the playlist's own sendSlide.
'use strict';
(function () {
  const K = { mounted: false, root: null, body: null, chips: null, active: 'slides', slideObs: null, binHome: null, binToggled: false };

  const dir = () => window.KairoVideoDirector;

  // ── Chips ────────────────────────────────────────────────────────────────
  const CHIPS = [
    { id: 'slides', label: 'Slides', available: () => !!(window.KairoService && window.KairoService.slidesFor) },
    { id: 'video', label: 'Video', available: () => !!document.getElementById('media-bin') },
    { id: 'images', label: 'Images', available: () => !!document.getElementById('media-bin') },
    // Overlays come from Settings → Overlays where the app has them.
    { id: 'overlays', label: 'Overlays', available: () => !!(window.KairoOverlays && window.KairoOverlays.load) },
    // Music: the app has no audio media yet, so there is nothing to list.
  ];

  function show(id) {
    if (!K.mounted) return;
    if (!CHIPS.some(c => c.id === id && c.available())) return;
    K.active = id;
    renderMedia();
  }

  // ── Media shelf ──────────────────────────────────────────────────────────
  function renderChips() {
    K.chips.replaceChildren();
    CHIPS.filter(c => c.available()).forEach(c => {
      const b = vBtn(c.label, `Show ${c.label.toLowerCase()}`, () => show(c.id), 'vd-chip' + (K.active === c.id ? ' is-on' : ''));
      b.setAttribute('aria-pressed', String(K.active === c.id));
      K.chips.appendChild(b);
    });
  }

  function releaseBin() {
    const bin = document.getElementById('media-bin');
    if (bin && K.binHome && K.binHome.parentNode && bin.parentNode !== K.binHome.parentNode) K.binHome.after(bin);
  }

  function renderMedia() {
    renderChips();
    K.slideObs?.disconnect(); K.slideObs = null;
    const body = K.body;
    releaseBin();
    body.replaceChildren();
    body.dataset.kind = K.active;
    if (K.active === 'slides') renderSlides(body);
    else if (K.active === 'overlays') renderOverlays(body);
    else renderBin(body);
  }

  function renderBin(body) {
    const bin = document.getElementById('media-bin');
    if (!bin) return;
    if (!K.binHome) { K.binHome = document.createComment('media-bin'); bin.before(K.binHome); }
    // If the operator had hidden the bin, show it so it loads its items.
    if (bin.classList.contains('hidden')) { document.getElementById('media-bin-toggle-btn')?.click(); K.binToggled = true; }
    body.appendChild(bin);
  }

  function renderSlides(body) {
    const svc = window.KairoService;
    const build = () => {
      body.replaceChildren();
      const items = ((svc && svc.service && svc.service.items) || []).filter(i => i.type === 'slides');
      let n = 0;
      items.forEach(item => {
        const slides = svc.slidesFor(item);
        if (!slides.length) return;
        body.appendChild(vEl('span', 'vd-group', item.title || 'Slides'));
        slides.forEach((s, i) => {
          const tile = vEl('button', 'vd-slide');
          tile.type = 'button';
          tile.title = `Send ${item.title || 'slide'} ${i + 1} to the screens`;
          if (s.image) { const img = vEl('img'); img.src = s.image; img.alt = ''; img.loading = 'lazy'; tile.appendChild(img); }
          else tile.appendChild(vEl('span', 'vd-slide-text', (s.text || s.label || '').slice(0, 90)));
          tile.appendChild(vEl('span', 'vd-slide-num', String(i + 1)));
          tile.addEventListener('click', () => svc.sendSlide(item, i));
          body.appendChild(tile);
          n++;
        });
      });
      if (!n) body.appendChild(vEl('p', 'vd-empty', 'No slides in this playlist.'));
    };
    build();
    const list = document.getElementById('svc-items-list');
    if (list) {
      K.slideObs = new MutationObserver(() => build());
      K.slideObs.observe(list, { childList: true });
    }
  }

  async function renderOverlays(body) {
    body.appendChild(vEl('p', 'vd-empty', 'Loading overlays'));
    let items = [];
    try { items = await window.KairoOverlays.load(); } catch {}
    if (K.active !== 'overlays' || !K.mounted) return;
    body.replaceChildren();
    if (!items.length) { body.appendChild(vEl('p', 'vd-empty', 'No overlays yet. Add them in Settings.')); return; }
    items.forEach(o => {
      const tile = vEl('button', 'vd-slide vd-overlay' + (o.live ? ' is-on' : ''));
      tile.type = 'button';
      tile.setAttribute('aria-pressed', String(!!o.live));
      tile.title = `${o.live ? 'Hide' : 'Show'} ${o.name || 'overlay'} on the screens`;
      tile.appendChild(vEl('span', 'vd-slide-text', o.name || 'Overlay'));
      tile.addEventListener('click', async () => {
        const on = !o.live;
        try {
          await fetch(`${SERVER}/api/overlays/${o.id}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ live: on }) });
          o.live = on;
          tile.classList.toggle('is-on', on); tile.setAttribute('aria-pressed', String(on));
        } catch {}
      });
      body.appendChild(tile);
    });
  }

  // ── Mount / refresh ──────────────────────────────────────────────────────
  function mount() {
    if (K.mounted) return;
    const center = document.querySelector('main.center-stage');
    if (!center) return;
    K.mounted = true;
    const root = vEl('section', 'vd-dock');
    root.setAttribute('aria-label', 'Inputs and media');
    const mediaBox = vEl('div', 'vd-media');
    K.chips = vEl('div', 'vd-chips');
    K.chips.setAttribute('role', 'group');
    K.chips.setAttribute('aria-label', 'Media');
    K.body = vEl('div', 'vd-media-body');
    mediaBox.append(K.chips, K.body);
    root.append(mediaBox);
    center.appendChild(root);
    K.root = root;

    if (!CHIPS.some(c => c.id === K.active && c.available())) K.active = (CHIPS.find(c => c.available()) || {}).id || 'slides';
    renderMedia();
  }

  function unmount() {
    if (!K.mounted) return;
    K.mounted = false;
    K.slideObs?.disconnect(); K.slideObs = null;
    releaseBin();
    if (K.binToggled) { document.getElementById('media-bin-toggle-btn')?.click(); K.binToggled = false; }
    K.binHome?.remove(); K.binHome = null;
    K.root?.remove(); K.root = null;
  }

  window.KairoVideoDock = { mount, unmount, show };
})();
