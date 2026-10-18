// KAIRO — Video director mode: the bottom area, two groups side by side.
//
// Playlist (left): only the playlist: its switcher and New button and the list
// of items, the same list as the sidebar on the normal screen (moved here while
// the mode is on). Choosing an item selects it instead of opening it.
// Media bin (right): one selector, Slides or Media Bin (owner, 2026-10-17: not
// toggles), remembered. Media Bin has a folder menu like the normal screen's: all
// media, the bin's own items, then each folder. Everything shows in one grid of
// equal tiles that fills the width (the size slider sets the smallest tile) and
// scrolls when there are more than fit.
//   Slides: the selected playlist item's slides (a deck or a song), as the
//   playlist's own slide cards (so a click sends and a right-click edits exactly
//   as on the normal screen), painted only as they scroll into view.
//   Media Bin: the chosen folder's images and videos, as the Media Bin's own cards.
// A click stages a slide or media item on Preview, as everywhere in Director mode;
// a double-click sends it at once.
'use strict';
(function () {
  const K = {
    mounted: false, root: null,
    pl: { body: null, obs: null, headHome: null, listHome: null, click: null },
    md: { chips: null, body: null, view: 'slides', folder: 'all', folders: [], binCount: 0, fetched: 0, fetching: false },
    selId: null,                // the playlist item whose slides the toggles show
  };

  const svc = () => window.KairoService;
  const VIEW_KEY = 'kairo-vd-media-view', FOLDER_KEY = 'kairo-vd-media-folder';
  const readPref = (k, d) => { try { return localStorage.getItem(k) || d; } catch { return d; } };
  const writePref = (k, v) => { try { localStorage.setItem(k, v); } catch {} };
  function setView(view) { K.md.view = view === 'media' ? 'media' : 'slides'; writePref(VIEW_KEY, K.md.view); }
  const TILE_KEY = 'kairo-vd-tile';
  function loadTile() { try { const n = Number(localStorage.getItem(TILE_KEY)); return n >= 90 && n <= 240 ? n : 120; } catch { return 120; } }
  function applyTile(n) { if (K.md.body) K.md.body.style.setProperty('--vd-tile', n + 'px'); }

  // For the Slides and Media tabs in the top bar: show that view.
  function show(id) {
    if (!K.mounted) return;
    if (id === 'slides' || id === 'media') { setView(id); renderMedia(); }
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
    setView('slides');   // choosing an item shows its slides
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
  // The folder menu: all media, the bin's own items, then each folder (as on the normal screen).
  const folderOptions = () => [['all', 'All media'], ['bin', 'Media Bin'], ...K.md.folders.map(f => [f.id, f.name])];

  // ── Media bin group ──────────────────────────────────────────────────────
  function toggleRow() {
    K.md.chips.replaceChildren();
    // Slides or Media Bin: one or the other.
    [['slides', 'Slides'], ['media', 'Media Bin']].forEach(([view, label]) => {
      const on = K.md.view === view;
      const b = vBtn(label, `Show ${label}`, () => { if (K.md.view !== view) { setView(view); renderMedia(); } }, 'vd-chip' + (on ? ' is-on' : ''));
      b.setAttribute('role', 'radio');
      b.setAttribute('aria-checked', String(on));
      K.md.chips.appendChild(b);
    });
    if (K.md.view !== 'media') return;
    const sel = vEl('select', 'media-bin-folder vd-folder');
    sel.title = 'Show one folder'; sel.setAttribute('aria-label', 'Folder');
    const opts = folderOptions();
    opts.forEach(([v, label]) => { const o = document.createElement('option'); o.value = v; o.textContent = label; sel.appendChild(o); });
    sel.value = opts.some(([v]) => v === K.md.folder) ? K.md.folder : 'all';
    sel.addEventListener('change', () => { K.md.folder = sel.value; writePref(FOLDER_KEY, sel.value); renderMedia(); });
    K.md.chips.appendChild(sel);
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
    (source === 'all' ? all : all.filter(it => it.binSource === source)).forEach(it => grid.appendChild(svc().buildMediaBinCard(it)));   // the bin's own card: click sends, right-click sets the fit
    emptyNote(grid, all.length ? 'Nothing in this folder yet.' : 'No media yet: add some from the Media tab.');
  }
  function emptyNote(grid, text) {
    if (grid.isConnected && !grid.children.length) grid.appendChild(vEl('p', 'vd-empty', text || 'Choose an item in the playlist to see its slides.'));
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
    if (K.md.view === 'media') {
      const source = folderOptions().some(([v]) => v === K.md.folder) ? K.md.folder : 'all';
      folderCards(grid, source, token);
      // A folder may have changed since it was last read.
      if (Date.now() - K.md.fetched > 8000) loadFolders();
      return;
    }
    slideCards(grid, false);   // a deck's slides
    slideCards(grid, true);    // or a song's
    emptyNote(grid);
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
    // Slides, songs and scripture can be added without leaving the board (the Slides screen's
    // "+ Add content"): the new item appears in the list below and its slides in the media area.
    K.pl.addBtn = vBtn('+ Add', 'Add slides, a song or scripture to this playlist', (e) => svc() && svc().openAddContent && svc().openAddContent(e.currentTarget), 'vd-mini vd-add vd-pl-add');
    plHead.appendChild(K.pl.addBtn);
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
    // One header row: Slides or Media Bin (and the folder menu), the tile size at the far right.
    const mdHead = vEl('div', 'vd-gh');
    K.md.chips = vEl('div', 'vd-chips vd-gh-ctl vd-gh-start');
    K.md.chips.setAttribute('role', 'radiogroup'); K.md.chips.setAttribute('aria-label', 'Show');
    const size = vEl('input', 'vd-size'); size.type = 'range'; size.min = '90'; size.max = '240'; size.step = '10';
    size.value = String(loadTile()); size.title = 'Smallest tile size'; size.setAttribute('aria-label', 'Tile size');
    size.addEventListener('input', () => { applyTile(Number(size.value)); });
    size.addEventListener('change', () => { try { localStorage.setItem(TILE_KEY, size.value); } catch {} repaintThumbs(); });
    mdHead.append(K.md.chips, size);
    K.md.body = vEl('div', 'vd-media-body');
    K.md.view = readPref(VIEW_KEY, 'slides') === 'media' ? 'media' : 'slides';
    K.md.folder = readPref(FOLDER_KEY, 'all');
    applyTile(loadTile());
    md.append(mdHead, K.md.body);

    host.append(pl, md);
    if (!K.selId && items().length) K.selId = items()[0].id;
    markSelected();
    renderMedia();
    loadFolders();
    // The playlist changes while the mode is on: items added or removed, slides edited.
    if (list) { K.pl.obs = new MutationObserver(() => { markSelected(); if (K.md.view === 'slides') renderMedia(); }); K.pl.obs.observe(list, { childList: true }); }
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
    K.pl.addBtn?.remove(); K.pl.addBtn = null;
    K.root?.replaceChildren(); K.root = null;
  }

  window.KairoVideoDock = { mount, unmount, show, refresh: () => { if (K.mounted) { K.md.fetched = 0; loadFolders(); markSelected(); } } };
})();
