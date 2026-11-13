// KAIRO — a deck as a ProPresenter 7 presentation (.pro).
//
// Pure: slides in, bytes out. A .pro is one protobuf message (rv.data.Presentation); the field numbers
// are the ones src-tauri/.../propresenter.rs reads (ProPresenter 21's own schema), and the layout of a
// slide (a cue holding one slide action, whose slide holds the elements) follows files ProPresenter
// itself wrote. Each Kairo slide becomes one slide in one group, so it opens as an ordinary song:
//   text      → a text element (font, size, colour, alignment, spacing, shadow, case) with its words as RTF
//   shapes    → rectangle / ellipse / rounded elements with a solid or gradient fill, stroke and shadow
// Pictures aren't carried (a .pro points at media files on disk), and blend-mode layers, frosted glass,
// glows and live clocks are left out or approximated, as in the PowerPoint export; `notes` says which.
//
// It takes the same drawing items as export_pptx.js (itemsFromLayers), so both exports show the same slide.
(function (root) {
  'use strict';

  const W = 1920, H = 1080;
  const num = (v, d = 0) => (Number.isFinite(Number(v)) ? Number(v) : d);
  const clamp = (n, lo, hi) => Math.max(lo, Math.min(hi, n));

  // ── protobuf writer ─────────────────────────────────────────────────────────────────────────────
  const enc = new TextEncoder();
  function varint(n) {
    const out = [];
    n = Math.max(0, Math.floor(n));
    while (n > 127) { out.push((n % 128) | 128); n = Math.floor(n / 128); }
    out.push(n);
    return out;
  }
  const cat = (parts) => {
    let len = 0;
    for (const p of parts) len += p.length;
    const out = new Uint8Array(len);
    let o = 0;
    for (const p of parts) { out.set(p, o); o += p.length; }
    return out;
  };
  const tag = (n, wire) => varint(n * 8 + wire);
  // Each helper returns the field's bytes; a message is cat([...fields]).
  const fVar = (n, v) => cat([tag(n, 0), varint(v)]);
  const fBool = (n, v) => fVar(n, v ? 1 : 0);
  const fDbl = (n, v) => { const b = new Uint8Array(8); new DataView(b.buffer).setFloat64(0, v, true); return cat([tag(n, 1), b]); };
  const fFlt = (n, v) => { const b = new Uint8Array(4); new DataView(b.buffer).setFloat32(0, v, true); return cat([tag(n, 5), b]); };
  const fBytes = (n, bytes) => cat([tag(n, 2), varint(bytes.length), bytes]);
  const fStr = (n, s) => fBytes(n, enc.encode(String(s)));
  const fMsg = (n, parts) => fBytes(n, cat(parts.filter(Boolean)));

  function uuid() {
    const b = new Uint8Array(16);
    (root.crypto || {}).getRandomValues ? root.crypto.getRandomValues(b) : b.forEach((_, i) => { b[i] = Math.floor(Math.random() * 256); });
    b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
    const h = [...b].map(x => x.toString(16).padStart(2, '0')).join('').toUpperCase();
    return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
  }
  const mUuid = (n, id) => fMsg(n, [fStr(1, id || uuid())]);

  // rv.data.Color: red, green, blue, alpha as floats 0–1.
  function mColor(n, c) {
    const h = (c && c.hex) || '000000';
    const ch = (i) => parseInt(h.slice(i, i + 2), 16) / 255;
    return fMsg(n, [fFlt(1, ch(0)), fFlt(2, ch(2)), fFlt(3, ch(4)), fFlt(4, c ? c.a : 1)]);
  }
  const mPoint = (n, x, y) => fMsg(n, [x ? fDbl(1, x) : null, y ? fDbl(2, y) : null]);

  // ── text as RTF ─────────────────────────────────────────────────────────────────────────────────
  // ProPresenter keeps a text element's words as Cocoa RTF: a font table, a colour table, one paragraph
  // style, then runs. Sizes are half-points (\fs204 = 102), so Kairo's canvas pixels carry over as they are.
  const rtfEsc = (s) => {
    let out = '';
    for (const ch of String(s)) {
      const cp = ch.codePointAt(0);
      if (ch === '\\' || ch === '{' || ch === '}') out += '\\' + ch;
      else if (ch === '\n') out += '\\\n';
      else if (cp > 126) { const u = cp > 0xFFFF ? [0xD800 + ((cp - 0x10000) >> 10), 0xDC00 + ((cp - 0x10000) & 0x3FF)] : [cp]; out += u.map(x => `\\uc0\\u${x > 32767 ? x - 65536 : x} `).join(''); }
      else out += ch;
    }
    return out;
  };
  // Words with *accent* marks → runs; the accent colour is a second colour in the table.
  function runsOf(text, accent) {
    if (!accent) return [{ t: text, accent: false }];
    const runs = [];
    const re = /\*([^*\n]+)\*/g;
    let last = 0, m;
    while ((m = re.exec(text))) {
      if (m.index > last) runs.push({ t: text.slice(last, m.index), accent: false });
      runs.push({ t: m[1], accent: true });
      last = re.lastIndex;
    }
    if (last < text.length) runs.push({ t: text.slice(last), accent: false });
    return runs.length ? runs : [{ t: text, accent: false }];
  }
  const rgb255 = (c) => [0, 2, 4].map(i => parseInt(c.hex.slice(i, i + 2), 16));
  function buildRtf(it, fontName, color, accentColor) {
    const [r, g, b] = rgb255(color);
    const sizeHalf = Math.round(num(it.font.size, 48) * 2);
    const align = { left: '\\ql', center: '\\qc', right: '\\qr', justify: '\\qj' }[it.align] || '\\ql';
    // Exact line height: font size × the layer's line-height, in twips (1/20 pt), negative = exact.
    const sl = -Math.round(num(it.font.size, 48) * num(it.font.lineHeight, 1.15) * 20);
    let colors = `\\red${r}\\green${g}\\blue${b};`;
    let expanded = `\\cssrgb\\c${Math.round(r / 255 * 100000)}\\c${Math.round(g / 255 * 100000)}\\c${Math.round(b / 255 * 100000)};`;
    const runs = runsOf(it.text, accentColor);
    if (accentColor && runs.some(x => x.accent)) {
      const [ar, ag, ab] = rgb255(accentColor);
      colors += `\\red${ar}\\green${ag}\\blue${ab};`;
      expanded += `\\cssrgb\\c${Math.round(ar / 255 * 100000)}\\c${Math.round(ag / 255 * 100000)}\\c${Math.round(ab / 255 * 100000)};`;
    }
    const kern = num(it.font.letterSpacing) ? `\\kerning1\\expnd${Math.round(num(it.font.letterSpacing) * 4)}\\expndtw${Math.round(num(it.font.letterSpacing) * 20)}` : '';
    const style = `\\f0${it.font.weight >= 600 ? '\\b' : ''}${it.font.italic ? '\\i' : ''}\\fs${sizeHalf} `;
    let body = '';
    runs.forEach((run) => { body += `${run.accent ? '\\cf2 ' : '\\cf1 '}${rtfEsc(run.t)}`; });
    return '{\\rtf1\\ansi\\ansicpg1252\\cocoartf2908\n\\cocoatextscaling0\\cocoaplatform0{\\fonttbl\\f0\\fswiss\\fcharset0 ' + rtfEsc(fontName) + ';}\n' +
      '{\\colortbl;' + colors + '}\n{\\*\\expandedcolortbl;' + expanded + '}\n\\deftab1680\n' +
      `\\pard\\pardeftab1680\\sl${sl}\\slmult0\\pardirnatural${align}\\partightenfactor0\n\n${style}${kern}${kern ? ' ' : ''}${body}}`;
  }

  // ── elements ────────────────────────────────────────────────────────────────────────────────────
  // The 4 corner points of an unrotated rectangle, in the unit square; ProPresenter keeps them on every shape.
  const pointMsg = (x, y) => fMsg(2, [fMsg(1, [x ? fDbl(1, x) : null, y ? fDbl(2, y) : null]), fMsg(2, []), fMsg(3, [])]);
  function pathMsg(typeNum, roundness) {
    const t = [fVar(1, typeNum)];
    if (typeNum === 11) t.push(fMsg(2, [fDbl(1, roundness)]));
    return fMsg(8, [fBool(1, true), pointMsg(0, 0), pointMsg(1, 0), pointMsg(1, 1), pointMsg(0, 1), fMsg(3, t)]);
  }
  const off = (enabled, key) => (enabled ? key : null);
  function shadowMsg(sh, n) {
    if (!sh || sh.enabled === false) {
      return fMsg(n, [fDbl(2, 315), fDbl(3, 5), fDbl(4, 5), mColor(5, { hex: '000000', a: 1 }), fDbl(6, 0.75)]);
    }
    const x = num(sh.x), y = num(sh.y);
    const c = (root.KairoPptx && root.KairoPptx.parseColor ? root.KairoPptx.parseColor(sh.color || '#000000', sh.opacity != null ? sh.opacity : 45) : { hex: '000000', a: 0.45 });
    // ProPresenter's angle runs counter-clockwise from "right"; screen y grows downward.
    const angle = (((-Math.atan2(y, x) * 180 / Math.PI) % 360) + 360) % 360;
    return fMsg(n, [fDbl(2, angle), fDbl(3, Math.hypot(x, y)), fDbl(4, num(sh.blur)), mColor(5, { hex: (c && c.hex) || '000000', a: 1 }), fDbl(6, c ? c.a : 0.45), fBool(7, true)]);
  }
  function elementBase(name, it, fillMsg, strokeMsg, shadow, pathM, textM) {
    // Bounds are the unrotated box; rotation is degrees, counter-clockwise.
    const rot = ((-num(it.rotation) % 360) + 360) % 360;
    return fMsg(1, [fMsg(1, [
      mUuid(1),
      fStr(2, name),
      fMsg(3, [mPoint(1, it.x, it.y), mPoint(2, it.w, it.h)]),
      rot ? fDbl(4, rot) : null,
      fDbl(5, clamp(it.opacity != null ? it.opacity / 100 : 1, 0, 1)),
      pathM,
      fillMsg,
      strokeMsg,
      shadowMsg(shadow, 11),
      fMsg(12, [fDbl(2, 0.05)]),
      textM,
      fMsg(14, []),
    ]), fVar(4, 3)]);
  }
  const fullBox = (it) => ({ x: it.x, y: it.y, w: Math.max(1, it.w), h: Math.max(1, it.h), rotation: it.rotation, opacity: it.opacity });

  function gradientMsg(fill) {
    const stops = (fill.stops || []).map((s) => {
      const c = root.KairoPptx.parseColor(s.color, fill.opacity);
      return c ? fMsg(4, [mColor(1, c), fDbl(2, clamp(num(s.pos), 0, 100) / 100), fDbl(3, 0.5)]) : null;
    }).filter(Boolean);
    // CSS runs clockwise from "up", ProPresenter counter-clockwise from "right".
    const angle = (((90 - num(fill.angle, 135)) % 360) + 360) % 360;
    return fMsg(2, [fVar(1, 0), fDbl(2, angle), fDbl(3, 1), ...stops]);
  }
  function shapeElement(it, notes) {
    const fill = it.fill;
    let fillM;
    // The fill message: colour (1) or gradient (2), switched on by field 4.
    if (fill && fill.kind === 'gradient') fillM = fMsg(9, [gradientMsg(fill), fBool(4, true)]);
    else if (fill && fill.color) fillM = fMsg(9, [mColor(1, fill.color), fBool(4, true)]);
    else fillM = fMsg(9, [mColor(1, { hex: '000000', a: 1 })]);
    const st = it.stroke;
    let strokeM = fMsg(10, [fDbl(2, 3), mColor(3, { hex: 'FFFFFF', a: 1 })]);
    if (st && num(st.width) > 0) {
      const c = root.KairoPptx.parseColor(st.color || '#FFFFFF', st.alpha != null ? st.alpha * 100 : (st.opacity != null ? st.opacity : 100)) || { hex: 'FFFFFF', a: 1 };
      strokeM = fMsg(10, [fDbl(2, num(st.width)), mColor(3, c), fBool(5, true)]);
    }
    let typeNum = 1, roundness = 0;
    if (it.shape === 'ellipse' || it.shape === 'circle') typeNum = 2;
    else if (num(it.radius) > 0 || (it.corners && it.corners.some(n => n > 0))) {
      typeNum = 11;
      const r = it.corners ? Math.max(...it.corners) : num(it.radius);
      roundness = clamp(r / Math.max(1, Math.min(it.w, it.h)), 0, 0.5);
    }
    if (it.clip) notes.add('Shapes cut to a custom outline (drawn as plain boxes)');
    return elementBase('Shape', fullBox(it), fillM, strokeM, it.drop, pathMsg(typeNum, roundness), null);
  }

  function textElement(it, notes) {
    const family = it.font.family || 'Arial';
    const bold = num(it.font.weight, 400) >= 600;
    const fontName = family.replace(/\s+/g, '') + (bold ? '-Bold' : '');
    const color = (it.fill && it.fill.kind === 'solid' && it.fill.color) || (it.fill && it.fill.stops && root.KairoPptx.parseColor(it.fill.stops[0].color)) || { hex: 'FFFFFF', a: 1 };
    if (it.fill && it.fill.kind === 'gradient') notes.add('Gradient text (shown in its first colour)');
    const accent = it.accent ? root.KairoPptx.parseColor(it.accent) : null;
    const fontMsg = fMsg(1, [fStr(1, fontName), fDbl(2, num(it.font.size, 48)), bold ? fBool(8, true) : null, it.font.italic ? fBool(4, true) : null, fStr(9, family)]);
    const alignNum = { left: 0, right: 1, center: 2, justify: 3 }[it.align] || 0;
    const attrs = fMsg(3, [
      fontMsg,
      it.font.caps ? fVar(2, 1) : null,
      mColor(3, color),
      fMsg(6, [fVar(1, alignNum), fDbl(5, 1), fDbl(7, 1)]),
      num(it.font.letterSpacing) ? fDbl(7, num(it.font.letterSpacing)) : null,
    ]);
    const textM = fMsg(13, [
      attrs,
      it.shadow && it.shadow.enabled !== false ? shadowMsg(it.shadow, 4) : fMsg(4, [fDbl(2, 315), fDbl(3, 5), fDbl(4, 5), mColor(5, { hex: '000000', a: 1 }), fDbl(6, 0.75)]),
      fStr(5, buildRtf(it, fontName, color, accent)),
      fVar(6, it.h > 0 ? 1 : 0),            // vertically centred in its box
      fMsg(8, []),
    ]);
    const noFill = fMsg(9, [mColor(1, { hex: '000000', a: 0 })]);
    const noStroke = fMsg(10, [fDbl(2, 3), mColor(3, { hex: 'FFFFFF', a: 1 })]);
    const box = fullBox({ ...it, h: it.h > 0 ? it.h : Math.round(num(it.font.size, 48) * num(it.font.lineHeight, 1.15) * (String(it.text).split('\n').length + 1)) });
    return elementBase('Text', box, noFill, noStroke, null, pathMsg(1, 0), textM);
  }

  // ── presentation ────────────────────────────────────────────────────────────────────────────────
  // slides: [{ items }]; images are not carried. Returns { bytes, notes }.
  function buildPro(slides, opts) {
    opts = opts || {};
    const notes = new Set();
    const groupId = uuid();
    const cueIds = slides.map(() => uuid());
    const cues = slides.map((sl, i) => {
      const elements = [];
      let skippedPicture = false;
      (sl.items || []).forEach((it) => {
        if (it.kind === 'shape') elements.push(shapeElement(it, notes));
        else if (it.kind === 'text') elements.push(textElement(it, notes));
        else if (it.kind === 'image') skippedPicture = true;
      });
      if (skippedPicture) notes.add('Pictures and picture backgrounds');
      // Elements are listed top first in a ProPresenter slide; Kairo's items run bottom first.
      elements.reverse();
      const base = fMsg(1, [
        ...elements,
        fBool(4, true),
        mColor(5, { hex: '000000', a: 1 }),
        fMsg(6, [fDbl(1, W), fDbl(2, H)]),
        mUuid(7),
      ]);
      const action = fMsg(10, [
        mUuid(1), fVar(6, 1), fVar(9, 11),
        fMsg(23, [fMsg(2, [base, fMsg(4, [fVar(3, 1)])])]),
      ]);
      return fMsg(13, [mUuid(1, cueIds[i]), fVar(5, 1), fMsg(8, []), action, fVar(12, 1)]);
    });
    const group = fMsg(12, [fMsg(1, [mUuid(1, groupId), fMsg(4, [])]), ...cueIds.map(id => fMsg(2, [fStr(1, id)]))]);
    const bytes = cat([
      fMsg(1, [fVar(1, 1), fMsg(2, [fVar(1, 27), fVar(2, 2)]), fVar(3, 1), fMsg(4, [fVar(1, 21), fVar(2, 4), fStr(4, '352583705')])]),
      mUuid(2),
      fStr(3, String(opts.title || 'Kairo slides')),
      fMsg(8, [fMsg(1, [fFlt(4, 1)])]),
      fMsg(9, [fVar(3, 1)]),
      group,
      ...cues,
      fMsg(14, []),
      fMsg(17, [fDbl(5, 300)]),
    ]);
    return { bytes, notes: [...notes] };
  }

  const api = { buildPro, buildRtf, uuid };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.KairoProPresenter = api;
})(typeof window !== 'undefined' ? window : globalThis);
