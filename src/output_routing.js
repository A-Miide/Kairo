// KAIRO — Output Control as routing: each kind of content on the left, each place it can go on the right,
// a string between them where it shows. The strings are the controls:
//   - pull a string off a content tile's port to an output to show it there;
//   - grab any live string (or its dot) and drag its end to another output to move it, or off the outputs to
//     take it away;
//   - click a dot to switch one string on or off (a keyboard press does the same).
// Outputs can be put in any order by dragging their grip (Alt + arrow keys on a focused output does the same);
// the order is remembered and the Cards view follows it.
// It reads and writes the same switches as the cards (ocChipState, setRowForOutputs in app_outputs.js), so the
// two views never disagree.
'use strict';

const OC_VIEW_KEY = 'kairo-oc-view';
const OC_ORDER_KEY = 'kairo-oc-output-order';
const ROUTE_COLORS = { bible: '#3D9BFF', slide: '#8C82FF', timer: '#2BD4E8', media: '#4FD27A', overlay: '#FF5C9A', message: '#FF8A4C', video: '#A8AEB4' };
const ROUTE_NODE_H = 38, ROUTE_GAP = 8, ROUTE_PAD = 6, ROUTE_OUT_GAP = 18, ROUTE_LEFT_W = 156, ROUTE_RIGHT_W = 190;
const ROUTE_DRAG_MIN_PX = 4;

function ocView() {
  try { return localStorage.getItem(OC_VIEW_KEY) === 'cards' ? 'cards' : 'routing'; } catch { return 'routing'; }
}

function ocViewSwitch(mk, rerender) {
  const bar = mk('div', 'oc-view-switch');
  bar.setAttribute('role', 'group');
  bar.setAttribute('aria-label', 'Output Control view');
  [['routing', 'Routing'], ['cards', 'Cards']].forEach(([id, label]) => {
    const b = mk('button', 'oc-view-btn' + (ocView() === id ? ' is-on' : ''), label);
    b.type = 'button';
    b.setAttribute('aria-pressed', String(ocView() === id));
    b.addEventListener('click', () => { try { localStorage.setItem(OC_VIEW_KEY, id); } catch { /* the view still switches */ } rerender(); });
    bar.appendChild(b);
  });
  return bar;
}

// The outputs in the order the operator chose; ones never placed (a new output) go last, in their usual order.
function ocOrderOutputs(outputs) {
  let saved = [];
  try { const v = JSON.parse(localStorage.getItem(OC_ORDER_KEY) || '[]'); if (Array.isArray(v)) saved = v; } catch { /* the usual order */ }
  const rank = (o) => { const i = saved.indexOf(o.id); return i < 0 ? saved.length : i; };
  return outputs.map((o, i) => ({ o, i })).sort((a, b) => rank(a.o) - rank(b.o) || a.i - b.i).map(x => x.o);
}
function ocSaveOrder(ids) {
  try { localStorage.setItem(OC_ORDER_KEY, JSON.stringify(ids)); } catch { /* the order lasts until the window closes */ }
}

