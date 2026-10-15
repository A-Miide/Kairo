// KAIRO — the input grid: cameras and capture cards as 2 or 4 live tiles.
//
// One reusable view with no knowledge of the director screen. The owner hands
// it the sources, the device list, the on-air / staged state and what a click
// does, and puts `el` and `controls` where it wants them, so the same grid can
// later be shown full screen on a chosen display without being rebuilt.
//   Grid on: tiles (two columns, or as many as fit); grid off: one input per row
//   as a list. No pages: the column scrolls. Only the tiles in view hold a capture, at the
//   hub's thumbnail size and rate, and none while the grid is off screen.
//   Click a tile: send it to Program. Shift-click or its Preview button: stage
//   it on Preview.
'use strict';
(function () {
  function h(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // opts: { getSources(), getDevices(), getState() → {programId, mixed, hasOutputs, previewId},
  //         onProgram(id), onPreview(id), onPlaceholder(source, el), onAdd(el), grid: true|false, onGrid(on),
  //         layout: 'row' (default, sized by height) | 'column' (fills a narrow column's width)
  //                 | 'fill' (every input at once, filling the space; for a full-screen multiview),
  //         readOnly: true (a mirror: no clicks, keys or drag), profile: 'thumb' | 'mid' }
  function create(opts) {
    const V = window.KairoVideoSources;
    const hub = window.KairoVideoHub;
    const el = h('div', 'vg');
    const controls = h('div', 'vg-controls');
    let gridOn = opts.grid !== false;
    let leases = [];
    let thumbsOn = true;
    let shown = true;

    // Grid | List: which of the two views is showing is always visible.
    const seg = h('div', 'vg-seg');
    seg.setAttribute('role', 'group'); seg.setAttribute('aria-label', 'View');
    const segBtns = [['grid', 'Grid', 'Tiles'], ['list', 'List', 'One row per input']].map(([k, label, tip]) => {
      const b = h('button', 'vg-seg-btn', label);
      b.type = 'button'; b.title = tip;
      b.addEventListener('click', () => { const on = k === 'grid'; if (on === gridOn) return; gridOn = on; try { opts.onGrid && opts.onGrid(gridOn); } catch {} render(); });
      seg.appendChild(b);
      return [k, b];
    });
    controls.append(seg);

    const plug = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 2v5M15 2v5M7 7h10v4a5 5 0 0 1-10 0V7zM12 16v6"/></svg>';

    // An input with no device yet, or whose device is not plugged in: a dashed
    // tile that keeps its name, opens nothing, and offers a device on click.
    function emptyTile(source, why) {
      const t = h('div', 'vg-tile is-empty');
      t.tabIndex = 0;
      t.setAttribute('role', 'button');
      t.setAttribute('aria-label', `${source.name}. ${why}. Choose a device for this input`);
      const mid = h('div', 'vg-empty-mid');
      mid.innerHTML = plug;
      mid.appendChild(h('span', 'vg-empty-why', why));
      t.appendChild(mid);
      t.appendChild(h('div', 'vg-name', source.name));
      t.appendChild(h('span', 'vg-state', why));
      const go = () => { if (opts.onPlaceholder) opts.onPlaceholder(source, t); };
      t.addEventListener('click', go);
      t.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(); } });
      return t;
    }

    // A read-only copy of a tile: the picture, the name and the on-air or Preview frame, nothing to click.
    function mirrorTile(source, st) {
      const onAir = !st.mixed && st.hasOutputs && st.programId === source.id;
      const staged = st.previewId === source.id;
      const t = h('div', 'vg-tile is-mirror' + (onAir ? ' is-pgm' : '') + (staged ? ' is-pvw' : ''));
      const v = h('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-hidden', 'true');
      t.appendChild(v);
      const devId = V.resolveDevice(source, opts.getDevices());
      if (devId && thumbsOn && shown) {
        leases.push(hub.lease(devId, opts.profile || 'thumb', (stream) => {
          if (!v.isConnected) return;
          v.srcObject = stream; v.play().catch(() => {});
        }, () => { t.dataset.msg = 'Unavailable'; }));
      }
      t.appendChild(h('div', 'vg-name', source.name));
      if (onAir) t.appendChild(h('span', 'vg-flag vg-flag-pgm', 'On air'));
      else if (staged) t.appendChild(h('span', 'vg-flag vg-flag-pvw', 'Preview'));
      return t;
    }

    function tile(source, st, index) {
      const state = V.stateOf(source, opts.getDevices());
      if (state === 'placeholder') return emptyTile(source, 'No device');
      if (state === 'missing') return emptyTile(source, 'Not connected');
      if (opts.readOnly) return mirrorTile(source, st);
      const onAir = !st.mixed && st.hasOutputs && st.programId === source.id;
      const staged = st.previewId === source.id;
      const t = h('div', 'vg-tile' + (onAir ? ' is-pgm' : '') + (staged ? ' is-pvw' : ''));
      t.tabIndex = 0;
      t.setAttribute('role', 'button');
      t.setAttribute('aria-label', `${source.name}${onAir ? ', on air' : staged ? ', on Preview' : ''}. Click to send to Program, Shift-click for Preview`);
      t.title = `Click: on air. Shift-click: Preview.${index < 9 ? ` Key ${index + 1}, Shift+${index + 1} for Preview.` : ''}`;
      const v = h('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-hidden', 'true');
      t.appendChild(v);
      const devId = V.resolveDevice(source, opts.getDevices());
      if (devId && thumbsOn && shown) {
        leases.push(hub.lease(devId, 'thumb', (stream) => {
          if (!v.isConnected) return;
          v.srcObject = stream; v.play().catch(() => {});
        }, () => { t.dataset.msg = 'Unavailable'; }));
      } else t.dataset.msg = devId ? '' : 'Not connected';
      t.appendChild(h('div', 'vg-name', source.name));
      if (index < 9) t.appendChild(h('span', 'vg-key', String(index + 1)));
      if (onAir) t.appendChild(h('span', 'vg-flag vg-flag-pgm', 'On air'));
      else if (staged) t.appendChild(h('span', 'vg-flag vg-flag-pvw', 'Preview'));
      t.appendChild(h('span', 'vg-state', onAir ? 'On air' : staged ? 'Preview' : 'Ready'));
      const pv = h('button', 'vg-pvw', 'Preview'); pv.type = 'button'; pv.title = `Stage ${source.name} on Preview`;
      pv.addEventListener('click', (e) => { e.stopPropagation(); opts.onPreview(source.id); });
      t.appendChild(pv);
      const go = (e) => (e.shiftKey ? opts.onPreview(source.id) : opts.onProgram(source.id));
      t.addEventListener('click', go);
      // Enter sends it; Space is the Take key of the whole screen.
      t.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); go(e); } });
      // Drag it onto Preview or Output. Draggable only while pressed, as elsewhere in the app.
      t.draggable = false;
      t.addEventListener('mousedown', (e) => { if (!e.target.closest('button')) t.draggable = true; });
      t.addEventListener('mouseup', () => { t.draggable = false; });
      t.addEventListener('dragstart', (e) => { e.dataTransfer.setData('application/x-kairo-input', source.id); e.dataTransfer.effectAllowed = 'copy'; });
      t.addEventListener('dragend', () => { t.draggable = false; });
      return t;
    }

    function render() {
      const sources = opts.getSources();
      const st = opts.getState();
      const fill = opts.layout === 'fill';

      // New leases are taken before the old ones go, so the hub keeps each camera open across a redraw.
      const old = leases;
      leases = [];
      el.className = fill ? 'vg vg-fill' : 'vg ' + (gridOn ? 'vg-grid' : 'vg-list') + (opts.layout === 'column' ? ' vg-col' : '');
      if (fill) el.style.gridTemplateColumns = `repeat(${sources.length <= 1 ? 1 : sources.length === 2 ? 2 : Math.ceil(Math.sqrt(sources.length))}, minmax(0, 1fr))`;
      el.replaceChildren();
      sources.forEach((s, i) => el.appendChild(tile(s, st, i)));
      if (!sources.length) {
        // One clear way in, instead of a blank area.
        const add = h('button', 'vg-add', '+ Add input');
        add.type = 'button';
        add.title = 'Add a camera or a named placeholder';
        add.addEventListener('click', () => { if (opts.onAdd) opts.onAdd(add); });
        el.appendChild(add);
      }
      old.forEach(release => release());
      segBtns.forEach(([k, b]) => { const on = (k === 'grid') === gridOn; b.classList.toggle('is-on', on); b.setAttribute('aria-pressed', String(on)); });
    }

    // Tiles run only while the grid is on screen and the window is visible.
    const io = new IntersectionObserver((es) => {
      const on = es.some(e => e.isIntersecting);
      if (on === shown) return;
      shown = on;
      render();
    });
    io.observe(el);
    const onVis = () => {
      const on = !document.hidden;
      if (on === thumbsOn) return;
      thumbsOn = on;
      render();
    };
    document.addEventListener('visibilitychange', onVis);
    thumbsOn = !document.hidden;

    function destroy() {
      io.disconnect();
      document.removeEventListener('visibilitychange', onVis);
      leases.forEach(release => release());
      leases = [];
      el.remove(); controls.remove();
    }

    return { el, controls, render, destroy, get gridOn() { return gridOn; } };
  }

  window.KairoVideoGrid = { create };
})();
