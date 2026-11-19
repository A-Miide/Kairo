// KAIRO — Output Control as routing: each kind of content on the left, each place it can go on the right,
// a line between them where it shows. Drag a content tile onto an output to send it there, or click a dot
// to switch one line on or off. It reads and writes the same switches as the cards (ocChipState,
// setRowForOutputs in app_outputs.js), so the two views never disagree.
'use strict';

const OC_VIEW_KEY = 'kairo-oc-view';
const ROUTE_COLORS = { bible: '#3D9BFF', slide: '#8C82FF', timer: '#2BD4E8', media: '#4FD27A', overlay: '#FF5C9A', message: '#FF8A4C', video: '#A8AEB4' };
const ROUTE_NODE_H = 38, ROUTE_GAP = 8, ROUTE_PAD = 6, ROUTE_OUT_GAP = 18, ROUTE_LEFT_W = 156, ROUTE_RIGHT_W = 190;

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
  const totalH = Math.max(leftH, outputs.length * outH + (outputs.length - 1) * ROUTE_OUT_GAP + ROUTE_PAD * 2);
  stage.style.height = totalH + 'px';

  const sel = { layer: null, out: null };
  const layerEls = [], outEls = [], dots = [];
  let dragKey = null;

  rows.forEach((row, i) => {
    const tile = mk('div', 'oc-route-layer');
    tile.style.top = (ROUTE_PAD + i * (ROUTE_NODE_H + ROUTE_GAP)) + 'px';
    tile.style.setProperty('--c', ROUTE_COLORS[row.key] || '#9AA0A6');
    tile.draggable = true;
    tile.tabIndex = 0;
    tile.title = 'Drag onto an output to show it there. Click to see where it goes.';
    tile.append(mk('span', 'oc-route-swatch'), mk('span', 'oc-route-name', row.label));
    tile.addEventListener('click', () => { sel.layer = sel.layer === i ? null : i; sel.out = null; paint(); });
    tile.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); tile.click(); } });
    tile.addEventListener('dragstart', (e) => { dragKey = row.key; e.dataTransfer.setData('text/plain', row.key); e.dataTransfer.effectAllowed = 'link'; stage.classList.add('is-dragging'); });
    tile.addEventListener('dragend', () => { dragKey = null; stage.classList.remove('is-dragging'); outEls.forEach(o => o.classList.remove('is-target')); });
    stage.appendChild(tile);
    layerEls.push(tile);
  });

  outputs.forEach((o, oi) => {
    const box = mk('div', 'oc-route-out');
    box.style.top = (ROUTE_PAD + oi * (outH + ROUTE_OUT_GAP)) + 'px';
    box.style.height = outH + 'px';
    box.tabIndex = 0;
    box.append(mk('span', 'oc-route-out-name', o.name), mk('span', 'oc-route-out-type', o.monitor ? 'Monitor' : outputTypeLabel(o.type)));
    box.addEventListener('click', () => { sel.out = sel.out === oi ? null : oi; sel.layer = null; paint(); });
    box.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); box.click(); } });
    box.addEventListener('dragover', (e) => {
      if (!dragKey) return;
      const row = rows.find(r => r.key === dragKey);
      if (row && !ocChipState(row, o, map, textOnly).disabled) { e.preventDefault(); e.dataTransfer.dropEffect = 'link'; box.classList.add('is-target'); }
    });
    box.addEventListener('dragleave', () => box.classList.remove('is-target'));
    box.addEventListener('drop', (e) => {
      e.preventDefault(); box.classList.remove('is-target');
      const row = rows.find(r => r.key === dragKey);
      if (!row || ocChipState(row, o, map, textOnly).disabled) return;
      setRowForOutputs(row, [o.id], true);
      renderOutputControlMatrix();
    });
    stage.appendChild(box);
    outEls.push(box);
  });

  const W = () => Math.max(520, stage.clientWidth || 640);

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
      const top = ROUTE_PAD + oi * (outH + ROUTE_OUT_GAP);
      rows.forEach((row, li) => {
        const st = ocChipState(row, o, map, textOnly);
        const color = ROUTE_COLORS[row.key] || '#9AA0A6';
        const x1 = ROUTE_LEFT_W, y1 = ROUTE_PAD + li * (ROUTE_NODE_H + ROUTE_GAP) + ROUTE_NODE_H / 2;
        const x2 = rx - 16, y2 = top + 16 + li * ((outH - 32) / (rows.length - 1));
        const lit = (sel.layer === null && sel.out === null) || sel.layer === li || sel.out === oi;
        const path = document.createElementNS(NS, 'path');
        const dx = Math.max(60, (x2 - x1) / 2);
        path.setAttribute('d', `M${x1} ${y1} C${x1 + dx} ${y1} ${x2 - dx} ${y2} ${x2} ${y2}`);
        path.setAttribute('fill', 'none');
        path.setAttribute('stroke', st.on ? color : 'var(--border)');
        path.setAttribute('stroke-width', st.on ? '2' : '1');
        if (!st.on) path.setAttribute('stroke-dasharray', '3 4');
        path.setAttribute('opacity', lit ? (st.on ? '0.9' : '0.6') : '0.1');
        const glow = st.on && lit && (sel.layer !== null || sel.out !== null);
        if (glow) path.style.filter = `drop-shadow(0 0 4px ${color}) drop-shadow(0 0 9px ${color})`;
        svg.appendChild(path);
        const dot = mk('button', 'oc-route-dot' + (st.on ? ' is-on' : '') + (st.disabled ? ' na' : ''));
        dot.type = 'button';
        dot.style.left = x2 + 'px'; dot.style.top = y2 + 'px';
        dot.style.setProperty('--c', color);
        dot.style.opacity = lit ? '1' : '0.25';
        if (glow) dot.style.boxShadow = `0 0 8px 2px ${color}`;
        dot.title = st.why || `${row.label} on ${o.name}: ${st.on ? 'on' : 'off'}. Click to switch.`;
        dot.setAttribute('aria-label', `${row.label} on ${o.name}`);
        dot.setAttribute('aria-pressed', String(st.on));
        dot.disabled = st.disabled;
        dot.addEventListener('click', () => { setRowForOutputs(row, [o.id], !st.on); renderOutputControlMatrix(); });
        stage.appendChild(dot);
        dots.push(dot);
      });
    });
  }

  root.appendChild(stage);
  root.appendChild(mk('p', 'oc-route-hint', 'Drag a kind of content onto an output to show it there. Click a dot to switch one line. Click a name to see only its lines.'));
  paint();
  if (typeof ResizeObserver === 'function') {
    let last = 0;
    new ResizeObserver(() => { const w = stage.clientWidth; if (w && Math.abs(w - last) > 4) { last = w; paint(); } }).observe(stage);
  }
  return root;
}
