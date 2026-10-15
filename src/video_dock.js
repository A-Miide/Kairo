// KAIRO — Video director mode: the bottom area, two groups side by side.
//
// Playlist (left): only the playlist: its switcher and New button and the list
// of items, the same list as the sidebar on the normal screen (moved here while
// the mode is on). Choosing an item selects it instead of opening it.
// Media bin (right): toggles for Slides, Video, Images, Music and Overlays, each
// switching that kind of content on or off in the bin (several at once, Slides
// on to begin with, remembered).
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
    md: { chips: null, body: null, kinds: new Set(['slides']), laneOff: null },
    selId: null,                // the playlist item whose slides the Slides chip shows
    binHome: null, binToggled: false,
    liveKey: null,              // the last slide sent from the media bin
  };

  const MD_CHIPS = [
    ['slides', 'Slides', () => !!(window.KairoService && window.KairoService.slidesFor)],
    ['video', 'Video', () => !!document.getElementById('media-bin')],
    ['images', 'Images', () => !!document.getElementById('media-bin')],
    ['music', 'Music', () => !!(window.KairoAudioLane && window.KairoAudioLane.tracks)],
    ['overlays', 'Overlays', () => !!(window.KairoOverlays && window.KairoOverlays.load)],
  ];

  const KINDS_KEY = 'kairo-vd-media-kinds';
  function loadKinds() {
    try {
      const v = JSON.parse(localStorage.getItem(KINDS_KEY) || 'null');
      if (Array.isArray(v)) return new Set(v.filter(k => MD_CHIPS.some(c => c[0] === k)));
    } catch {}
    return new Set(['slides']);
  }
  const TILE_KEY = 'kairo-vd-tile';
  function loadTile() { try { const n = Number(localStorage.getItem(TILE_KEY)); return n >= 90 && n <= 240 ? n : 120; } catch { return 120; } }
  function applyTile(n) { if (K.md.body) K.md.body.style.setProperty('--vd-tile', n + 'px'); }

  // The folder list: the bin's own folder menu, behind a small icon instead of a second labelled control.
  function openFolderMenu(anchor) {
    const sel = document.getElementById('media-bin-folder');
    if (!sel) return;
    document.querySelectorAll('.vd-menu').forEach(m => m.remove());
    const menu = vEl('div', 'vd-menu');
    [...sel.options].forEach(o => {
      const b = vBtn(o.textContent, `Show ${o.textContent}`, () => { sel.value = o.value; sel.dispatchEvent(new Event('change')); menu.remove(); }, 'vd-menu-item' + (o.value === sel.value ? ' is-on' : ''));
      menu.appendChild(b);
    });
    document.body.appendChild(menu);
    const r = anchor.getBoundingClientRect();
    menu.style.left = Math.max(8, Math.min(window.innerWidth - 200, r.left)) + 'px';
    menu.style.top = Math.min(window.innerHeight - menu.offsetHeight - 8, r.bottom + 4) + 'px';
    const off = (e) => { if (!menu.contains(e.target)) { menu.remove(); document.removeEventListener('mousedown', off, true); } };
    document.addEventListener('mousedown', off, true);
  }

  function saveKinds() { try { localStorage.setItem(KINDS_KEY, JSON.stringify([...K.md.kinds])); } catch {} }

  function toggleRow() {
    K.md.chips.replaceChildren();
    MD_CHIPS.filter(c => c[2]()).forEach(([id, label]) => {
      const on = K.md.kinds.has(id);
      const b = vBtn(label, `${on ? 'Hide' : 'Show'} ${label.toLowerCase()}`, () => {
        if (K.md.kinds.has(id)) K.md.kinds.delete(id); else K.md.kinds.add(id);
        saveKinds(); renderMedia();
      }, 'vd-chip' + (on ? ' is-on' : ''));
      b.setAttribute('aria-pressed', String(on));
      K.md.chips.appendChild(b);
    });
  }

  // For the Slides and Media tabs in the top bar: turn that kind on.
  function show(id) {
    if (!K.mounted) return;
    if (MD_CHIPS.some(c => c[0] === id && c[2]())) { K.md.kinds.add(id); saveKinds(); renderMedia(); }
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
    if (!K.md.kinds.has('slides')) { K.md.kinds.add('slides'); saveKinds(); }
    renderMedia();
  }

  // ── Slides: the selected item's slides as real thumbnails ────────────────
  // Each tile is painted by the playlist's own thumbnail painter (theme
  // background, fonts, layers), but only once it scrolls into view.
  let thumbIO = null;
  function thumbObserver() {
    if (thumbIO) return thumbIO;
    thumbIO = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        const host = e.target;
        if (host.__vdPainted && host.__vdPainted === host.clientWidth) continue;
        const d = host.__vdSlide;
        if (!d || !host.clientWidth || !window.KairoService || !window.KairoService.paintSlideThumb) continue;
        try { window.KairoService.paintSlideThumb(host, d.item, d.s, d.i); host.__vdPainted = host.clientWidth; } catch {}
      }
    }, { root: null, rootMargin: '120px' });
    return thumbIO;
  }
  // The tile size changed: paint again at the new width.
  function repaintThumbs() {
    K.md.body && K.md.body.querySelectorAll('.vd-slide-thumb').forEach(h => { h.__vdPainted = 0; thumbObserver().unobserve(h); thumbObserver().observe(h); });
  }
  // The last slide sent from here shows the on-air frame until a Clear.
  function markLive() {
    K.md.body && K.md.body.querySelectorAll('.vd-slide').forEach(t => t.classList.toggle('is-live', t.dataset.key === K.liveKey));
  }

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
      tile.dataset.key = `${item.id}:${i}`;
      tile.title = `Send ${item.title || 'slide'} ${i + 1} to the screens`;
      const thumb = vEl('div', 'vd-slide-thumb');
      thumb.__vdSlide = { item, s, i };
      tile.appendChild(thumb);
      tile.appendChild(vEl('span', 'vd-slide-num', `${i + 1}.`));
      tile.addEventListener('click', () => { K.liveKey = tile.dataset.key; markLive(); svc.sendSlide(item, i); });
      // Drag a slide onto Preview to stage it. Draggable only while pressed.
      tile.draggable = false;
      tile.addEventListener('mousedown', () => { tile.draggable = true; });
      tile.addEventListener('mouseup', () => { tile.draggable = false; });
      tile.addEventListener('dragstart', (e) => { e.dataTransfer.setData('application/x-kairo-slide', JSON.stringify({ itemId: item.id, index: i })); e.dataTransfer.effectAllowed = 'copy'; });
      tile.addEventListener('dragend', () => { tile.draggable = false; });
      body.appendChild(tile);
      thumbObserver().observe(thumb);
    });
    markLive();
  }

  // ── Media bin group ──────────────────────────────────────────────────────
  function releaseBin() {
    const bin = document.getElementById('media-bin');
    if (bin && K.binHome && K.binHome.parentNode && bin.parentNode !== K.binHome.parentNode) K.binHome.after(bin);
  }

  // One section per kind that is on, all in the same tile size.
  function section(kind, label) {
    const sec = vEl('div', 'vd-kind');
    sec.dataset.kind = kind;
    sec.appendChild(vEl('span', 'vd-kind-head', label));
    const tiles = vEl('div', 'vd-kind-tiles');
    sec.appendChild(tiles);
    K.md.body.appendChild(sec);
    return tiles;
  }

  function renderMedia() {
    toggleRow();
    K.md.laneOff && K.md.laneOff(); K.md.laneOff = null;
    releaseBin();
    const body = K.md.body;
    body.replaceChildren();
    const on = (id) => K.md.kinds.has(id) && MD_CHIPS.some(c => c[0] === id && c[2]());
    if (on('slides')) renderSlides(section('slides', 'Slides'));
    if (on('video') || on('images')) renderBin(section('bin', on('video') && on('images') ? 'Video and images' : on('video') ? 'Video' : 'Images'), on('video'), on('images'));
    if (on('music')) renderMusic(section('music', 'Music'));
    if (on('overlays')) renderOverlays(section('overlays', 'Overlays'));
    if (K.md.folderBtn) K.md.folderBtn.hidden = !(on('video') || on('images'));
    if (!body.children.length) body.appendChild(vEl('p', 'vd-empty', 'Switch on what you want to see.'));
  }

  function renderBin(body, video, images) {
    const bin = document.getElementById('media-bin');
    if (!bin) return;
    if (!K.binHome) { K.binHome = document.createComment('media-bin'); bin.before(K.binHome); }
    // If the operator had hidden the bin, show it so it loads its items.
    if (bin.classList.contains('hidden')) { document.getElementById('media-bin-toggle-btn')?.click(); K.binToggled = true; }
    // Which of its cards show: videos carry a badge, images do not.
    body.parentElement.dataset.show = video && images ? 'both' : video ? 'video' : 'images';
    body.appendChild(bin);
  }

  function renderMusic(body) {
    const lane = window.KairoAudioLane;
    const draw = () => {
      if (!K.mounted || !K.md.kinds.has('music') || !body.isConnected) return;
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
    if (!K.md.kinds.has('overlays') || !K.mounted || !body.isConnected) return;
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
    const mdHead = vEl('div', 'vd-md-head');
    mdHead.appendChild(vEl('span', 'vd-label', 'Media bin'));
    const mdRow = vEl('div', 'vd-md-row');
    K.md.chips = vEl('div', 'vd-chips');
    K.md.chips.setAttribute('role', 'group'); K.md.chips.setAttribute('aria-label', 'Media filter');
    // A folder icon at the end of the toggles (shown while the bin's own kinds are on), then the tile size at the far right.
    K.md.folderBtn = vBtn('', 'Choose a media folder', (e) => openFolderMenu(e.currentTarget), 'vd-icon-btn');
    K.md.folderBtn.setAttribute('aria-label', 'Choose a media folder');
    K.md.folderBtn.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/></svg>';
    const size = vEl('input', 'vd-size'); size.type = 'range'; size.min = '90'; size.max = '240'; size.step = '10';
    size.value = String(loadTile()); size.title = 'Tile size'; size.setAttribute('aria-label', 'Tile size');
    size.addEventListener('input', () => applyTile(Number(size.value)));
    size.addEventListener('change', () => { try { localStorage.setItem(TILE_KEY, size.value); } catch {} repaintThumbs(); });
    mdRow.append(K.md.chips, K.md.folderBtn, size);
    mdHead.appendChild(mdRow);
    K.md.body = vEl('div', 'vd-media-body');
    K.md.kinds = loadKinds();
    applyTile(loadTile());
    md.append(mdHead, K.md.body);

    host.append(pl, md);
    // A Clear button takes the on-air frame off the slide tiles.
    K.clearOff = (e) => { if (e.target.closest && e.target.closest('.rs-layer-clear-bar button') && e.target.closest('.rs-layer-clear-bar button').id !== 'output-back-btn') { K.liveKey = null; markLive(); } };
    document.addEventListener('click', K.clearOff, true);
    if (!K.selId && items().length) K.selId = items()[0].id;
    markSelected();
    renderMedia();
    // The playlist changes while the mode is on: items added or removed, slides edited.
    if (list) { K.pl.obs = new MutationObserver(() => { markSelected(); if (K.md.kinds.has('slides')) renderMedia(); }); K.pl.obs.observe(list, { childList: true }); }
  }

  function unmount() {
    if (!K.mounted) return;
    K.mounted = false;
    K.pl.obs?.disconnect(); K.pl.obs = null;
    thumbIO && thumbIO.disconnect(); thumbIO = null;
    document.removeEventListener('click', K.clearOff, true);
    document.querySelectorAll('.vd-menu').forEach(m => m.remove());
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
