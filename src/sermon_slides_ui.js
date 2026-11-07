// KAIRO — sermon slides in the slide editor: the deck's own panel (sermon_slides.js makes the slides).
//
// A sermon deck opens in the slide editor like any slides item: its slides in the list on the left,
// the slide in the middle, every part an ordinary layer. Under the right panel's Look tab, this panel
// holds what is the deck's (its look and background preset, and what of the document it includes) and what to review on this slide. A slide's own background is its Background layer, set
// under Style like any slide. A slide edited by hand keeps its styling when the deck changes
// (sermon_slides.js regenerate).
(function () {
  'use strict';
  const S = () => window.KairoSermonSlides;
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
  const TAG = { title: 'Title', story: 'Introduction', section: 'Section', closing: 'Closing', note: 'Note', line: 'Line' };

  // ctx: { item, slideIndex }, onChange(newSlideIndex): the editor draws the deck again.
  function mountPanel(host, ctx, onChange) {
    const api = S();
    const { item } = ctx;
    const deck = item.sermon;
    host.replaceChildren();
    if (!deck) { host.hidden = true; return; }
    host.hidden = false;
    const block = item.blocks[ctx.slideIndex];
    // A look, layout, background or colour change (`restyle`) asks, once per deck, whether slides
    // changed by hand take it too (their words are kept either way); a contents change never restyles.
    const apply = async (keepPart, restyle = true) => {
      const edited = (item.blocks || []).filter(b => b.gen?.edited).length;
      let restyleEdited = false;
      if (restyle && edited) {
        if (!restyleAnswers.has(item)) {
          const ask = typeof confirmDialog === 'function' ? confirmDialog : async () => true;
          const yes = await ask(`${edited} slide${edited === 1 ? ' was' : 's were'} changed by hand and ${edited === 1 ? 'keeps its' : 'keep their'} own style. Change ${edited === 1 ? 'it' : 'them'} to the new look too? Your words stay.`,
            { title: 'Slides you changed', confirmLabel: edited === 1 ? 'Change it too' : 'Change them too', cancelLabel: 'Keep my changes' });
          restyleAnswers.set(item, yes);
        }
        restyleEdited = restyleAnswers.get(item);
      }
      item.blocks = api.regenerate(item, { restyleEdited });
      const at = keepPart ? item.blocks.findIndex(b => b.gen?.part === keepPart) : -1;
      onChange(at >= 0 ? at : Math.min(ctx.slideIndex, item.blocks.length - 1));
    };

    // The editor's own section shape (ts-props-section, its small grey label), no helper sentences.
    const section = (label, ...children) => {
      const sec = el('div', 'ts-props-section ssp-sec');
      sec.append(el('div', 'ts-props-section-label', label), ...children);
      return sec;
    };

    // ── the deck's style: one button and pop-out (styleButton, below); Apply changes the deck. ──
    const styleBtn = styleButton({ outline: deck.outline, kit: deck.kit, colors: deck.colors, layout: deck.layout, style: deck.style, part: block?.gen?.part },
      (c) => {
        if (c.kit) { deck.kit = c.kit; delete deck.colors; }   // a look brings its own colours
        if (c.layout) {
          deck.layout = c.layout;
          // A position applied to every slide belonged to the old layout.
          for (const props of Object.values(deck.common || {})) { delete props.pos; delete props.align; }
        }
        if (c.style) deck.style = c.style;
        remember(deck);
        apply(block?.gen?.part);
      });
    const deckSec = section('Deck look', styleBtn);

    // ── the deck's colours: the look's, or the operator's own ──
    const kit = api.withColors(api.kitById(deck.kit), deck.colors);
    const colorSec = section('Colours', ...colorRows(kit, (key, hex) => {
      deck.colors = { ...(deck.colors || {}), [key]: hex };
      remember(deck);
      clearTimeout(colorTimer);
      colorTimer = setTimeout(() => apply(block?.gen?.part), 250);
    }), ...api.kitWarnings(kit).slice(0, 1).map(w => el('p', 'ssp-flag', w)));

    // ── this slide: only what to review (its background is the Background layer, set under Style) ──
    const review = [];
    if (block?.gen?.shortened && block.notes) {
      // Cut from a longer sentence: the whole of it, one click away.
      const d = el('details', 'ssp-cut');
      d.append(el('summary', null, 'Shortened · show the whole sentence'), el('p', null, block.notes));
      review.push(d);
    }
    if (block?.gen?.tooLong) review.push(el('p', 'ssp-flag', 'Too long to fit at a readable size'));
    // What this slide's line is: a point, a section line or an introduction. The reader's guess, changed in one click.
    const part = api.partsOf(deck.outline).find(x => x.id === block?.gen?.part);
    if (part && !['title', 'verse', 'note'].includes(part.kind)) {
      const now = (deck.kinds || {})[part.id] || (part.kind === 'closing' ? 'section' : part.kind);
      const kinds = el('div', 'ssp-switch ssp-kinds');
      kinds.setAttribute('role', 'group');
      kinds.setAttribute('aria-label', 'Treat this line as');
      for (const [k, label] of [['point', 'Point'], ['section', 'Section'], ['story', 'Introduction']]) {
        const b = el('button', 'ssp-switch-btn' + (now === k ? ' is-on' : ''), label);
        b.type = 'button';
        b.setAttribute('aria-pressed', String(now === k));
        b.addEventListener('click', () => { if (now === k) return; deck.kinds = { ...(deck.kinds || {}), [part.id]: k }; apply(part.id, false); });
        kinds.append(b);
      }
      review.push(kinds);
    }
    const slideSec = review.length ? section('This slide', ...review) : null;

    // ── contents: what of the document is in the deck ──
    const list = el('ul', 'ssp-parts');
    const include = new Set(deck.include);
    let pointNo = 0;
    for (const base of api.partsOf(deck.outline)) {
      // As the operator labelled it, with the points counted again.
      const p = deck.kinds && deck.kinds[base.id] ? { ...base, kind: deck.kinds[base.id] } : base;
      if (p.kind === 'point') p.n = ++pointNo;
      const li = el('li', 'ssp-part' + (include.has(p.id) ? '' : ' is-off') + (block?.gen?.part === p.id ? ' is-current' : ''));
      const box = el('input');
      box.type = 'checkbox';
      box.checked = include.has(p.id);
      box.id = `ssp-part-${p.id}`;
      const lab = el('label');
      lab.htmlFor = box.id;
      const tag = p.kind === 'point' ? `Point ${p.n}` : p.kind === 'verse' ? (api.normaliseRef(p.refs[0]) || p.refs[0] || 'Verse') : TAG[p.kind] || p.kind;
      lab.append(el('span', 'ssp-part-tag', tag), el('span', 'ssp-part-text', p.text));
      box.addEventListener('change', () => {
        if (box.checked) include.add(p.id); else include.delete(p.id);
        deck.include = [...include];
        apply(box.checked ? p.id : block?.gen?.part, false);
      });
      // The words open that part's slide.
      lab.addEventListener('click', (e) => {
        const at = item.blocks.findIndex(b => b.gen?.part === p.id);
        if (at >= 0) { e.preventDefault(); onChange(at); }
      });
      li.append(box, lab);
      list.append(li);
    }
    host.append(...[deckSec, colorSec, slideSec, section('In the deck', list)].filter(Boolean));
  }

  // Background, second tone, text and accent: the editor's own colour field (a swatch and its hex).
  let colorTimer = null;
  const COLOR_KEYS = [['bg', 'Background'], ['bg2', 'Second tone'], ['text', 'Text'], ['accent', 'Accent']];
  function colorRows(kit, onPick) {
    const grid = el('div', 'ssp-colors');
    for (const [key, name] of COLOR_KEYS) {
      const row = el('div', 'ssp-color');
      row.append(el('span', 'ssp-color-name', name));
      if (typeof colorField === 'function') row.append(colorField(kit[key], (hex) => onPick(key, hex), name));
      else {
        const inp = el('input');
        inp.type = 'color';
        inp.value = kit[key];
        inp.setAttribute('aria-label', name);
        inp.addEventListener('input', () => onPick(key, inp.value));
        row.append(inp);
      }
      grid.append(row);
    }
    return [grid];
  }

  // ── The Style button and its pop-out, shared by the slide editor and the import step ──
  // state: { outline, kit, colors, layout, style, part }. The button shows a preview of the slide
  // (`part`, else the first point) and "Look · Layout". The pop-out is one panel: a large preview of
  // the slide, a Look | Layout switch and one grid of thumbnails at a time, each drawn with the
  // slide's own words (a look in the draft's layout, a layout in the draft's look). Choices there
  // are a draft: the preview and the grids follow them, and nothing else changes until Apply. Apply closes the pop-out and
  // calls onPick once with what changed ({ kit?, style?, layout? }); closing it any other way keeps
  // what was there. A look brings its own colours and background and keeps the layout; a layout
  // keeps the look. The caller keeps the button and hands it a new state with `btn.update(state)`,
  // or draws a new one.
  function styleButton(state, onPick) {
    const api = S(), KS = window.KairoService;
    const layoutIn = (v) => api.LAYOUTS.find(l => l.id === (v.layout || 'left')) || api.LAYOUTS[0];
    let draft = null;   // the open pop-out's choices: { kit, colors, layout, style }
    const view = () => draft || state;
    const thumbOf = (host, o = {}) => {
      host.replaceChildren();
      const v = view();
      const blocks = api.newDeck(state.outline, { kit: o.kit || v.kit, style: o.style || v.style, colors: 'colors' in o ? o.colors : v.colors, layout: o.layout || layoutIn(v).id }).blocks;
      const b = blocks.find(x => x.gen.part === state.part) || blocks.find(x => x.gen.kind === 'point') || blocks[0];
      if (!b) return;
      // The preview uses the look's own faces (and a layout's script), so they're asked for here.
      if (typeof loadGoogleFont === 'function') for (const f of new Set(b.layers.map(l => l.font && l.font.family).filter(Boolean))) loadGoogleFont(f);
      if (KS?.paintLookLayers) requestAnimationFrame(() => KS.paintLookLayers(host, { layers: b.layers }, {}, {}));
    };
    // A look's background is the one its design uses.
    const styleOf = (kitId) => (api.DESIGNS.find(d => d.kit === kitId) || {}).style || 'glow';
    const nameOf = (v) => `${v.colors ? api.kitById(v.kit).name + ' (own colours)' : api.kitById(v.kit).name}`;
    const btn = el('button', 'ssp-picker ssp-style-btn');
    btn.type = 'button';
    btn.setAttribute('aria-haspopup', 'dialog');
    const nowThumb = el('span', 'ssp-picker-thumb');
    nowThumb.setAttribute('aria-hidden', 'true');
    const names = el('span', 'ssp-picker-name');
    btn.append(nowThumb, names);
    btn.insertAdjacentHTML('beforeend', '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>');
    let panel = null;   // the open pop-out's parts
    const looksOn = (k) => k.id === view().kit && !view().colors;
    // The button, as the deck is.
    const syncButton = () => {
      const saved = draft; draft = null;
      thumbOf(nowThumb);
      draft = saved;
      names.textContent = `${nameOf(state)} · ${layoutIn(state).name}`;
      btn.setAttribute('aria-label', `Style: ${names.textContent.replace(/ · /g, ', ')}`);
    };
    // Looks and layouts are thumbnails of the slide. Each is drawn when it is on screen and what it
    // shows has changed (a look is drawn in the draft's layout; a layout, in the draft's look), so
    // a long list costs only what is seen.
    const lookKey = () => draft.layout;
    const layoutKey = () => [draft.kit, JSON.stringify(draft.colors || ''), draft.style].join('|');
    const paintVisible = () => {
      if (!panel || !panel.body.isConnected) return;
      const box = panel.body.getBoundingClientRect();
      // Only a thumbnail that has a size can be drawn to scale.
      const seen = (th) => { if (!th.offsetWidth) return false; const r = th.getBoundingClientRect(); return r.bottom > box.top - 60 && r.top < box.bottom + 60; };
      if (!panel.lookView.hidden) for (const t of panel.looks) {
        if (t.key === lookKey() || !seen(t.th)) continue;
        t.key = lookKey();
        thumbOf(t.th, { kit: t.k.id, style: styleOf(t.k.id), colors: undefined });
      }
      if (!panel.layoutView.hidden) for (const t of panel.layouts) {
        if (t.key === layoutKey() || !seen(t.th)) continue;
        t.key = layoutKey();
        thumbOf(t.th, { layout: t.l.id });
      }
    };
    // The open pop-out, as the draft is.
    const syncPanel = () => {
      if (!panel || !panel.preview.isConnected) { panel = null; return; }
      const layoutNow = layoutIn(draft);
      thumbOf(panel.preview);
      panel.lookName.textContent = nameOf(draft);
      panel.layoutName.textContent = layoutNow.name;
      for (const { k, tile } of panel.looks) { tile.classList.toggle('is-on', looksOn(k)); tile.setAttribute('aria-selected', String(looksOn(k))); }
      for (const { l, tile } of panel.layouts) { tile.classList.toggle('is-on', l.id === layoutNow.id); tile.setAttribute('aria-selected', String(l.id === layoutNow.id)); }
      // What Apply would change.
      panel.apply.disabled = !changes();
      paintVisible();
    };
    const changes = () => {
      const c = {};
      if (draft.kit !== state.kit || (state.colors && !draft.colors)) c.kit = draft.kit;
      if (layoutIn(draft).id !== layoutIn(state).id) c.layout = layoutIn(draft).id;
      if (draft.style !== state.style) c.style = draft.style;
      return Object.keys(c).length ? c : null;
    };
    const tileOf = (label, th, onClick) => {
      const tile = el('button', 'ssp-pop-item');
      tile.type = 'button';
      tile.setAttribute('role', 'option');
      tile.setAttribute('aria-label', label);
      tile.append(th, el('span', 'ssp-pop-name', label));
      tile.addEventListener('click', onClick);
      return tile;
    };
    const thumbBox = () => { const th = el('span', 'ssp-layout-thumb'); th.setAttribute('aria-hidden', 'true'); return th; };
    const open = () => KS?.openPopover?.(btn, (pop) => {
      draft = { kit: state.kit, colors: state.colors, layout: layoutIn(state).id, style: state.style };
      const wrap = el('div', 'ssp-pop-grid-wrap');
      // One list at a time: the switch under the preview shows what each holds now.
      const switchBtn = (label) => { const v = el('span', 'ssp-pop-value'), b = el('button', 'ssp-switch-btn', label); b.type = 'button'; b.setAttribute('role', 'tab'); b.append(v); return [b, v]; };
      const preview = el('span', 'ssp-layout-thumb ssp-pop-preview');
      preview.setAttribute('aria-hidden', 'true');
      const [lookSwitch, lookName] = switchBtn('Look');
      const [layoutSwitch, layoutName] = switchBtn('Layout');
      // Looks and layouts are lists of thumbnails in an area of fixed height that scrolls, so more
      // of either never make the pop-out taller.
      const lookGrid = el('div', 'ssp-pop-grid ssp-pop-layouts');
      lookGrid.setAttribute('role', 'listbox');
      lookGrid.setAttribute('aria-label', 'Look');
      const looks = api.KITS.map(k => {
        const th = thumbBox();
        // A look brings its own colours and background; the layout stays.
        const tile = tileOf(k.name, th, () => { draft.kit = k.id; draft.colors = undefined; draft.style = styleOf(k.id); syncPanel(); });
        lookGrid.append(tile);
        return { k, tile, th, key: null };
      });
      const layoutGrid = el('div', 'ssp-pop-grid ssp-pop-layouts');
      layoutGrid.setAttribute('role', 'listbox');
      layoutGrid.setAttribute('aria-label', 'Layout');
      const layouts = api.LAYOUTS.map(l => {
        const th = thumbBox();
        const tile = tileOf(l.name, th, () => { draft.layout = l.id; syncPanel(); });
        layoutGrid.append(tile);
        return { l, tile, th, key: null };
      });
      // Apply: the one place the deck changes.
      const foot = el('div', 'ssp-pop-foot');
      const cancel = el('button', 'modal-btn secondary', 'Cancel');
      cancel.type = 'button';
      cancel.addEventListener('click', () => KS.closePopover());
      const apply = el('button', 'modal-btn primary ssp-apply', 'Apply');
      apply.type = 'button';
      apply.addEventListener('click', () => {
        const c = changes();
        KS.closePopover();
        if (!c) return;
        onPick(c);
      });
      foot.append(cancel, apply);
      const lookView = el('div', 'ssp-view');
      lookView.append(lookGrid);
      const layoutView = el('div', 'ssp-view');
      layoutView.hidden = true;
      layoutView.append(layoutGrid);
      const body = el('div', 'ssp-pop-body');
      body.append(lookView, layoutView);
      let queued = false;
      body.addEventListener('scroll', () => { if (!queued) { queued = true; requestAnimationFrame(() => { queued = false; paintVisible(); }); } });
      const switcher = el('div', 'ssp-switch');
      switcher.setAttribute('role', 'tablist');
      switcher.append(lookSwitch, layoutSwitch);
      const showView = (which) => {
        lookView.hidden = which !== 'look';
        layoutView.hidden = which !== 'layout';
        lookSwitch.classList.toggle('is-on', which === 'look');
        layoutSwitch.classList.toggle('is-on', which === 'layout');
        lookSwitch.setAttribute('aria-selected', String(which === 'look'));
        layoutSwitch.setAttribute('aria-selected', String(which === 'layout'));
        body.scrollTop = 0;
        requestAnimationFrame(syncPanel);
      };
      lookSwitch.addEventListener('click', () => showView('look'));
      layoutSwitch.addEventListener('click', () => showView('layout'));
      wrap.append(preview, switcher, body, foot);
      pop.append(wrap);
      panel = { preview, body, lookName, layoutName, looks, layouts, apply, lookView, layoutView };
      showView('look');
    });
    btn.update = (next) => { Object.assign(state, next); syncButton(); syncPanel(); };
    btn.addEventListener('click', open);
    btn.open = open;
    syncButton();
    return btn;
  }

  // Each deck's answer to "change the slides you edited too?", asked once while it's open.
  const restyleAnswers = new WeakMap();

  // The last look, background and layout used, for the next deck.
  function remember(deck) {
    try { localStorage.setItem('kairo-sermon-slides', JSON.stringify({ kit: deck.kit, style: deck.style, layout: deck.layout, colors: deck.colors })); } catch { /* not kept */ }
  }
  function lastChoice() {
    try { return JSON.parse(localStorage.getItem('kairo-sermon-slides') || '{}'); } catch { return {}; }
  }

  if (window.KairoSermonSlides) Object.assign(window.KairoSermonSlides, { mountPanel, lastChoice, remember, styleButton });
})();
