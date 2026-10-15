// KAIRO — Video director mode: the dock, one bar along the bottom.
//
// Left, Inputs: the cameras and capture cards as a row of tiles of their own.
// One click sends an input to Program with the current transition; Shift-click
// or the small Preview button stages it on Preview instead.
// Right, a media shelf with filter chips: Slides (only slides), Video, Images
// and, when the app has them, Overlays. Video and Images reuse the Media Bin's
// own cards (moved here while the mode is on, so clicking one does exactly
// what it does there); Slides sends through the playlist's own sendSlide.
// Thumbnails take their stream from the hub (one low-rate capture per camera)
// and run only while the dock is on screen.
'use strict';
(function () {
  const K = { mounted: false, root: null, inputs: null, body: null, chips: null, active: 'slides', leases: [], thumbsOn: true, io: null, onVis: null, slideObs: null, binHome: null, binToggled: false };

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

  // ── Inputs ───────────────────────────────────────────────────────────────
  function inputTile(id, name, flag, source) {
    const t = vEl('div', 'vd-tile' + (flag === 'PGM' ? ' is-pgm' : '') + (flag === 'PVW' ? ' is-pvw' : ''));
    t.tabIndex = 0;
    t.setAttribute('role', 'button');
    t.setAttribute('aria-label', `${name}. Click to send to Program, Shift-click for Preview`);
    t.title = 'Click: send to Program. Shift-click: stage on Preview.';
    const thumb = vEl('div', 'vd-thumb');
    if (source) {
      const v = vEl('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-hidden', 'true');
      thumb.appendChild(v);
      const devId = previewKey(source);
      if (devId && K.thumbsOn) {
        K.leases.push(window.KairoVideoHub.lease(devId, 'thumb', (stream) => {
          if (!v.isConnected) return;
          v.srcObject = stream; v.play().catch(() => {});
        }, () => { thumb.dataset.msg = 'Unavailable'; }));
      } else thumb.dataset.msg = devId ? '' : 'Not connected';
    } else thumb.classList.add('vd-thumb-none');
    t.append(thumb, vEl('div', 'vd-tile-name', name));
    if (flag) t.appendChild(vEl('span', 'vd-flag ' + (flag === 'PGM' ? 'vd-flag-pgm' : 'vd-flag-pvw'), flag));
    const pvwBtn = vBtn('Preview', `Stage ${name} on Preview`, (e) => { e.stopPropagation(); dir().setPreview(id); }, 'vd-tile-pvw');
    t.appendChild(pvwBtn);
    const go = (e) => (e.shiftKey ? dir().setPreview(id) : dir().sendToProgram(id));
    t.addEventListener('click', go);
    t.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(e); } });
    return t;
  }

  function renderInputs() {
    const st = dir().state();
    const sources = videoSourceList();
    const ps = st.program;
    const old = K.leases;
    K.leases = [];
    const row = K.inputs;
    row.replaceChildren();
    const flagFor = (id) => (!ps.mixed && ps.on.length && (ps.id || null) === (id || null) ? 'PGM' : (st.previewId || null) === (id || null) ? 'PVW' : '');
    row.appendChild(inputTile('', 'No video', flagFor(''), null));
    sources.forEach(s => row.appendChild(inputTile(s.id, s.name, flagFor(s.id), s)));
    if (!sources.length) row.appendChild(vEl('p', 'vd-empty', 'No inputs yet.'));
    old.forEach(release => release());
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
    const inputsBox = vEl('div', 'vd-inputs');
    const head = vEl('div', 'vd-dock-head');
    head.appendChild(vEl('span', 'vd-label', 'Inputs'));
    head.appendChild(vBtn('Manage', 'Add, rename or remove inputs', () => {
      document.getElementById('settings-modal')?.classList.remove('hidden');
      if (typeof showSettingsPane === 'function') showSettingsPane('sources');
    }, 'vd-mini'));
    K.inputs = vEl('div', 'vd-input-row');
    inputsBox.append(head, K.inputs);
    const mediaBox = vEl('div', 'vd-media');
    K.chips = vEl('div', 'vd-chips');
    K.chips.setAttribute('role', 'group');
    K.chips.setAttribute('aria-label', 'Media');
    K.body = vEl('div', 'vd-media-body');
    mediaBox.append(K.chips, K.body);
    root.append(inputsBox, mediaBox);
    center.appendChild(root);
    K.root = root;

    // Thumbnails run only while the dock is on screen and the window is visible.
    let shown = true;
    const update = () => {
      const on = shown && !document.hidden;
      if (on === K.thumbsOn) return;
      K.thumbsOn = on;
      renderInputs();
    };
    K.io = new IntersectionObserver((es) => { shown = es.some(e => e.isIntersecting); update(); });
    K.io.observe(root);
    K.onVis = update;
    document.addEventListener('visibilitychange', K.onVis);
    K.thumbsOn = !document.hidden;
    if (!CHIPS.some(c => c.id === K.active && c.available())) K.active = (CHIPS.find(c => c.available()) || {}).id || 'slides';
    renderInputs();
    renderMedia();
  }

  function unmount() {
    if (!K.mounted) return;
    K.mounted = false;
    K.io?.disconnect(); K.io = null;
    document.removeEventListener('visibilitychange', K.onVis);
    K.slideObs?.disconnect(); K.slideObs = null;
    K.leases.forEach(release => release()); K.leases = [];
    releaseBin();
    if (K.binToggled) { document.getElementById('media-bin-toggle-btn')?.click(); K.binToggled = false; }
    K.binHome?.remove(); K.binHome = null;
    K.root?.remove(); K.root = null;
  }

  function refresh() {
    if (!K.mounted || !dir().state().active) return;
    renderInputs();
  }

  window.KairoVideoDock = { mount, unmount, refresh, show };
})();
