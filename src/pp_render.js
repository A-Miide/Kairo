// KAIRO — ProPresenter elements, drawn as ProPresenter draws them.
//
// A `pp` layer (made by the server's propresenter.rs from a .pro, .probundle,
// .proplaylist or .protheme) keeps a ProPresenter element exactly: its box,
// rotation, opacity and flip; its shape path (bezier points as fractions of
// the box); its fill (colour, gradient, picture or video with ProPresenter's
// scale and alignment, background blur); stroke; shadow; feather; and its text
// as ProPresenter stores it — paragraphs of runs from the RTF (font, size,
// colour, letter spacing, underline, strike, baseline, outline) plus the
// element's text attributes (capitalisation, gradient or picture fill,
// stroke, shadow, margins, vertical alignment, shrink-to-fit, transforms).
//
// The one renderer every surface uses — the output screens (display.html),
// the previews and thumbnails (service.js's paintLookLayers) and Theme
// Studio's canvas — so an imported theme looks the same everywhere.
//
// Everything is laid out on the theme's own 1920×1080 design canvas, in the
// slide's own units, and the whole canvas is scaled to the surface it sits on.
(function () {
  'use strict';

  const DESIGN_W = 1920, DESIGN_H = 1080;

  // ── colours ──────────────────────────────────────────────────────────────
  const rgba = (c, mult = 1) => {
    if (!Array.isArray(c)) return 'transparent';
    const [r, g, b, a] = c;
    const ch = (v) => Math.round(Math.max(0, Math.min(1, v || 0)) * 255);
    const alpha = Math.max(0, Math.min(1, (a == null ? 1 : a) * mult));
    return `rgba(${ch(r)},${ch(g)},${ch(b)},${+alpha.toFixed(4)})`;
  };

  // ProPresenter gradients: angle in degrees, stops at 0–1.
  function cssGradient(g) {
    if (!g || !Array.isArray(g.stops) || !g.stops.length) return 'none';
    const stops = [...g.stops].sort((a, b) => a.pos - b.pos).map(s => `${rgba(s.color)} ${(s.pos * 100).toFixed(3)}%`).join(', ');
    if (g.type === 'radial') return `radial-gradient(circle at center, ${stops})`;
    if (g.type === 'angle') return `conic-gradient(from ${90 - (g.angle || 0)}deg, ${stops})`;
    // ProPresenter's 0° runs left → right, counter-clockwise positive; CSS 90deg is left → right, clockwise.
    return `linear-gradient(${90 - (g.angle || 0)}deg, ${stops})`;
  }

  // ── fonts ────────────────────────────────────────────────────────────────
  // A ProPresenter font is its PostScript name ("AstridRegular",
  // "Montserrat-Bold"). Each gets an @font-face that asks for exactly that
  // installed face; the family with the weight and style the name implies is
  // the fallback (a variable font's named instance, for one).
  const declared = new Set();
  let styleEl = null;
  function fontAlias(ps) {
    if (!ps) return null;
    const alias = 'pp:' + ps;
    if (!declared.has(ps)) {
      declared.add(ps);
      if (!styleEl) { styleEl = document.createElement('style'); styleEl.id = 'pp-fonts'; document.head.appendChild(styleEl); }
      const q = ps.replace(/"/g, '');
      styleEl.appendChild(document.createTextNode(`@font-face{font-family:"${alias}";src:local("${q}"),local("${q.replace(/-/g, ' ')}");}\n`));
    }
    return alias;
  }
  const WEIGHTS = [[/thin|hairline/i, 100], [/extra.?light|ultra.?light/i, 200], [/light/i, 300], [/semi.?bold|demi.?bold/i, 600],
    [/extra.?bold|ultra.?bold/i, 800], [/black|heavy/i, 900], [/medium/i, 500], [/bold/i, 700]];
  function impliedWeight(ps, bold) {
    const face = (ps || '').split('-').slice(1).join('-') || '';
    for (const [re, w] of WEIGHTS) if (re.test(face)) return w;
    return bold ? 700 : 400;
  }
  function fontCss(run, base) {
    const ps = run.font || base.name || '';
    const family = base.family && ps.startsWith(base.family.replace(/\s+/g, '')) ? base.family : (ps.split('-')[0] || base.family || '');
    const weight = impliedWeight(ps, run.b || base.bold);
    // The named face only when it already is that weight: "Montserrat-Regular"
    // marked bold is the family's bold, not Regular bolded by the engine
    // (WebKit fakes bold by overprinting offset copies — visible ghosts).
    const alias = weight === impliedWeight(ps, false) ? fontAlias(ps) : null;
    const fam = [alias && `"${alias}"`, family && `"${family}"`, base.family && `"${base.family}"`, 'sans-serif'].filter(Boolean);
    return {
      family: [...new Set(fam)].join(', '),
      weight,
      italic: !!(run.i || base.italic || /italic|oblique/i.test(ps)),
    };
  }

  // A face's ascent and descent per point (hhea, as CoreText reads it).
  const metricCache = new Map();
  let measureCtx = null;
  function fontMetrics(fc) {
    const key = fc.family + '|' + fc.weight + '|' + fc.italic;
    let m = metricCache.get(key);
    if (!m) {
      measureCtx = measureCtx || document.createElement('canvas').getContext('2d');
      measureCtx.font = `${fc.italic ? 'italic ' : ''}${fc.weight} 100px ${fc.family}`;
      const t = measureCtx.measureText('Hg');
      m = { asc: (t.fontBoundingBoxAscent || 76) / 100, desc: (t.fontBoundingBoxDescent || 24) / 100 };
      if (document.fonts && document.fonts.check(measureCtx.font)) metricCache.set(key, m);
    }
    return m;
  }
  // ProPresenter's line pitch, measured against ProPresenter 21.4 itself:
  // the paragraph's line-height multiple × 1.19 × the face's natural height
  // (ascent + descent). Exact line spacing (RTF \sl without \slmult) is in points.
  const PP_LINE_FACTOR = 1.19;

  // ── text content ─────────────────────────────────────────────────────────
  const CAPS = { all: 'uppercase', title: 'none', start: 'capitalize', small: 'none', none: 'none' };
  // ProPresenter's Title Case: every word capitalised except articles, short
  // conjunctions and prepositions of three letters or fewer — unless the word
  // starts the text, or follows a sentence end, colon or opening quote.
  const MINOR = new Set(['a', 'an', 'the', 'and', 'but', 'or', 'nor', 'for', 'so', 'yet', 'as', 'at', 'by', 'in', 'of', 'off', 'on', 'per', 'to', 'via', 'vs']);
  function titleCase(t, state) {
    return t.replace(/[\p{L}\p{N}'’]+|[^\p{L}\p{N}'’]+/gu, (w) => {
      if (!/[\p{L}\p{N}]/u.test(w)) { if (/[.!?:;‘“"(\[]\s*$/.test(w) || /[\n\u2028]/.test(w)) state.start = true; return w; }
      const lower = w.toLowerCase();
      const out = (!state.start && MINOR.has(lower)) ? lower : w.charAt(0).toUpperCase() + w.slice(1);
      state.start = false;
      return out;
    });
  }

  function transformText(t, mode, delim) {
    switch (mode) {
      case 'singleLine': return t.replace(/\s*[\n\u2028]+\s*/g, delim || ' ');
      case 'replaceLineReturns': return t.replace(/[\n\u2028]+/g, delim || ' ');
      case 'wordPerLine': return t.split(/\s+/).filter(Boolean).join('\n');
      case 'charPerLine': return [...t.replace(/\s+/g, '')].join('\n');
      default: return t;
    }
  }

  // The paragraphs to draw: the stored ones, or — for a box a theme binds to
  // the verse or the reference — that content in the box's first style.
  function paragraphsFor(layer, content) {
    const text = layer.text || {};
    const stored = Array.isArray(text.paragraphs) ? text.paragraphs : [];
    let words = null;
    if (layer.binding === 'verse') words = content.verseText || '';
    else if (layer.binding === 'reference') words = content.referenceText || '';
    if (words == null) {
      if (text.transform && text.transform !== 'none') {
        const plain = stored.map(p => (p.runs || []).map(r => r.t).join('')).join('\n');
        const style = (stored[0] && stored[0].runs && stored[0].runs[0]) || {};
        return transformText(plain, text.transform, text.delimiter).split('\n').map(line => ({ ...(stored[0] || {}), runs: [{ ...style, t: line }] }));
      }
      return stored;
    }
    const firstPara = stored[0] || { align: text.paragraph && text.paragraph.align };
    const firstRun = (firstPara.runs && firstPara.runs[0]) || {};
    words = transformText(String(words), text.transform, text.delimiter);
    return words.split('\n').map(line => ({ ...firstPara, runs: [{ ...firstRun, t: line }] }));
  }

  const ALIGN = { left: 'left', right: 'right', center: 'center', justify: 'justify', natural: 'start' };

  function buildText(layer, content, box, textScale) {
    const text = layer.text || {};
    const base = text.font || {};
    const paras = paragraphsFor(layer, content);
    if (!paras.length || !paras.some(p => (p.runs || []).some(r => r.t && r.t.length))) return null;
    const m = text.margins || {};
    const holder = document.createElement('div');
    holder.className = 'pp-text';
    holder.style.cssText = `position:absolute;left:${(m.l || 0) * textScale}px;top:${(m.t || 0) * textScale}px;right:${(m.r || 0) * textScale}px;bottom:${(m.b || 0) * textScale}px;overflow:visible;`;
    const flow = document.createElement('div');
    flow.className = 'pp-flow';
    flow.style.cssText = 'position:absolute;left:0;right:0;top:0;';
    holder.appendChild(flow);
    holder.style.display = 'block';
    // Where ProPresenter puts the text block: from the first line's ascent to
    // the last line's baseline, aligned top / middle / bottom in the box.
    const place = (asc) => {
      if (!holder.isConnected) return;
      const marks = [];
      const mark = (pd, atEnd) => {
        const m = document.createElement('span');
        m.style.cssText = 'display:inline-block;width:0;height:0;vertical-align:baseline;';
        if (atEnd) pd.appendChild(m); else pd.insertBefore(m, pd.firstChild);
        marks.push(m);
        return m;
      };
      const ps = flow.children;
      if (!ps.length) return;
      flow.style.top = '0px';
      const top0 = flow.getBoundingClientRect().top;
      const k = holder.getBoundingClientRect().height / (holder.offsetHeight || 1) || 1;   // the stage's scale
      const b1 = (mark(ps[0], false).getBoundingClientRect().top - top0) / k;
      const bn = (mark(ps[ps.length - 1], true).getBoundingClientRect().top - top0) / k;
      marks.forEach(m => m.remove());
      const boxH = holder.offsetHeight;
      const blockH = bn - b1 + asc;
      let blockTop = 0;
      if (text.valign === 'middle') blockTop = (boxH - blockH) / 2;
      else if (text.valign === 'bottom') blockTop = boxH - blockH;
      flow.style.top = (blockTop + asc - b1) + 'px';
    };

    const paint = (fit) => {
      flow.innerHTML = '';
      const capsMode = CAPS[text.caps] || 'none';
      const titleState = { start: true };
      let firstAsc = 0;
      paras.forEach((p, pi) => {
        const pd = document.createElement('div');
        const pr = text.paragraph || {};
        const align = p.align && p.align !== 'natural' ? p.align : (pr.align || 'natural');
        const runs = (p.runs && p.runs.length) ? p.runs : [{ t: '' }];
        const lead = runs.find(r => r.t && r.t.length) || runs[0];
        const leadSize = (lead.size || base.size || 36) * textScale * fit;
        const leadFont = fontCss(lead, base);
        const fm = fontMetrics(leadFont);
        const natural = (fm.asc + fm.desc) * leadSize;
        const mult = p.lineMult || pr.lineMult || 1;
        let lineH = p.lineExact ? p.lineExact * textScale * fit : natural * mult * PP_LINE_FACTOR;
        if (pr.maxLine > 0) lineH = Math.min(lineH, pr.maxLine * textScale * fit);
        if (pr.minLine > 0) lineH = Math.max(lineH, pr.minLine * textScale * fit);
        if (!p.lineExact && pr.lineSpacing) lineH += pr.lineSpacing * textScale * fit;
        const before = (p.before != null ? p.before : (pi > 0 ? pr.before : 0)) || 0;
        const after = (p.after != null ? p.after : pr.after) || 0;
        const indent = (p.indent != null ? p.indent : pr.indent) || 0;
        const first = (p.firstIndent != null ? p.firstIndent : pr.firstIndent) || 0;
        const tail = (p.tailIndent != null ? p.tailIndent : pr.tailIndent) || 0;
        pd.style.cssText = `margin:0;padding:0;text-align:${ALIGN[align] || 'start'};line-height:${lineH}px;` +
          `margin-top:${before * textScale * fit}px;margin-bottom:${after * textScale * fit}px;` +
          `padding-left:${indent * textScale * fit}px;text-indent:${(first - indent) * textScale * fit}px;` +
          `padding-right:${(tail < 0 ? -tail : 0) * textScale * fit}px;` +
          `font-family:${leadFont.family};font-size:${leadSize}px;font-weight:${leadFont.weight};font-style:${leadFont.italic ? 'italic' : 'normal'};` +
          `white-space:pre-wrap;overflow-wrap:break-word;word-break:normal;text-transform:${capsMode};` +
          (text.caps === 'small' ? 'font-variant-caps:small-caps;' : '') +
          (text.noLigatures ? 'font-variant-ligatures:none;' : '');
        let any = false;
        runs.forEach(r => {
          const t = (r.t || '').replace(/\u2028/g, '\n');
          if (!t) return;
          any = true;
          const s = document.createElement('span');
          const fc = fontCss(r, base);
          const size = (r.size || base.size || 36) * textScale * fit;
          const st = s.style;
          st.fontFamily = fc.family;
          st.fontWeight = fc.weight;
          st.fontStyle = fc.italic ? 'italic' : 'normal';
          st.fontSize = size + 'px';
          const kern = r.kern != null ? r.kern : (text.kerning || 0);
          if (kern) st.letterSpacing = (kern * textScale * fit) + 'px';
          if (r.base) { st.position = 'relative'; st.top = (-r.base * textScale * fit) + 'px'; }
          if (r.script === 1 || text.superscript > 0) st.verticalAlign = 'super';
          if (r.script === -1 || text.superscript < 0) st.verticalAlign = 'sub';
          if (r.caps) st.textTransform = 'uppercase';
          if (r.smallCaps) st.fontVariantCaps = 'small-caps';
          if (r.noLig) st.fontVariantLigatures = 'none';
          // fill
          const colour = r.color || text.fill || [1, 1, 1, 1];
          if (text.gradientFill && text.gradientFill.gradient) {
            st.backgroundImage = cssGradient(text.gradientFill.gradient);
            st.webkitBackgroundClip = 'text'; st.backgroundClip = 'text'; st.color = 'transparent';
          } else if (text.mediaFill && text.mediaFill.src) {
            st.backgroundImage = `url("${authed(text.mediaFill.src)}")`;
            st.backgroundSize = 'cover'; st.backgroundPosition = 'center';
            st.webkitBackgroundClip = 'text'; st.backgroundClip = 'text'; st.color = 'transparent';
          } else {
            st.color = rgba(colour);
          }
          // outline: NSStrokeWidth, a percent of the point size; negative strokes and fills.
          const sw = r.strokeW != null ? r.strokeW : (text.strokeWidth || 0);
          if (sw) {
            const px = Math.abs(sw) / 100 * size;
            st.webkitTextStrokeWidth = px + 'px';
            st.webkitTextStrokeColor = rgba(r.strokeColor || text.strokeColor || colour);
            if (sw > 0) st.color = 'transparent';
            st.paintOrder = 'stroke fill';
          }
          const ul = r.ul || (text.underline && text.underline.style);
          const stk = r.st || (text.strike && text.strike.style);
          const lines = [];
          if (ul) lines.push('underline');
          if (stk) lines.push('line-through');
          if (lines.length) {
            st.textDecorationLine = lines.join(' ');
            const dstyle = (ul || stk) === 3 ? 'double' : (r.ul === 4 ? 'dotted' : r.ul === 5 ? 'dashed' : 'solid');
            st.textDecorationStyle = dstyle;
            if ((ul || stk) === 2) st.textDecorationThickness = Math.max(2, size / 12) + 'px';
            const dc = r.ulColor || r.stColor || (text.underline && text.underline.color) || (text.strike && text.strike.color);
            if (Array.isArray(dc) && dc[3] > 0) st.textDecorationColor = rgba(dc);
          }
          const bg = r.bg || text.background;
          if (Array.isArray(bg) && bg[3] > 0) { st.backgroundColor = rgba(bg); st.webkitBoxDecorationBreak = 'clone'; st.boxDecorationBreak = 'clone'; }
          s.textContent = text.caps === 'title' ? titleCase(t, titleState) : t;
          pd.appendChild(s);
        });
        if (!any) pd.appendChild(document.createTextNode('\u200b'));
        if (pi === 0) firstAsc = fm.asc * leadSize;
        flow.appendChild(pd);
      });
      place(firstAsc);
      // Text shadow (the text tab's own, apart from the shape's).
      const sh = text.shadow;
      flow.style.textShadow = sh && sh.enable ? shadowCss(sh, textScale * fit, 'text') : 'none';
    };

    let fit = 1;
    paint(fit);
    // Shrink (and/or grow) to the box, as ProPresenter's text Scale behaviour does.
    // A box a theme binds to the verse or reference takes any length of text:
    // when ProPresenter's own setting is "none", it still shrinks rather than
    // spill past the box (text that fits is untouched).
    const mode = (text.scale === 'none' || !text.scale) && (layer.binding === 'verse' || layer.binding === 'reference') ? 'down' : text.scale;
    const fitToBox = () => {
      if (!holder.isConnected || !(mode === 'down' || mode === 'up' || mode === 'upDown')) return;
      const avail = holder.clientHeight, availW = holder.clientWidth;
      if (!avail || !availW) return;
      const fits = (f) => { paint(f); return flow.offsetHeight <= avail + 0.5 && flow.scrollWidth <= availW + 0.5; };
      let lo = mode === 'up' ? 1 : 0.05, hi = mode === 'down' ? 1 : 8;
      if (mode !== 'up' && fits(1)) { if (mode === 'down') return; lo = 1; }
      if (mode === 'up' && !fits(1)) { paint(1); return; }
      for (let i = 0; i < 18; i++) { const mid = (lo + hi) / 2; if (fits(mid)) lo = mid; else hi = mid; }
      fit = lo; paint(fit);
    };
    holder.__ppFit = fitToBox;
    holder.__ppPaint = () => paint(fit);
    return holder;
  }

  // ── shadows ──────────────────────────────────────────────────────────────
  // ProPresenter: angle in degrees (0 = right, counter-clockwise), offset and
  // radius in points, colour × opacity.
  function shadowCss(sh, k, kind) {
    const a = (sh.angle || 0) * Math.PI / 180;
    const dx = Math.cos(a) * (sh.offset || 0) * k;
    const dy = -Math.sin(a) * (sh.offset || 0) * k;
    const col = rgba(sh.color || [0, 0, 0, 1], sh.opacity == null ? 1 : sh.opacity);
    const blur = (sh.radius || 0) * k;
    return kind === 'text' ? `${dx}px ${dy}px ${blur}px ${col}` : `drop-shadow(${dx}px ${dy}px ${blur / 2}px ${col})`;
  }

  // ── shape path ───────────────────────────────────────────────────────────
  function pathD(shape, w, h) {
    const pts = (shape && shape.points) || [];
    if (pts.length < 2) return `M0 0H${w}V${h}H0Z`;
    const P = (v) => `${(v[0] * w).toFixed(3)} ${(v[1] * h).toFixed(3)}`;
    let d = `M${P(pts[0].p)}`;
    const seg = (a, b) => {
      const straight = !a.curved && !b.curved && a.q1[0] === a.p[0] && a.q1[1] === a.p[1] && b.q0[0] === b.p[0] && b.q0[1] === b.p[1];
      d += straight ? ` L${P(b.p)}` : ` C${P(a.q1)} ${P(b.q0)} ${P(b.p)}`;
    };
    for (let i = 1; i < pts.length; i++) seg(pts[i - 1], pts[i]);
    if (shape.closed !== false) { seg(pts[pts.length - 1], pts[0]); d += ' Z'; }
    return d;
  }

  const authed = (u) => (typeof authedMediaUrl === 'function' ? authedMediaUrl(u) : u);

  const OBJ_POS = {
    middleCenter: 'center', topLeft: 'left top', topCenter: 'center top', topRight: 'right top', middleRight: 'right center',
    bottomRight: 'right bottom', bottomCenter: 'center bottom', bottomLeft: 'left bottom', middleLeft: 'left center',
  };

  function mediaNode(m, w, h, opts) {
    const isVideo = m.kind === 'video';
    const el = document.createElement(isVideo ? 'video' : 'img');
    if (isVideo) {
      el.muted = true; el.loop = true; el.playsInline = true;
      el.autoplay = opts.motion === 'live';
      if (opts.motion !== 'live') el.preload = 'metadata';
    } else {
      el.decoding = 'async';
      el.draggable = false;
    }
    el.src = authed(m.src);
    const fit = m.scale === 'fill' ? 'cover' : m.scale === 'stretch' ? 'fill' : 'contain';
    let css = `position:absolute;left:0;top:0;width:${w}px;height:${h}px;object-fit:${fit};object-position:${OBJ_POS[m.align] || 'center'};pointer-events:none;`;
    if (m.scale === 'custom' && m.customBounds) {
      const b = m.customBounds;
      css = `position:absolute;left:${b.x}px;top:${b.y}px;width:${b.w}px;height:${b.h}px;object-fit:fill;pointer-events:none;`;
    }
    const t = [];
    if (m.flipH) t.push('scaleX(-1)');
    if (m.flipV) t.push('scaleY(-1)');
    if (m.rotation) t.push(`rotate(${m.rotation}deg)`);
    if (t.length) css += `transform:${t.join(' ')};`;
    if (m.crop) {
      const c = m.crop;
      css += `clip-path:inset(${c.t || 0}px ${c.r || 0}px ${c.b || 0}px ${c.l || 0}px);`;
    }
    if (m.blurred) css += 'filter:blur(20px);';
    el.style.cssText = css;
    return el;
  }

  // ── one element ──────────────────────────────────────────────────────────
  function render(layer, content = {}, opts = {}) {
    // A full-canvas stage at the design size, scaled to whatever holds it.
    const stage = document.createElement('div');
    stage.className = 'pp-stage';
    stage.style.cssText = 'position:absolute;inset:0;overflow:visible;pointer-events:none;';
    const canvas = document.createElement('div');
    canvas.style.cssText = `position:absolute;left:0;top:0;width:${DESIGN_W}px;height:${DESIGN_H}px;transform-origin:0 0;`;
    stage.appendChild(canvas);

    const pos = layer.pos || { x: 0, y: 0, w: DESIGN_W, h: DESIGN_H };
    const w = Math.max(0, pos.w), h = Math.max(0, pos.h);
    const slideH = (layer.slideSize && layer.slideSize.h) || DESIGN_H;
    const textScale = DESIGN_H / slideH;

    const box = document.createElement('div');
    box.className = 'pp-el';
    const tf = [];
    if (layer.rotation) tf.push(`rotate(${layer.rotation}deg)`);
    if (layer.flip === 'horizontal' || layer.flip === 'both') tf.push('scaleX(-1)');
    if (layer.flip === 'vertical' || layer.flip === 'both') tf.push('scaleY(-1)');
    box.style.cssText = `position:absolute;left:${pos.x}px;top:${pos.y}px;width:${w}px;height:${h}px;` +
      `opacity:${layer.opacity == null ? 1 : layer.opacity};${tf.length ? `transform:${tf.join(' ')};` : ''}transform-origin:50% 50%;`;

    const shape = layer.shape || {};
    const d = pathD(shape, w, h);
    const fill = layer.fill || {};
    const stroke = layer.stroke || {};

    // The shape: fill (colour / gradient / picture / video / background effect) and stroke.
    const art = document.createElement('div');
    art.style.cssText = `position:absolute;inset:0;overflow:visible;`;
    if (layer.shadow && layer.shadow.enable) art.style.filter = shadowCss(layer.shadow, 1, 'shape');
    const isRect = !shape.points || shape.type === 'rectangle' || (shape.points.length === 4 && !shape.points.some(p => p.curved));
    const clip = isRect ? '' : `clip-path:path("${d}");`;
    if (fill.enable) {
      if (fill.media && fill.media.src) {
        const mwrap = document.createElement('div');
        mwrap.style.cssText = `position:absolute;inset:0;overflow:hidden;${clip}`;
        mwrap.appendChild(mediaNode(fill.media, w, h, opts));
        art.appendChild(mwrap);
      } else if (fill.gradient) {
        const g = document.createElement('div');
        g.style.cssText = `position:absolute;inset:0;background:${cssGradient(fill.gradient)};${clip}`;
        art.appendChild(g);
      } else if (fill.color) {
        const c = document.createElement('div');
        c.style.cssText = `position:absolute;inset:0;background:${rgba(fill.color)};${clip}`;
        art.appendChild(c);
      }
      if (fill.backgroundEffect && (fill.backgroundEffect.blur || fill.backgroundEffect.invert)) {
        const fx = document.createElement('div');
        const be = fill.backgroundEffect;
        const f = be.invert ? 'invert(1)' : `blur(${be.blur || 0}px)${be.saturation != null ? ` saturate(${be.saturation})` : ''}`;
        fx.style.cssText = `position:absolute;inset:0;-webkit-backdrop-filter:${f};backdrop-filter:${f};${clip}`;
        art.insertBefore(fx, art.firstChild);
      }
    }
    if (stroke.enable && stroke.width > 0) {
      const ns = 'http://www.w3.org/2000/svg';
      const svg = document.createElementNS(ns, 'svg');
      svg.setAttribute('width', w); svg.setAttribute('height', h);
      svg.setAttribute('viewBox', `0 0 ${w} ${h}`);
      svg.style.cssText = 'position:absolute;left:0;top:0;overflow:visible;';
      const p = document.createElementNS(ns, 'path');
      p.setAttribute('d', d);
      p.setAttribute('fill', 'none');
      p.setAttribute('stroke', rgba(stroke.color));
      p.setAttribute('stroke-width', stroke.width);
      const dash = Array.isArray(stroke.pattern) && stroke.pattern.length ? stroke.pattern.map(v => v * stroke.width)
        : stroke.style === 'squareDash' ? [1, 1].map(v => v * stroke.width)
        : stroke.style === 'shortDash' ? [2, 2].map(v => v * stroke.width)
        : stroke.style === 'longDash' ? [4, 2].map(v => v * stroke.width) : null;
      if (dash) p.setAttribute('stroke-dasharray', dash.join(' '));
      svg.appendChild(p);
      art.appendChild(svg);
    }
    // Feather: the edges fade out over the radius (a fraction of the box, or points).
    const fe = layer.feather || {};
    if (fe.enable && fe.radius > 0) {
      const r = fe.radius <= 1 ? fe.radius * Math.min(w, h) : fe.radius;
      const pr = (r / Math.max(w, 1) * 100).toFixed(3), pv = (r / Math.max(h, 1) * 100).toFixed(3);
      const mask = `linear-gradient(to right, transparent, #000 ${pr}%, #000 ${100 - pr}%, transparent), linear-gradient(to bottom, transparent, #000 ${pv}%, #000 ${100 - pv}%, transparent)`;
      art.style.webkitMaskImage = mask; art.style.maskImage = mask;
      art.style.webkitMaskComposite = 'source-in'; art.style.maskComposite = 'intersect';
    }
    if (!opts.textOnly) box.appendChild(art);

    // The text.
    let textNode = null;
    if (layer.text) {
      textNode = buildText(layer, content, box, textScale);
      if (textNode) {
        if (!isRect) textNode.style.clipPath = `path("${d}")`;
        if (layer.scroller) {
          const s = layer.scroller;
          const flowEl = textNode.querySelector('.pp-flow');
          flowEl.style.whiteSpace = 'nowrap';
          flowEl.style.width = 'max-content';
          const dur = Math.max(2, (w + 1000) / Math.max(10, s.rate * 60 || 60));
          const dir = s.direction === 1 ? 'normal' : 'reverse';
          flowEl.animate([{ transform: `translateX(${w}px)` }, { transform: 'translateX(-100%)' }], { duration: dur * 1000, iterations: s.repeat ? Infinity : 1, direction: dir });
          textNode.style.overflow = 'hidden';
        }
        box.appendChild(textNode);
      }
    }
    canvas.appendChild(box);

    // Scale the design canvas to the stage, and lay the text out again once
    // its fonts have arrived (and whenever the stage changes size).
    const relayout = () => {
      const k = stage.clientWidth / DESIGN_W;
      if (k > 0) canvas.style.transform = `scale(${k})`;
      if (textNode) { textNode.__ppPaint && textNode.__ppPaint(); textNode.__ppFit && textNode.__ppFit(); }
    };
    if (typeof ResizeObserver === 'function') {
      let lastW = -1;
      new ResizeObserver(() => { if (stage.clientWidth !== lastW) { lastW = stage.clientWidth; relayout(); } }).observe(stage);
    }
    if (opts.scale) canvas.style.transform = `scale(${opts.scale})`;
    if (textNode && document.fonts) {
      const faces = new Set();
      (layer.text.paragraphs || []).forEach(p => (p.runs || []).forEach(r => { const fc = fontCss(r, layer.text.font || {}); faces.add(`${fc.italic ? 'italic ' : ''}${fc.weight} 40px ${fc.family}`); }));
      Promise.all([...faces].map(f => document.fonts.load(f).catch(() => null))).then(() => { metricCache.clear(); if (stage.isConnected) relayout(); });
    }
    requestAnimationFrame(() => { if (stage.isConnected) relayout(); });
    return stage;
  }

  // Words a layer would show (for search, the slide list and outputs that carry text only).
  function plainText(layer) {
    const ps = (layer.text && layer.text.paragraphs) || [];
    return ps.map(p => (p.runs || []).map(r => r.t || '').join('').replace(/\u2028/g, '\n')).join('\n');
  }

  window.KairoPP = { render, plainText, rgba, cssGradient };
})();
