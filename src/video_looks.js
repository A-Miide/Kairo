// KAIRO — Looks: saved Preview setups for Video director mode.
//
// A Look is a recipe for what the output shows, made from things the app
// already has: the video input (or none), which layers are on (bible, slide,
// media, timer, video) and which overlays are up. Loading a Look only changes
// what Preview shows; nothing goes out until Take. Looks persist in
// settings.directorLooks. They are small chips directly under Preview:
// click loads one into Preview, Shift-click loads and takes it, Alt+1-9 loads
// the nth, right-click offers Rename, Update from current, Duplicate and Delete,
// and chips drag to reorder. Two starters are made the first time.
'use strict';
(function () {
  const LAYER_KEYS = ['bible', 'slide', 'media', 'timer', 'video'];
  const STARTERS = [
    { name: 'Scripture over video', videoId: '@first', layers: { bible: true, slide: true, media: true, timer: true, video: true }, overlays: null },
    { name: 'Video only', videoId: '@first', layers: { bible: false, slide: false, media: false, timer: false, video: true }, overlays: null },
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
        name: String(r.name || 'Look').replace(/\s+/g, ' ').trim().slice(0, 40) || 'Look',
        videoId: r.videoId === undefined ? null : (typeof r.videoId === 'string' ? r.videoId : null),
        layers,
        overlays: Array.isArray(r.overlays) ? r.overlays.filter(x => typeof x === 'string') : null,
      });
    }
    return out;
  }

  // ctx: { loadToPreview(look), takeNow(), capture() → {videoId, layers, overlays},
  //        previewLookId(), liveLookId(), onChange() }
  function create(ctx) {
    let looks = normalize(settings.directorLooks);
    if (!looks) {
      looks = normalize(STARTERS);
      persist();
    }
    const el = document.createElement('div');
    el.className = 'vd-looks';
    el.setAttribute('role', 'group');
    el.setAttribute('aria-label', 'Looks');
    let menu = null, renaming = null, dragId = null;

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

    function closeMenu() {
      if (menu) { menu.remove(); menu = null; document.removeEventListener('mousedown', outside, true); }
    }
    function outside(e) { if (menu && !menu.contains(e.target)) closeMenu(); }

    async function saveCurrent() {
      const cap = await ctx.capture();
      const look = { id: uid(), name: uniqueName('Look ' + (looks.length + 1)), videoId: cap.videoId, layers: cap.layers, overlays: cap.overlays };
      looks.push(look);
      persist(); render();
      startRename(look.id);
    }
    async function updateFromCurrent(id) {
      const l = byId(id); if (!l) return;
      const cap = await ctx.capture();
      l.videoId = cap.videoId; l.layers = cap.layers; l.overlays = cap.overlays;
      persist(); render();
    }
    function duplicate(id) {
      const l = byId(id); if (!l) return;
      const copy = { ...l, id: uid(), layers: { ...l.layers }, overlays: l.overlays ? [...l.overlays] : null, name: uniqueName(l.name + ' copy') };
      looks.splice(looks.indexOf(l) + 1, 0, copy);
      persist(); render();
    }
    function remove(id) {
      looks = looks.filter(l => l.id !== id);
      persist(); render(); ctx.onChange();
    }
    function startRename(id) { renaming = id; render(); const i = el.querySelector('.vd-look-input'); if (i) { i.focus(); i.select(); } }
    function commitRename(id, value) {
      const l = byId(id);
      renaming = null;
      if (l) { const v = String(value || '').replace(/\s+/g, ' ').trim().slice(0, 40); if (v) l.name = uniqueName(v, id); persist(); }
      render();
    }

    function openMenu(e, look) {
      e.preventDefault();
      closeMenu();
      menu = document.createElement('div');
      menu.className = 'vd-menu';
      [['Rename', () => startRename(look.id)], ['Update from current', () => updateFromCurrent(look.id)], ['Duplicate', () => duplicate(look.id)], ['Delete', () => remove(look.id)]].forEach(([label, fn]) => {
        const b = document.createElement('button');
        b.type = 'button'; b.className = 'vd-menu-item' + (label === 'Delete' ? ' is-danger' : ''); b.textContent = label;
        b.addEventListener('click', () => { closeMenu(); fn(); });
        menu.appendChild(b);
      });
      document.body.appendChild(menu);
      menu.style.left = Math.min(window.innerWidth - 190, e.clientX) + 'px';
      menu.style.top = Math.min(window.innerHeight - menu.offsetHeight - 8, e.clientY) + 'px';
      document.addEventListener('mousedown', outside, true);
    }

    function chip(look, index) {
      if (renaming === look.id) {
        const inp = document.createElement('input');
        inp.className = 'vd-look-input'; inp.type = 'text'; inp.maxLength = 40; inp.value = look.name;
        inp.setAttribute('aria-label', 'Look name');
        inp.addEventListener('keydown', (ev) => { if (ev.key === 'Enter') { ev.preventDefault(); commitRename(look.id, inp.value); } else if (ev.key === 'Escape') { ev.preventDefault(); renaming = null; render(); } });
        inp.addEventListener('blur', () => { if (renaming === look.id) commitRename(look.id, inp.value); });
        return inp;
      }
      const b = document.createElement('button');
      b.type = 'button';
      const isPvw = ctx.previewLookId() === look.id, isLive = ctx.liveLookId() === look.id;
      b.className = 'vd-look' + (isPvw ? ' is-pvw' : '') + (isLive ? ' is-live' : '');
      b.dataset.id = look.id;
      b.title = `${look.name}. Click: load into Preview. Shift-click: load and take.${index < 9 ? ` Alt+${index + 1}.` : ''}`;
      b.setAttribute('aria-pressed', String(isPvw));
      if (isLive) { const dot = document.createElement('span'); dot.className = 'vd-look-dot'; dot.setAttribute('aria-label', 'On air'); b.appendChild(dot); }
      b.appendChild(document.createTextNode(look.name));
      b.addEventListener('click', (ev) => { ctx.loadToPreview(look); if (ev.shiftKey) ctx.takeNow(); });
      b.addEventListener('contextmenu', (ev) => openMenu(ev, look));
      // Drag to reorder. Draggable only while pressed.
      b.draggable = false;
      b.addEventListener('mousedown', () => { b.draggable = true; });
      b.addEventListener('mouseup', () => { b.draggable = false; });
      b.addEventListener('dragstart', (ev) => { dragId = look.id; ev.dataTransfer.setData('application/x-kairo-look', look.id); ev.dataTransfer.effectAllowed = 'move'; });
      b.addEventListener('dragend', () => { dragId = null; b.draggable = false; render(); });
      b.addEventListener('dragover', (ev) => { if (dragId && dragId !== look.id) { ev.preventDefault(); b.classList.add('is-drop'); } });
      b.addEventListener('dragleave', () => b.classList.remove('is-drop'));
      b.addEventListener('drop', (ev) => {
        ev.preventDefault();
        const from = looks.findIndex(l => l.id === dragId), to = looks.findIndex(l => l.id === look.id);
        if (from >= 0 && to >= 0 && from !== to) { const [m] = looks.splice(from, 1); looks.splice(to, 0, m); persist(); }
        dragId = null; render();
      });
      return b;
    }

    function render() {
      el.replaceChildren();
      looks.forEach((l, i) => el.appendChild(chip(l, i)));
      const add = document.createElement('button');
      add.type = 'button'; add.className = 'vd-look vd-look-add';
      add.textContent = '+ Save current as Look';
      add.title = 'Save what the output shows now (video, layers, overlays) as a Look';
      add.addEventListener('click', saveCurrent);
      el.appendChild(add);
    }
    render();

    return {
      el, render,
      all: () => looks,
      byId,
      loadIndex(n) { const l = looks[n]; if (l) ctx.loadToPreview(l); return !!l; },
      destroy() { closeMenu(); el.remove(); },
    };
  }

  window.KairoVideoLooks = { create, normalize };
})();
