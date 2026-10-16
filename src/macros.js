// Macros (Settings → Macros): one cue that does several things in order — load a Layout,
// switch an overlay on, start the music, show a message, wait, Send. Run from the top bar's
// Macros button, a Stream Deck or OSC (/kairo/macro-1 … macro-9, in the order listed here).
//
// Saved in settings.macros: [{ id, name, steps: [step] }], where a step is one of
//   { kind: 'action', action: <a remote action id> }      anything Stream Deck/OSC can do
//   { kind: 'message', text, to: 'monitors'|'screens'|'all' }
//   { kind: 'clear-message' }
//   { kind: 'overlay', overlay: <overlay id>, on: true|false }
//   { kind: 'wait', seconds }
(function () {
  const MAX_WAIT = 600;
  const running = new Set();
  let editing = null;   // the macro being edited (a copy), or null

  const { el } = window.KairoUI;
  const uid = () => 'macro-' + Math.random().toString(36).slice(2, 8);
  const list = () => (Array.isArray(settings.macros) ? settings.macros.filter(m => m && Array.isArray(m.steps)) : []);
  function save(next) {
    settings.macros = next;
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ macros: next });
    renderTop();
  }
  // What a macro can call: every remote action except the macros themselves (no loops).
  const actions = () => (typeof REMOTE_ACTIONS !== 'undefined' ? REMOTE_ACTIONS : []).filter(a => !/^macro-/.test(a.id));
  const actionLabel = (id) => (actions().find(a => a.id === id) || {}).label || id;
  let overlays = [];
  async function loadOverlays() {
    try { overlays = window.KairoOverlays && window.KairoOverlays.load ? await window.KairoOverlays.load() : []; } catch { overlays = []; }
  }

  function describe(s) {
    switch (s.kind) {
      case 'action': return actionLabel(s.action);
      case 'message': return `Message${s.to === 'screens' ? ' on the screens' : s.to === 'all' ? ' everywhere' : ' to the Monitors'}: “${s.text || ''}”`;
      case 'clear-message': return 'Take the message down';
      case 'overlay': return `Overlay “${(overlays.find(o => o.id === s.overlay) || {}).name || 'removed overlay'}” ${s.on ? 'on' : 'off'}`;
      case 'wait': return `Wait ${s.seconds || 0} s`;
      default: return '';
    }
  }

  // ── Running ──
  async function run(macro) {
    if (!macro || running.has(macro.id)) return;
    running.add(macro.id);
    renderTop(); renderPane();
    try {
      for (const [i, s] of macro.steps.entries()) {
        if (!running.has(macro.id)) break;   // stopped
        // One step failing is said, and the rest still run (a Send after a missing overlay
        // should still go out).
        try { switch (s.kind) {
          case 'action': if (typeof window.runRemoteAction === 'function' && !/^macro-/.test(s.action)) window.runRemoteAction(s.action); break;
          case 'message': if (typeof sendMonitorMessage === 'function' && String(s.text || '').trim()) await sendMonitorMessage(String(s.text).trim(), s.to || 'monitors'); break;
          case 'clear-message': if (typeof sendMonitorMessage === 'function') await sendMonitorMessage('', 'all'); break;
          case 'overlay':
            if (s.overlay) await fetch(`${SERVER}/api/overlays/${encodeURIComponent(s.overlay)}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ live: !!s.on }) });
            break;
          case 'wait': await new Promise(r => setTimeout(r, Math.min(MAX_WAIT, Math.max(0, Number(s.seconds) || 0)) * 1000)); break;
          default: break;
        } } catch (err) {
          window.kairoProblem?.(`Macro “${macro.name || 'Macro'}”, step ${i + 1} (${describe(s)}) didn't work: ${err && err.message ? err.message : err}`);
        }
        // A beat between steps, so each lands before the next (a Layout before its Send).
        await new Promise(r => setTimeout(r, 120));
      }
    } finally {
      running.delete(macro.id);
      renderTop(); renderPane();
    }
  }
  function stop(macro) { running.delete(macro.id); renderTop(); renderPane(); }
  function runIndex(n) { const m = list()[n - 1]; if (m) run(m); }

  // ── Top bar: a Macros button and its list ──
  let topBtn = null, pop = null;
  function closePop() { if (pop) pop.close(); }
  function openPop() {
    const menu = el('div');
    list().forEach((m, i) => {
      const b = el('button', 'mc-pop-item' + (running.has(m.id) ? ' is-running' : ''));
      b.type = 'button'; b.setAttribute('role', 'menuitem');
      b.append(el('span', 'mc-pop-name', m.name || `Macro ${i + 1}`), el('span', 'mc-pop-key', running.has(m.id) ? 'Running…' : (i < 9 ? `macro-${i + 1}` : '')));
      b.addEventListener('click', () => { closePop(); if (running.has(m.id)) stop(m); else run(m); });
      menu.appendChild(b);
    });
    pop = window.KairoUI.popover(topBtn, menu, { cls: 'mc-pop', role: 'menu', label: 'Macros', onClose: () => { pop = null; } });
  }
  function renderTop() {
    const listen = document.getElementById('listen-btn');
    if (!listen) return;
    if (!topBtn) {
      topBtn = el('button', 'tb-macro-btn', 'Macros');
      topBtn.type = 'button';
      topBtn.title = 'Run a macro';
      topBtn.setAttribute('aria-haspopup', 'menu');
      topBtn.addEventListener('click', () => (pop && pop.isOpen() ? closePop() : openPop()));
      const stream = document.getElementById('stream-btn');
      listen.parentNode.insertBefore(topBtn, stream || listen);
    }
    topBtn.hidden = list().length === 0;
    topBtn.classList.toggle('is-running', running.size > 0);
  }

  // ── Settings → Macros ──
  const host = () => document.getElementById('macros-panel');
  function stepEditor(s, i, steps, rerender) {
    const row = el('div', 'mc-step');
    row.appendChild(el('span', 'mc-step-n', String(i + 1)));
    const kind = el('select', 'setting-input mc-kind');
    [['action', 'Do'], ['message', 'Show a message'], ['clear-message', 'Take the message down'], ['overlay', 'Overlay'], ['wait', 'Wait']]
      .forEach(([v, l]) => kind.appendChild(Object.assign(document.createElement('option'), { value: v, textContent: l, selected: s.kind === v })));
    kind.addEventListener('change', () => { steps[i] = { kind: kind.value }; if (kind.value === 'action') steps[i].action = actions()[0]?.id; if (kind.value === 'wait') steps[i].seconds = 2; if (kind.value === 'message') steps[i].to = 'screens'; if (kind.value === 'overlay') { steps[i].overlay = overlays[0]?.id || ''; steps[i].on = true; } rerender(); });
    row.appendChild(kind);
    const fields = el('div', 'mc-fields');
    if (s.kind === 'action') {
      const a = el('select', 'setting-input');
      actions().forEach(x => a.appendChild(Object.assign(document.createElement('option'), { value: x.id, textContent: x.label, selected: x.id === s.action })));
      a.addEventListener('change', () => { s.action = a.value; });
      fields.appendChild(a);
    } else if (s.kind === 'message') {
      const t = el('input', 'setting-input'); t.placeholder = 'The message'; t.maxLength = 200; t.value = s.text || '';
      t.addEventListener('input', () => { s.text = t.value; });
      const to = el('select', 'setting-input mc-narrow');
      [['monitors', 'Monitors'], ['screens', 'Audience screens'], ['all', 'Everywhere']].forEach(([v, l]) => to.appendChild(Object.assign(document.createElement('option'), { value: v, textContent: l, selected: (s.to || 'monitors') === v })));
      to.addEventListener('change', () => { s.to = to.value; });
      fields.append(t, to);
    } else if (s.kind === 'overlay') {
      const o = el('select', 'setting-input');
      if (!overlays.length) o.appendChild(Object.assign(document.createElement('option'), { value: '', textContent: 'No overlays yet (Settings → Overlays)' }));
      overlays.forEach(x => o.appendChild(Object.assign(document.createElement('option'), { value: x.id, textContent: x.name, selected: x.id === s.overlay })));
      o.addEventListener('change', () => { s.overlay = o.value; });
      const on = el('select', 'setting-input mc-narrow');
      [['1', 'On'], ['0', 'Off']].forEach(([v, l]) => on.appendChild(Object.assign(document.createElement('option'), { value: v, textContent: l, selected: (s.on ? '1' : '0') === v })));
      on.addEventListener('change', () => { s.on = on.value === '1'; });
      fields.append(o, on);
    } else if (s.kind === 'wait') {
      const n = el('input', 'setting-input mc-narrow'); n.type = 'number'; n.min = '0'; n.max = String(MAX_WAIT); n.step = '0.5'; n.value = s.seconds ?? 2;
      n.addEventListener('input', () => { s.seconds = Math.min(MAX_WAIT, Math.max(0, Number(n.value) || 0)); });
      fields.append(n, el('span', 'setting-hint', 'seconds'));
    }
    row.appendChild(fields);
    const tools = el('div', 'mc-step-tools');
    const btn = (label, title, fn, disabled) => { const b = el('button', 'mc-icon', label); b.type = 'button'; b.title = title; b.setAttribute('aria-label', title); b.disabled = !!disabled; b.addEventListener('click', fn); return b; };
    tools.append(
      btn('↑', 'Move up', () => { [steps[i - 1], steps[i]] = [steps[i], steps[i - 1]]; rerender(); }, i === 0),
      btn('↓', 'Move down', () => { [steps[i + 1], steps[i]] = [steps[i], steps[i + 1]]; rerender(); }, i === steps.length - 1),
      btn('✕', 'Remove step', () => { steps.splice(i, 1); rerender(); }),
    );
    row.appendChild(tools);
    return row;
  }

  function renderPane() {
    const h = host();
    if (!h) return;
    h.innerHTML = '';
    const card = el('div', 'st-card');
    const head = el('div', 'st-card-head');
    head.appendChild(el('div', 'st-card-title', 'Your macros'));
    const add = el('button', 'modal-btn primary', '+ New macro'); add.type = 'button';
    add.disabled = !!editing;
    add.addEventListener('click', () => { editing = { id: uid(), name: `Macro ${list().length + 1}`, steps: [{ kind: 'action', action: actions()[0]?.id }], isNew: true }; renderPane(); });
    head.appendChild(add);
    card.appendChild(head);
    const body = el('div', 'st-card-body');
    const all = list();
    if (!all.length && !editing) body.appendChild(el('p', 'setting-hint mc-empty', 'A macro does several things with one press: for example load Layout 2, switch the welcome overlay on, start the music and Send. Run it from the Macros button in the top bar, a Stream Deck or OSC.'));
    all.forEach((m, i) => {
      if (editing && editing.id === m.id) { body.appendChild(editor()); return; }
      const row = el('div', 'mc-row');
      const text = el('div', 'mc-text');
      text.append(el('div', 'mc-name', m.name || `Macro ${i + 1}`), el('div', 'setting-hint', m.steps.map(describe).filter(Boolean).join(' → ') || 'No steps'));
      const tools = el('div', 'mc-tools');
      if (i < 9) tools.appendChild(el('code', 'mc-key', `macro-${i + 1}`));
      const runB = el('button', 'modal-btn secondary', running.has(m.id) ? 'Stop' : 'Run'); runB.type = 'button';
      runB.addEventListener('click', () => (running.has(m.id) ? stop(m) : run(m)));
      const edit = el('button', 'modal-btn secondary', 'Edit'); edit.type = 'button'; edit.disabled = !!editing;
      edit.addEventListener('click', () => { editing = JSON.parse(JSON.stringify(m)); renderPane(); });
      tools.append(runB, edit);
      row.append(text, tools);
      body.appendChild(row);
    });
    if (editing && editing.isNew) body.appendChild(editor());
    card.appendChild(body);
    h.appendChild(card);
    h.appendChild(el('p', 'setting-hint mc-foot', 'Stream Deck and OSC run the first nine as /kairo/macro-1 to /kairo/macro-9, in the order shown here.'));
  }

  function editor() {
    const m = editing;
    const box = el('div', 'mc-editor');
    const name = el('input', 'setting-input mc-name-input'); name.value = m.name || ''; name.maxLength = 40; name.placeholder = 'Name';
    name.addEventListener('input', () => { m.name = name.value; });
    box.appendChild(name);
    const steps = el('div', 'mc-steps');
    const draw = () => { steps.innerHTML = ''; m.steps.forEach((s, i) => steps.appendChild(stepEditor(s, i, m.steps, draw))); };
    draw();
    box.appendChild(steps);
    const addStep = el('button', 'modal-btn secondary mc-add-step', '+ Add step'); addStep.type = 'button';
    addStep.addEventListener('click', () => { m.steps.push({ kind: 'action', action: actions()[0]?.id }); draw(); });
    box.appendChild(addStep);
    const foot = el('div', 'st-form-buttons');
    if (!m.isNew) {
      const del = el('button', 'modal-btn secondary st-delete', 'Delete'); del.type = 'button';
      del.addEventListener('click', () => { save(list().filter(x => x.id !== m.id)); editing = null; renderPane(); });
      foot.appendChild(del);
    }
    const cancel = el('button', 'modal-btn secondary', 'Cancel'); cancel.type = 'button';
    cancel.addEventListener('click', () => { editing = null; renderPane(); });
    const ok = el('button', 'modal-btn primary', m.isNew ? 'Add macro' : 'Save'); ok.type = 'button';
    ok.addEventListener('click', () => {
      const clean = { id: m.id, name: (m.name || '').trim().slice(0, 40) || 'Macro', steps: m.steps.filter(s => s && s.kind) };
      save(m.isNew ? [...list(), clean] : list().map(x => (x.id === m.id ? clean : x)));
      editing = null; renderPane();
    });
    foot.append(cancel, ok);
    box.appendChild(foot);
    return box;
  }

  const style = document.createElement('style');
  style.textContent = `
    #macros-panel { display: flex; flex-direction: column; gap: 10px; }
    .mc-row { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 12px 0; border-bottom: 1px solid var(--border); }
    .mc-row:last-child { border-bottom: 0; }
    .mc-text { min-width: 0; }
    .mc-name { font-size: 13.5px; font-weight: 700; color: var(--text); }
    .mc-text .setting-hint { margin: 3px 0 0; }
    .mc-tools { display: flex; align-items: center; gap: 8px; flex: none; }
    .mc-editor .st-form-buttons .modal-btn { white-space: nowrap; }
    .mc-key { font-size: 11px; color: var(--text-3, #8b8b96); }
    .mc-empty { margin: 12px 0; }
    .mc-foot { margin: 0 2px; }
    .mc-editor { border: 1px solid var(--border); border-radius: 10px; padding: 12px; margin: 10px 0; display: flex; flex-direction: column; gap: 10px; background: rgba(255,255,255,0.02); }
    .mc-name-input { max-width: 320px; }
    .mc-steps { display: flex; flex-direction: column; }
    .mc-step { display: grid; grid-template-columns: 18px minmax(0, 1fr) auto; grid-template-areas: "n kind tools" ". fields fields"; gap: 6px 8px; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--border); }
    .mc-step:last-child { border-bottom: 0; }
    .mc-step-n { grid-area: n; } .mc-kind { grid-area: kind; max-width: 260px; } .mc-fields { grid-area: fields; } .mc-step-tools { grid-area: tools; }
    .mc-fields:empty { display: none; }
    @media (min-width: 1100px) { .mc-step { grid-template-columns: 18px 190px minmax(0, 1fr) auto; grid-template-areas: "n kind fields tools"; } }
    .mc-step-n { font-size: 12px; color: var(--text-3, #8b8b96); text-align: center; }
    .mc-fields { display: flex; flex-wrap: wrap; gap: 6px 8px; align-items: center; min-width: 0; }
    .mc-fields .setting-input { flex: 1 1 140px; min-width: 0; }
    .mc-fields .mc-narrow { flex: 0 1 120px; min-width: 80px; }
    .mc-fields .setting-hint { margin: 0; }
    .mc-step-tools { display: flex; gap: 2px; }
    .mc-icon { width: 26px; height: 28px; padding: 0; border-radius: 6px; border: 1px solid var(--border); background: transparent; color: var(--text-2, var(--text)); cursor: pointer; }
    .mc-icon:disabled { opacity: .35; cursor: default; }
    .mc-add-step { align-self: flex-start; }
    .tb-macro-btn { height: 30px; padding: 0 12px; border-radius: 999px; border: 1px solid var(--border-md, rgba(255,255,255,0.14)); background: transparent; color: var(--text); font: inherit; font-size: 12.5px; font-weight: 600; cursor: pointer; margin-right: 8px; }
    .tb-macro-btn[hidden] { display: none; }
    .tb-macro-btn.is-running { border-color: #b5832c; }
    .mc-pop { width: 240px; max-height: 60vh; padding: 6px; border-radius: 10px; }
    .mc-pop-item { width: 100%; display: flex; justify-content: space-between; gap: 10px; padding: 8px 10px; border: 0; border-radius: 7px; background: transparent; color: var(--text); font: inherit; font-size: 13px; cursor: pointer; text-align: left; }
    .mc-pop-item:hover, .mc-pop-item:focus-visible { background: rgba(255,255,255,0.06); }
    .mc-pop-item.is-running .mc-pop-key { color: #e0a64a; }
    .mc-pop-key { font-size: 11px; color: var(--text-3, #8b8b96); }
  `;
  document.head.appendChild(style);

  window.KairoMacros = { run, runIndex, list };
  document.addEventListener('click', (e) => {
    const b = e.target.closest && e.target.closest('[data-pane="macros"]');
    if (b) loadOverlays().then(renderPane);
  });
  const start = () => { loadOverlays().then(() => { renderPane(); renderTop(); }); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else setTimeout(start, 0);
  // Settings load after start: show the button once macros are known.
  setInterval(renderTop, 1500);
})();