function buildOutputRouting(outputs, map, textOnly, mk) {
  const rows = OC_ROWS.filter(r => r.key !== 'textOnly');
  const root = mk('div', 'oc-route');
  const stage = mk('div', 'oc-route-stage');
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('class', 'oc-route-lines');
  svg.setAttribute('aria-hidden', 'true');
  stage.appendChild(svg);

  const leftH = ROUTE_PAD * 2 + rows.length * ROUTE_NODE_H + (rows.length - 1) * ROUTE_GAP;
  const outH = Math.max(110, Math.floor((leftH - (outputs.length - 1) * ROUTE_OUT_GAP) / outputs.length));
  const slotTop = (n) => ROUTE_PAD + n * (outH + ROUTE_OUT_GAP);
  const totalH = Math.max(leftH, outputs.length * outH + (outputs.length - 1) * ROUTE_OUT_GAP + ROUTE_PAD * 2);
  stage.style.height = totalH + 'px';

  const sel = { layer: null, out: null };
  const layerEls = [], outEls = [], dots = [], ports = [];
  let drag = null, move = null;
  const rowY = (li) => ROUTE_PAD + li * (ROUTE_NODE_H + ROUTE_GAP) + ROUTE_NODE_H / 2;
  const colorOf = (row) => ROUTE_COLORS[row.key] || '#9AA0A6';

  function saveOrderFrom(list) {
    ocSaveOrder(list.map(o => o.id));
    renderOutputControlMatrix();
  }
  const stageXY = (e) => { const r = stage.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
  const outputAt = (x, y) => outEls.findIndex(b => x >= b.offsetLeft && x <= b.offsetLeft + b.offsetWidth && y >= b.offsetTop && y <= b.offsetTop + b.offsetHeight);

  rows.forEach((row, i) => {
    const tile = mk('div', 'oc-route-layer');
    tile.style.top = (ROUTE_PAD + i * (ROUTE_NODE_H + ROUTE_GAP)) + 'px';
    tile.style.setProperty('--c', colorOf(row));
    tile.tabIndex = 0;
    tile.title = 'Click to see where it goes. Pull a string from the dot on its right edge to an output.';
    tile.append(mk('span', 'oc-route-swatch'), mk('span', 'oc-route-name', row.label));
    tile.addEventListener('click', () => { sel.layer = sel.layer === i ? null : i; sel.out = null; paint(); });
    tile.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); tile.click(); } });
    stage.appendChild(tile);
    layerEls.push(tile);
    // The port: where a new string starts.
    const port = mk('span', 'oc-route-port');
    port.style.left = ROUTE_LEFT_W + 'px'; port.style.top = rowY(i) + 'px';
    port.style.setProperty('--c', colorOf(row));
    port.title = `Pull a string to an output to show ${row.label} there`;
    port.addEventListener('pointerdown', (e) => startDrag(e, i, null));
    stage.appendChild(port);
    ports.push(port);
  });

  outputs.forEach((o, oi) => {
    const box = mk('div', 'oc-route-out');
    box.style.top = slotTop(oi) + 'px';
    box.style.height = outH + 'px';
    box.tabIndex = 0;
    const grip = mk('span', 'oc-route-grip');
    grip.title = 'Drag to put this output in a different place';
    grip.setAttribute('aria-hidden', 'true');
    grip.addEventListener('pointerdown', (e) => startMove(e, oi));
    box.append(grip, mk('span', 'oc-route-out-name', o.name), mk('span', 'oc-route-out-type', o.monitor ? 'Monitor' : outputTypeLabel(o.type)));
    box.addEventListener('click', () => { sel.out = sel.out === oi ? null : oi; sel.layer = null; paint(); });
    box.addEventListener('keydown', (e) => {
      if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && e.altKey) {
        e.preventDefault();
        const to = oi + (e.key === 'ArrowUp' ? -1 : 1);
        if (to < 0 || to >= outputs.length) return;
        const list = outputs.slice(); list.splice(to, 0, list.splice(oi, 1)[0]);
        saveOrderFrom(list);
      } else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); box.click(); }
    });
    stage.appendChild(box);
    outEls.push(box);
  });

  const W = () => Math.max(520, stage.clientWidth || 640);
  const curve = (x1, y1, x2, y2) => { const dx = Math.max(60, (x2 - x1) / 2); return `M${x1} ${y1} C${x1 + dx} ${y1} ${x2 - dx} ${y2} ${x2} ${y2}`; };

  // ── pulling a string ──────────────────────────────────────────────────────────────────────────
  // li: the content; from: the output the string comes off (null: a new string).
  function startDrag(e, li, from) {
    if (e.button != null && e.button !== 0) return;
    e.preventDefault();
    const [x, y] = stageXY(e);
    const live = document.createElementNS(NS, 'path');
    live.setAttribute('fill', 'none');
    live.setAttribute('stroke', colorOf(rows[li]));
    live.setAttribute('stroke-width', '2.5');
    live.setAttribute('stroke-linecap', 'round');
    live.style.filter = `drop-shadow(0 0 5px ${colorOf(rows[li])})`;
    live.style.pointerEvents = 'none';
    svg.appendChild(live);
    drag = { li, from, sx: x, sy: y, moved: false, live, pointer: e.pointerId };
    stage.classList.add('is-dragging');
    try { stage.setPointerCapture(e.pointerId); } catch { /* the pointer is followed without capture */ }
    updateDrag(x, y);
  }
  function updateDrag(x, y) {
    const { li, live } = drag;
    live.setAttribute('d', curve(ROUTE_LEFT_W, rowY(li), x, y));
    const at = outputAt(x, y);
    outEls.forEach((b, i) => b.classList.toggle('is-target', i === at && !ocChipState(rows[li], outputs[i], map, textOnly).disabled));
  }
  function endDrag(e, cancelled) {
    const d = drag; drag = null;
    stage.classList.remove('is-dragging');
    outEls.forEach(b => b.classList.remove('is-target'));
    d.live.remove();
    try { stage.releasePointerCapture?.(d.pointer); } catch { /* already released */ }
    if (cancelled) { paint(); return; }
    const [x, y] = stageXY(e);
    const row = rows[d.li];
    if (!d.moved) {
      // A press that never moved: on a dot it switches the string, on a string or a port it does nothing.
      if (d.dot) { const o = outputs[d.dot.oi]; setRowForOutputs(row, [o.id], !d.dot.on); renderOutputControlMatrix(); } else paint();
      return;
    }
    const at = outputAt(x, y);
    if (at >= 0) {
      const o = outputs[at];
      if (ocChipState(row, o, map, textOnly).disabled) { paint(); return; }
      if (at === d.from) { paint(); return; }
      if (d.from != null) setRowForOutputs(row, [outputs[d.from].id], false);
      setRowForOutputs(row, [o.id], true);
      renderOutputControlMatrix();
    } else if (d.from != null) {
      setRowForOutputs(row, [outputs[d.from].id], false);   // let go away from every output: the string comes off
      renderOutputControlMatrix();
    } else paint();
  }

  // ── putting outputs in order ──────────────────────────────────────────────────────────────────
  function startMove(e, oi) {
    if (e.button != null && e.button !== 0) return;
    e.preventDefault(); e.stopPropagation();
    const box = outEls[oi];
    const marker = mk('div', 'oc-route-ins');
    stage.appendChild(marker);
    move = { oi, startY: stageXY(e)[1], base: slotTop(oi), slot: oi, marker, pointer: e.pointerId };
    box.classList.add('is-moving');
    try { stage.setPointerCapture(e.pointerId); } catch { /* the pointer is followed without capture */ }
    marker.style.top = (slotTop(oi) - ROUTE_OUT_GAP / 2) + 'px';
  }
  function updateMove(y) {
    const m = move, box = outEls[m.oi];
    const top = Math.max(ROUTE_PAD, Math.min(slotTop(outputs.length - 1), m.base + (y - m.startY)));
    box.style.top = top + 'px';
    m.slot = Math.max(0, Math.min(outputs.length - 1, Math.round((top - ROUTE_PAD) / (outH + ROUTE_OUT_GAP))));
    m.marker.style.top = (slotTop(m.slot) + (m.slot > m.oi ? outH + ROUTE_OUT_GAP / 2 : -ROUTE_OUT_GAP / 2)) + 'px';
  }
  function endMove(cancelled) {
    const m = move; move = null;
    m.marker.remove();
    outEls[m.oi].classList.remove('is-moving');
    try { stage.releasePointerCapture?.(m.pointer); } catch { /* already released */ }
    if (cancelled || m.slot === m.oi) { outEls[m.oi].style.top = slotTop(m.oi) + 'px'; return; }
    const list = outputs.slice(); list.splice(m.slot, 0, list.splice(m.oi, 1)[0]);
    saveOrderFrom(list);
  }

  stage.addEventListener('pointermove', (e) => {
    const [x, y] = stageXY(e);
    if (drag) {
      if (!drag.moved && Math.hypot(x - drag.sx, y - drag.sy) >= ROUTE_DRAG_MIN_PX) drag.moved = true;
      updateDrag(x, y);
    } else if (move) updateMove(y);
  });
  stage.addEventListener('pointerup', (e) => { if (drag) endDrag(e, false); else if (move) endMove(false); });
  stage.addEventListener('pointercancel', (e) => { if (drag) endDrag(e, true); else if (move) endMove(true); });
  stage.addEventListener('keydown', (e) => { if (e.key === 'Escape') { if (drag) endDrag(e, true); else if (move) endMove(true); } });

  function paint() {
    svg.innerHTML = '';
    dots.forEach(d => d.remove());
    dots.length = 0;
    layerEls.forEach((t, i) => t.classList.toggle('is-sel', sel.layer === i));
    outEls.forEach((b, i) => b.classList.toggle('is-sel', sel.out === i));
    const w = W();
    svg.setAttribute('viewBox', `0 0 ${w} ${totalH}`);
    svg.setAttribute('width', w); svg.setAttribute('height', totalH);
    const rx = w - ROUTE_RIGHT_W;
    outputs.forEach((o, oi) => {
      const top = slotTop(oi);
      rows.forEach((row, li) => {
        const st = ocChipState(row, o, map, textOnly);
        const color = colorOf(row);
        const x1 = ROUTE_LEFT_W, y1 = rowY(li);
        const x2 = rx - 16, y2 = top + 16 + li * ((outH - 32) / (rows.length - 1));
        const lit = (sel.layer === null && sel.out === null) || sel.layer === li || sel.out === oi;
        const d = curve(x1, y1, x2, y2);
        const path = document.createElementNS(NS, 'path');
        path.setAttribute('d', d);
        path.setAttribute('fill', 'none');
        path.setAttribute('stroke', st.on ? color : 'var(--border)');
        path.setAttribute('stroke-width', st.on ? '2' : '1');
        if (!st.on) path.setAttribute('stroke-dasharray', '3 4');
        path.setAttribute('opacity', lit ? (st.on ? '0.9' : '0.6') : '0.1');
        const glow = st.on && lit && (sel.layer !== null || sel.out !== null);
        if (glow) path.style.filter = `drop-shadow(0 0 4px ${color}) drop-shadow(0 0 9px ${color})`;
        path.style.pointerEvents = 'none';
        svg.appendChild(path);
        // A live string can be grabbed anywhere along it.
        if (st.on) {
          const hit = document.createElementNS(NS, 'path');
          hit.setAttribute('d', d);
          hit.setAttribute('fill', 'none');
          hit.setAttribute('stroke', 'transparent');
          hit.setAttribute('stroke-width', '14');
          hit.style.pointerEvents = 'stroke';
          hit.style.cursor = 'grab';
          hit.style.touchAction = 'none';
          const t = document.createElementNS(NS, 'title');
          t.textContent = `${row.label} on ${o.name}: drag to another output, or off the outputs to take it away`;
          hit.appendChild(t);
          hit.addEventListener('pointerdown', (e) => startDrag(e, li, oi));
          svg.appendChild(hit);
        }
        const dot = mk('button', 'oc-route-dot' + (st.on ? ' is-on' : '') + (st.disabled ? ' na' : ''));
        dot.type = 'button';
        dot.style.left = x2 + 'px'; dot.style.top = y2 + 'px';
        dot.style.setProperty('--c', color);
        dot.style.opacity = lit ? '1' : '0.25';
        if (glow) dot.style.boxShadow = `0 0 8px 2px ${color}`;
        dot.title = st.why || `${row.label} on ${o.name}: ${st.on ? 'on' : 'off'}. Click to switch it, or drag to move it.`;
        dot.setAttribute('aria-label', `${row.label} on ${o.name}`);
        dot.setAttribute('aria-pressed', String(st.on));
        dot.disabled = st.disabled;
        // Pressed with a pointer: a drag moves the string, a click switches it. A key press switches it.
        dot.addEventListener('pointerdown', (e) => { if (st.disabled) return; startDrag(e, li, st.on ? oi : null); drag.dot = { oi, on: st.on }; });
        dot.addEventListener('click', (e) => { if (e.detail === 0) { setRowForOutputs(row, [o.id], !st.on); renderOutputControlMatrix(); } });
        stage.appendChild(dot);
        dots.push(dot);
      });
    });
  }

  root.appendChild(stage);
  root.appendChild(mk('p', 'oc-route-hint', 'Pull a string from a content dot to an output. Drag a string, or its dot, to move it, or off the outputs to take it away. Click a dot to switch one. Drag an output by its grip to reorder.'));
  paint();
  if (typeof ResizeObserver === 'function') {
    let last = 0;
    new ResizeObserver(() => { const w = stage.clientWidth; if (w && Math.abs(w - last) > 4 && !drag && !move) { last = w; paint(); } }).observe(stage);
  }
  return root;
}
