// KAIRO — Looks: saved Preview setups for Video director mode.
//
// A Look is a recipe for what the output shows, made from things the app
// already has: the video input (or none), which layers are on (bible, slide,
// media, timer, video) and which overlays are up. Loading a Look only changes
// what Preview shows; nothing goes out until Take. Looks persist in
// settings.directorLooks. They live in a dialog opened from the Displays header
// (so they never take room in the layout): each Look is a card with a small
// summary; click loads it into Preview and closes the dialog (Shift-click keeps
// it open), and a card can be renamed, updated from what the output shows now,
// duplicated, deleted and dragged to reorder. Alt+1-9 load the nth Look without
// opening the dialog. Two starters are made the first time.
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

  const LAYER_LABELS = { bible: 'Bible', slide: 'Slide', media: 'Media', timer: 'Timer', video: 'Video' };

  // ctx: { loadToPreview(look), capture() → Promise<{videoId, layers, overlays}>,
  //        previewLookId(), liveLookId(), videoName(look), onChange() }
  function create(ctx) {
    let looks = normalize(settings.directorLooks);
    if (!looks) { looks = normalize(STARTERS); persist(); }
    let modal = null, prevFocus = null, renaming = null, dragId = null, keyHandler = null;

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

    async function saveCurrent() {
      const cap = await ctx.capture();
      const look = { id: uid(), name: uniqueName('Look ' + (looks.length + 1)), videoId: cap.videoId, layers: cap.layers, overlays: cap.overlays };
      looks.push(look);
      persist(); renaming = look.id; render();
      const i = modal && modal.querySelector('.vd-card-input'); if (i) { i.focus(); i.select(); }
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
    function remove(id) { looks = looks.filter(l => l.id !== id); persist(); render(); ctx.onChange(); }
    function commitRename(id, value) {
      const l = byId(id);
      renaming = null;
      if (l) { const v = String(value || '').replace(/\s+/g, ' ').trim().slice(0, 40); if (v) l.name = uniqueName(v, id); persist(); }
      render(); ctx.onChange();
    }

    function card(look, index) {
      const isPvw = ctx.previewLookId() === look.id, isLive = ctx.liveLookId() === look.id;
      const c = el('div', 'vd-card' + (isPvw ? ' is-pvw' : '') + (isLive ? ' is-live' : ''));
      c.dataset.id = look.id;
      c.tabIndex = 0;
      c.setAttribute('role', 'button');
      c.setAttribute('aria-label', `${look.name}. Load into Preview`);
      const top = el('div', 'vd-card-top');
      if (renaming === look.id) {
        const inp = el('input', 'vd-card-input'); inp.type = 'text'; inp.maxLength = 40; inp.value = look.name; inp.setAttribute('aria-label', 'Look name');
        inp.addEventListener('click', (e) => e.stopPropagation());
        inp.addEventListener('keydown', (e) => { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); commitRename(look.id, inp.value); } else if (e.key === 'Escape') { e.preventDefault(); renaming = null; render(); } });
        inp.addEventListener('blur', () => { if (renaming === look.id) commitRename(look.id, inp.value); });
        top.appendChild(inp);
      } else {
        if (isLive) top.appendChild(el('span', 'vd-look-dot'));
        top.appendChild(el('span', 'vd-card-name', look.name));
        if (index < 9) top.appendChild(el('span', 'vd-card-key', `Alt+${index + 1}`));
      }
      c.appendChild(top);
      const sum = el('div', 'vd-card-sum');
      sum.appendChild(el('div', 'vd-card-line', `Input: ${ctx.videoName(look)}`));
      const lay = el('div', 'vd-card-layers');
      for (const k of LAYER_KEYS) lay.appendChild(el('span', 'vd-layer' + (look.layers[k] ? ' is-on' : ''), LAYER_LABELS[k]));
      sum.appendChild(lay);
      sum.appendChild(el('div', 'vd-card-line', look.overlays == null ? 'Overlays: unchanged' : look.overlays.length ? `Overlays: ${look.overlays.length} on` : 'Overlays: none'));
      c.appendChild(sum);
      const acts = el('div', 'vd-card-acts');
      [['Rename', () => { renaming = look.id; render(); const i = modal.querySelector('.vd-card-input'); if (i) { i.focus(); i.select(); } }], ['Update', () => updateFromCurrent(look.id)], ['Duplicate', () => duplicate(look.id)], ['Delete', () => remove(look.id)]].forEach(([label, fn]) => {
        const b = el('button', 'vd-mini' + (label === 'Delete' ? ' vd-danger' : ''), label);
        b.type = 'button';
        b.setAttribute('aria-label', `${label} ${look.name}`);
        b.addEventListener('click', (e) => { e.stopPropagation(); fn(); });
        acts.appendChild(b);
      });
      c.appendChild(acts);
      const load = (e) => { ctx.loadToPreview(look); if (!e.shiftKey) close(); };
      c.addEventListener('click', load);
      c.addEventListener('keydown', (e) => { if ((e.key === 'Enter' || e.key === ' ') && e.target === c) { e.preventDefault(); load(e); } });
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

    function render() {
      if (!modal) return;
      const grid = modal.querySelector('.vd-cards');
      grid.replaceChildren();
      looks.forEach((l, i) => grid.appendChild(card(l, i)));
      if (!looks.length) grid.appendChild(el('p', 'setting-hint', 'No Looks yet. Save what the output shows now.'));
    }

    function focusables() { return [...modal.querySelectorAll('button, input, [tabindex="0"]')].filter(e => !e.disabled && e.offsetParent !== null); }
    function open() {
      if (modal) return;
      prevFocus = document.activeElement;
      modal = el('div', 'modal vd-looks-modal');
      modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true'); modal.setAttribute('aria-label', 'Looks');
      const overlay = el('div', 'modal-overlay');
      overlay.addEventListener('mousedown', close);
      const content = el('div', 'modal-content');
      content.style.width = '620px';
      const head = el('div', 'modal-header');
      head.appendChild(el('h2', null, 'Looks'));
      const x = el('button', 'modal-close-btn'); x.type = 'button'; x.textContent = 'Close';
      x.addEventListener('click', close);
      head.appendChild(x);
      const body = el('div', 'modal-body');
      body.style.cssText = 'display:block;padding:16px 20px;max-height:60vh;overflow-y:auto;';
      body.appendChild(el('p', 'setting-hint', 'A Look sets up Preview. Nothing goes out until you take it.'));
      body.appendChild(el('div', 'vd-cards'));
      const foot = el('div', 'modal-footer');
      const save = el('button', 'modal-btn primary', 'Save current as Look'); save.type = 'button';
      save.addEventListener('click', saveCurrent);
      const done = el('button', 'modal-btn secondary', 'Done'); done.type = 'button';
      done.addEventListener('click', close);
      foot.append(save, done);
      content.append(head, body, foot);
      modal.append(overlay, content);
      document.body.appendChild(modal);
      keyHandler = (e) => {
        if (!modal) return;
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); return; }
        if (e.key !== 'Tab') return;
        const f = focusables(); if (!f.length) return;
        const first = f[0], last = f[f.length - 1];
        if (e.shiftKey && (document.activeElement === first || !modal.contains(document.activeElement))) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && (document.activeElement === last || !modal.contains(document.activeElement))) { e.preventDefault(); first.focus(); }
      };
      document.addEventListener('keydown', keyHandler, true);
      render();
      const f = focusables(); (f[0] || x).focus();
    }
    function close() {
      if (!modal) return;
      document.removeEventListener('keydown', keyHandler, true); keyHandler = null;
      modal.remove(); modal = null; renaming = null;
      if (prevFocus && prevFocus.focus && prevFocus.isConnected) prevFocus.focus();
    }

    return {
      open, close, render,
      all: () => looks,
      byId,
      loadIndex(n) { const l = looks[n]; if (l) ctx.loadToPreview(l); return !!l; },
      isOpen: () => !!modal,
      destroy() { close(); },
    };
  }

  window.KairoVideoLooks = { create, normalize };
})();
