// KAIRO — Looks: saved Preview setups for Video director mode.
//
// A Look is a recipe for what the output shows, made from things the app
// already has: the video input (or none), which layers are on (bible, slide,
// media, timer, video), which overlays are up and, optionally, a screen theme.
// Loading a Look only changes what Preview shows; nothing goes out until it is
// sent to the screen. Looks persist in settings.directorLooks. They live in a
// dialog opened from the Displays header: a grid of tiles, each a 16:9 picture of
// the screen with the name and Alt+number under it and one row of layer icons.
// Click loads it into Preview and closes the dialog (Shift-click keeps it open);
// the "..." menu (or right-click) renames, updates from the current output,
// duplicates, picks the screen theme and deletes (with an undo). Tiles drag to
// reorder. Alt+1-9 load the nth Look without opening the dialog.
'use strict';
(function () {
  // What this feature is called, in one place: a rename is a one-line change.
  const LABEL = { many: 'Layouts', one: 'Layout' };
  const LAYER_KEYS = ['bible', 'slide', 'media', 'timer', 'video'];
  const STARTERS = [
    { name: 'Scripture over video', videoId: '@first', layers: { bible: true, slide: true, media: true, timer: true, video: true }, overlays: null },
    { name: 'Video only', videoId: '@first', layers: { bible: false, slide: false, media: false, timer: false, video: true }, overlays: null },
  ];
  // Looks that frame the picture with a screen theme (Theme Studio, group "Screens").
  const ALL_ON = { bible: true, slide: true, media: true, timer: true, video: true };
  const SPLIT_STARTERS = [
    { name: 'Split, video left', videoId: '@first', layers: ALL_ON, overlays: null, themeId: 'screen-split-left' },
    { name: 'Split, video right', videoId: '@first', layers: ALL_ON, overlays: null, themeId: 'screen-split-right' },
  ];
  const uid = () => 'look-' + Math.random().toString(36).slice(2, 8);

  function normalize(raw) {
    if (!Array.isArray(raw)) return null;
    const out = [];
    const seen = new Set();
    for (const r of raw) {
      if (!r || typeof r !== 'object') continue;
      const id = typeof r.id === 'string' && r.id && !seen.has(r.id) ? r.id : uid();
      seen.add(id);
      const layers = {};
      for (const k of LAYER_KEYS) layers[k] = !(r.layers && r.layers[k] === false);
      out.push({
        id,
        name: String(r.name || LABEL.one).replace(/\s+/g, ' ').trim().slice(0, 40) || LABEL.one,
        videoId: r.videoId === undefined ? null : (typeof r.videoId === 'string' ? r.videoId : null),
        layers,
        overlays: Array.isArray(r.overlays) ? r.overlays.filter(x => typeof x === 'string') : null,
        themeId: typeof r.themeId === 'string' && r.themeId ? r.themeId : null,   // a screen theme, or null: the normal theme stays
      });
    }
    return out;
  }

  const LAYER_LABELS = { bible: 'Bible', slide: 'Slide', media: 'Media', timer: 'Timer', video: 'Video' };
  const svg = (d) => `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;
  const LAYER_ICONS = {
    bible: svg('<path d="M4 4.5A2.5 2.5 0 0 1 6.5 2H20v18H6.5A2.5 2.5 0 0 0 4 22.5z"/><path d="M12 6v8M9 9h6"/>'),
    slide: svg('<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>'),
    media: svg('<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/>'),
    timer: svg('<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2 2M9 2h6"/>'),
    video: svg('<rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/>'),
  };
  const CAM = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg>';
  const DOTS = '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/></svg>';
  const THUMB_PX = 56;   // renderLookThumbnail draws at this width; tiles scale it up

  // ctx: { loadToPreview(look), capture() → Promise<{videoId, layers, overlays, themeId}>,
  //        previewLookId(), liveLookId(), videoName(look), onChange(),
  //        screenThemes() → [theme], renderThumb(el, theme), editTheme(id) }
  function create(ctx) {
    let looks = normalize(settings.directorLooks);
    if (!looks) { looks = normalize([...STARTERS, ...SPLIT_STARTERS]); settings.directorLooksSplit = true; saveSettingsPatch && saveSettingsPatch({ directorLooksSplit: true }); persist(); }
    // Looks saved before the split starters existed get them once.
    else if (settings.directorLooksSplit !== true) {
      looks.push(...normalize(SPLIT_STARTERS).filter(n => !looks.some(l => l.name === n.name)));
      settings.directorLooksSplit = true;
      if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ directorLooksSplit: true });
      persist();
    }
    let modal = null, prevFocus = null, renaming = null, dragId = null, keyHandler = null, menu = null, undoDel = null, undoTimer = null;
    let overlayNames = {};

    function persist() {
      settings.directorLooks = looks;
      if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ directorLooks: looks });
    }
    function uniqueName(base, exceptId) {
      const taken = new Set(looks.filter(l => l.id !== exceptId).map(l => l.name.toLowerCase()));
      let n = base, i = 2;
      while (taken.has(n.toLowerCase())) n = `${base} ${i++}`;
      return n;
    }
    const byId = (id) => looks.find(l => l.id === id);
    const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
    const themeOf = (look) => (look.themeId && ctx.screenThemes ? ctx.screenThemes().find(t => t.id === look.themeId) : null) || null;

    async function saveCurrent() {
      const cap = await ctx.capture();
      const look = { id: uid(), name: uniqueName(LABEL.one + ' ' + (looks.length + 1)), videoId: cap.videoId, layers: cap.layers, overlays: cap.overlays, themeId: cap.themeId || null };
      looks.push(look);
      persist(); renaming = look.id; render();
      const i = modal && modal.querySelector('.vd-card-input'); if (i) { i.focus(); i.select(); }
    }
    async function updateFromCurrent(id) {
      const l = byId(id); if (!l) return;
      const cap = await ctx.capture();
      l.videoId = cap.videoId; l.layers = cap.layers; l.overlays = cap.overlays; l.themeId = cap.themeId || null;
      persist(); render();
    }
    function duplicate(id) {
      const l = byId(id); if (!l) return;
      const copy = { ...l, id: uid(), layers: { ...l.layers }, overlays: l.overlays ? [...l.overlays] : null, name: uniqueName(l.name + ' copy') };
      looks.splice(looks.indexOf(l) + 1, 0, copy);
      persist(); render();
    }
    // Delete with an undo: the Look comes back to where it was.
    function remove(id) {
      const index = looks.findIndex(l => l.id === id);
      if (index < 0) return;
      const [gone] = looks.splice(index, 1);
      undoDel = { look: gone, index };
      clearTimeout(undoTimer); undoTimer = setTimeout(() => { undoDel = null; paintToast(); }, 7000);
      persist(); render(); paintToast(); ctx.onChange();
    }
    function restoreDeleted() {
      if (!undoDel) return;
      looks.splice(Math.min(undoDel.index, looks.length), 0, undoDel.look);
      undoDel = null; clearTimeout(undoTimer);
      persist(); render(); paintToast(); ctx.onChange();
    }
    function paintToast() {
      const t = modal && modal.querySelector('.vd-lk-toast');
      if (!t) return;
      t.replaceChildren();
      t.hidden = !undoDel;
      if (!undoDel) return;
      t.append(el('span', null, `Deleted ${undoDel.look.name}`));
      const b = el('button', 'vd-mini', 'Undo'); b.type = 'button';
      b.addEventListener('click', restoreDeleted);
      t.appendChild(b);
    }
    function commitRename(id, value) {
      const l = byId(id);
      renaming = null;
      if (l) { const v = String(value || '').replace(/\s+/g, ' ').trim().slice(0, 40); if (v) l.name = uniqueName(v, id); persist(); }
      render(); ctx.onChange();
    }

    // ── The picture of a Look: its screen theme, or a plain sketch of its layers. ──
    function thumb(look) {
      const box = el('div', 'vd-lk-thumb');
      const theme = themeOf(look);
      if (theme && ctx.renderThumb) {
        const inner = el('div', 'vd-lk-th-in');
        try { ctx.renderThumb(inner, theme); } catch {}
        box.appendChild(inner);
        const zone = (theme.layers || []).find(l => l && l.type === 'videozone' && l.pos);
        if (zone && look.layers.video) {
          const W = window.KAIRO_DESIGN_W || 1920, H = window.KAIRO_DESIGN_H || 1080;
          const z = el('div', 'vd-lk-zone'); z.innerHTML = CAM;
          z.style.cssText = `left:${zone.pos.x / W * 100}%;top:${zone.pos.y / H * 100}%;width:${zone.pos.w / W * 100}%;height:${zone.pos.h / H * 100}%`;
          box.appendChild(z);
        }
      } else {
        box.classList.add('is-plain');
        if (look.layers.video) { const c = el('div', 'vd-lk-cam'); c.innerHTML = CAM; box.appendChild(c); }
        if (look.layers.bible || look.layers.slide) box.appendChild(el('div', 'vd-lk-bar'));
      }
      return box;
    }
    function fitThumbs() {
      if (!modal) return;
      modal.querySelectorAll('.vd-lk-thumb').forEach(b => {
        const inner = b.querySelector('.vd-lk-th-in');
        if (inner) inner.style.transform = `scale(${(b.clientWidth || THUMB_PX) / THUMB_PX})`;
      });
    }

    function infoLine(look) {
      const parts = [];
      if (look.videoId) { const n = ctx.videoName(look); if (n && n !== 'No video') parts.push(n); }
      if (Array.isArray(look.overlays) && look.overlays.length) parts.push(look.overlays.map(id => overlayNames[id] || null).filter(Boolean).join(', ') || `${look.overlays.length} overlay${look.overlays.length > 1 ? 's' : ''}`);
      return parts.join(' · ');
    }

    function tile(look, index) {
      const isPvw = ctx.previewLookId() === look.id, isLive = ctx.liveLookId() === look.id;
      const c = el('div', 'vd-lk' + (isPvw ? ' is-pvw' : '') + (isLive ? ' is-live' : ''));
      c.dataset.id = look.id;
      c.tabIndex = 0;
      c.setAttribute('role', 'button');
      c.setAttribute('aria-label', `${look.name}${isLive ? ', live' : ''}. Load into Preview`);
      const pic = thumb(look);
      if (isLive) pic.appendChild(el('span', 'vd-lk-live', 'Live'));
      const more = el('button', 'vd-lk-more'); more.type = 'button'; more.innerHTML = DOTS;
      more.title = 'More'; more.setAttribute('aria-label', `More for ${look.name}`);
      more.addEventListener('click', (e) => { e.stopPropagation(); openMenu(look, more); });
      pic.appendChild(more);
      c.appendChild(pic);
      const row = el('div', 'vd-lk-row');
      if (renaming === look.id) {
        const inp = el('input', 'vd-card-input'); inp.type = 'text'; inp.maxLength = 40; inp.value = look.name; inp.setAttribute('aria-label', LABEL.one + ' name');
        inp.addEventListener('click', (e) => e.stopPropagation());
        inp.addEventListener('keydown', (e) => { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); commitRename(look.id, inp.value); } else if (e.key === 'Escape') { e.preventDefault(); renaming = null; render(); } });
        inp.addEventListener('blur', () => { if (renaming === look.id) commitRename(look.id, inp.value); });
        row.appendChild(inp);
      } else {
        row.appendChild(el('span', 'vd-lk-name', look.name));
        if (index < 9) row.appendChild(el('span', 'vd-card-key', `Alt+${index + 1}`));
      }
      c.appendChild(row);
      const icons = el('div', 'vd-lk-icons');
      for (const k of LAYER_KEYS) {
        const i = el('span', 'vd-lk-ic' + (look.layers[k] ? ' is-on' : '')); i.innerHTML = LAYER_ICONS[k];
        i.title = `${LAYER_LABELS[k]} ${look.layers[k] ? 'on' : 'off'}`;
        i.setAttribute('role', 'img'); i.setAttribute('aria-label', i.title);
        icons.appendChild(i);
      }
      c.appendChild(icons);
      const info = infoLine(look);
      if (info) c.appendChild(el('div', 'vd-lk-info', info));

      const load = (e) => { ctx.loadToPreview(look); if (!e.shiftKey) close(); };
      c.addEventListener('click', load);
      c.addEventListener('keydown', (e) => { if ((e.key === 'Enter' || e.key === ' ') && e.target === c) { e.preventDefault(); load(e); } });
      c.addEventListener('contextmenu', (e) => { e.preventDefault(); openMenu(look, more); });
      // Drag to reorder. Draggable only while pressed.
      c.draggable = false;
      c.addEventListener('mousedown', (e) => { if (!e.target.closest('button,input')) c.draggable = true; });
      c.addEventListener('mouseup', () => { c.draggable = false; });
      c.addEventListener('dragstart', (e) => { dragId = look.id; e.dataTransfer.setData('application/x-kairo-look', look.id); e.dataTransfer.effectAllowed = 'move'; });
      c.addEventListener('dragend', () => { dragId = null; c.draggable = false; render(); });
      c.addEventListener('dragover', (e) => { if (dragId && dragId !== look.id) { e.preventDefault(); c.classList.add('is-drop'); } });
      c.addEventListener('dragleave', () => c.classList.remove('is-drop'));
      c.addEventListener('drop', (e) => {
        e.preventDefault();
        const from = looks.findIndex(l => l.id === dragId), to = looks.findIndex(l => l.id === look.id);
        if (from >= 0 && to >= 0 && from !== to) { const [m] = looks.splice(from, 1); looks.splice(to, 0, m); persist(); }
        dragId = null; render();
      });
      return c;
    }

    // ── The "..." menu and the screen theme picker ──
    function closeMenu() { if (menu) { menu.remove(); menu = null; } }
    function placeMenu(anchor) {
      const content = modal.querySelector('.modal-content');
      const cr = content.getBoundingClientRect(), ar = anchor.getBoundingClientRect();
      menu.style.top = Math.min(cr.height - menu.offsetHeight - 8, ar.bottom - cr.top + 4) + 'px';
      menu.style.left = Math.max(8, Math.min(cr.width - menu.offsetWidth - 8, ar.right - cr.left - menu.offsetWidth)) + 'px';
    }
    function openMenu(look, anchor) {
      closeMenu();
      menu = el('div', 'vd-lk-menu'); menu.setAttribute('role', 'menu');
      const item = (label, fn, cls) => {
        const b = el('button', 'vd-lk-mi' + (cls ? ' ' + cls : ''), label); b.type = 'button'; b.setAttribute('role', 'menuitem');
        b.addEventListener('click', (e) => { e.stopPropagation(); fn(); });
        menu.appendChild(b);
      };
      item('Rename', () => { closeMenu(); renaming = look.id; render(); const i = modal.querySelector('.vd-card-input'); if (i) { i.focus(); i.select(); } });
      item('Update from current', () => { closeMenu(); updateFromCurrent(look.id); });
      item('Duplicate', () => { closeMenu(); duplicate(look.id); });
      if (ctx.screenThemes && ctx.screenThemes().length) item('Screen theme', () => openThemes(look, anchor));
      if (ctx.customizeTheme) {
        const has = !!themeOf(look);
        item(has ? 'Customize in Theme Studio' : 'Create a screen theme…', () => {
          closeMenu();
          const id = ctx.customizeTheme(look);
          if (!id) return;
          look.themeId = id; persist(); ctx.onChange();
          close();   // the dialog reopens with the new picture when you come back
        });
        menu.lastChild.title = 'Built-in themes are copied first, so the original stays as it is.';
      }
      menu.appendChild(el('div', 'vd-lk-sep'));
      item('Delete', () => { closeMenu(); remove(look.id); }, 'is-danger');
      modal.querySelector('.modal-content').appendChild(menu);
      placeMenu(anchor);
      menu.querySelector('button').focus();
    }
    function openThemes(look, anchor) {
      closeMenu();
      menu = el('div', 'vd-lk-menu vd-lk-themes'); menu.setAttribute('role', 'listbox'); menu.setAttribute('aria-label', 'Screen theme');
      const choose = (id) => {
        look.themeId = id; closeMenu(); persist(); render(); ctx.onChange();
        if (ctx.previewLookId() === look.id) ctx.loadToPreview(look);   // Preview follows the change
      };
      const opt = (id, label, theme) => {
        const b = el('button', 'vd-lk-opt' + ((look.themeId || null) === id ? ' is-on' : '')); b.type = 'button'; b.setAttribute('role', 'option');
        b.setAttribute('aria-selected', String((look.themeId || null) === id));
        const pic = el('div', 'vd-lk-thumb vd-lk-opt-pic');
        if (theme && ctx.renderThumb) { const inner = el('div', 'vd-lk-th-in'); try { ctx.renderThumb(inner, theme); } catch {} pic.appendChild(inner); } else pic.classList.add('is-plain');
        b.append(pic, el('span', null, label));
        b.addEventListener('click', (e) => { e.stopPropagation(); choose(id); });
        menu.appendChild(b);
      };
      opt(null, 'None', null);
      ctx.screenThemes().forEach(t => opt(t.id, t.name.replace(/^Screen\s*[—-]\s*/, ''), t));
      const cur = themeOf(look);
      if (cur && ctx.editTheme) {
        const e = el('button', 'vd-lk-mi', 'Edit in Theme Studio'); e.type = 'button';
        e.addEventListener('click', (ev) => { ev.stopPropagation(); close(); ctx.editTheme(cur.id); });
        menu.appendChild(e);
      }
      modal.querySelector('.modal-content').appendChild(menu);
      placeMenu(anchor);
      fitMenuThumbs();
      (menu.querySelector('.is-on') || menu.querySelector('button')).focus();
    }
    function fitMenuThumbs() {
      if (!menu) return;
      menu.querySelectorAll('.vd-lk-thumb').forEach(b => {
        const inner = b.querySelector('.vd-lk-th-in');
        if (inner) inner.style.transform = `scale(${(b.clientWidth || THUMB_PX) / THUMB_PX})`;
      });
    }

    function render() {
      if (!modal) return;
      closeMenu();
      const grid = modal.querySelector('.vd-cards');
      grid.replaceChildren();
      looks.forEach((l, i) => grid.appendChild(tile(l, i)));
      const add = el('button', 'vd-lk vd-lk-new'); add.type = 'button';
      add.innerHTML = `<span class="vd-lk-new-pic"><span aria-hidden="true">+</span></span><span class="vd-lk-name">New ${LABEL.one} from current</span>`;
      add.addEventListener('click', saveCurrent);
      grid.appendChild(add);
      fitThumbs();
    }

    function focusables() { return [...modal.querySelectorAll('button, input, [tabindex="0"]')].filter(e => !e.disabled && e.offsetParent !== null); }
    function open() {
      if (modal) return;
      prevFocus = document.activeElement;
      modal = el('div', 'modal vd-looks-modal');
      modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true'); modal.setAttribute('aria-label', LABEL.many);
      const overlay = el('div', 'modal-overlay');
      overlay.addEventListener('mousedown', close);
      const content = el('div', 'modal-content vd-lk-content');
      const head = el('div', 'modal-header');
      const title = el('div', 'vd-lk-title'); title.append(el('h2', null, LABEL.many), el('span', 'vd-lk-sub', 'Loads into Preview. Nothing goes out until you send it. Layers apply to the display screens; NDI, Syphon and DeckLink keep their own.'));
      const x = el('button', 'modal-close-btn'); x.type = 'button'; x.textContent = 'Close';
      x.addEventListener('click', close);
      head.append(title, x);
      const body = el('div', 'modal-body vd-lk-body');
      body.appendChild(el('div', 'vd-cards'));
      const toast = el('div', 'vd-lk-toast'); toast.hidden = true; toast.setAttribute('role', 'status');
      content.append(head, body, toast);
      modal.append(overlay, content);
      document.body.appendChild(modal);
      keyHandler = (e) => {
        if (!modal) return;
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (menu) closeMenu(); else close(); return; }
        if (e.key !== 'Tab') return;
        const f = focusables(); if (!f.length) return;
        const first = f[0], last = f[f.length - 1];
        if (e.shiftKey && (document.activeElement === first || !modal.contains(document.activeElement))) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && (document.activeElement === last || !modal.contains(document.activeElement))) { e.preventDefault(); first.focus(); }
      };
      document.addEventListener('keydown', keyHandler, true);
      body.addEventListener('mousedown', (e) => { if (menu && !menu.contains(e.target) && !e.target.closest('.vd-lk-more')) closeMenu(); });
      window.addEventListener('resize', fitThumbs);
      render();
      paintToast();
      ctx.onToggle && ctx.onToggle(true);
      const f = focusables(); (f[0] || x).focus();
      // Overlay names for the info line arrive a moment later.
      if (window.KairoOverlays && window.KairoOverlays.load) {
        window.KairoOverlays.load().then(items => { overlayNames = Object.fromEntries((items || []).map(i => [i.id, i.name || i.label || ''])); if (modal) render(); }).catch(() => {});
      }
    }
    function close() {
      if (!modal) return;
      document.removeEventListener('keydown', keyHandler, true); keyHandler = null;
      window.removeEventListener('resize', fitThumbs);
      closeMenu();
      modal.remove(); modal = null; renaming = null;
      ctx.onToggle && ctx.onToggle(false);
      if (prevFocus && prevFocus.focus && prevFocus.isConnected) prevFocus.focus();
    }

    return {
      open, close, render,
      all: () => looks,
      byId,
      loadIndex(n) { const l = looks[n]; if (l) ctx.loadToPreview(l); return !!l; },
      isOpen: () => !!modal,
      destroy() { close(); clearTimeout(undoTimer); },
    };
  }

  const API = { create, normalize, LABEL };
  if (typeof window !== 'undefined') window.KairoVideoLooks = API;
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
})();
