// KAIRO — Video director mode: the bottom area, two groups side by side.
//
// Playlist (left): only the playlist: its switcher and New button and the list
// of items, the same list as the sidebar on the normal screen (moved here while
// the mode is on). Choosing an item selects it instead of opening it.
// Media bin (right): toggles for Slides, Songs and then one for each media
// folder in the Media library (the bin itself and each folder, by its own
// name). Several can be on at once, Slides is on to begin with, and the choice
// is remembered. Everything shows in one grid of equal tiles that fills the
// width (the size slider sets the smallest tile) and scrolls when there are
// more than fit.
//   Slides / Songs: the selected playlist item's slides, as the playlist's own
//   slide cards (so a click sends and a right-click edits exactly as on the
//   normal screen), painted only as they scroll into view.
//   A folder: its images and videos, as the Media Bin's own cards.
// Slide and media sends go straight out; only the verses you send by hand are
// staged on Preview.
'use strict';
(function () {
  const K = {
    mounted: false, root: null,
    pl: { body: null, obs: null, headHome: null, listHome: null, click: null },
    md: { chips: null, body: null, kinds: new Set(['slides']), folders: [], binCount: 0, fetched: 0, fetching: false },
    selId: null,                // the playlist item whose slides the toggles show
  };

  const svc = () => window.KairoService;
  const KINDS_KEY = 'kairo-vd-media-kinds2';
  function loadKinds() {
    try { const v = JSON.parse(localStorage.getItem(KINDS_KEY) || 'null'); if (Array.isArray(v)) return new Set(v.filter(k => typeof k === 'string')); } catch {}
    return new Set(['slides']);
  }
  function saveKinds() { try { localStorage.setItem(KINDS_KEY, JSON.stringify([...K.md.kinds])); } catch {} }
  const TILE_KEY = 'kairo-vd-tile';
  function loadTile() { try { const n = Number(localStorage.getItem(TILE_KEY)); return n >= 90 && n <= 240 ? n : 120; } catch { return 120; } }
  function applyTile(n) { if (K.md.body) K.md.body.style.setProperty('--vd-tile', n + 'px'); }

  // For the Slides and Media tabs in the top bar: turn that toggle on.
  function show(id) {
    if (!K.mounted) return;
    const kind = id === 'slides' ? 'slides' : null;
    if (kind) { K.md.kinds.add(kind); saveKinds(); renderMedia(); }
  }

  // ── Playlist: selection ──────────────────────────────────────────────────
  const items = () => ((svc() && svc().service && svc().service.items) || []);
  function markSelected() {
    const list = document.getElementById('svc-items-list');
    if (!list) return;
    if (!items().some(i => i.id === K.selId)) K.selId = null;
    list.querySelectorAll('.svc-item').forEach(r => r.classList.toggle('vd-selected', r.dataset.id === K.selId));
  }
  function select(id) {
    K.selId = id;
    markSelected();
    const it = items().find(i => i.id === id);
    const need = it && it.type === 'song' ? 'songs' : 'slides';
    if (!K.md.kinds.has(need)) { K.md.kinds.add(need); saveKinds(); }
    renderMedia();
  }

  // ── Lazy painting of slide thumbnails ────────────────────────────────────
  let thumbIO = null;
  function thumbObserver() {
    if (thumbIO) return thumbIO;
    thumbIO = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        const host = e.target;
        if (host.__vdPainted && host.__vdPainted === host.clientWidth) continue;
        const d = host.__vdSlide;
        if (!d || !host.clientWidth || !svc() || !svc().paintSlideThumb) continue;
        try {
          const label = host.__pendingPaint && host.__pendingPaint.label;
          svc().paintSlideThumb(host, d.item, d.s, d.i);
          if (label) host.appendChild(label);
          host.__vdPainted = host.clientWidth;
        } catch {}
      }
    }, { root: null, rootMargin: '120px' });
    return thumbIO;
  }
  // The tile size changed: paint again at the new width.
  function repaintThumbs() {
    K.md.body && K.md.body.querySelectorAll('.svc-slide-preview').forEach(h => { h.__vdPainted = 0; thumbObserver().unobserve(h); thumbObserver().observe(h); });
  }

  // ── Media folders: the bin itself and each folder, by their own names ────
  async function loadFolders() {
    if (K.md.fetching) return;
    K.md.fetching = true;
    try {
      const [binRes, foldersRes] = await Promise.all([
        fetch(`${SERVER}/api/media/bin`).then(r => r.json()).catch(() => ({})),
        fetch(`${SERVER}/api/media/folders`).then(r => r.json()).catch(() => ({})),
      ]);
      K.md.binCount = (binRes.items || []).length;
      K.md.folders = (foldersRes.folders || []).map(f => ({ id: f.id, name: f.name || 'Folder' }));
      K.md.fetched = Date.now();
    } finally { K.md.fetching = false; }
    if (K.mounted) renderMedia();
  }
  // The toggles for media: the bin (when it has something) and every folder.
  const mediaToggles = () => {
    const list = K.md.folders.map(f => ({ id: 'folder:' + f.id, label: f.name, source: f.id }));
    if (K.md.folders.length || K.md.binCount) list.unshift({ id: 'folder:bin', label: 'Media Bin', source: 'bin' });
    return list;
  };

  // ── Media bin group ──────────────────────────────────────────────────────
  function toggleRow() {
    K.md.chips.replaceChildren();
    const all = [{ id: 'slides', label: 'Slides' }, { id: 'songs', label: 'Songs' }].concat(mediaToggles());
    all.forEach(({ id, label }) => {
      const on = K.md.kinds.has(id);
      const b = vBtn(label, `${on ? 'Hide' : 'Show'} ${label}`, () => {
        if (K.md.kinds.has(id)) K.md.kinds.delete(id); else K.md.kinds.add(id);
        saveKinds(); renderMedia();
      }, 'vd-chip' + (on ? ' is-on' : ''));
      b.setAttribute('aria-pressed', String(on));
      K.md.chips.appendChild(b);
    });
  }

  function slideCards(grid, wantSong) {
    const item = items().find(i => i.id === K.selId);
    if (!item || (item.type === 'song') !== wantSong) return 0;
    const slides = svc().slidesFor(item);
    slides.forEach((s, i) => {
      const card = svc().slideCard(item, s, i);          // the playlist's own card: click sends, right-click edits
      const preview = card.querySelector('.svc-slide-preview');
      if (preview) { preview.__vdSlide = { item, s, i }; if (!s.image) thumbObserver().observe(preview); }
      card.dataset.vdItem = item.id;
      // A click stages the slide (orange outline); a double-click sends it, and then it shows as live.
      card.addEventListener('click', () => setTimeout(() => window.KairoVideoDirector && window.KairoVideoDirector.refresh(), 700));
      // Drag a slide onto Preview to stage it. Draggable only while pressed.
      card.draggable = false;
      card.addEventListener('mousedown', () => { card.draggable = true; });
      card.addEventListener('mouseup', () => { card.draggable = false; });
      card.addEventListener('dragstart', (e) => { e.dataTransfer.setData('application/x-kairo-slide', JSON.stringify({ itemId: item.id, index: i })); e.dataTransfer.effectAllowed = 'copy'; });
      card.addEventListener('dragend', () => { card.draggable = false; });
      grid.appendChild(card);
    });
    return slides.length;
  }

  async function folderCards(grid, source, token) {
    let all = [];
    try { all = await svc().loadMediaBinItems(); } catch {}
    if (token !== K.token || !K.mounted) return;
    all.filter(it => it.binSource === source).forEach(it => grid.appendChild(svc().buildMediaBinCard(it)));   // the bin's own card: click sends, right-click sets the fit
    emptyNote(grid);
  }
  function emptyNote(grid) {
    if (grid.isConnected && !grid.children.length) grid.appendChild(vEl('p', 'vd-empty', 'Switch on what you want to see, or choose an item in the playlist.'));
  }

  function renderMedia() {
    if (!K.mounted) return;
    toggleRow();
    const body = K.md.body;
    thumbIO && thumbIO.disconnect(); thumbIO = null;
    body.replaceChildren();
    const grid = vEl('div', 'vd-grid');
    body.appendChild(grid);
    const token = K.token = (K.token || 0) + 1;
    if (K.md.kinds.has('slides')) slideCards(grid, false);
    if (K.md.kinds.has('songs')) slideCards(grid, true);
    const folderIds = [...K.md.kinds].filter(k => k.startsWith('folder:') && mediaToggles().some(t => t.id === k)).map(k => k.slice(7));
    if (folderIds.length) {
      (async () => { for (const id of folderIds) await folderCards(grid, id, token); })();
      // A folder may have changed since it was last read.
      if (Date.now() - K.md.fetched > 8000) loadFolders();
    }
    if (!folderIds.length) emptyNote(grid);
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
    const plHead = vEl('div', 'vd-gh vd-pl-top');
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
    // One header row: the title, the toggles beside it, the tile size at the far right.
    const mdHead = vEl('div', 'vd-gh');
    mdHead.appendChild(vEl('span', 'vd-label vd-gh-title', 'Media bin'));
    K.md.chips = vEl('div', 'vd-chips vd-gh-ctl vd-gh-start');
    K.md.chips.setAttribute('role', 'group'); K.md.chips.setAttribute('aria-label', 'Media filter');
    const size = vEl('input', 'vd-size'); size.type = 'range'; size.min = '90'; size.max = '240'; size.step = '10';
    size.value = String(loadTile()); size.title = 'Smallest tile size'; size.setAttribute('aria-label', 'Tile size');
    size.addEventListener('input', () => { applyTile(Number(size.value)); });
    size.addEventListener('change', () => { try { localStorage.setItem(TILE_KEY, size.value); } catch {} repaintThumbs(); });
    mdHead.append(K.md.chips, size);
    K.md.body = vEl('div', 'vd-media-body');
    K.md.kinds = loadKinds();
    applyTile(loadTile());
    md.append(mdHead, K.md.body);

    host.append(pl, md);
    if (!K.selId && items().length) K.selId = items()[0].id;
    markSelected();
    renderMedia();
    loadFolders();
    // The playlist changes while the mode is on: items added or removed, slides edited.
    if (list) { K.pl.obs = new MutationObserver(() => { markSelected(); if (K.md.kinds.has('slides') || K.md.kinds.has('songs')) renderMedia(); }); K.pl.obs.observe(list, { childList: true }); }
  }

  function unmount() {
    if (!K.mounted) return;
    K.mounted = false;
    K.pl.obs?.disconnect(); K.pl.obs = null;
    thumbIO && thumbIO.disconnect(); thumbIO = null;
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
