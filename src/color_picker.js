// KAIRO — the colour picker. Every colour swatch in the app (an <input type="color">) opens this
// instead of the system's own: a shade square, a hue bar, the colour's hex code and its R, G and B to
// type, an eyedropper where the system has one, and the last colours used. It writes the swatch's
// value and fires the swatch's own input (while choosing) and change (when done) events, so whatever
// listens to a swatch works unchanged. A swatch with data-native-picker keeps the system picker.
(function () {
  'use strict';
  const RECENT_KEY = 'kairo-recent-colours';
  const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
  const parseHex = (v) => {
    const m = String(v || '').trim().replace(/^#/, '').match(/^([0-9a-f]{3}|[0-9a-f]{6})$/i);
    if (!m) return null;
    return '#' + (m[1].length === 3 ? m[1].split('').map(c => c + c).join('') : m[1]).toLowerCase();
  };
  const hexToRgb = (h) => { const n = parseInt(h.slice(1), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
  const rgbToHex = (r, g, b) => '#' + [r, g, b].map(v => clamp(Math.round(v), 0, 255).toString(16).padStart(2, '0')).join('');
  function rgbToHsv(r, g, b) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), d = max - Math.min(r, g, b);
    let h = 0;
    if (d) h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return [(h * 60 + 360) % 360, max ? d / max : 0, max];
  }
  function hsvToRgb(h, s, v) {
    const f = (n) => { const k = (n + h / 60) % 6; return (v - v * s * Math.max(0, Math.min(k, 4 - k, 1))) * 255; };
    return [f(5), f(3), f(1)];
  }
  const recent = () => { try { return JSON.parse(localStorage.getItem(RECENT_KEY) || '[]'); } catch { return []; } };
  const remember = (hex) => { try { localStorage.setItem(RECENT_KEY, JSON.stringify([hex, ...recent().filter(x => x !== hex)].slice(0, 10))); } catch { /* not kept */ } };

  let pop = null, swatch = null, hsv = [0, 0, 0], startHex = '', parts = null;
  const hexNow = () => rgbToHex(...hsvToRgb(...hsv));

  function emit(type) {
    if (!swatch) return;
    swatch.value = hexNow();
    swatch.dispatchEvent(new Event(type, { bubbles: true }));
  }

  // Draws the panel from the current colour; `skip` is the field being typed in, left as it is.
  function paint(skip) {
    const { sv, svThumb, hue, hueThumb, preview, hexIn, rgbIn } = parts;
    sv.style.background = `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, transparent), hsl(${hsv[0]}, 100%, 50%)`;
    svThumb.style.left = `${hsv[1] * 100}%`;
    svThumb.style.top = `${(1 - hsv[2]) * 100}%`;
    hueThumb.style.left = `${(hsv[0] / 360) * 100}%`;
    const hex = hexNow();
    svThumb.style.background = hex;
    preview.style.background = hex;
    if (skip !== hexIn) { hexIn.value = hex.slice(1).toUpperCase(); hexIn.classList.remove('is-invalid'); }
    hexToRgb(hex).forEach((v, i) => { if (skip !== rgbIn[i]) rgbIn[i].value = v; });
    hue.setAttribute('aria-valuenow', String(Math.round(hsv[0])));
  }

  function setHex(hex, skip) {
    const [h, s, v] = rgbToHsv(...hexToRgb(hex));
    hsv = [s ? h : hsv[0], s, v];      // a grey keeps the hue it had, so the bar doesn't jump
    paint(skip);
    emit('input');
  }

  // Drag on the square or the bar: follows the pointer, input while moving, change on letting go.
  function drag(el, onMove) {
    el.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      el.setPointerCapture(e.pointerId);
      const move = (ev) => { const r = el.getBoundingClientRect(); onMove(clamp((ev.clientX - r.left) / r.width, 0, 1), clamp((ev.clientY - r.top) / r.height, 0, 1)); paint(); emit('input'); };
      const up = () => { el.removeEventListener('pointermove', move); el.removeEventListener('pointerup', up); emit('change'); };
      el.addEventListener('pointermove', move);
      el.addEventListener('pointerup', up);
      move(e);
    });
  }

  function build() {
    const el = (tag, cls, attrs = {}) => Object.assign(document.createElement(tag), cls ? { className: cls } : {}, attrs);
    pop = el('div', 'kcp');
    pop.setAttribute('role', 'dialog');
    pop.setAttribute('aria-label', 'Colour');
    const sv = el('div', 'kcp-sv');
    const svThumb = el('div', 'kcp-thumb');
    sv.appendChild(svThumb);
    sv.tabIndex = 0;
    sv.setAttribute('aria-label', 'Shade: arrow keys change it');
    const row = el('div', 'kcp-row');
    const preview = el('div', 'kcp-preview');
    const hue = el('div', 'kcp-hue');
    const hueThumb = el('div', 'kcp-hue-thumb');
    hue.appendChild(hueThumb);
    hue.tabIndex = 0;
    hue.setAttribute('role', 'slider');
    hue.setAttribute('aria-label', 'Hue');
    hue.setAttribute('aria-valuemin', '0');
    hue.setAttribute('aria-valuemax', '360');
    row.append(preview, hue);
    if ('EyeDropper' in window) {
      const pick = el('button', 'kcp-dropper', { type: 'button', title: 'Pick a colour from the screen' });
      pick.setAttribute('aria-label', 'Pick a colour from the screen');
      pick.innerHTML = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m2 22 1-1h3l9-9"/><path d="M3 21v-3l9-9"/><path d="m15 6 3.4-3.4a2.1 2.1 0 1 1 3 3L18 9l.4.4a2.1 2.1 0 1 1-3 3l-3.8-3.8a2.1 2.1 0 1 1 3-3l.4.4Z"/></svg>';
      pick.addEventListener('click', async () => {
        try { const r = await new window.EyeDropper().open(); const hex = parseHex(r.sRGBHex); if (hex) { setHex(hex); emit('change'); } } catch { /* cancelled */ }
      });
      row.appendChild(pick);
    }
    const fields = el('div', 'kcp-fields');
    const hexWrap = el('label', 'kcp-field kcp-hex');
    const hexIn = el('input', '', { type: 'text', maxLength: 7, spellcheck: false, autocomplete: 'off' });
    hexIn.setAttribute('aria-label', 'Hex code');
    hexWrap.append(el('span', '', { textContent: '#' }), hexIn);
    fields.appendChild(hexWrap);
    const rgbIn = ['R', 'G', 'B'].map(ch => {
      const wrap = el('label', 'kcp-field');
      const inp = el('input', '', { type: 'text', inputMode: 'numeric', maxLength: 3 });
      inp.setAttribute('aria-label', { R: 'Red', G: 'Green', B: 'Blue' }[ch]);
      wrap.append(inp);
      fields.appendChild(wrap);
      return inp;
    });
    const labels = el('div', 'kcp-labels');
    ['Hex', 'R', 'G', 'B'].forEach(t => labels.appendChild(el('span', '', { textContent: t })));
    const recentRow = el('div', 'kcp-recent');
    pop.append(sv, row, fields, labels, recentRow);
    parts = { sv, svThumb, hue, hueThumb, preview, hexIn, rgbIn, recentRow };

    drag(sv, (x, y) => { hsv[1] = x; hsv[2] = 1 - y; });
    drag(hue, (x) => { hsv[0] = x * 360; });
    sv.addEventListener('keydown', (e) => {
      const step = e.shiftKey ? 0.1 : 0.01;
      const d = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] }[e.key];
      if (!d) return;
      e.preventDefault();
      hsv[1] = clamp(hsv[1] + d[0], 0, 1); hsv[2] = clamp(hsv[2] + d[1], 0, 1);
      paint(); emit('input');
    });
    hue.addEventListener('keydown', (e) => {
      const d = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1 }[e.key];
      if (!d) return;
      e.preventDefault();
      hsv[0] = (hsv[0] + d * (e.shiftKey ? 10 : 1) + 360) % 360;
      paint(); emit('input');
    });
    hexIn.addEventListener('input', () => { const hex = parseHex(hexIn.value); if (hex && hexIn.value.replace('#', '').length === 6) setHex(hex, hexIn); });
    hexIn.addEventListener('change', () => {
      const hex = parseHex(hexIn.value);
      if (hex) { setHex(hex); emit('change'); } else { hexIn.classList.add('is-invalid'); }
    });
    rgbIn.forEach((inp) => {
      const read = () => rgbIn.map(i => clamp(parseInt(i.value, 10) || 0, 0, 255));
      inp.addEventListener('input', () => { if (/^\d{1,3}$/.test(inp.value)) setHex(rgbToHex(...read()), inp); });
      inp.addEventListener('change', () => { setHex(rgbToHex(...read())); emit('change'); });
    });
    pop.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { e.stopPropagation(); close(); swatch?.focus(); }
      if (e.key === 'Enter' && e.target.tagName === 'INPUT') e.target.blur();
    });
    document.body.appendChild(pop);
  }

  function paintRecent() {
    const { recentRow } = parts;
    recentRow.replaceChildren();
    recent().forEach(hex => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'kcp-recent-swatch';
      b.style.background = hex;
      b.title = hex.toUpperCase();
      b.setAttribute('aria-label', `Recent colour ${hex.toUpperCase()}`);
      b.addEventListener('click', () => { setHex(hex); emit('change'); });
      recentRow.appendChild(b);
    });
    recentRow.hidden = !recentRow.childElementCount;
  }

  function place(anchor) {
    const r = anchor.getBoundingClientRect();
    const pw = pop.offsetWidth, ph = pop.offsetHeight;
    let left = r.left, top = r.bottom + 6;
    if (top + ph > window.innerHeight - 8) top = Math.max(8, r.top - ph - 6);
    left = clamp(left, 8, window.innerWidth - pw - 8);
    pop.style.left = `${left}px`;
    pop.style.top = `${top}px`;
  }

  function onOutside(e) { if (pop && !pop.contains(e.target) && e.target !== swatch) close(); }

  function open(input) {
    if (swatch && swatch !== input) close();
    if (!pop) build();
    swatch = input;
    startHex = parseHex(input.value) || '#000000';
    const [h, s, v] = rgbToHsv(...hexToRgb(startHex));
    hsv = [h, s, v];
    pop.hidden = false;
    paint();
    paintRecent();
    place(input);
    document.addEventListener('pointerdown', onOutside, true);
    parts.hexIn.focus();
    parts.hexIn.select();
  }

  function close() {
    if (!pop || pop.hidden) return;
    pop.hidden = true;
    document.removeEventListener('pointerdown', onOutside, true);
    if (swatch) {
      const hex = hexNow();
      if (hex !== startHex) { remember(hex); emit('change'); }
    }
    swatch = null;
  }

  // Every colour swatch opens this one, by mouse or keyboard.
  const isSwatch = (t) => t instanceof HTMLInputElement && t.type === 'color' && !t.disabled && !('nativePicker' in t.dataset);
  document.addEventListener('click', (e) => {
    if (!isSwatch(e.target)) return;
    e.preventDefault();
    if (swatch === e.target && pop && !pop.hidden) close(); else open(e.target);
  }, true);
  document.addEventListener('keydown', (e) => {
    if (!isSwatch(e.target) || (e.key !== 'Enter' && e.key !== ' ')) return;
    e.preventDefault();
    open(e.target);
  }, true);
  window.addEventListener('resize', () => { if (pop && !pop.hidden) close(); });

  window.KairoColorPicker = { open, close };
})();
