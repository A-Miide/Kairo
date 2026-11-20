// KAIRO — Lyric layouts that set the whole slide as one piece of type (text animations 'stacked', 'stacked-two-tone'
// and 'slide-in', dispatched from word_split.js's applyMotionText, so the Theme Studio canvas, Live Preview, the
// display output and thumbnails all draw them the same way).
//
// Block: the lines are re-broken (at the text's own punctuation first), every line is sized to fill the same width,
// so short lines run big and long ones small, and the lines are stacked one even gap apart, measured on the letters
// themselves. In capitals a short closing word may stand as tall as the two lines beside it ("SON." beside "THAT HE
// GAVE HIS / ONLY BEGOTTEN"). Several layouts are tried and scored on fixed rules (the smallest letters readable,
// the box filled, sizes that swing but not wildly, no line left on "the" or "and"); text too long to read as a
// poster falls back to one even size. The same words always lay out the same way. Two-tone puts the last part in
// the highlight colour.
//
// Slide in: the slide's own lines, each sized to the same width, come in from alternate sides and stop, still while
// the slide is up; at the next slide they carry on out the way they were going as the new ones come in.
//
// The layer's own font, case (its Case menu: capitals or as typed), colour and shadow are used as they are.
'use strict';

(function () {
const STOP = new Set(['a', 'an', 'the', 'and', 'or', 'but', 'of', 'to', 'in', 'on', 'at', 'for', 'is', 'was', 'by', 'with', 'as', 'that', 'his', 'her', 'my', 'thy', 'our', 'your', 'their', 'be', 'i', 'he', 'we', 'ye', 'o', 'from', 'unto', 'into', 'upon', 'not']);

const RULES = {
  gap: 0.13,       // the gap between two lines, a share of the bigger one's capital height
  minCap: 0.055,   // the smallest capitals allowed, a share of the box's height
  maxRatio: 3.2,   // the biggest line at most this many times the smallest
  maxLines: 6,
  tallMax: 5,      // letters in a closing word that may stand tall
};

const bare = (w) => w.toLowerCase().replace(/[^\p{L}\p{N}']/gu, '');

// Words coloured in the slide editor are written *like this* in the text (layer_geometry.js accentHtml). Here each
// such word carries a mark through the layout (invisible, never measured) and is drawn in the layer's accent colour.
const MARK = '\u0001';
// The words of a slide's last lyric line, as written: in a two-tone look they take the second colour wherever the
// layout breaks them (owner: "if I import in 2 lines, the second line stays yellow").
const LAST = '\u0002';
const marked = (text) => String(text || '').replace(/\*([^*\n]+)\*/g, (_, run) => run.split(/(\s+)/).map(w => (/\S/.test(w) ? MARK + w : w)).join(''));
const unmarked = (text) => text.split(MARK).join('').split(LAST).join('');
const isMarked = (w) => w.replace(LAST, '').startsWith(MARK);
const isLast = (w) => w.startsWith(LAST);

// Words, and how strongly the text may break after each: 3 at the end of a line or sentence, 2 at , ; :
function tokens(text) {
  const out = [];
  const lines = marked(text).split('\n').map(l => l.replace(/\s+/g, ' ').trim()).filter(Boolean);
  lines.forEach((line, li) => {
    const ws = line.split(' ').map(w => (lines.length > 1 && li === lines.length - 1 ? LAST + w : w));
    ws.forEach((w, i) => {
      const last = i === ws.length - 1;
      const brk = last ? 3 : /[.!?]["'’”]?$/.test(w) ? 3 : /[,;:]["'’”]?$/.test(w) ? 2 : 0;
      out.push({ w, brk, endLine: last && li < lines.length - 1 });
    });
  });
  return out;
}

// A line as shown: a comma, semicolon, colon or full stop ending it is dropped (it hung beside the next line).
function shown(words) {
  return words.map(x => x.w).join(' ').replace(/[,;:.]+(["'’”]?)$/, '$1');
}

function lineCost(ws, isLast, isFirst = false, calmTop = false) {
  const chars = ws.map(x => x.w).join(' ').length;
  const end = ws[ws.length - 1];
  let c = isLast ? 0 : end.brk === 3 ? -3 : end.brk === 2 ? -2 : 0.3;
  if (!isLast && STOP.has(bare(end.w))) c += 5;
  if (ws.length === 1 && STOP.has(bare(ws[0].w))) c += 8;
  // A line of only small words ("TO BE", "OF THE") would stand big for nothing.
  else if (ws.every(x => STOP.has(bare(x.w)))) c += 6;
  if (chars < 5) c += 3;
  if (chars > 22) c += (chars - 22) * 0.3; // a long line fitted to the width runs small
  // The block ends on a short line, so it runs big and the weight sits low (the reference ends on "TOUR"); a long
  // closing line made a staircase shrinking downwards.
  if (isLast && chars > 14) c += (chars - 14) * 0.35;
  // A second way of breaking, scored on balance against the first: no short (so huge) opening line.
  if (calmTop && isFirst && chars < 14) c += (14 - chars) * 0.5;
  return c;
}

// The best split of `toks` into exactly k lines.
function bestSplit(toks, k, calmTop = false) {
  const n = toks.length;
  if (k > n) return null;
  const INF = 1e9;
  const dp = Array.from({ length: k + 1 }, () => new Array(n + 1).fill(INF));
  const from = Array.from({ length: k + 1 }, () => new Array(n + 1).fill(-1));
  dp[0][0] = 0;
  for (let j = 1; j <= k; j++) {
    for (let e = j; e <= n; e++) {
      for (let s = j - 1; s < e; s++) {
        if (dp[j - 1][s] >= INF) continue;
        const c = dp[j - 1][s] + lineCost(toks.slice(s, e), j === k && e === n, j === 1, calmTop);
        if (c < dp[j][e]) { dp[j][e] = c; from[j][e] = s; }
      }
    }
  }
  if (dp[k][n] >= INF) return null;
  const cuts = [];
  for (let j = k, e = n; j > 0; j--) { const s = from[j][e]; cuts.unshift(toks.slice(s, e)); e = s; }
  return { cost: dp[k][n], lines: cuts };
}

const gapBetween = (capAbove, capBelow) => RULES.gap * Math.max(capAbove, capBelow);

// Sizes and heights of a stack of rows in a box `w` wide. `measure(text)` gives the ink of `text` at size 1:
// { w, l, up, down } (width, left bearing, height above and below the baseline) plus the font's own ascent/descent
// { fA, fD }. Rows: { kind:'line', text } or { kind:'tall', left:[a, b], big }.
// `uniform`: every line the one size at which the longest fits the width (each line then as wide as it is).
function geometry(rows, box, measure, { uniform = false } = {}) {
  const W = box.w;
  const one = uniform ? Math.min(...rows.filter(r => r.kind === 'line').map(r => W / measure(r.text).w)) : 0;
  const placed = rows.map(r => {
    if (r.kind === 'line') {
      const k = measure(r.text), s = uniform ? one : W / k.w;
      return { ...r, s, k, cap: s * k.up, capTop: s * k.up, capBottom: s * k.up, desc: s * k.down };
    }
    const kl = r.left.map(measure), kb = measure(r.big);
    const per = (kl[0].up + kl[0].down) / kl[0].w + RULES.gap * kl[1].up / kl[1].w + kl[1].up / kl[1].w;
    const colGap = 0.035 * W;
    const Lw = (W - colGap) / (1 + (per / kb.up) * kb.w);
    const sl = kl.map(k => Lw / k.w), h = per * Lw, sb = h / kb.up;
    return { ...r, sl, sb, Lw, colGap, kl, kb, cap: h, capTop: sl[0] * kl[0].up, capBottom: sl[1] * kl[1].up, desc: Math.max(sl[1] * kl[1].down, sb * kb.down) };
  });
  let H = 0;
  placed.forEach((p, i) => { H += p.cap + p.desc + (i ? gapBetween(placed[i - 1].capBottom, p.capTop) : 0); });
  const scale = Math.min(1, box.h / H);
  const caps = placed.flatMap(p => p.kind === 'line' ? [p.cap] : [p.sl[0] * p.kl[0].up, p.sl[1] * p.kl[1].up, p.cap]).map(c => c * scale);
  return { placed, H, scale, W, minCap: Math.min(...caps), maxCap: Math.max(...caps) };
}

function score(g, cost, box) {
  const rows = g.placed.reduce((n, p) => n + (p.kind === 'tall' ? 2 : 1), 0);
  const ratio = g.maxCap / g.minCap;
  const min = RULES.minCap * box.h;
  let s = -cost;
  s += 8 * (g.H * g.scale) / box.h;                       // fill the box
  s += 5 * Math.min(1, g.minCap / (0.09 * box.h));        // bigger smallest letters
  if (rows > 4) s -= 1.5 * (rows - 4);                     // not too many lines
  if (g.scale < 0.8) s -= 8 * (0.8 - g.scale);             // a narrow tower wastes the width
  if (ratio >= 1.25 && ratio <= RULES.maxRatio) s += 2;    // sizes swing...
  if (ratio > RULES.maxRatio) s -= 4 * (ratio - RULES.maxRatio); // ...but not wildly
  if (g.minCap < min) s -= 40 * (min - g.minCap) / min;    // every line readable
  // Balance (owner, 19 Nov 2026: "top heavy, lacks balance" — FOR GOD huge over three smaller lines). A line's weight
  // grows with the square of its size; where the weight sits, from 0 (top) to 1 (bottom), should be the middle or
  // lower, as in the reference: big lines at both ends or at the foot, never one big line crowning small ones.
  let y = 0, sum = 0, at = 0;
  g.placed.forEach((p, i) => {
    if (i) y += gapBetween(g.placed[i - 1].capBottom, p.capTop);
    const wgt = p.cap * p.cap;
    sum += wgt; at += wgt * (y + p.cap / 2);
    y += p.cap + p.desc;
  });
  const centre = sum ? at / sum / g.H : 0.5;
  if (centre < 0.5) s -= 40 * (0.5 - centre);
  return s;
}

// The block layout for `text` in a box { w, h }: the best-scoring stack, or { plain: true } when no stack keeps every
// line readable. `caps`: the text is shown in capitals (a closing word may then stand tall).
function planBlock(text, box, measure, { caps = false } = {}) {
  const toks = tokens(text);
  if (!toks.length) return null;
  const up = (t) => caps ? t.toUpperCase() : t;
  let best = null;
  const splits = [];
  for (let k = 1; k <= Math.min(RULES.maxLines, toks.length); k++) splits.push(bestSplit(toks, k), bestSplit(toks, k, true));
  for (const sp of splits) {
    if (!sp) continue;
    const rows = sp.lines.map(ws => ({ kind: 'line', text: up(shown(ws)) }));
    const cands = [{ rows, cost: sp.cost }];
    const last = sp.lines[sp.lines.length - 1], prev = sp.lines[sp.lines.length - 2];
    const word = bare(last[last.length - 1].w);
    if (caps && prev && prev.length >= 2 && last.length >= 3 && word.length <= RULES.tallMax && !STOP.has(word)) {
      const tall = { kind: 'tall', left: [up(shown(prev)), up(shown(last.slice(0, -1)))], big: up(shown([last[last.length - 1]])) };
      cands.push({ rows: [...rows.slice(0, -2), tall], cost: sp.cost - 1, tall: true });
    }
    for (const c of cands) {
      const g = geometry(c.rows, box, measure);
      const sc = score(g, c.cost, box);
      if (!best || sc > best.score) best = { ...c, g, score: sc };
    }
  }
  if (!best || best.g.minCap < RULES.minCap * box.h * 0.999) return { plain: true };
  return best;
}

// ── Drawing (browser only) ──────────────────────────────────────────────────────────────────────────────────────
// Its own styles, added once: the display window keeps a stylesheet of its own, and these must look the same there.
const CSS = `
.kairo-lb, .kairo-si { position: absolute; inset: 0; }
.kairo-lb-line { position: absolute; white-space: nowrap; line-height: 1; }
.kairo-lb-plain { position: absolute; left: 0; right: 0; text-align: center; line-height: 1.14; }
.kairo-lb-w { display: inline-block; animation: kairo-lb-in 360ms cubic-bezier(.2,.8,.2,1) both; }
.kairo-still .kairo-lb-w { animation: none; }
.kairo-grad .kairo-lb-w { display: inline; animation-name: kairo-lb-fade; }
@keyframes kairo-lb-fade { from { opacity: 0; } to { opacity: 1; } }
@keyframes kairo-lb-in { 0% { opacity: 0; transform: translateY(16px) scale(.5); } 70% { opacity: 1; transform: translateY(0) scale(1.08); } 100% { opacity: 1; transform: none; } }
`;
// Draws again whenever the box changes size (a resized editor, a window moved to another screen), and on the
// layer's font loading (the first measure may be a fallback face).
function repaintOnChange(container, wrap, paint) {
  // fonts.ready can settle before the layer's face is even asked for (a face loads when first used), and lines
  // measured with the stand-in face came out about 6% too wide. So the first draw on the page (only there is the
  // layer's font known) loads that face, and draws again once it is in.
  let fontAsked = false;
  const first = () => {
    paint();
    if (fontAsked || !container.isConnected) return;
    const f = fontOf(container);
    if (!f.family || !document.fonts?.load) return;
    fontAsked = true;
    document.fonts.load(`${f.style} ${f.weight} 100px ${f.family}`, 'A').then(() => requestAnimationFrame(paint)).catch(() => {});
  };
  requestAnimationFrame(first);
  document.fonts?.ready?.then(() => requestAnimationFrame(first));
  if (typeof ResizeObserver === 'undefined') return;
  let last = '';
  const ro = new ResizeObserver(() => {
    if (!wrap.isConnected) { ro.disconnect(); return; }
    const size = `${container.clientWidth}x${container.clientHeight}`;
    if (size !== last) { last = size; requestAnimationFrame(first); }
  });
  ro.observe(container);
}

function ensureStyles() {
  if (document.getElementById('kairo-lyric-layout-css')) return;
  const st = document.createElement('style');
  st.id = 'kairo-lyric-layout-css';
  st.textContent = CSS;
  document.head.appendChild(st);
}

let canvas = null;
function measurer(font) {
  canvas = canvas || document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  const cache = new Map();
  return (marked) => {
    if (cache.has(marked)) return cache.get(marked);
    const text = unmarked(marked);
    ctx.font = `${font.style} ${font.weight} 100px ${font.family}`;
    const t = ctx.measureText(text);
    const letters = ctx.measureText(text.replace(/[^\p{L}\p{N}\s]/gu, '') || text);
    const r = { w: Math.max(1e-3, (t.actualBoundingBoxLeft + t.actualBoundingBoxRight) / 100), l: t.actualBoundingBoxLeft / 100,
      up: letters.actualBoundingBoxAscent / 100, down: letters.actualBoundingBoxDescent / 100,
      fA: t.fontBoundingBoxAscent / 100, fD: t.fontBoundingBoxDescent / 100 };
    cache.set(marked, r);
    return r;
  };
}

function fontOf(el) {
  const cs = getComputedStyle(el);
  return { style: cs.fontStyle, weight: cs.fontWeight, family: cs.fontFamily, caps: cs.textTransform === 'uppercase' };
}

// A layer filled with a gradient paints it on its own box, clipped to its letters; these lines are boxes of their own,
// so the layer showed nothing. Each line takes the gradient instead.
function gradientOf(el) {
  const cs = getComputedStyle(el);
  const clip = cs.webkitBackgroundClip || cs.backgroundClip;
  return clip === 'text' && cs.backgroundImage && cs.backgroundImage !== 'none' ? cs.backgroundImage : null;
}
function fill(line, gradient, color) {
  if (color) { line.style.color = color; line.style.backgroundImage = 'none'; return; }
  if (!gradient) return;
  line.style.backgroundImage = gradient;
  line.style.webkitBackgroundClip = 'text';
  line.style.backgroundClip = 'text';
  line.style.color = 'transparent';
  line.classList.add('kairo-grad'); // its words in the line's own text, so the clip reaches them
  // A text shadow on gradient-filled letters is painted over the gradient (it read as a dark smudge): the layer's first
  // shadow goes underneath the letters instead.
  const shadow = getComputedStyle(line).textShadow;
  const first = shadow && shadow !== 'none' ? shadow.split(/,(?![^(]*\))/)[0].trim() : '';
  const m = /^(rgba?\([^)]*\)|#[0-9a-f]+|\w+)\s+(.+)$/i.exec(first);
  line.style.textShadow = 'none';
  if (m) line.style.filter = `drop-shadow(${m[2].split(/\s+/).slice(0, 3).join(' ')} ${m[1]})`;
}

// Where a piece of text goes so its ink's top-left is at (x, y), at `size` px.
function placeText(parent, text, x, y, size, k, cls, delay, color, wordColor, gradient, lastColor) {
  const d = document.createElement('div');
  d.className = 'kairo-lb-line';
  text.split(' ').forEach((w, j) => {
    if (j) d.appendChild(document.createTextNode(' '));
    const s = document.createElement('span');
    s.className = cls; s.textContent = unmarked(w);
    if (isMarked(w) && wordColor) s.style.color = wordColor;
    else if (isLast(w) && lastColor) s.style.color = lastColor;
    s.style.animationDelay = `${delay.next()}ms`;
    d.appendChild(s);
  });
  d.style.fontSize = size + 'px';
  const base = size * (1 - k.fA - k.fD) / 2 + k.fA * size;
  d.style.left = (x + k.l * size) + 'px';
  d.style.top = (y - (base - k.up * size)) + 'px';
  parent.appendChild(d);
  fill(d, gradient, color); // once on the page: the shadow it moves is read from the layer
}

// The same words drawn again a moment later are an edit (a colour, a size changed in Theme Studio) or a repaint, not a
// new slide: they change in place instead of playing their entrance again, which hid every change while editing.
let lastDrawn = { text: null, at: 0 };
function redrawOfSame(text) {
  const same = lastDrawn.text === text && Date.now() - lastDrawn.at < 1500;
  lastDrawn = { text, at: Date.now() };
  return same;
}

function renderBlock(container, text, speedMultiplier = 1, opts = {}) {
  ensureStyles();
  container.innerHTML = '';
  const wrap = document.createElement('div');
  wrap.className = 'kairo-lb' + (opts.still || redrawOfSame(text) || opts.quiet ? ' kairo-still' : '');
  container.appendChild(wrap);
  const paint = () => {
    if (!wrap.isConnected) return;
    const w = container.clientWidth, h = container.clientHeight;
    if (w < 10 || h < 10) return;
    wrap.innerHTML = '';
    const font = fontOf(container);
    const res = planBlock(text, { w, h }, measurer(font), { caps: font.caps });
    const gradient = gradientOf(container);
    if (!res) return;
    const step = Math.max(30, Math.round(90 * speedMultiplier));
    let n = 0;
    const delay = { next: () => n++ * step };
    if (res.plain) {
      const p = document.createElement('div');
      p.className = 'kairo-lb-plain';
      fill(p, gradient, null);
      p.textContent = unmarked(marked(text)).split('\n').map(l => l.trim()).filter(Boolean).join(' ');
      wrap.appendChild(p);
      let lo = 8, hi = Math.max(16, h);
      while (hi - lo > 1) { const m = (lo + hi) >> 1; p.style.fontSize = m + 'px'; (p.offsetHeight <= h && p.scrollWidth <= w + 1) ? (lo = m) : (hi = m); }
      p.style.fontSize = lo + 'px';
      p.style.top = ((h - p.offsetHeight) / 2) + 'px';
      return;
    }
    const g = res.g, k0 = g.scale, Wd = g.W * k0;
    // Two-tone: the last lyric line's own words (marked LAST) when the slide has several lines; one line (a verse):
    // its closing rows.
    const byLine = String(text).split('\n').filter(l => l.trim()).length > 1;
    const accentFrom = opts.accent && !byLine ? Math.max(0, g.placed.length - Math.max(1, Math.round(g.placed.length / 3))) : Infinity;
    const lastColor = opts.accent && byLine ? opts.accent : null;
    let y = (h - g.H * k0) / 2;
    const x0 = (w - Wd) / 2;
    g.placed.forEach((p, i) => {
      const color = i >= accentFrom ? opts.accent : null;
      if (i) y += gapBetween(g.placed[i - 1].capBottom, p.capTop) * k0;
      if (p.kind === 'line') {
        placeText(wrap, p.text, x0, y, p.s * k0, p.k, 'kairo-lb-w', delay, color, opts.wordColor, gradient, lastColor);
      } else {
        const s0 = p.sl[0] * k0, s1 = p.sl[1] * k0;
        placeText(wrap, p.left[0], x0, y, s0, p.kl[0], 'kairo-lb-w', delay, color, opts.wordColor, gradient, lastColor);
        placeText(wrap, p.left[1], x0, y + (p.kl[0].up + p.kl[0].down) * s0 + RULES.gap * p.kl[1].up * s1, s1, p.kl[1], 'kairo-lb-w', delay, color, opts.wordColor, gradient, lastColor);
        placeText(wrap, p.big, x0 + (p.Lw + p.colGap) * k0, y, p.sb * k0, p.kb, 'kairo-lb-w', delay, color, opts.wordColor, gradient, lastColor);
      }
      y += (p.cap + p.desc) * k0;
    });
  };
  repaintOnChange(container, wrap, paint);
  return wrap.querySelectorAll('.kairo-lb-w');
}

// The rows of a sliding slide: its own lines, or a single long line (a verse) split into up to four.
function slideRows(text, maxRows = 4) {
  const lines = marked(text).split('\n').map(l => l.replace(/\s+/g, ' ').trim()).filter(Boolean);
  if (lines.length !== 1) {
    const rows = lines.slice(0, maxRows).map(l => l.replace(/[,;:.]+$/, ''));
    return rows.map((r, i) => (rows.length > 1 && i === rows.length - 1 ? r.split(' ').map(w => LAST + w).join(' ') : r));
  }
  const toks = tokens(lines[0]);
  const k = Math.min(maxRows, Math.max(1, Math.ceil(lines[0].length / 28)), toks.length);
  const sp = bestSplit(toks, k);
  return (sp ? sp.lines.map(shown) : lines);
}

// The slide-in layout: the slide's own lines kept, all one size (the longest fits the width), centred; when that leaves
// the letters too small to read, the slide's words are re-broken into up to four even lines.
function planSlide(text, box, measure, { caps = false } = {}) {
  const rows = slideRows(text).map(t => ({ kind: 'line', text: caps ? t.toUpperCase() : t }));
  if (!rows.length) return null;
  const g = geometry(rows, box, measure, { uniform: true });
  if (g.minCap >= RULES.minCap * box.h) return { rows, g };
  const toks = tokens(text);
  for (let k = Math.min(4, toks.length); k >= 1; k--) {
    const sp = bestSplit(toks, k);
    if (!sp) continue;
    const re = sp.lines.map(ws => ({ kind: 'line', text: caps ? shown(ws).toUpperCase() : shown(ws) }));
    const g2 = geometry(re, box, measure, { uniform: true });
    if (g2.minCap >= RULES.minCap * box.h) return { rows: re, g: g2 };
  }
  return { plain: true };
}

// Slide in: the lines come in from alternate sides (the first from the right), stop, and stay still while the slide is
// up; at the next slide they carry on out the way they were going while the new lines come in. The motion is the
// change of slide, so it keeps time with it on every output.
// The last slide drawn in this window: an output builds a new text box for each slide, so the old lines are gone by
// the time the new ones come; a copy of them goes out instead. (Same text again, as in thumbnails, isn't a change.)
let lastSlide = null;
function renderSlideIn(container, text, speedMultiplier = 1, opts = {}) {
  ensureStyles();
  // A thumbnail neither animates nor counts as the last slide shown (the next slide on screen goes out from that).
  const quiet = opts.still || redrawOfSame(text) || !!opts.quiet;
  let old = quiet ? null : container.querySelector(':scope > .kairo-si:not(.kairo-si-out)');
  if (!old && !quiet && lastSlide && !lastSlide.wrap.isConnected && lastSlide.text !== text
      && lastSlide.w === container.clientWidth && lastSlide.h === container.clientHeight) {
    old = lastSlide.wrap.cloneNode(true);
  }
  container.innerHTML = '';
  const dur = Math.round(650 * speedMultiplier), stagger = Math.round(90 * speedMultiplier);
  if (old && old.children.length && old.animate) {
    old.classList.add('kairo-si-out');
    container.appendChild(old);
    const w = container.clientWidth || 1920;
    [...old.children].forEach((row, i) => {
      const dir = Number(row.dataset.dir) || -1;
      const a = row.animate([{ transform: 'none', opacity: 1 }, { transform: `translateX(${dir * w * 1.1}px)`, opacity: 0 }],
        { duration: dur, delay: i * stagger, easing: 'cubic-bezier(.5,0,.75,0)', fill: 'forwards' });
      if (i === old.children.length - 1) a.onfinish = () => old.remove();
    });
  }
  const wrap = document.createElement('div');
  wrap.className = 'kairo-si';
  container.appendChild(wrap);
  let entered = false;
  let rows = [];
  const paint = () => {
    if (!wrap.isConnected) return;
    const w = container.clientWidth, h = container.clientHeight;
    if (w < 10 || h < 10) return;
    const font = fontOf(container);
    const res = planSlide(text, { w, h }, measurer(font), { caps: font.caps });
    if (!res || res.plain) { renderBlock(container, text, speedMultiplier, opts); return; }
    // Laid out again on a resize or once the font loads: the same lines move, so an entrance under way carries on.
    if (rows.length !== res.g.placed.length) {
      wrap.innerHTML = '';
      rows = res.g.placed.map(() => { const r = document.createElement('div'); r.className = 'kairo-lb-line'; wrap.appendChild(r); return r; });
    }
    const g = res.g, k0 = g.scale;
    let y = (h - g.H * k0) / 2;
    const x0 = (w - g.W * k0) / 2;
    g.placed.forEach((p, i) => {
      if (i) y += gapBetween(g.placed[i - 1].capBottom, p.capTop) * k0;
      const row = rows[i];
      const size = p.s * k0, k = p.k;
      row.textContent = '';
      p.text.split(' ').forEach((wd, j) => {
        if (j) row.append(' ');
        const sp = document.createElement('span');
        sp.textContent = unmarked(wd);
        if (isMarked(wd) && opts.wordColor) sp.style.color = opts.wordColor;
        else if (isLast(wd) && opts.accent) sp.style.color = opts.accent;
        row.append(sp);
      });
      row.style.fontSize = size + 'px';
      row.style.color = '';
      const marksLast = g.placed.some(q => q.text.includes(LAST));
      fill(row, gradientOf(container), opts.accent && !marksLast && i === g.placed.length - 1 ? opts.accent : null);
      const base = size * (1 - k.fA - k.fD) / 2 + k.fA * size;
      row.style.left = ((w - k.w * size) / 2 + k.l * size) + 'px'; // centred
      row.style.top = (y - (base - k.up * size)) + 'px';
      row.dataset.dir = i % 2 ? '1' : '-1'; // the way it travels: in from the right going left, or from the left going right
      y += (p.cap + p.desc) * k0;
    });
    if (opts.still) return;
    lastSlide = { wrap, text, w, h };
    if (entered || quiet || !wrap.animate) return;
    entered = true;
    rows.forEach((row, i) => {
      const dir = Number(row.dataset.dir);
      row.animate([{ transform: `translateX(${-dir * w * 1.1}px)`, opacity: 0 }, { transform: 'none', opacity: 1 }],
        { duration: dur, delay: (old ? dur * 0.6 : 0) + i * stagger, easing: 'cubic-bezier(.2,.8,.2,1)', fill: 'backwards' });
    });
  };
  repaintOnChange(container, wrap, paint);
  return wrap.children;
}

const api = { planBlock, planSlide, slideRows, tokens, bestSplit, renderBlock, renderSlideIn, RULES };
if (typeof module !== 'undefined' && module.exports) module.exports = api;
if (typeof window !== 'undefined') window.KairoLyricLayout = api;
})();
