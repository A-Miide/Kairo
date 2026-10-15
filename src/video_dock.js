// KAIRO — Video director mode: the bottom area, two groups side by side.
//
// Playlist (left): only the playlist: its switcher and New button and the list
// of items, the same list as the sidebar on the normal screen (moved here while
// the mode is on). Choosing an item selects it instead of opening it.
// Media bin (right): chips for Slides, Video, Images, Music and Overlays.
// Slides shows the selected playlist item's slides (songs and scripture too),
// and a click sends one through the playlist's own sendSlide. Video and Images
// reuse the Media Bin's own cards, Music plays through the audio lane, and
// Overlays switch on and off. Slide, media, music and overlay sends go straight
// out; only the verses you send by hand are staged on Preview.
'use strict';
(function () {
  const K = {
    mounted: false, root: null,
    pl: { body: null, obs: null, headHome: null, listHome: null, click: null },
    md: { chips: null, body: null, kind: 'slides', laneOff: null },
    selId: null,                // the playlist item whose slides the Slides chip shows
    binHome: null, binToggled: false,
  };

  const MD_CHIPS = [
    ['slides', 'Slides', () => !!(window.KairoService && window.KairoService.slidesFor)],
    ['video', 'Video', () => !!document.getElementById('media-bin')],
    ['images', 'Images', () => !!document.getElementById('media-bin')],
    ['music', 'Music', () => !!(window.KairoAudioLane && window.KairoAudioLane.tracks)],
    ['overlays', 'Overlays', () => !!(window.KairoOverlays && window.KairoOverlays.load)],
  ];

  function chipRow(host, list, current, onPick) {
    host.replaceChildren();
    list.forEach(([id, label]) => {
      const b = vBtn(label, `Show ${label.toLowerCase()}`, () => onPick(id), 'vd-chip' + (current === id ? ' is-on' : ''));
      b.setAttribute('aria-pressed', String(current === id));
      host.appendChild(b);
    });
  }

  // For the Slides and Media tabs in the top bar.
  function show(id) {
    if (!K.mounted) return;
    if (MD_CHIPS.some(c => c[0] === id && c[2]())) { K.md.kind = id; renderMedia(); }
  }

  // ── Playlist: selection ──────────────────────────────────────────────────
  const items = () => ((window.KairoService && window.KairoService.service && window.KairoService.service.items) || []);
  function markSelected() {
    const list = document.getElementById('svc-items-list');
    if (!list) return;
    if (!items().some(i => i.id === K.selId)) K.selId = null;
    list.querySelectorAll('.svc-item').forEach(r => r.classList.toggle('vd-selected', r.dataset.id === K.selId));
  }
  function select(id) {
    K.selId = id;
    markSelected();
    if (K.md.kind !== 'slides') K.md.kind = 'slides';
    renderMedia();
  }

  // ── Slides chip: the selected item's slides ──────────────────────────────
  function renderSlides(body) {
    const svc = window.KairoService;
    const item = items().find(i => i.id === K.selId);
    if (!item) { body.appendChild(vEl('p', 'vd-empty', items().length ? 'Choose an item in the playlist to see its slides.' : 'Nothing in this playlist yet.')); return; }
    const slides = svc.slidesFor(item);
    if (!slides.length) { body.appendChild(vEl('p', 'vd-empty', 'This item has no slides.')); return; }
    body.appendChild(vEl('span', 'vd-group', item.title || 'Untitled'));
    slides.forEach((s, i) => {
      const tile = vEl('button', 'vd-slide');
      tile.type = 'button';
      tile.title = `Send ${item.title || 'slide'} ${i + 1} to the screens`;
      if (s.image) { const img = vEl('img'); img.src = s.image; img.alt = ''; img.loading = 'lazy'; tile.appendChild(img); }
      else tile.appendChild(vEl('span', 'vd-slide-text', (s.text || s.label || '').slice(0, 90)));
      tile.appendChild(vEl('span', 'vd-slide-num', String(i + 1)));
      tile.addEventListener('click', () => svc.sendSlide(item, i));
      body.appendChild(tile);
    });
  }

  // ── Media bin group ──────────────────────────────────────────────────────
  function releaseBin() {
    const bin = document.getElementById('media-bin');
    if (bin && K.binHome && K.binHome.parentNode && bin.parentNode !== K.binHome.parentNode) K.binHome.after(bin);
  }

  function renderMedia() {
    const avail = MD_CHIPS.filter(c => c[2]());
    if (!avail.some(c => c[0] === K.md.kind)) K.md.kind = (avail[0] || [])[0];
    chipRow(K.md.chips, avail.map(c => [c[0], c[1]]), K.md.kind, (id) => { K.md.kind = id; renderMedia(); });
    K.md.laneOff && K.md.laneOff(); K.md.laneOff = null;
    releaseBin();
    const body = K.md.body;
    body.replaceChildren();
    body.dataset.kind = K.md.kind;
    if (K.md.kind === 'slides') renderSlides(body);
    else if (K.md.kind === 'music') renderMusic(body);
    else if (K.md.kind === 'overlays') renderOverlays(body);
    else if (K.md.kind) renderBin(body);
  }

  function renderBin(body) {
    const bin = document.getElementById('media-bin');
    if (!bin) return;
    if (!K.binHome) { K.binHome = document.createComment('media-bin'); bin.before(K.binHome); }
    // If the operator had hidden the bin, show it so it loads its items.
    if (bin.classList.contains('hidden')) { document.getElementById('media-bin-toggle-btn')?.click(); K.binToggled = true; }
    body.appendChild(bin);
  }

  function renderMusic(body) {
    const lane = window.KairoAudioLane;
    const draw = () => {
      if (!K.mounted || K.md.kind !== 'music') return;
      body.replaceChildren();
      const st = lane.state();
      if (!lane.tracks.length) { body.appendChild(vEl('p', 'vd-empty', 'No music yet. Add some in Settings, Audio.')); return; }
      lane.tracks.forEach((t, i) => {
        const playing = st.index === i && st.playing;
        const tile = vEl('button', 'vd-slide vd-overlay' + (playing ? ' is-on' : ''));
        tile.type = 'button';
        tile.setAttribute('aria-pressed', String(playing));
        tile.title = playing ? `Pause ${t.label}` : `Play ${t.label}`;
        tile.append(vEl('span', 'vd-slide-text', t.label), vEl('span', 'vd-slide-num', playing ? 'Playing' : 'Music'));
        tile.addEventListener('click', () => (i === st.index ? lane.toggle() : lane.play(i)));
        body.appendChild(tile);
      });
    };
    lane.refresh().then(draw);
    K.md.laneOff = lane.onChange(draw);
  }

  async function renderOverlays(body) {
    body.appendChild(vEl('p', 'vd-empty', 'Loading overlays'));
    let items = [];
    try { items = await window.KairoOverlays.load(); } catch {}
    if (K.md.kind !== 'overlays' || !K.mounted) return;
    body.replaceChildren();
    if (!items.length) { body.appendChild(vEl('p', 'vd-empty', 'No overlays yet. Add some in Settings, Overlays.')); return; }
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

  // ── Mount / unmount ──────────────────────────────────────────────────────
  function group(label) {
    const sec = vEl('section', 'vd-section vd-group-box');
    sec.setAttribute('aria-label', label);
    return sec;
  }

  function mount(host) {
    if (K.mounted || !host) return;
    K.mounted = true;
    K.root = host;

    const pl = group('Playlist');
    const plHead = vEl('div', 'vd-section-head vd-pl-top');
    // The playlist's own switcher and New button, so playlists are managed here.
    const home = document.querySelector('.ls-playlist-section .ls-transcript-header');
    if (home) { K.pl.headHome = document.createComment('playlist-head'); home.before(K.pl.headHome); home.classList.add('vd-pl-head'); plHead.appendChild(home); }
    else plHead.appendChild(vEl('span', 'vd-label', 'Playlist'));
    // ...and its list of items, the same one the sidebar shows.
    const list = document.getElementById('svc-items-list');
    K.pl.body = vEl('div', 'vd-pl-list');
    if (list) { K.pl.listHome = document.createComment('playlist-list'); list.before(K.pl.listHome); K.pl.body.appendChild(list); }
    pl.append(plHead, K.pl.body);
    // A click on an item selects it here (it would open the sidebar's slide stack, which this mode does not show).
    K.pl.click = (e) => {
      const row = e.target.closest('.svc-item');
      if (!row || e.target.closest('button,input,textarea')) return;
      e.stopPropagation(); e.preventDefault();
      select(row.dataset.id);
    };
    list && list.addEventListener('click', K.pl.click, true);

    const md = group('Media bin');
    const mdHead = vEl('div', 'vd-section-head');
    mdHead.appendChild(vEl('span', 'vd-label', 'Media bin'));
    K.md.chips = vEl('div', 'vd-chips');
    K.md.chips.setAttribute('role', 'group'); K.md.chips.setAttribute('aria-label', 'Media filter');
    mdHead.appendChild(K.md.chips);
    K.md.body = vEl('div', 'vd-media-body');
    md.append(mdHead, K.md.body);

    host.append(pl, md);
    if (!K.selId && items().length) K.selId = items()[0].id;
    markSelected();
    renderMedia();
    // The playlist changes while the mode is on: items added or removed, slides edited.
    if (list) { K.pl.obs = new MutationObserver(() => { markSelected(); if (K.md.kind === 'slides') renderMedia(); }); K.pl.obs.observe(list, { childList: true }); }
  }

  function unmount() {
    if (!K.mounted) return;
    K.mounted = false;
    K.pl.obs?.disconnect(); K.pl.obs = null;
    K.md.laneOff && K.md.laneOff(); K.md.laneOff = null;
    releaseBin();
    if (K.binToggled) { document.getElementById('media-bin-toggle-btn')?.click(); K.binToggled = false; }
    K.binHome?.remove(); K.binHome = null;
    const list = document.getElementById('svc-items-list');
    if (list) {
      list.removeEventListener('click', K.pl.click, true);
      list.querySelectorAll('.vd-selected').forEach(r => r.classList.remove('vd-selected'));
      if (K.pl.listHome && K.pl.listHome.parentNode) K.pl.listHome.after(list);
    }
    K.pl.listHome?.remove(); K.pl.listHome = null; K.pl.click = null;
    const head = document.querySelector('.vd-pl-head');
    if (head && K.pl.headHome && K.pl.headHome.parentNode) { K.pl.headHome.after(head); head.classList.remove('vd-pl-head'); }
    K.pl.headHome?.remove(); K.pl.headHome = null;
    K.root?.replaceChildren(); K.root = null;
  }

  window.KairoVideoDock = { mount, unmount, show };
})();
