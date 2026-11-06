// KAIRO — the sermon slides design screen (sermon_slides.js makes the slides; this shows them).
//
// Opened from the import dialog when a document reads as a sermon outline. One screen: the slide
// large, every slide as a thumbnail with what to review, and on the right the choices that apply to
// the whole deck (look, background, footer) and the selected slide's own words. "Add to playlist"
// hands the slides back; they stay editable in the playlist like any slide.
(function () {
  'use strict';
  const S = () => window.KairoSermonSlides;
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

  // One slide drawn into a 16:9 box from its layers (pp_render.js draws each text box and shape).
  function drawSlide(host, block) {
    host.replaceChildren();
    const kit = S().kitById(block.gen && block.gen.kit);
    host.style.background = kit.bg;
    for (const layer of block.layers || []) {
      if (layer.type === 'pp' && window.KairoPP) host.appendChild(window.KairoPP.render(layer, {}, { small: true }));
    }
  }
  // How far the headline had to shrink: under the readable size, the slide is flagged.
  function headlinePx(host, block) {
    const layer = (block.layers || []).find(l => l.name === 'Headline');
    if (!layer) return Infinity;
    const holders = host.querySelectorAll('.pp-text');
    const i = (block.layers || []).filter(l => l.type === 'pp' && l.text).indexOf(layer);
    const fit = Number(holders[i]?.dataset.fit || 1);
    const size = Math.max(...(layer.text.paragraphs || []).flatMap(p => (p.runs || []).map(r => r.size || 0)), layer.text.font.size || 0);
    return size * fit;
  }

  function openDesigner(outline, { title = 'Sermon slides', onAdd, onCancel } = {}) {
    const api = S();
    let prefs = {};
    try { prefs = JSON.parse(localStorage.getItem('kairo-sermon-slides') || '{}'); } catch { prefs = {}; }
    const state = {
      kit: api.KITS.some(k => k.id === prefs.kit) ? prefs.kit : api.KITS[0].id,
      style: api.BACKGROUNDS.some(b => b.id === prefs.style) ? prefs.style : 'glow',
      footer: null,
      selected: 0,
      tooSmall: new Set(),
    };
    // What goes in: every part of the document, the operator's choice (on by default: what makes a slide).
    const parts = api.partsOf(outline);
    const include = new Set(parts.filter(p => p.on).map(p => p.id));
    // A slide's own background, by part, so it survives the deck being made again.
    const edits = {};
    let blocks = api.build(outline, { kit: state.kit, style: state.style, include });
    state.footer = blocks[0]?.gen.footer || '';
    function rebuild() {
      const keep = blocks[state.selected]?.gen.part;
      blocks = api.build(outline, { kit: state.kit, style: state.style, footer: state.footer, include }).map((b) => {
        const e = edits[b.gen.part];
        return e && e.style ? api.restyle(b, { style: e.style, own: true }) : b;
      });
      const at = blocks.findIndex(b => b.gen.part === keep);
      state.selected = Math.max(0, Math.min(blocks.length - 1, at < 0 ? state.selected : at));
      state.tooSmall.clear();
    }
    const remember = () => { try { localStorage.setItem('kairo-sermon-slides', JSON.stringify({ kit: state.kit, style: state.style })); } catch { /* not kept */ } };

    // ── the screen ──
    const overlay = el('div', 'ssd-overlay');
    overlay.setAttribute('role', 'dialog');
    overlay.setAttribute('aria-modal', 'true');
    overlay.setAttribute('aria-label', 'Sermon slides');
    const box = el('div', 'ssd');
    const head = el('div', 'ssd-head');
    const titleEl = el('div', 'ssd-title');
    titleEl.append(el('b', null, 'Sermon slides'), el('span', null, title));
    const close = el('button', 'modal-close-btn');
    close.type = 'button';
    close.setAttribute('aria-label', 'Close');
    close.innerHTML = '<svg width="16" height="16" viewBox="0 0 16 16"><path d="M3 3l10 10M13 3L3 13" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg><span>Close</span>';
    head.append(titleEl, close);

    const main = el('div', 'ssd-main');
    // The document's parts: tick what goes on a slide. Nothing the reader set aside is hidden.
    const contents = el('div', 'ssd-contents');
    const contentsHead = el('div', 'ssd-panel-title', 'Contents');
    const contentsHint = el('p', 'ssd-hint', 'Everything in the document. Ticked parts become slides.');
    const list = el('ul', 'ssd-parts');
    const TAG = { title: 'Title', story: 'Introduction', section: 'Section', closing: 'Closing', note: 'Note', line: 'Line' };
    for (const p of parts) {
      const li = el('li', 'ssd-part');
      li.dataset.part = p.id;
      const box = el('input');
      box.type = 'checkbox';
      box.checked = include.has(p.id);
      box.id = `ssd-part-${p.id}`;
      const lab = el('label');
      lab.htmlFor = box.id;
      const tag = p.kind === 'point' ? `Point ${p.n}` : p.kind === 'verse' ? (api.normaliseRef(p.refs[0]) || p.refs[0] || 'Verse') : TAG[p.kind] || p.kind;
      lab.append(el('span', 'ssd-part-tag', tag), el('span', 'ssd-part-text', p.text));
      box.addEventListener('change', () => {
        if (box.checked) include.add(p.id); else include.delete(p.id);
        li.classList.toggle('is-off', !box.checked);
        rebuild();
        const at = blocks.findIndex(b => b.gen.part === p.id);
        if (box.checked && at >= 0) state.selected = at;
        refresh(true);
      });
      li.classList.toggle('is-off', !box.checked);
      // The words select that part's slide.
      lab.addEventListener('click', (e) => {
        const at = blocks.findIndex(b => b.gen.part === p.id);
        if (at >= 0 && e.target !== box) { e.preventDefault(); state.selected = at; refresh(false); }
      });
      li.append(box, lab);
      list.append(li);
    }
    contents.append(contentsHead, contentsHint, list);
    const stageWrap = el('div', 'ssd-stage-wrap');
    const stage = el('div', 'ssd-stage');
    const grid = el('div', 'ssd-grid');
    stageWrap.append(stage, grid);

    const side = el('div', 'ssd-side');
    // The deck's choices, grouped in one panel (owner: every input grouped).
    const deckPanel = el('div', 'ssd-panel');
    deckPanel.append(el('div', 'ssd-panel-title', 'Whole deck'));
    const lookLabel = el('div', 'ssd-label', 'Look');
    const looks = el('div', 'ssd-looks');
    for (const k of api.KITS) {
      const b = el('button', 'ssd-look');
      b.type = 'button';
      b.dataset.kit = k.id;
      b.title = k.name;
      const sw = el('span', 'ssd-swatch');
      sw.style.background = `radial-gradient(circle at 15% 10%, ${k.bg2} 0%, ${k.bg} 70%)`;
      const aa = el('span', 'ssd-swatch-aa', 'Aa');
      aa.style.color = k.text;
      aa.style.fontFamily = `"${k.display.family}", serif`;
      const dot = el('span', 'ssd-swatch-dot');
      dot.style.background = k.accent;
      sw.append(aa, dot);
      b.append(sw, el('span', 'ssd-look-name', k.name));
      b.addEventListener('click', () => { state.kit = k.id; remember(); restyleAll(); });
      looks.append(b);
    }
    const bgLabel = el('div', 'ssd-label', 'Background');
    const bgSeg = el('div', 'ssd-seg');
    for (const bg of api.BACKGROUNDS) {
      const b = el('button', null, bg.name);
      b.type = 'button';
      b.dataset.style = bg.id;
      b.addEventListener('click', () => { state.style = bg.id; remember(); restyleAll(); });
      bgSeg.append(b);
    }
    const footLabel = el('label', 'ssd-label', 'Footer');
    const footer = el('input', 'setting-input');
    footer.id = 'ssd-footer';
    footLabel.htmlFor = 'ssd-footer';
    footer.value = state.footer;
    footer.placeholder = 'Your church name';
    footer.addEventListener('change', () => { state.footer = footer.value.trim(); restyleAll(); });
    deckPanel.append(lookLabel, looks, bgLabel, bgSeg, footLabel, footer);

    // The selected slide: its own background, what to review, and its whole sentence when cut.
    const slidePanel = el('div', 'ssd-panel');
    const slideTitle = el('div', 'ssd-panel-title', 'This slide');
    const ownLabel = el('div', 'ssd-label', 'Background');
    const ownSeg = el('div', 'ssd-seg ssd-seg-4');
    for (const opt of [{ id: '', name: 'As deck' }, ...api.BACKGROUNDS]) {
      const b = el('button', null, opt.name);
      b.type = 'button';
      b.dataset.style = opt.id;
      b.addEventListener('click', () => {
        const i = state.selected;
        blocks[i] = opt.id ? api.restyle(blocks[i], { style: opt.id, own: true }) : api.restyle(blocks[i], { style: state.style, own: false });
        const e = (edits[blocks[i].gen.part] = edits[blocks[i].gen.part] || {});
        if (opt.id) e.style = opt.id; else delete e.style;
        refresh(false, true);
      });
      ownSeg.append(b);
    }
    const editHint = el('p', 'ssd-hint', 'Words are changed in the slide editor, which opens when the deck is added: double-click them on the slide.');
    const notes = el('div', 'ssd-notes');
    const checks = el('ul', 'ssd-checks');
    slidePanel.append(slideTitle, ownLabel, ownSeg, editHint, checks, notes);
    side.append(deckPanel, slidePanel);
    main.append(contents, stageWrap, side);

    const foot = el('div', 'ssd-foot');
    const summary = el('span', 'ssd-summary');
    const cancel = el('button', 'modal-btn secondary', 'Cancel');
    cancel.type = 'button';
    const add = el('button', 'modal-btn primary', 'Add to playlist');
    add.type = 'button';
    foot.append(summary, cancel, add);
    box.append(head, main, foot);
    overlay.append(box);
    document.body.append(overlay);

    // ── behaviour ──
    function issuesOf(i) {
      const b = blocks[i];
      const out = api.issues(b.gen, api.kitById(b.gen.kit));
      if (state.tooSmall.has(i)) out.push('Too long to fit at a readable size: shorten the words.');
      return out;
    }
    function restyleAll() {
      rebuild();
      refresh(true);
    }
    // After fonts have arrived, how big each headline really came out.
    function measure(host, i) {
      setTimeout(() => {
        (document.fonts ? document.fonts.ready : Promise.resolve()).then(() => setTimeout(() => {
          const px = headlinePx(host, blocks[i]);
          const small = px < api.MIN_HEADLINE_PX * 0.999;
          if (small !== state.tooSmall.has(i)) { if (small) state.tooSmall.add(i); else state.tooSmall.delete(i); renderSide(); markThumbs(); }
        }, 120));
      }, 60);
    }
    function markParts() {
      const current = blocks[state.selected]?.gen.part;
      list.querySelectorAll('.ssd-part').forEach(li => li.classList.toggle('is-current', li.dataset.part === current));
    }
    function markThumbs() {
      markParts();
      [...grid.children].forEach((t, i) => {
        const n = issuesOf(i).length;
        t.classList.toggle('is-on', i === state.selected);
        t.querySelector('.ssd-badge').hidden = !n;
        t.querySelector('.ssd-badge').title = issuesOf(i).join('\n');
      });
      const review = blocks.filter((_, i) => issuesOf(i).length).length;
      summary.textContent = `${blocks.length} slides${review ? ` · ${review} to review` : ' · nothing to review'}`;
    }
    function renderSide() {
      const b = blocks[state.selected];
      ownSeg.querySelectorAll('button').forEach(x => x.classList.toggle('is-on', b.gen.ownStyle ? x.dataset.style === b.gen.style : x.dataset.style === ''));
      slideTitle.textContent = `This slide · ${state.selected + 1} of ${blocks.length}`;
      checks.replaceChildren(...issuesOf(state.selected).map(t => el('li', null, t)));
      checks.hidden = !checks.children.length;
      const full = b.notes && b.gen.kind === 'point' && b.gen.shortened ? b.notes : '';
      notes.replaceChildren(...(full ? [el('b', null, 'The whole sentence'), el('p', null, full)] : []));
      notes.hidden = !full;
    }
    // all: every thumbnail again (the deck changed); thumb: only the selected slide's thumbnail.
    function refresh(all, thumb) {
      looks.querySelectorAll('.ssd-look').forEach(b => b.classList.toggle('is-on', b.dataset.kit === state.kit));
      bgSeg.querySelectorAll('button').forEach(b => b.classList.toggle('is-on', b.dataset.style === state.style));
      drawSlide(stage, blocks[state.selected]);
      measure(stage, state.selected);
      if (thumb && !all) {
        const art = grid.children[state.selected]?.querySelector('.ssd-thumb-art');
        if (art) drawSlide(art, blocks[state.selected]);
      }
      if (all || grid.children.length !== blocks.length) {
        grid.replaceChildren(...blocks.map((b, i) => {
          const t = el('button', 'ssd-thumb');
          t.type = 'button';
          t.setAttribute('aria-label', `Slide ${i + 1}`);
          const art = el('div', 'ssd-thumb-art');
          const n = el('span', 'ssd-thumb-n', String(i + 1));
          const badge = el('span', 'ssd-badge', '!');
          t.append(art, n, badge);
          t.addEventListener('click', () => { state.selected = i; refresh(false); });
          requestAnimationFrame(() => { drawSlide(art, b); measure(art, i); });
          return t;
        }));
      }
      renderSide();
      markThumbs();
    }
    const finish = (fn) => { overlay.remove(); document.removeEventListener('keydown', onKey); fn && fn(); };
    function onKey(e) {
      if (e.key === 'Escape') finish(onCancel);
      if (e.target.closest && e.target.closest('input, textarea, [contenteditable]')) return;
      if (e.key === 'ArrowRight' && state.selected < blocks.length - 1) { state.selected += 1; refresh(false); }
      if (e.key === 'ArrowLeft' && state.selected > 0) { state.selected -= 1; refresh(false); }
    }
    document.addEventListener('keydown', onKey);
    close.addEventListener('click', () => finish(onCancel));
    cancel.addEventListener('click', () => finish(onCancel));
    add.addEventListener('click', () => finish(() => onAdd && onAdd(blocks)));
    refresh(true);
    return { close: () => finish(onCancel) };
  }

  if (window.KairoSermonSlides) window.KairoSermonSlides.openDesigner = openDesigner;
})();
