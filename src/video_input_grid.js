// KAIRO — the input grid: cameras and capture cards as 2 or 4 live tiles.
//
// One reusable view with no knowledge of the director screen. The owner hands
// it the sources, the device list, the on-air / staged state and what a click
// does, and puts `el` and `controls` where it wants them, so the same grid can
// later be shown full screen on a chosen display without being rebuilt.
//   2-up: two large tiles side by side. 4-up: a 2 by 2 grid. More inputs than
//   fit are paged. Only the tiles on the visible page hold a capture, at the
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
  //         onProgram(id), onPreview(id), onPlaceholder(source, el), mode: 2|4, onMode(mode),
  //         layout: 'row' (default, sized by height) | 'column' (fills a narrow column's width) }
  function create(opts) {
    const V = window.KairoVideoSources;
    const hub = window.KairoVideoHub;
    const el = h('div', 'vg');
    const controls = h('div', 'vg-controls');
    let mode = opts.mode === 2 ? 2 : 4;
    let page = 0;
    let leases = [];
    let thumbsOn = true;
    let shown = true;

    // 2-up / 4-up switch
    const sw = h('div', 'vg-switch');
    sw.setAttribute('role', 'group');
    sw.setAttribute('aria-label', 'Tiles shown');
    const modeBtns = [2, 4].map(n => {
      const b = h('button', 'vg-mode', String(n));
      b.type = 'button';
      b.setAttribute('aria-label', `${n}-up`);
      b.title = n === 2 ? 'Two large tiles' : 'Four tiles, two by two';
      b.addEventListener('click', () => { mode = n; page = 0; try { opts.onMode && opts.onMode(n); } catch {} render(); });
      sw.appendChild(b);
      return [n, b];
    });
    // Pager
    const pager = h('div', 'vg-pager');
    const prev = h('button', 'vg-page', '‹'); prev.type = 'button'; prev.title = 'Previous inputs'; prev.setAttribute('aria-label', 'Previous inputs');
    const next = h('button', 'vg-page', '›'); next.type = 'button'; next.title = 'Next inputs'; next.setAttribute('aria-label', 'Next inputs');
    const count = h('span', 'vg-count');
    prev.addEventListener('click', () => { page = Math.max(0, page - 1); render(); });
    next.addEventListener('click', () => { page += 1; render(); });
    pager.append(prev, count, next);
    controls.append(pager, sw);

    const plug = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 2v5M15 2v5M7 7h10v4a5 5 0 0 1-10 0V7zM12 16v6"/></svg>';

    // An input with no device yet, or whose device is not plugged in: a dashed
    // tile that keeps its name, opens nothing, and offers a device on click.
    function emptyTile(source, why) {
      const t = h('div', 'vg-tile is-empty');
      t.tabIndex = 0;
      t.setAttribute('role', 'button');
      t.setAttribute('aria-label', `${source.name}. ${why}. Choose a device for this input`);
      t.title = 'Choose a device for this input';
      const mid = h('div', 'vg-empty-mid');
      mid.innerHTML = plug;
      mid.appendChild(h('span', 'vg-empty-why', why));
      t.appendChild(mid);
      t.appendChild(h('div', 'vg-name', source.name));
      const go = () => { if (opts.onPlaceholder) opts.onPlaceholder(source, t); };
      t.addEventListener('click', go);
      t.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(); } });
      return t;
    }

    function tile(source, st) {
      const state = V.stateOf(source, opts.getDevices());
      if (state === 'placeholder') return emptyTile(source, 'No device');
      if (state === 'missing') return emptyTile(source, 'Not connected');
      const onAir = !st.mixed && st.hasOutputs && st.programId === source.id;
      const staged = st.previewId === source.id;
      const t = h('div', 'vg-tile' + (onAir ? ' is-pgm' : '') + (staged ? ' is-pvw' : ''));
      t.tabIndex = 0;
      t.setAttribute('role', 'button');
      t.setAttribute('aria-label', `${source.name}${onAir ? ', on air' : staged ? ', on Preview' : ''}. Click to send to Program, Shift-click for Preview`);
      t.title = 'Click: send to Program. Shift-click: stage on Preview.';
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
      if (onAir) t.appendChild(h('span', 'vg-flag vg-flag-pgm', 'On air'));
      else if (staged) t.appendChild(h('span', 'vg-flag vg-flag-pvw', 'Preview'));
      const pv = h('button', 'vg-pvw', 'Preview'); pv.type = 'button'; pv.title = `Stage ${source.name} on Preview`;
      pv.addEventListener('click', (e) => { e.stopPropagation(); opts.onPreview(source.id); });
      t.appendChild(pv);
      const go = (e) => (e.shiftKey ? opts.onPreview(source.id) : opts.onProgram(source.id));
      t.addEventListener('click', go);
      t.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(e); } });
      return t;
    }

    function render() {
      const sources = opts.getSources();
      const st = opts.getState();
      const per = mode;
      const pages = Math.max(1, Math.ceil(sources.length / per));
      page = Math.min(page, pages - 1);
      // New leases are taken before the old ones go, so the hub keeps each camera open across a redraw.
      const old = leases;
      leases = [];
      el.className = 'vg vg-' + mode + (opts.layout === 'column' ? ' vg-col' : '');
      el.replaceChildren();
      const visible = sources.slice(page * per, page * per + per);
      visible.forEach(s => el.appendChild(tile(s, st)));
      if (!sources.length) el.appendChild(h('p', 'vg-empty', 'No video sources yet.'));
      old.forEach(release => release());
      modeBtns.forEach(([n, b]) => { b.classList.toggle('is-on', n === mode); b.setAttribute('aria-pressed', String(n === mode)); });
      pager.hidden = pages <= 1;
      count.textContent = `${page + 1}/${pages}`;
      prev.disabled = page === 0;
      next.disabled = page >= pages - 1;
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

    return { el, controls, render, destroy, get mode() { return mode; } };
  }

  window.KairoVideoGrid = { create };
})();
