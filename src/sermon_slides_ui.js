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

    // ── the deck ──
    const looks = el('div', 'ssp-looks');
    for (const k of api.KITS) {
      const b = el('button', 'ssp-look' + (k.id === deck.kit ? ' is-on' : ''));
      b.type = 'button';
      b.title = k.name;
      b.setAttribute('aria-label', k.name);
      const sw = el('span', 'ssp-swatch');
      sw.style.background = `radial-gradient(circle at 15% 10%, ${k.bg2} 0%, ${k.bg} 70%)`;
      const aa = el('span', 'ssp-aa', 'Aa');
      aa.style.color = k.text;
      aa.style.fontFamily = `"${k.display.family}", serif`;
      const dot = el('span', 'ssp-dot');
      dot.style.background = k.accent;
      sw.append(aa, dot);
      b.append(sw, el('span', 'ssp-look-name', k.name));
      // A look brings its own colours: the operator's own are set again from it.
      b.addEventListener('click', () => { deck.kit = k.id; delete deck.colors; remember(deck); apply(block?.gen?.part); });
      looks.append(b);
      if (typeof loadGoogleFont === 'function') loadGoogleFont(k.display.family);
    }
    const deckSec = section('Deck look', looks,
      segment(api.BACKGROUNDS, deck.style, (id) => { deck.style = id; remember(deck); apply(block?.gen?.part); }, 'Background'),
      segment(api.LAYOUTS, deck.layout || 'left', (id) => {
        deck.layout = id;
        // A position applied to every slide belonged to the old layout.
        for (const props of Object.values(deck.common || {})) { delete props.pos; delete props.align; }
        remember(deck);
        apply(block?.gen?.part);
      }, 'Layout'));

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
    const slideSec = review.length ? section('This slide', ...review) : null;

    // ── contents: what of the document is in the deck ──
    const list = el('ul', 'ssp-parts');
    const include = new Set(deck.include);
    for (const p of api.partsOf(deck.outline)) {
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

  // Each deck's answer to "change the slides you edited too?", asked once while it's open.
  const restyleAnswers = new WeakMap();

  function segment(options, current, onPick, label) {
    const seg = el('div', 'ssp-seg');
    seg.setAttribute('role', 'group');
    if (label) seg.setAttribute('aria-label', label);
    seg.style.setProperty('--n', Math.min(options.length, 3));   // more than three wrap to a second row
    for (const o of options) {
      const b = el('button', o.id === current ? 'is-on' : '', o.name);
      b.type = 'button';
      b.setAttribute('aria-pressed', String(o.id === current));
      b.addEventListener('click', () => onPick(o.id));
      seg.append(b);
    }
    return seg;
  }
  // The last look, background and layout used, for the next deck.
  function remember(deck) {
    try { localStorage.setItem('kairo-sermon-slides', JSON.stringify({ kit: deck.kit, style: deck.style, layout: deck.layout, colors: deck.colors })); } catch { /* not kept */ }
  }
  function lastChoice() {
    try { return JSON.parse(localStorage.getItem('kairo-sermon-slides') || '{}'); } catch { return {}; }
  }

  if (window.KairoSermonSlides) Object.assign(window.KairoSermonSlides, { mountPanel, lastChoice, remember });
})();
