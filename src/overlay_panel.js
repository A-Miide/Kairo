// Settings → Overlays: add pictures and videos, switch each on or off, and set
// where it sits, how big it is and how see-through, in a small viewer that
// shows the screen with every overlay that is on. Any number can be on.
(function () {
  const host = () => document.getElementById('overlays-panel');
  let items = [];
  let selectedId = null;
  let dragging = false;
  const sendTimers = {};   // one timer per overlay, so two overlays edited at once both go out
  const pending = {};

  const api = (path, body, method = 'POST') => fetch(`${SERVER}${path}`, {
    method, headers: { 'Content-Type': 'application/json' }, body: body ? JSON.stringify(body) : undefined,
  }).then(r => r.json()).catch(() => ({}));

  // Edits made while dragging a slider go out at most every 60 ms.
  function patch(id, fields, now) {
    Object.assign(items.find(o => o.id === id) || {}, fields);
    pending[id] = Object.assign(pending[id] || {}, fields);
    redrawStage();
    const send = () => { const f = pending[id]; delete pending[id]; sendTimers[id] = null; if (f) api(`/api/overlays/${id}`, f); };
    if (now) { clearTimeout(sendTimers[id]); send(); return; }
    if (!sendTimers[id]) sendTimers[id] = setTimeout(send, 60);
  }

  // (This copy used `if (text)`, so 0 showed nothing: the shared one shows it.)
  const { el } = window.KairoUI;
  const mediaSrc = (o) => (typeof authedMediaUrl === 'function' ? authedMediaUrl(o.url) : o.url);

  let stage = null;
  function redrawStage() {
    if (!stage) return;
    window.KairoOverlays.render(stage.layer, items.map(o => ({ ...o, live: o.live || o.id === selectedId })));
    stage.layer.querySelectorAll('.kairo-overlay').forEach(n => n.classList.toggle('selected', n.dataset.overlayId === selectedId));
  }

  function buildStage() {
    const wrap = el('div', 'ov-stage');
    const layer = el('div', 'ov-stage-layer');
    wrap.appendChild(layer);
    stage = { wrap, layer };
    // Drag the selected overlay to move it.
    wrap.addEventListener('pointerdown', (e) => {
      const o = items.find(i => i.id === selectedId);
      const target = e.target.closest('.kairo-overlay');
      if (!o || !target || target.dataset.overlayId !== o.id) return;
      dragging = true;
      wrap.setPointerCapture(e.pointerId);
      const move = (ev) => {
        const r = wrap.getBoundingClientRect();
        const x = Math.min(100, Math.max(0, ((ev.clientX - r.left) / r.width) * 100));
        const y = Math.min(100, Math.max(0, ((ev.clientY - r.top) / r.height) * 100));
        patch(o.id, { x: Math.round(x * 10) / 10, y: Math.round(y * 10) / 10 });
        syncSliders();
      };
      const up = () => { dragging = false; wrap.removeEventListener('pointermove', move); wrap.removeEventListener('pointerup', up); patch(o.id, {}, true); };
      wrap.addEventListener('pointermove', move);
      wrap.addEventListener('pointerup', up);
    });
    return wrap;
  }

  let sliders = {};
  function syncSliders() {
    const o = items.find(i => i.id === selectedId);
    if (!o) return;
    Object.entries(sliders).forEach(([k, s]) => { s.input.value = String(k === 'opacity' ? Math.round(o.opacity * 100) : o[k]); s.out.textContent = `${s.input.value}%`; });
  }

  function slider(label, key, min, max, o) {
    const row = el('label', 'ov-row');
    row.appendChild(el('span', 'ov-label', label));
    const input = document.createElement('input');
    input.type = 'range'; input.min = String(min); input.max = String(max); input.step = '1';
    const out = el('span', 'ov-val');
    sliders[key] = { input, out };
    input.value = String(key === 'opacity' ? Math.round(o.opacity * 100) : Math.round(o[key]));
    out.textContent = `${input.value}%`;
    input.addEventListener('input', () => {
      out.textContent = `${input.value}%`;
      patch(o.id, { [key]: key === 'opacity' ? Number(input.value) / 100 : Number(input.value) });
    });
    row.append(input, out);
    return row;
  }

  const SET_KEY = 'kairo-overlay-set';
  let currentSet = 'Main';
  try { currentSet = localStorage.getItem(SET_KEY) || 'Main'; } catch {}
  const setsOf = () => {
    const names = [...new Set(items.map(o => o.set || 'Main'))];
    if (!names.includes(currentSet)) names.push(currentSet);
    return names;
  };

  // A pill switch (same look as the output on/off switches).
  // In a clickable row: its clicks are its own.
  const toggle = (checked, title, onChange) => window.KairoUI.toggle(checked, title, onChange, { stopClicks: true });

  function render() {
    const h = host();
    if (!h) return;
    h.innerHTML = '';
    sliders = {};
    const inSet = items.filter(o => (o.set || 'Main') === currentSet);
    if (!inSet.some(o => o.id === selectedId)) selectedId = inSet[0]?.id || null;

    // Top bar: the sets on the left, the actions on the right (Add is the main one).
    const bar = el('div', 'ov-toolbar');
    const setBar = el('div', 'ov-sets');
    setsOf().forEach((name) => {
      const b = el('button', 'ov-set' + (name === currentSet ? ' active' : ''), name);
      b.type = 'button';
      b.addEventListener('click', () => { currentSet = name; try { localStorage.setItem(SET_KEY, name); } catch {} render(); });
      setBar.appendChild(b);
    });
    const newSet = el('button', 'ov-set', '+ New set'); newSet.type = 'button';
    newSet.addEventListener('click', () => {
      const input = document.createElement('input'); input.className = 'setting-input'; input.placeholder = 'Set name'; input.maxLength = 40;
      newSet.replaceWith(input); input.focus();
      const done = () => { const n = input.value.trim(); if (n) { currentSet = n.slice(0, 40); try { localStorage.setItem(SET_KEY, currentSet); } catch {} } render(); };
      input.addEventListener('keydown', (e) => { if (e.key === 'Enter') done(); if (e.key === 'Escape') render(); });
      input.addEventListener('blur', done);
    });
    setBar.appendChild(newSet);

    const actions = el('div', 'ov-actions');
    // The whole set on or off with one switch.
    const anyOn = inSet.some(o => o.live);
    const setSwitch = el('label', 'ov-setswitch');
    setSwitch.title = `Show everything in "${currentSet}"`;
    setSwitch.appendChild(el('span', '', currentSet));
    const sw = toggle(anyOn, '', (on) => api('/api/overlays/set-live', { set: currentSet, on }));
    sw.querySelector('input').disabled = !inSet.length;
    setSwitch.appendChild(sw);
    const clear = el('button', 'modal-btn secondary', 'Turn all off');
    clear.type = 'button';
    clear.addEventListener('click', () => api('/api/overlays/clear'));
    const add = el('button', 'modal-btn primary', '+ Add overlay');
    add.type = 'button';
    const file = document.createElement('input');
    file.type = 'file'; file.accept = 'image/*,video/*'; file.multiple = true; file.style.display = 'none';
    const msg = el('span', 'setting-hint ov-msg');
    add.addEventListener('click', () => file.click());
    file.addEventListener('change', async () => {
      msg.textContent = '';
      for (const f of Array.from(file.files)) {
        const buf = new Uint8Array(await f.arrayBuffer());
        let bin = ''; for (let k = 0; k < buf.length; k += 0x8000) bin += String.fromCharCode.apply(null, buf.subarray(k, k + 0x8000));
        const d = await api('/api/overlays/upload', { filename: f.name, dataBase64: btoa(bin) });
        if (d.item) { selectedId = d.item.id; await api(`/api/overlays/${d.item.id}`, { set: currentSet }); } else msg.textContent = d.error || 'Couldn’t add that file.';
      }
      file.value = '';
      await refresh();
    });
    actions.append(setSwitch, clear, add, file);
    bar.append(setBar, actions);
    const about = el('p', 'setting-hint ov-about', 'Pictures and videos that stay on top of everything on the screens, like a logo, a \u201CLive\u201D badge or a lower-third. Group them into sets, such as one for a special service, and switch a whole set on together.');
    h.append(bar, about, msg);

    // Body: the files of this set on the left, the screen on the right.
    const body = el('div', 'ov-body');
    const left = el('div', 'ov-left');
    const right = el('div', 'ov-right');
    right.appendChild(buildStage());

    if (!inSet.length) {
      left.appendChild(el('p', 'setting-hint', `Nothing in "${currentSet}" yet.`));
    } else {
      const list = el('div', 'ov-list');
      inSet.forEach((o) => {
        const r = el('div', 'ov-item' + (o.id === selectedId ? ' selected' : ''));
        const thumb = o.kind === 'video' ? el('video', 'ov-thumb') : el('img', 'ov-thumb');
        thumb.src = mediaSrc(o); if (o.kind === 'video') { thumb.muted = true; thumb.preload = 'metadata'; }
        const name = el('span', 'ov-name', o.name);
        r.append(thumb, name, toggle(o.live, 'Show on the screens', (on) => patch(o.id, { live: on }, true)));
        r.addEventListener('click', () => { selectedId = o.id; render(); });
        list.appendChild(r);
      });
      left.appendChild(list);

      const o = items.find(i => i.id === selectedId);
      const ed = el('div', 'ov-edit');
      const nameIn = document.createElement('input'); nameIn.className = 'setting-input'; nameIn.value = o.name; nameIn.maxLength = 60;
      nameIn.addEventListener('change', () => patch(o.id, { name: nameIn.value.trim() || o.name }, true));
      ed.appendChild(nameIn);
      ed.appendChild(slider('Size', 'w', 2, 100, o));
      ed.appendChild(slider('Horizontal', 'x', 0, 100, o));
      ed.appendChild(slider('Vertical', 'y', 0, 100, o));
      ed.appendChild(slider('Opacity', 'opacity', 0, 100, o));
      const mv = el('label', 'ov-row'); mv.appendChild(el('span', 'ov-label', 'Set'));
      const sel = document.createElement('select'); sel.className = 'setting-input';
      setsOf().forEach(n => { const op = document.createElement('option'); op.value = n; op.textContent = n; if (n === (o.set || 'Main')) op.selected = true; sel.appendChild(op); });
      sel.addEventListener('change', async () => { await api(`/api/overlays/${o.id}`, { set: sel.value }); refresh(); });
      mv.appendChild(sel); ed.appendChild(mv);
      const launch = el('div', 'ov-row ov-launch');
      launch.append(el('span', 'ov-label', 'Show when Kairo opens'), toggle(o.showOnLaunch, '', (on) => patch(o.id, { showOnLaunch: on }, true)));
      ed.appendChild(launch);
      const del = el('button', 'modal-btn secondary', 'Delete overlay'); del.type = 'button';
      del.addEventListener('click', async () => { await api(`/api/overlays/${o.id}`, null, 'DELETE'); refresh(); });
      ed.appendChild(del);
      left.appendChild(ed);
    }
    body.append(left, right);
    h.appendChild(body);
    redrawStage();
  }

  async function refresh() { items = await window.KairoOverlays.load(); render(); }

  // WebKit does not always focus a slider while it is dragged, so a held pointer on one counts too.
  let sliding = false;
  document.addEventListener('pointerdown', (e) => { if (e.target.matches?.('#overlays-panel input[type=range]')) sliding = true; }, true);
  const letGo = () => { if (sliding) { sliding = false; setTimeout(() => { if (!busy()) refresh(); }, 0); } };
  window.addEventListener('pointerup', letGo, true);
  window.addEventListener('pointercancel', letGo, true);

  function busy() {
    if (dragging || sliding) return true;
    const a = document.activeElement;
    return !!(a && host()?.contains(a) && a.tagName === 'INPUT' && ['range', 'text', 'number'].includes(a.type));
  }
  // Let go of a control: catch up with whatever changed meanwhile.
  document.addEventListener('focusout', (e) => {
    if (host()?.contains(e.target) && e.target.tagName === 'INPUT') setTimeout(() => { if (!busy()) refresh(); }, 0);
  });

  window.KairoOverlayPanel = {
    // The server echoes every change back. While a slider or field is being used the
    // panel must not be redrawn under it (that is what broke the sliders), and the
    // echo is older than what is on screen, so it is ignored until the control is let go.
    setItems(next) {
      if (busy()) return;
      items = next || [];
      if (host()?.offsetParent !== null) render();
    },
    refresh,
  };

  document.querySelector('.settings-nav-item[data-pane="overlays"]')?.addEventListener('click', refresh);
})();
