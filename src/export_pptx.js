// KAIRO — a deck as a PowerPoint file (.pptx).
//
// Pure: layers in, bytes out. No DOM and no network, so it runs the same in the app and in tests.
// The caller (service.js, "Export as PowerPoint") resolves each slide's look, its own overrides and its
// words into layers, and fetches the pictures; this turns them into real, editable PowerPoint shapes:
//   text      → a text box (font, size, bold, italic, colour, spacing, alignment, shadow, case)
//   shapes    → rectangles, rounded or ellipse shapes, cut-out shapes, gradients, strokes, shadows
//   pictures  → pictures, cropped or fitted as the layer says
// Effects PowerPoint can't draw (blend modes such as the grain's Overlay, frosting behind glass, soft
// glows in a gradient) are left out or approximated, and reported in `notes` so the person is told.
//
// A .pptx is a zip of XML parts. The zip here stores its files uncompressed: PowerPoint, Keynote and
// Google Slides all read that, and it needs no compressor.
(function (root) {
  'use strict';

  const W = 1920, H = 1080;
  const EMU = 6350;                    // 1 canvas px; 1920 px = 12,192,000 EMU = 13.333 in
  const emu = (px) => Math.round(px * EMU);
  const NS = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"';

  // ── text and colour helpers ─────────────────────────────────────────────────────────────────────
  const esc = (s) => String(s == null ? '' : s)
    .replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F￾￿]/g, '')
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  const clamp = (n, lo, hi) => Math.max(lo, Math.min(hi, n));
  const num = (v, d = 0) => (Number.isFinite(Number(v)) ? Number(v) : d);

  // "#RGB", "#RRGGBB", "#RRGGBBAA", "rgb()", "rgba()" → { hex: 'RRGGBB', a: 0..1 }; null for none/transparent.
  function parseColor(c, opacity) {
    let hex = null, a = 1;
    const s = String(c == null ? '' : c).trim();
    let m;
    if ((m = /^#([0-9a-f]{3})$/i.exec(s))) hex = m[1].split('').map(x => x + x).join('');
    else if ((m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(s))) { hex = m[1]; if (m[2]) a = parseInt(m[2], 16) / 255; }
    else if ((m = /^rgba?\(\s*(\d+)[ ,]+(\d+)[ ,]+(\d+)(?:[ ,/]+([\d.]+%?))?\s*\)$/i.exec(s))) {
      hex = [m[1], m[2], m[3]].map(x => clamp(+x, 0, 255).toString(16).padStart(2, '0')).join('');
      if (m[4] != null) a = /%$/.test(m[4]) ? parseFloat(m[4]) / 100 : parseFloat(m[4]);
    }
    if (!hex) return null;
    if (opacity != null) a *= clamp(num(opacity, 100), 0, 100) / 100;
    return { hex: hex.toUpperCase(), a: clamp(a, 0, 1) };
  }
  const colorXml = (c) => (c ? `<a:srgbClr val="${c.hex}">${c.a < 1 ? `<a:alpha val="${Math.round(c.a * 100000)}"/>` : ''}</a:srgbClr>` : '');
  const solidFill = (c) => (c ? `<a:solidFill>${colorXml(c)}</a:solidFill>` : '<a:noFill/>');

  // The gradient's stops, as the app reads them (layer_geometry.js gradientStops).
  function gradientStops(layer) {
    if (Array.isArray(layer.gradientStops) && layer.gradientStops.length >= 2) return layer.gradientStops.slice().sort((x, y) => x.pos - y.pos);
    return [{ pos: 0, color: layer.color || '#ffffff' }, { pos: 100, color: layer.color2 || '#1a1a2e' }];
  }
  // CSS angles run clockwise from "up"; PowerPoint's from "right".
  const pptAngle = (css) => Math.round((((num(css, 135) - 90) % 360) + 360) % 360 * 60000);
  function gradFill(stops, angle, opacity) {
    const gs = stops.map(s => {
      const c = parseColor(s.color, opacity);
      return c ? `<a:gs pos="${Math.round(clamp(num(s.pos), 0, 100) * 1000)}">${colorXml(c)}</a:gs>` : '';
    }).join('');
    return `<a:gradFill rotWithShape="1"><a:gsLst>${gs}</a:gsLst><a:lin ang="${pptAngle(angle)}" scaled="0"/></a:gradFill>`;
  }
  // A drop or text shadow: offset, blur and colour → PowerPoint's outer shadow.
  function shadowXml(sh) {
    if (!sh || sh.enabled === false) return '';
    const c = parseColor(sh.color || '#000000', sh.opacity != null ? sh.opacity : 45);
    if (!c) return '';
    const x = num(sh.x), y = num(sh.y);
    const dist = Math.hypot(x, y), dir = ((Math.atan2(y, x) * 180 / Math.PI) + 360) % 360;
    return `<a:effectLst><a:outerShdw blurRad="${emu(num(sh.blur))}" dist="${emu(dist)}" dir="${Math.round(dir * 60000)}" algn="ctr" rotWithShape="0">${colorXml(c)}</a:outerShdw></a:effectLst>`;
  }

  // ── layers → drawing items ──────────────────────────────────────────────────────────────────────
  // Same decisions as the app's slide painter (service.js paintLookLayers), in plain data:
  // item = { kind: 'shape' | 'text' | 'image', x, y, w, h, rotation, ... }
  const FILL_KEYS = ['fill', 'color', 'color2', 'angle', 'opacity', 'src', 'dim', 'gradientStops', 'gradientGlows', 'corners', 'stroke', 'drop', 'glass'];
  function withOverride(layer, ov, keys) {
    if (!ov) return layer;
    let out = layer;
    for (const k of keys) {
      if (ov[k] === undefined || ov[k] === layer[k]) continue;
      if (out === layer) out = { ...layer };
      if (ov[k] === null) delete out[k]; else out[k] = ov[k];
    }
    return out;
  }
  const fullBox = { x: 0, y: 0, w: W, h: H };

  // opts: { fillKeys, layerKeys } (the app's own lists, when given). Returns { items, notes: [strings], fonts: [families] }.
  function itemsFromLayers(layers, style, content, opts) {
    opts = opts || {};
    style = style || {};
    content = content || {};
    const items = [], notes = new Set(), fonts = new Set();
    const fillKeys = opts.fillKeys || root.FILL_OVERRIDE_KEYS || FILL_KEYS;
    const layerKeys = opts.layerKeys || root.LAYER_OVERRIDE_KEYS || ['rotation', 'accentColor', 'grayscale', 'fade', 'blendMode'];
    // A reference or translation with no verse line to caption it is left out (as on the output).
    const hasVerseLayer = layers.some(l => l.type === 'text' && l.binding === 'verse' && l.visible !== false);
    const note = (s) => notes.add(s);

    for (let layer of layers) {
      if (!layer || layer.visible === false) continue;
      const ov = style[layer.id] || {};
      if (ov.visible === false) continue;
      layer = withOverride(layer, ov, layerKeys);
      const mode = ov.blendMode !== undefined ? ov.blendMode : layer.blendMode;

      if (layer.type === 'background') {
        let glows = [];
        layer = withOverride(layer, ov, fillKeys);
        if (layer.fill === 'none' || layer.fill === 'transparent') continue;
        const box = layer.pos || fullBox;
        if (mode && mode !== 'Normal') note('Blend modes');
        const it = { kind: 'shape', ...box, rotation: num(layer.rotation), shape: layer.shape || 'rect', radius: num(layer.radius), corners: Array.isArray(layer.corners) && layer.corners.length === 4 ? layer.corners.map(n => num(n)) : null,
          clip: Array.isArray(layer.clip) && layer.clip.length >= 3 ? layer.clip : null, stroke: layer.stroke && layer.stroke.enabled && layer.stroke.width > 0 ? layer.stroke : null,
          drop: layer.drop && layer.drop.enabled ? layer.drop : null };
        if (layer.fill === 'image') {
          // A picture fill, dimmed: the picture, then a black layer over it.
          if (layer.src) items.push({ kind: 'image', ...box, src: layer.src, fit: 'cover', opacity: layer.opacity != null ? layer.opacity : 100 });
          const dim = clamp(num(layer.dim), 0, 90);
          if (dim > 0) items.push({ ...it, fill: { kind: 'solid', color: parseColor('#000000', dim) } });
          continue;
        }
        if (layer.glass) {
          // Frosting behind the shape can't be drawn: a faint light tint and a bright edge stand in for it.
          it.fill = { kind: 'solid', color: parseColor(layer.color || '#FFFFFF', Math.min(layer.opacity != null ? layer.opacity : 10, 40) + 8) };
          it.stroke = { color: '#FFFFFF', width: 1.5, alpha: 0.35 };
          note('Frosted glass (shown as a light tint)');
        } else if (layer.fill === 'gradient') {
          it.fill = { kind: 'gradient', stops: gradientStops(layer), angle: layer.angle != null ? layer.angle : 135, opacity: layer.opacity };
          glows = Array.isArray(layer.gradientGlows) ? layer.gradientGlows : [];
        } else {
          it.fill = { kind: 'solid', color: parseColor(layer.color, layer.opacity) };
        }
        if (it.fill && it.fill.kind === 'solid' && !it.fill.color) continue;
        items.push(it);
        // Soft spots of light over the base: a radial fade from each glow's centre, reaching rx / ry (% of the box).
        for (const g of glows) {
          const gc = parseColor(g.color, layer.opacity);
          if (!gc) continue;
          const gw = box.w * num(g.rx, 50) / 100 * 2, gh = box.h * num(g.ry, 50) / 100 * 2;
          items.push({ kind: 'image', x: box.x + box.w * num(g.x, 50) / 100 - gw / 2, y: box.y + box.h * num(g.y, 50) / 100 - gh / 2, w: gw, h: gh, fit: 'fill',
            src: `glow:${gc.hex}:${Math.round(gc.a * 100)}`, name: 'Glow' });
        }
        continue;
      }

      if (layer.type === 'image') {
        if (!layer.src) continue;
        if (mode && mode !== 'Normal') { note('Texture layers that blend (such as grain)'); continue; }
        if (/\.(mp4|mov|webm|m4v)(\?|$)/i.test(layer.src)) { note('Video backgrounds'); continue; }
        items.push({ kind: 'image', ...(layer.pos || fullBox), rotation: num(layer.rotation), src: layer.src, fit: layer.fit === 'fill' ? 'fill' : (layer.fit || 'contain'),
          opacity: layer.opacity != null ? layer.opacity : 100, gray: num(layer.grayscale) >= 50 });
        continue;
      }

      if (layer.type === 'text') {
        const isVerse = layer.binding === 'verse', isTr = layer.binding === 'verse_translated', isRef = layer.binding === 'reference';
        if (hasVerseLayer && (isRef || isTr) && !String(content.verseText || '').trim()) continue;
        let text;
        if (isVerse) text = content.verseText;
        else if (isTr) text = content.translatedText || '';
        else if (isRef) text = content.versionText && content.referenceText && layer.showVersion !== false ? `${content.referenceText} (${content.versionText})` : (content.referenceText || '');
        else if (layer.binding === 'custom' || !layer.binding) text = layer.customText || '';
        else { note('Live clocks and countdowns'); continue; }
        text = String(text == null ? '' : text);
        if (!text.trim()) continue;
        const f = layer.font || {};
        const family = (ov.font && ov.font.family) || f.family || 'Arial';
        const size = num(ov.font && ov.font.size != null ? ov.font.size : f.size, 48);
        const transform = (ov.font && ov.font.transform) || f.transform || 'none';
        if (transform === 'lowercase') text = text.toLowerCase();
        else if (transform === 'capitalize') text = text.replace(/\b(\p{L})/gu, (m) => m.toUpperCase());
        const box = ov.pos || layer.pos || { x: 100, y: 400, w: W - 200, h: 0 };
        const color = ov.color || layer.color;
        const opacity = ov.opacity != null ? ov.opacity : layer.opacity;
        fonts.add(family);
        items.push({ kind: 'text', x: box.x, y: box.y, w: box.w, h: num(box.h), rotation: num(layer.rotation), text, accent: layer.accentColor || null,
          font: { family, size, weight: num(ov.font && ov.font.weight != null ? ov.font.weight : f.weight, 400), italic: !!(ov.font && ov.font.italic != null ? ov.font.italic : f.italic),
            lineHeight: num(ov.font && ov.font.lineHeight != null ? ov.font.lineHeight : f.lineHeight, 1.15), letterSpacing: num(ov.font && ov.font.letterSpacing != null ? ov.font.letterSpacing : f.letterSpacing), caps: transform === 'uppercase' },
          align: ov.align || layer.align || 'left',
          fill: layer.colorFill === 'gradient' && !ov.color ? { kind: 'gradient', stops: gradientStops(layer), angle: layer.angle != null ? layer.angle : 135, opacity } : { kind: 'solid', color: parseColor(color, opacity) },
          shadow: ov.shadow || layer.shadow });
        continue;
      }

      if (layer.type === 'pp' || layer.type === 'motion' || layer.type === 'image-cycle') note('Imported ProPresenter elements and motion graphics');
    }
    // The colour behind everything: the first full-slide background (a gradient's middle stop).
    const base = (() => {
      for (const it of items) {
        if (it.kind !== 'shape' || it.w < W * 0.9 || it.h < H * 0.9 || !it.fill) continue;
        if (it.fill.kind === 'solid') return it.fill.color;
        const st = it.fill.stops || [];
        const mid = st[Math.floor((st.length - 1) / 2)];
        if (mid) return parseColor(mid.color);
      }
      return null;
    })();
    const mix = (c, under) => {
      const m = (a, b) => Math.round(parseInt(a, 16) * c.a + parseInt(b, 16) * (1 - c.a)).toString(16).padStart(2, '0');
      return { hex: (m(c.hex.slice(0, 2), under.hex.slice(0, 2)) + m(c.hex.slice(2, 4), under.hex.slice(2, 4)) + m(c.hex.slice(4, 6), under.hex.slice(4, 6))).toUpperCase(), a: 1 };
    };
    for (const it of items) {
      if (it.kind === 'text' && it.fill && it.fill.kind === 'solid' && it.fill.color && it.fill.color.a < 0.6 && base) it.fill = { kind: 'solid', color: mix(it.fill.color, base) };
    }
    return { items, notes: [...notes], fonts: [...fonts] };
  }

  // ── slide XML ───────────────────────────────────────────────────────────────────────────────────
  const xfrm = (x, y, w, h, rot) => `<a:xfrm${rot ? ` rot="${Math.round(((rot % 360) + 360) % 360 * 60000)}"` : ''}><a:off x="${emu(x)}" y="${emu(y)}"/><a:ext cx="${Math.max(1, emu(w))}" cy="${Math.max(1, emu(h))}"/></a:xfrm>`;

  function geometryXml(it) {
    const w = emu(it.w), h = emu(it.h);
    if (it.clip) {
      // A shape cut to its own outline: points as [x%, y%] of its box.
      const pts = it.clip.map(([px, py]) => `<a:pt x="${Math.round(clamp(num(px), -50, 150) / 100 * w)}" y="${Math.round(clamp(num(py), -50, 150) / 100 * h)}"/>`);
      return `<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="r" b="b"/><a:pathLst><a:path w="${w}" h="${h}"><a:moveTo>${pts[0]}</a:moveTo>${pts.slice(1).map(p => `<a:lnTo>${p}</a:lnTo>`).join('')}<a:close/></a:path></a:pathLst></a:custGeom>`;
    }
    const prst = (name, adj) => `<a:prstGeom prst="${name}"><a:avLst>${adj != null ? `<a:gd name="adj" fmla="val ${adj}"/>` : ''}</a:avLst></a:prstGeom>`;
    if (it.shape === 'ellipse') return prst('ellipse');
    if (it.shape === 'pill') return prst('roundRect', 50000);
    if (it.shape === 'triangle') return prst('triangle', 50000);
    if (it.shape === 'diamond') return prst('diamond');
    const r4 = it.corners || [it.radius, it.radius, it.radius, it.radius];
    if (r4.every(r => !r)) return prst('rect');
    if (r4.every(r => r === r4[0])) return prst('roundRect', Math.round(clamp(r4[0] / Math.max(1, Math.min(it.w, it.h)), 0, 0.5) * 100000));
    // Each corner its own rounding: a path that walks the edges and arcs the corners.
    const cap = Math.min(it.w, it.h) / 2;
    const [tl, tr, br, bl] = r4.map(r => emu(clamp(num(r), 0, cap)));
    const arc = (r, st) => (r > 0 ? `<a:arcTo wR="${r}" hR="${r}" stAng="${st}" swAng="5400000"/>` : '');
    return `<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="r" b="b"/><a:pathLst><a:path w="${w}" h="${h}"><a:moveTo><a:pt x="${tl}" y="0"/></a:moveTo>`
      + `<a:lnTo><a:pt x="${w - tr}" y="0"/></a:lnTo>${arc(tr, 16200000)}<a:lnTo><a:pt x="${w}" y="${h - br}"/></a:lnTo>${arc(br, 0)}`
      + `<a:lnTo><a:pt x="${bl}" y="${h}"/></a:lnTo>${arc(bl, 5400000)}<a:lnTo><a:pt x="0" y="${tl}"/></a:lnTo>${arc(tl, 10800000)}<a:close/></a:path></a:pathLst></a:custGeom>`;
  }

  function fillXml(f) {
    if (!f) return '<a:noFill/>';
    if (f.kind === 'gradient') return gradFill(f.stops, f.angle, f.opacity);
    return solidFill(f.color);
  }

  function shapeXml(it, id) {
    let line = '<a:ln><a:noFill/></a:ln>';
    if (it.stroke) {
      const c = parseColor(it.stroke.color || '#FFFFFF', it.stroke.alpha != null ? it.stroke.alpha * 100 : 100);
      line = `<a:ln w="${emu(num(it.stroke.width, 2))}">${solidFill(c)}</a:ln>`;
    }
    return `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="${esc(it.name || 'Shape ' + id)}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>`
      + `<p:spPr>${xfrm(it.x, it.y, it.w, it.h, it.rotation)}${geometryXml(it)}${fillXml(it.fill)}${line}${shadowXml(it.drop)}</p:spPr>`
      + `<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp>`;
  }

  // The words of a text layer, as paragraphs of runs; "*word*" shows in the accent colour.
  function runsOf(line, accent) {
    if (!accent || !/\*[^*\n]+\*/.test(line)) return [{ text: line }];
    const out = [];
    let last = 0, m;
    const re = /\*([^*\n]+)\*/g;
    while ((m = re.exec(line))) {
      if (m.index > last) out.push({ text: line.slice(last, m.index) });
      out.push({ text: m[1], color: accent });
      last = m.index + m[0].length;
    }
    if (last < line.length) out.push({ text: line.slice(last) });
    return out;
  }

  function textXml(it, id) {
    const f = it.font;
    const sz = Math.max(100, Math.round(f.size * 50));                  // hundredths of a point: 1 canvas px = 0.5 pt
    const lines = String(it.text).replace(/\r/g, '').split('\n');
    const algn = { center: 'ctr', right: 'r', justify: 'just' }[it.align] || 'l';
    const lnSpc = `<a:lnSpc><a:spcPts val="${Math.max(100, Math.round(f.lineHeight * f.size * 50))}"/></a:lnSpc>`;
    const shadow = shadowXml(it.shadow && it.shadow.enabled ? { ...it.shadow, opacity: it.shadow.opacity != null ? it.shadow.opacity : 60 } : null);
    // The layer's own opacity applies to an accent colour too.
    const baseAlpha = it.fill && it.fill.kind === 'solid' && it.fill.color ? it.fill.color.a : 1;
    const fillFor = (color) => {
      const c = color ? parseColor(color) : null;
      return c ? solidFill({ ...c, a: baseAlpha }) : fillXml(it.fill);
    };
    const rPr = (color) => `<a:rPr lang="en-US" sz="${sz}" b="${f.weight >= 600 ? 1 : 0}" i="${f.italic ? 1 : 0}"${f.letterSpacing ? ` spc="${Math.round(f.letterSpacing * 50)}"` : ''}${f.caps ? ' cap="all"' : ''} dirty="0">`
      + `${fillFor(color)}${shadow}<a:latin typeface="${esc(f.family)}"/><a:cs typeface="${esc(f.family)}"/></a:rPr>`;
    const paras = lines.map(line => {
      const runs = runsOf(line, it.accent).map(r => `<a:r>${rPr(r.color || null)}<a:t>${esc(r.text)}</a:t></a:r>`).join('');
      return `<a:p><a:pPr algn="${algn}">${lnSpc}</a:pPr>${runs || ''}<a:endParaRPr lang="en-US" sz="${sz}"/></a:p>`;
    }).join('');
    const auto = it.h <= 0;
    const h = auto ? Math.ceil(lines.length * f.lineHeight * f.size) : it.h;
    return `<p:sp><p:nvSpPr><p:cNvPr id="${id}" name="${esc(it.name || 'Text ' + id)}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr>`
      + `<p:spPr>${xfrm(it.x, it.y, it.w, h, it.rotation)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>`
      + `<p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0" rtlCol="0" anchor="${auto ? 't' : 'ctr'}">${auto ? '<a:spAutoFit/>' : '<a:noAutofit/>'}</a:bodyPr><a:lstStyle/>${paras}</p:txBody></p:sp>`;
  }

  // A picture, placed as its fit asks: stretched, cropped to fill, or shrunk to sit inside its box.
  function pictureXml(it, id, rId, img) {
    let { x, y, w, h } = it;
    let crop = '';
    const fit = it.fit;
    if (fit !== 'fill' && img && img.w > 0 && img.h > 0 && w > 0 && h > 0) {
      const ar = img.w / img.h, br = w / h;
      if (fit === 'cover') {
        const k = (v) => Math.round(clamp(v, 0, 0.9) * 100000);
        if (ar > br) { const s = (1 - br / ar) / 2; crop = `<a:srcRect l="${k(s)}" r="${k(s)}"/>`; }
        else if (ar < br) { const s = (1 - ar / br) / 2; crop = `<a:srcRect t="${k(s)}" b="${k(s)}"/>`; }
      } else if (ar > br) { const nh = w / ar; y += (h - nh) / 2; h = nh; }
      else { const nw = h * ar; x += (w - nw) / 2; w = nw; }
    }
    const alpha = it.opacity != null && it.opacity < 100 ? `<a:alphaModFix amt="${Math.round(clamp(it.opacity, 0, 100) * 1000)}"/>` : '';
    return `<p:pic><p:nvPicPr><p:cNvPr id="${id}" name="${esc(it.name || 'Picture ' + id)}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>`
      + `<p:blipFill><a:blip r:embed="${rId}">${it.gray ? '<a:grayscl/>' : ''}${alpha}</a:blip>${crop || ''}<a:stretch><a:fillRect/></a:stretch></p:blipFill>`
      + `<p:spPr>${xfrm(x, y, w, h, it.rotation)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>`;
  }

  // ── a soft glow as a picture ────────────────────────────────────────────────────────────────────
  // The glow's colour fading to nothing at its edge, as a small PNG the viewer scales up smoothly.
  // Drawn as a picture because radial gradients with a clear end are not honoured by every viewer.
  function adler32(b) { let a = 1, c = 0; for (let i = 0; i < b.length; i++) { a = (a + b[i]) % 65521; c = (c + a) % 65521; } return ((c << 16) | a) >>> 0; }
  function glowImage(key) {
    const m = /^glow:([0-9A-F]{6}):(\d+)$/.exec(key);
    if (!m) return null;
    const N = 96, r = parseInt(m[1].slice(0, 2), 16), g = parseInt(m[1].slice(2, 4), 16), b = parseInt(m[1].slice(4, 6), 16), a = clamp(+m[2], 0, 100) / 100;
    const raw = new Uint8Array((N * 4 + 1) * N);
    for (let y = 0; y < N; y++) {
      const row = y * (N * 4 + 1);
      for (let x = 0; x < N; x++) {
        const d = Math.min(1, Math.hypot((x + 0.5) / N - 0.5, (y + 0.5) / N - 0.5) * 2);
        const o = row + 1 + x * 4;
        raw[o] = r; raw[o + 1] = g; raw[o + 2] = b; raw[o + 3] = Math.round(255 * a * (1 - d));
      }
    }
    const blocks = [];
    for (let i = 0; i < raw.length; i += 65535) {
      const part = raw.subarray(i, Math.min(raw.length, i + 65535));
      const h = new Uint8Array(5);
      h[0] = i + 65535 >= raw.length ? 1 : 0; h[1] = part.length & 255; h[2] = part.length >> 8; h[3] = ~part.length & 255; h[4] = (~part.length >> 8) & 255;
      blocks.push(h, part);
    }
    const ad = adler32(raw);
    const z = [Uint8Array.from([0x78, 0x01]), ...blocks, Uint8Array.from([ad >>> 24, (ad >>> 16) & 255, (ad >>> 8) & 255, ad & 255])];
    const cat = (parts) => { const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let at = 0; for (const p of parts) { out.set(p, at); at += p.length; } return out; };
    const chunk = (type, data) => {
      const t = utf8(type), body = cat([t, data]), out = new Uint8Array(12 + data.length), v = new DataView(out.buffer);
      v.setUint32(0, data.length); out.set(body, 4); v.setUint32(8 + data.length, crc32(body));
      return out;
    };
    const ihdr = new Uint8Array(13), hv = new DataView(ihdr.buffer);
    hv.setUint32(0, N); hv.setUint32(4, N); ihdr[8] = 8; ihdr[9] = 6;
    return { bytes: cat([Uint8Array.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', cat(z)), chunk('IEND', new Uint8Array(0))]), ext: 'png', w: N, h: N };
  }

  // ── the package ─────────────────────────────────────────────────────────────────────────────────
  const THEME = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Kairo"><a:themeElements><a:clrScheme name="Kairo"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F1F24"/></a:dk2><a:lt2><a:srgbClr val="EDEDED"/></a:lt2><a:accent1><a:srgbClr val="F5301A"/></a:accent1><a:accent2><a:srgbClr val="FF9130"/></a:accent2><a:accent3><a:srgbClr val="8F8F97"/></a:accent3><a:accent4><a:srgbClr val="57575F"/></a:accent4><a:accent5><a:srgbClr val="22C55E"/></a:accent5><a:accent6><a:srgbClr val="3B82F6"/></a:accent6><a:hlink><a:srgbClr val="3B82F6"/></a:hlink><a:folHlink><a:srgbClr val="8F8F97"/></a:folHlink></a:clrScheme><a:fontScheme name="Kairo"><a:majorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Kairo"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="28575"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`;
  const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n';
  const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
  const rels = (list) => `${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${list.map(([id, type, target]) => `<Relationship Id="${id}" Type="${REL}/${type}" Target="${target}"/>`).join('')}</Relationships>`;

  // slides: [{ items }]; images: { [src]: { bytes: Uint8Array, ext: 'png'|'jpeg', w, h } }; opts: { title }.
  // Returns the .pptx's bytes.
  function buildPptx(slides, images, opts) {
    opts = opts || {};
    images = images || {};
    const files = [];
    const add = (name, data) => files.push({ name, data: typeof data === 'string' ? utf8(data) : data });
    const media = new Map();        // src → { name, ext }
    const slideXmls = slides.map((s, si) => {
      const slideRels = [['rId1', 'slideLayout', '../slideLayouts/slideLayout1.xml']];
      const have = new Map();       // src → rId in this slide
      let id = 1;
      const shapes = s.items.map(it => {
        id += 1;
        if (it.kind === 'text') return textXml(it, id);
        if (it.kind === 'image') {
          const img = images[it.src] || (it.src.indexOf('glow:') === 0 ? glowImage(it.src) : null);
          if (!img || !img.bytes) return '';
          if (!media.has(it.src)) { images[it.src] = img; media.set(it.src, { name: `image${media.size + 1}.${img.ext === 'jpeg' ? 'jpeg' : 'png'}`, ext: img.ext === 'jpeg' ? 'jpeg' : 'png' }); }
          if (!have.has(it.src)) { const rId = `rId${slideRels.length + 1}`; have.set(it.src, rId); slideRels.push([rId, 'image', `../media/${media.get(it.src).name}`]); }
          return pictureXml(it, id, have.get(it.src), img);
        }
        return shapeXml(it, id);
      }).join('');
      const bg = '<p:bg><p:bgPr><a:solidFill><a:srgbClr val="000000"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>';
      add(`ppt/slides/slide${si + 1}.xml`, `${XML}<p:sld ${NS}><p:cSld>${bg}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>${shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`);
      add(`ppt/slides/_rels/slide${si + 1}.xml.rels`, rels(slideRels));
      return si + 1;
    });
    for (const [src, m] of media) add(`ppt/media/${m.name}`, images[src].bytes);

    const n = slideXmls.length;
    const exts = [...new Set([...media.values()].map(m => m.ext))];
    add('[Content_Types].xml', `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${exts.map(e => `<Default Extension="${e}" ContentType="image/${e}"/>`).join('')}`
      + `<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>`
      + `<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>`
      + `<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>`
      + `<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>`
      + `<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>`
      + slideXmls.map(i => `<Override PartName="/ppt/slides/slide${i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>`).join('') + '</Types>');
    add('_rels/.rels', `${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${REL}/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="${REL}/extended-properties" Target="docProps/app.xml"/></Relationships>`);
    const now = new Date().toISOString().replace(/\.\d+Z$/, 'Z');
    add('docProps/core.xml', `${XML}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>${esc(opts.title || 'Kairo slides')}</dc:title><dc:creator>Kairo</dc:creator><dcterms:created xsi:type="dcterms:W3CDTF">${now}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">${now}</dcterms:modified></cp:coreProperties>`);
    add('docProps/app.xml', `${XML}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Kairo</Application><Slides>${n}</Slides></Properties>`);
    add('ppt/presentation.xml', `${XML}<p:presentation ${NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>${slideXmls.map(i => `<p:sldId id="${255 + i}" r:id="rId${i + 1}"/>`).join('')}</p:sldIdLst><p:sldSz cx="${emu(W)}" cy="${emu(H)}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>`);
    add('ppt/_rels/presentation.xml.rels', rels([['rId1', 'slideMaster', 'slideMasters/slideMaster1.xml'], ...slideXmls.map(i => [`rId${i + 1}`, 'slide', `slides/slide${i}.xml`]), [`rId${n + 2}`, 'theme', 'theme/theme1.xml']]));
    const tree = '<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree>';
    add('ppt/slideMasters/slideMaster1.xml', `${XML}<p:sldMaster ${NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>${tree}</p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>`);
    add('ppt/slideMasters/_rels/slideMaster1.xml.rels', rels([['rId1', 'slideLayout', '../slideLayouts/slideLayout1.xml'], ['rId2', 'theme', '../theme/theme1.xml']]));
    add('ppt/slideLayouts/slideLayout1.xml', `${XML}<p:sldLayout ${NS} type="blank" preserve="1"><p:cSld name="Blank">${tree}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>`);
    add('ppt/slideLayouts/_rels/slideLayout1.xml.rels', rels([['rId1', 'slideMaster', '../slideMasters/slideMaster1.xml']]));
    add('ppt/theme/theme1.xml', THEME);
    // [Content_Types].xml first, as Office writes it.
    files.unshift(files.splice(files.findIndex(f => f.name === '[Content_Types].xml'), 1)[0]);
    return zip(files);
  }

  // ── zip, stored ─────────────────────────────────────────────────────────────────────────────────
  const utf8 = (s) => (typeof TextEncoder === 'function' ? new TextEncoder().encode(s) : Uint8Array.from(Buffer.from(s, 'utf8')));
  let crcTable = null;
  function crc32(bytes) {
    if (!crcTable) { crcTable = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320 ^ (c >>> 1) : c >>> 1; crcTable[n] = c >>> 0; } }
    let c = 0xFFFFFFFF;
    for (let i = 0; i < bytes.length; i++) c = crcTable[(c ^ bytes[i]) & 255] ^ (c >>> 8);
    return (c ^ 0xFFFFFFFF) >>> 0;
  }
  function zip(files) {
    const parts = [], central = [];
    let offset = 0;
    const dosTime = 0x6000, dosDate = 0x5C21;       // a fixed time, so the same deck gives the same file
    for (const f of files) {
      const name = utf8(f.name), data = f.data, crc = crc32(data);
      const local = new Uint8Array(30 + name.length);
      const lv = new DataView(local.buffer);
      lv.setUint32(0, 0x04034b50, true); lv.setUint16(4, 20, true); lv.setUint16(6, 0x0800, true); lv.setUint16(8, 0, true);
      lv.setUint16(10, dosTime, true); lv.setUint16(12, dosDate, true); lv.setUint32(14, crc, true); lv.setUint32(18, data.length, true); lv.setUint32(22, data.length, true);
      lv.setUint16(26, name.length, true); lv.setUint16(28, 0, true); local.set(name, 30);
      parts.push(local, data);
      const cd = new Uint8Array(46 + name.length);
      const cv = new DataView(cd.buffer);
      cv.setUint32(0, 0x02014b50, true); cv.setUint16(4, 20, true); cv.setUint16(6, 20, true); cv.setUint16(8, 0x0800, true); cv.setUint16(10, 0, true);
      cv.setUint16(12, dosTime, true); cv.setUint16(14, dosDate, true); cv.setUint32(16, crc, true); cv.setUint32(20, data.length, true); cv.setUint32(24, data.length, true);
      cv.setUint16(28, name.length, true); cv.setUint32(42, offset, true); cd.set(name, 46);
      central.push(cd);
      offset += local.length + data.length;
    }
    const cdSize = central.reduce((n, c) => n + c.length, 0);
    const end = new Uint8Array(22);
    const ev = new DataView(end.buffer);
    ev.setUint32(0, 0x06054b50, true); ev.setUint16(8, files.length, true); ev.setUint16(10, files.length, true); ev.setUint32(12, cdSize, true); ev.setUint32(16, offset, true);
    const all = [...parts, ...central, end];
    const out = new Uint8Array(all.reduce((n, p) => n + p.length, 0));
    let at = 0;
    for (const p of all) { out.set(p, at); at += p.length; }
    return out;
  }

  const api = { itemsFromLayers, buildPptx, parseColor, crc32, zip };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.KairoPptx = api;
})(typeof window !== 'undefined' ? window : globalThis);
