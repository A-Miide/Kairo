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
    // What the tiles were built from. A cut changes only who is on air or staged, which is
    // patched in place; a rebuild closes and reopens every thumbnail, which showed as a blink.
    let built = null;
    let tileEls = [];
    let tileCtl = [];
    let curSt = { programId: null, mixed: false, hasOutputs: false, previewId: null };
    // Only the input on air and the one on Preview run live; the others show a still of their last
    // picture, or a camera silhouette until they have one (no camera running for a thumbnail nobody
    // is watching: owner, 2026-10-17). The still is taken from a camera that is on air when it goes off.
    const stills = new Map();   // source id → data URL
    const SILHOUETTE = '<svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg>';
    const isLiveSource = (id) => (!curSt.mixed && curSt.hasOutputs && curSt.programId === id) || curSt.previewId === id;
    function frameOf(video) {
      try {
        if (!video || !video.videoWidth) return null;
        const c = document.createElement('canvas');
        c.width = 320; c.height = Math.max(1, Math.round(320 * video.videoHeight / video.videoWidth));
        c.getContext('2d').drawImage(video, 0, 0, c.width, c.height);
        return c.toDataURL('image/jpeg', 0.7);
      } catch { return null; }
    }

    // Grid | List: which of the two views is showing is always visible.
    const seg = h('div', 'vg-seg');
    seg.setAttribute('role', 'group'); seg.setAttribute('aria-label', 'View');
    const ICON = {
      grid: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>',
      list: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/></svg>',
    };
    const segBtns = [['grid', 'Grid'], ['list', 'List']].map(([k, label]) => {
      const b = h('button', 'vg-seg-btn');
      b.type = 'button'; b.title = label; b.setAttribute('aria-label', label);
      b.innerHTML = ICON[k];
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
      t.addEventListener('contextmenu', (e) => { if (!opts.onSettings) return; e.preventDefault(); opts.onSettings(source.id, t); });
      t.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(); } });
      return t;
    }

    // A tile opens its camera only while it can be seen AND is on air or on Preview (`opts.wantLive`): scrolled out of
    // the Inputs list, clipped, or just not in use, it lets go of the device (the hub closes it a moment later if
    // nothing else holds it) and shows a still. `opts.sourceId` names the still. Returns { sync } to re-decide.
    function liveWhileVisible(t, v, devId, profile, onError, opts = {}) {
      let release = null;
      let heard = false;      // the observer has spoken at least once
      let seen = false;
      let grabbing = false;
      const still = h('img', 'vg-still'); still.alt = ''; still.hidden = true;
      const sil = h('div', 'vg-silhouette'); sil.innerHTML = SILHOUETTE;
      t.insertBefore(sil, t.firstChild); t.insertBefore(still, t.firstChild);
      const wanted = () => !opts.wantLive || opts.wantLive();
      const showStill = () => {
        const url = opts.sourceId && stills.get(opts.sourceId);
        still.hidden = !url; if (url) still.src = url;
        sil.hidden = !!url;
        v.hidden = true;
      };
      const take = () => hub.lease(devId, profile, (stream) => {
        if (!v.isConnected) return;
        v.srcObject = stream; v.play().catch(() => {});
      }, onError);
      // One picture for a tile that has none yet: the camera is opened for a moment and let go.
      const grab = () => {
        if (grabbing || !opts.sourceId || stills.has(opts.sourceId)) return;
        grabbing = true;
        const tmp = document.createElement('video'); tmp.muted = true; tmp.playsInline = true;
        let rel = null, done = false;
        const finish = () => { if (done) return; done = true; if (rel) rel(); tmp.srcObject = null; grabbing = false; showStill(); };
        rel = hub.lease(devId, profile, (stream) => {
          tmp.srcObject = stream; tmp.play().catch(() => {});
          const t0 = Date.now();
          const poll = () => {
            if (done) return;
            if (tmp.videoWidth && tmp.readyState >= 2) { const f = frameOf(tmp); if (f) stills.set(opts.sourceId, f); finish(); }
            else if (Date.now() - t0 > 4000) finish();
            else setTimeout(poll, 150);
          };
          setTimeout(poll, 250);
        }, finish);
      };
      const sync = () => {
        const on = seen && wanted();
        if (on && !release) { release = take(); v.hidden = false; still.hidden = true; sil.hidden = true; }
        else if (!on && release) {
          const f = opts.sourceId ? frameOf(v) : null;
          if (f) stills.set(opts.sourceId, f);
          release(); release = null; v.srcObject = null;
        }
        if (!on) { showStill(); if (seen) grab(); }
      };
      // No camera until the tile is seen: a tile scrolled out of the list never opens one.
      const io = new IntersectionObserver((es) => { heard = true; seen = es.some(e => e.isIntersecting); sync(); });
      io.observe(t);
      // Where the observer stays silent (a window that is not painting), look at the geometry instead.
      const fallback = setTimeout(() => {
        if (heard || !t.isConnected) return;
        const r = t.getBoundingClientRect();
        let box = { top: 0, bottom: window.innerHeight, left: 0, right: window.innerWidth };
        for (let p = t.parentElement; p; p = p.parentElement) {
          const o = getComputedStyle(p).overflowY;
          if (o === 'auto' || o === 'scroll' || o === 'hidden') { const pr = p.getBoundingClientRect(); box = { top: Math.max(box.top, pr.top), bottom: Math.min(box.bottom, pr.bottom), left: Math.max(box.left, pr.left), right: Math.min(box.right, pr.right) }; }
        }
        seen = r.bottom > box.top && r.top < box.bottom && r.right > box.left && r.left < box.right;
        sync();
      }, 1200);
      leases.push(() => { clearTimeout(fallback); io.disconnect(); if (release) { const f = opts.sourceId ? frameOf(v) : null; if (f) stills.set(opts.sourceId, f); release(); release = null; } });
      return { sync };
    }

    // A read-only copy of a tile: the picture, the name and the on-air or Preview frame, nothing to click.
    function mirrorTile(source, st) {
      const onAir = !st.mixed && st.hasOutputs && st.programId === source.id;
      const staged = st.previewId === source.id;
      const t = h('div', 'vg-tile is-mirror' + (onAir ? ' is-pgm' : '') + (staged ? ' is-pvw' : ''));
      const v = h('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-hidden', 'true');
      t.appendChild(v);
      const devId = V.resolveDevice(source, opts.getDevices());
      if (devId && thumbsOn && shown) liveWhileVisible(t, v, devId, opts.profile || 'thumb', () => { t.dataset.msg = 'Unavailable'; });
      t.appendChild(h('div', 'vg-name', source.name));
      if (onAir) t.appendChild(h('span', 'vg-flag vg-flag-pgm', 'On air'));
      else if (staged) t.appendChild(h('span', 'vg-flag vg-flag-pvw', 'Preview'));
      return t;
    }

    let ctl = null;
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
      t.title = `Click: on air. Shift-click: Preview. Right-click: settings.${source.key ? ` Key ${source.key.toUpperCase()}, Shift+${source.key.toUpperCase()} for Preview.` : index < 9 ? ` Key ${index + 1}, Shift+${index + 1} for Preview.` : ''}`;
      const v = h('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-hidden', 'true');
      t.appendChild(v);
      const devId = V.resolveDevice(source, opts.getDevices());
      ctl = null;
      if (devId && thumbsOn && shown) ctl = liveWhileVisible(t, v, devId, 'thumb', () => { t.dataset.msg = 'Unavailable'; }, { wantLive: () => isLiveSource(source.id), sourceId: source.id });
      else t.dataset.msg = devId ? '' : 'Not connected';
      t.__ctl = ctl;
      t.appendChild(h('div', 'vg-name', source.name));
      if (source.key) t.appendChild(h('span', 'vg-key', source.key.toUpperCase()));
      else if (index < 9) t.appendChild(h('span', 'vg-key', String(index + 1)));
      if (onAir) t.appendChild(h('span', 'vg-flag vg-flag-pgm', 'On air'));
      else if (staged) t.appendChild(h('span', 'vg-flag vg-flag-pvw', 'Preview'));
      t.appendChild(h('span', 'vg-state', onAir ? 'On air' : staged ? 'Preview' : 'Ready'));
      const pv = h('button', 'vg-pvw', 'Preview'); pv.type = 'button'; pv.title = `Stage ${source.name} on Preview`;
      pv.addEventListener('click', (e) => { e.stopPropagation(); opts.onPreview(source.id); });
      t.appendChild(pv);
      const go = (e) => (e.shiftKey ? opts.onPreview(source.id) : opts.onProgram(source.id));
      t.addEventListener('click', go);
      // Right-click: this input's settings.
      t.addEventListener('contextmenu', (e) => { if (!opts.onSettings) return; e.preventDefault(); opts.onSettings(source.id, t); });
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

    // Who is on air or staged, written onto the tiles that are already there.
    function paint(sources, st) {
      curSt = st;
      tileEls.forEach(t => { try { t.__ctl && t.__ctl.sync(); } catch {} });
      sources.forEach((source, i) => {
        const t = tileEls[i];
        if (!t || t.classList.contains('is-empty')) return;
        const onAir = !st.mixed && st.hasOutputs && st.programId === source.id;
        const staged = st.previewId === source.id;
        t.classList.toggle('is-pgm', onAir);
        t.classList.toggle('is-pvw', staged);
        const word = onAir ? 'On air' : staged ? 'Preview' : null;
        t.querySelectorAll('.vg-flag').forEach(f => f.remove());
        if (word) {
          const f = h('span', 'vg-flag ' + (onAir ? 'vg-flag-pgm' : 'vg-flag-pvw'), word);
          const anchor = t.querySelector('.vg-state') || null;
          t.insertBefore(f, anchor);
        }
        const state = t.querySelector('.vg-state');
        if (state) state.textContent = word || 'Ready';
        if (!opts.readOnly) t.setAttribute('aria-label', `${source.name}${onAir ? ', on air' : staged ? ', on Preview' : ''}. Click to send to Program, Shift-click for Preview`);
      });
    }

    function shape(sources) {
      const devices = opts.getDevices();
      return JSON.stringify([
        sources.map(s => [s.id, s.name, s.key || '', V.stateOf(s, devices), V.resolveDevice(s, devices) || '']),
        gridOn, shown, thumbsOn, opts.layout || '', !!opts.readOnly,
      ]);
    }

    function render() {
      const sources = opts.getSources();
      const st = opts.getState();
      curSt = st;
      const shapeNow = shape(sources);
      if (built === shapeNow && sources.length) { paint(sources, st); return; }
      built = shapeNow;
      const fill = opts.layout === 'fill';

      // New leases are taken before the old ones go, so the hub keeps each camera open across a redraw.
      const old = leases;
      leases = [];
      el.className = fill ? 'vg vg-fill' : 'vg ' + (gridOn ? 'vg-grid' : 'vg-list') + (opts.layout === 'column' ? ' vg-col' : '');
      if (fill) el.style.gridTemplateColumns = `repeat(${sources.length <= 1 ? 1 : sources.length === 2 ? 2 : Math.ceil(Math.sqrt(sources.length))}, minmax(0, 1fr))`;
      el.replaceChildren();
      tileEls = sources.map((s, i) => tile(s, st, i));
      tileEls.forEach(t => el.appendChild(t));
      if (!sources.length) {
        // One clear way in, instead of a blank area.
        const add = h('button', 'vg-add vg-add-first');
        add.type = 'button';
        add.innerHTML = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg><span>Add your first camera</span>';
        add.setAttribute('aria-label', 'Add your first camera');
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
