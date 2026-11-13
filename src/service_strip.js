// KAIRO — the order of service on every tab.
//
// A small strip in the left column (it is there on Bible, Slides, Timer, wherever the operator is) showing the
// segment that is running, its countdown, and, once it is over its time, red overtime with the three choices:
// Next, +5 min, Keep going. Nothing moves on by itself. Click the strip for the whole plan. The Plan editor (the
// Timer tab's Plan button, or "Edit plan" here) is where the service start time and each segment's length are set.
// The arithmetic is service_plan.js; starting and stopping segments is service.js (the same countdown as a card).
'use strict';

(function () {
  const SP = window.KairoServicePlan;
  if (!SP) return;
  const SERVER = `${location.protocol}//${location.host}`;
  const $ = (id) => document.getElementById(id);
  const esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const svc = () => window.KairoService;
  // Small inline icons (Kairo has no icon font).
  const SVG = (d, extra = '') => `<svg class="svp-svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" ${extra}>${d}</svg>`;
  const ICON = {
    clock: SVG('<circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/>'),
    over: SVG('<circle cx="12" cy="12" r="9"/><line x1="12" y1="7" x2="12" y2="13"/><circle cx="12" cy="16.5" r="0.6" fill="currentColor"/>'),
    skip: SVG('<polygon points="5 4 15 12 5 20 5 4" fill="currentColor"/><line x1="19" y1="5" x2="19" y2="19"/>'),
    check: SVG('<polyline points="20 6 9 17 4 12"/>'),
    play: SVG('<polygon points="6 4 20 12 6 20 6 4" fill="currentColor"/>'),
    circle: SVG('<circle cx="12" cy="12" r="7"/>'),
    lock: SVG('<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>'),
  };

  let segments = [];
  let serviceStart = '';            // 'HH:MM', the Plan editor's setting
  let runByClock = false;           // Auto-Start: the service and its fixed-time segments start themselves on the device's clock
  let expanded = false;
  let keptGoingFor = null;          // "Keep going": the live segment whose overtime prompt was set aside
  let lastTick = null;              // { triggerId, remainingMs, at } from the server's timer
  let timer = null;

  const startMs = () => {
    const set = SP.clockToMs(serviceStart, Date.now());
    if (set != null) return set;
    const first = [...segments].sort((a, b) => (a.order || 0) - (b.order || 0)).find((s) => s.startedAt);
    return first ? first.startedAt : Date.now();
  };
  const planned = () => segments.some((s) => SP.lengthSec(s) != null || (s.trigger && s.trigger.params && s.trigger.params.endAtTime));
  const problem = (msg) => { try { window.kairoProblem ? window.kairoProblem(msg) : console.warn(msg); } catch {} };

  async function loadSetting() {
    try {
      const r = await fetch(`${SERVER}/api/settings`);
      const s = await r.json();
      serviceStart = typeof s.serviceStart === 'string' ? s.serviceStart : '';
      runByClock = s.serviceRunByClock === true;
    } catch {}
  }

  // "9:00", or "9:00 · in 12:30" when the start is within the hour; "9:00 · late" once it has passed.
  function startsIn(ms) {
    const secs = Math.round((ms - Date.now()) / 1000);
    if (secs > 0 && secs <= 3600) return `${SP.timeOfDay(ms)} · in ${SP.clockText(secs)}`;
    if (secs < -30) return `${SP.timeOfDay(ms)} · late`;
    return SP.timeOfDay(ms);
  }

  // Auto-Start (the operator switched it on): the first segment starts at the service start time, and a
  // segment with a fixed time starts at that time. Each only once, and only close to its time, so opening Kairo
  // late or stopping a segment by hand doesn't set it off again.
  const GRACE_MS = 5 * 60 * 1000;
  function clockTick() {
    if (!runByClock || !svc()) return;
    const now = Date.now();
    const day = new Date(now).toDateString();
    const once = (key) => { try { const k = `svp-auto:${day}:${key}`; if (localStorage.getItem(k)) return false; localStorage.setItem(k, '1'); return true; } catch { return false; } };
    const started = segments.some((s) => s.status === 'live' || s.status === 'done');
    const sorted = [...segments].filter((s) => s.included !== false).sort((a, b) => (a.order || 0) - (b.order || 0));
    // Fixed times first: they start whatever else is running.
    for (const s of sorted) {
      const at = s.status === 'pending' && s.fixedAt ? SP.clockToMs(s.fixedAt, now) : null;
      if (at != null && now >= at && now < at + GRACE_MS && once(`fixed:${s.id}`)) { svc().planStart(s.id).then(() => svc().loadSegments()); return; }
    }
    const first = sorted.find((s) => s.status === 'pending');
    const start = SP.clockToMs(serviceStart, now);
    if (!started && first && start != null && now >= start && now < start + GRACE_MS && once(`start:${serviceStart}`)) {
      svc().planStart(first.id).then(() => svc().loadSegments());
    }
  }

  function liveRemainingFix(plan) {
    // Trust the server's own countdown for the live segment when it is fresh (another computer's clock may differ).
    const row = plan.rows[plan.liveIndex];
    if (!row || !lastTick || lastTick.triggerId !== row.seg.triggerId || Date.now() - lastTick.at > 3000) return;
    const rem = Math.round((lastTick.remainingMs - (Date.now() - lastTick.at)) / 1000);
    row.remainingSec = rem;
    row.overSec = rem < 0 ? -rem : 0;
  }

  function driftText(sec) {
    if (Math.abs(sec) < 30) return { text: 'on plan', cls: 'ok' };
    return sec > 0 ? { text: `${SP.clockText(sec)} behind`, cls: 'bad' } : { text: `${SP.clockText(sec)} ahead`, cls: 'ok' };
  }

  function render() {
    const el = $('svp-strip');
    if (!el) return;
    const live = segments.find((s) => s.status === 'live');
    if (!planned() && !live) { el.hidden = true; return; }
    el.hidden = false;
    const plan = SP.buildPlan(segments, startMs(), Date.now());
    liveRemainingFix(plan);
    const row = plan.rows[plan.liveIndex];
    const next = plan.rows[plan.nextIndex];
    const drift = driftText(plan.driftSec);
    const over = !!row && row.overSec > 0;
    const doneAll = !row && !next && plan.rows.length > 0 && plan.rows.every((r) => r.status === 'done');
    const asked = over && keptGoingFor !== `${row.seg.id}:${row.seg.startedAt}`;
    el.className = `svp-strip${over ? ' over' : ''}`;

    // One compact row (name, countdown), a hairline bar, and buttons only when they are needed: the strip shares a
    // short column with the Live Transcript Log and must not push it out of sight.
    const driftChip = drift.cls === 'bad' ? `<span class="svp-drift bad" title="Behind plan">${drift.text}</span>` : '';
    let body = '';
    if (row) {
      const total = (row.plannedEndMs - row.plannedStartMs) / 1000 + (Number(row.seg.extendedSec) || 0);
      const pct = over ? 100 : total > 0 ? Math.min(100, Math.max(0, 100 * (total - row.remainingSec) / total)) : 0;
      const nextLabel = next ? `Next: ${next.seg.name} at ${SP.timeOfDay(next.projStartMs)}` : `Last segment, ends ${SP.timeOfDay(plan.projectedEndMs)}`;
      body = `<div class="svp-line" data-act="toggle" role="button" tabindex="0" aria-expanded="${expanded}" title="${esc(nextLabel)}. Click for the whole plan.">
          <span class="svp-ico">${over ? ICON.over : ICON.clock}</span><span class="svp-name">${esc(row.seg.name)}</span>${driftChip}
          <span class="svp-count${over ? ' red' : ''}">${over ? '+' : ''}${SP.clockText(over ? row.overSec : row.remainingSec)}</span>
          ${asked ? '' : `<button type="button" class="svp-skip" data-act="next" aria-label="${next ? 'Next segment' : 'Finish'}" title="${next ? 'Next segment' : 'Finish the service'}">${ICON.skip}</button>`}</div>
        <div class="svp-bar${over ? ' red' : ''}"><i style="width:${pct.toFixed(1)}%"></i></div>
        ${asked ? `<div class="svp-btns"><button type="button" data-act="next">${next ? 'Next' : 'Finish'}</button><button type="button" data-act="plus">+5 min</button><button type="button" data-act="keep">Keep going</button></div>` : ''}`;
    } else if (doneAll) {
      body = `<div class="svp-line" data-act="toggle" role="button" tabindex="0" aria-expanded="${expanded}"><span class="svp-ico">${ICON.check}</span><span class="svp-name">Service finished</span>
          <span class="svp-count">${plan.driftSec > 30 ? `+${SP.clockText(plan.driftSec)}` : plan.driftSec < -30 ? `-${SP.clockText(plan.driftSec)}` : 'on plan'}</span></div>
        <div class="svp-btns"><button type="button" data-act="reset">Run again</button></div>`;
    } else if (next) {
      body = `<div class="svp-line" data-act="toggle" role="button" tabindex="0" aria-expanded="${expanded}" title="Service ends ${SP.timeOfDay(plan.plannedEndMs)}. Click for the whole plan.">
          <span class="svp-ico">${ICON.clock}</span><span class="svp-name">${esc(next.seg.name)}</span><span class="svp-count dim">${startsIn(next.plannedStartMs)}</span>
          <button type="button" class="svp-go" data-act="next">Start</button></div>`;
    }

    const list = !expanded ? '' : `<div class="svp-plan">${plan.rows.map((r) => {
      let right;
      if (r.status === 'done') right = `<span class="${r.overSec > 0 ? 'red' : 'okc'}">ran ${SP.clockText(r.actualSec || 0)}</span>`;
      else if (r.status === 'live') right = r.overSec > 0 ? `<span class="red">+${SP.clockText(r.overSec)}</span>` : `<span>${SP.clockText(r.remainingSec)} left</span>`;
      else if (r.unplanned) right = '<span class="muted">no length</span>';
      else {
        const slid = Math.abs(r.projStartMs - r.plannedStartMs) >= 60000;
        right = `<span class="${slid ? 'warn' : ''}">${SP.timeOfDay(r.projStartMs)}${r.seg.fixedAt ? ' fixed' : ''}</span>`;
      }
      const icon = r.status === 'done' ? ICON.check : r.status === 'live' ? ICON.play : r.seg.fixedAt ? ICON.lock : ICON.circle;
      return `<div class="svp-row ${r.status}"><span class="svp-ico">${icon}</span><span class="svp-rname">${esc(r.seg.name)}</span>${right}</div>`;
    }).join('')}<div class="svp-foot"><span>Ends ${SP.timeOfDay(plan.projectedEndMs)}</span><button type="button" data-act="plan">Edit</button></div></div>`;

    el.innerHTML = `${body}${list}`;
  }

  async function act(name) {
    const S = svc();
    if (name === 'toggle') { expanded = !expanded; render(); return; }
    if (name === 'plan') { openEditor(); return; }
    if (name === 'keep') {
      const live = segments.find((s) => s.status === 'live');
      if (live) keptGoingFor = `${live.id}:${live.startedAt}`;
      render();
      return;
    }
    if (!S) return;
    let r;
    if (name === 'next') r = await S.planNext();
    else if (name === 'plus') r = await S.planExtend(300);
    else if (name === 'reset') {
      await Promise.all(segments.map((s) => fetch(`${SERVER}/api/segments/${s.id}/reset`, { method: 'POST' }).catch(() => {})));
      await S.loadSegments();
      return;
    }
    if (r && r.needsLength) { problem(r.error); openEditor(); }
    else if (r && !r.ok && r.error) problem(r.error);
    await S.loadSegments();
  }

  // ── the order of service editor ────────────────────────────────────────────────────────────────
  // Include or leave out a segment, add one, delete one, set lengths and fixed times, reorder. Saving applies at once to
  // a service already running: the strip's times, drift and end time are recalculated from the new list.
  function openEditor() {
    if ($('svp-editor')) return;
    const sorted = [...segments].sort((a, b) => (a.order || 0) - (b.order || 0));
    const wrap = document.createElement('div');
    wrap.id = 'svp-editor';
    wrap.className = 'svp-editor-back';
    const minutes = (s) => { const l = SP.lengthSec(s); return l == null ? '' : String(Math.round(l / 6) / 10); };
    const hhmm = (v) => (v && /^\d{1,2}:\d{2}$/.test(v) ? String(v).padStart(5, '0') : '');
    const rowHtml = (s) => `<div class="svp-erow${s && s.included === false ? ' out' : ''}" ${s ? `data-id="${esc(s.id)}"` : 'data-new="1"'}>
        <label class="svp-inc" title="In the service. Switch off to leave it out of the plan and Next (it stays in the Timer tab)."><input type="checkbox" class="svp-include" aria-label="In the service" ${!s || s.included !== false ? 'checked' : ''}></label>
        ${s ? `<span class="svp-ename">${esc(s.name)}</span>` : '<input type="text" class="svp-newname" aria-label="Segment name" placeholder="Segment name" maxlength="60">'}
        <input type="number" min="0" step="0.5" class="svp-len" aria-label="Length in minutes" value="${s ? esc(minutes(s)) : ''}" placeholder="min">
        <input type="time" class="svp-fixed" aria-label="Fixed start time" value="${s ? esc(hhmm(s.fixedAt)) : ''}">
        <span class="svp-ewhen"></span>
        <button type="button" class="svp-up" aria-label="Move up">&uarr;</button>
        <button type="button" class="svp-down" aria-label="Move down">&darr;</button>
        <button type="button" class="svp-del" aria-label="Delete segment" title="Delete this segment">Delete</button></div>`;
    wrap.innerHTML = `<div class="svp-editor" role="dialog" aria-label="Order of service">
      <div class="svp-etop"><h3>Order of service</h3><button type="button" id="svp-add" class="svp-addbtn">+ Add segment</button></div>
      <div class="svp-field">
        <label>Starts at <input type="time" id="svp-start" value="${esc(hhmm(serviceStart))}"></label>
        <button type="button" id="svp-usenow" class="svp-addbtn">Use now</button>
        <span class="svp-startnote" id="svp-startnote"></span>
        <span class="svp-devtime">Now <b id="svp-now"></b></span>
      </div>
      <label class="svp-opt" title="The service starts itself at its start time, and each fixed segment at its time, by the device's clock."><input type="checkbox" id="svp-clock" ${runByClock ? 'checked' : ''}> Auto-Start</label>
      <div class="svp-cols"><span>In</span><span>Segment</span><span>Length (min)</span><span title="Starts at this time, whatever came before">Fixed at</span><span>Planned</span><span></span><span></span><span></span></div>
      <div class="svp-rows" id="svp-rows">${sorted.map(rowHtml).join('')}</div>
      <div class="svp-ebtns"><button type="button" id="svp-cancel">Cancel</button><button type="button" id="svp-save" class="primary">Save order of service</button></div></div>`;
    document.body.appendChild(wrap);

    const rowsEl = () => wrap.querySelector('#svp-rows');
    const allRows = () => [...rowsEl().querySelectorAll('.svp-erow')];
    const live = (r) => !r.classList.contains('gone');
    const read = (r) => ({ el: r, id: r.dataset.id || null, isNew: !r.dataset.id, name: (r.querySelector('.svp-newname') || {}).value || '',
      include: r.querySelector('.svp-include').checked, len: parseFloat(r.querySelector('.svp-len').value), fixed: r.querySelector('.svp-fixed').value });
    const tidy = () => {
      const rows = allRows();
      rows.forEach((r, i) => { r.querySelector('.svp-up').disabled = i === 0; r.querySelector('.svp-down').disabled = i === rows.length - 1; });
    };
    const preview = () => {
      const start = SP.clockToMs($('svp-start').value, Date.now()) ?? Date.now();
      const rows = allRows().filter(live).map(read);
      const fake = rows.map((x, i) => {
        const base = x.id ? sorted.find((s) => s.id === x.id) : { id: `new${i}`, name: x.name };
        return { ...base, order: i, included: x.include, fixedAt: x.fixed || null, trigger: { params: { mode: 'duration', durationSec: x.len > 0 ? x.len * 60 : 0 } } };
      });
      const plan = SP.buildPlan(fake.map((f) => ({ ...f, status: f.status === 'live' ? 'pending' : f.status === 'done' ? 'pending' : f.status || 'pending' })), start, Date.now());
      allRows().forEach((r) => { r.querySelector('.svp-ewhen').textContent = ''; });
      plan.rows.forEach((pr) => {
        const row = rows.find((x) => (x.id || `new${rows.indexOf(x)}`) === pr.seg.id);
        if (row && !pr.unplanned) row.el.querySelector('.svp-ewhen').textContent = `${SP.timeOfDay(pr.plannedStartMs)} to ${SP.timeOfDay(pr.plannedEndMs)}`;
      });
    };
    wrap.addEventListener('input', (e) => {
      if (e.target.classList.contains('svp-include')) e.target.closest('.svp-erow').classList.toggle('out', !e.target.checked);
      preview();
    });
    wrap.addEventListener('click', (e) => {
      if (e.target === wrap) { wrap.remove(); return; }
      const btn = e.target.closest('button');
      if (!btn) return;
      const row = btn.closest('.svp-erow');
      if (row && btn.classList.contains('svp-up') && row.previousElementSibling) row.parentNode.insertBefore(row, row.previousElementSibling);
      if (row && btn.classList.contains('svp-down') && row.nextElementSibling) row.parentNode.insertBefore(row.nextElementSibling, row);
      if (row && btn.classList.contains('svp-del')) {
        if (!row.dataset.id) row.remove();                 // one just added: gone at once
        else { row.classList.toggle('gone'); btn.textContent = row.classList.contains('gone') ? 'Undo' : 'Delete'; }
      }
      if (btn.id === 'svp-add') {
        // Goes in after the segment now running (or the last one finished), so a late addition slots into the service where it is.
        const rows = allRows();
        let at = -1;
        rows.forEach((r, i) => { const s = sorted.find((x) => x.id === r.dataset.id); if (s && (s.status === 'live' || s.status === 'done')) at = i; });
        const holder = document.createElement('div');
        holder.innerHTML = rowHtml(null);
        const fresh = holder.firstElementChild;
        if (at >= 0 && rows[at].nextElementSibling) rows[at].parentNode.insertBefore(fresh, rows[at].nextElementSibling); else rowsEl().appendChild(fresh);
        fresh.querySelector('.svp-newname').focus();
      }
      if (btn.id === 'svp-cancel') wrap.remove();
      if (btn.id === 'svp-save') save();
      tidy();
      preview();
    });
    document.addEventListener('keydown', function esc1(e) { if (e.key === 'Escape') { wrap.remove(); document.removeEventListener('keydown', esc1); } });
    // The device's own clock, live, so a start or fixed time is read against the time it really is.
    const clockNote = () => {
      if (!document.body.contains(wrap)) { clearInterval(clockTimer); return; }
      const now = Date.now();
      $('svp-now').textContent = new Date(now).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' });
      const start = SP.clockToMs($('svp-start').value, now);
      const note = $('svp-startnote');
      if (start == null) { note.textContent = ''; note.className = 'svp-startnote'; return; }
      const secs = Math.round((start - now) / 1000);
      if (secs > 0) { note.textContent = `in ${SP.clockText(secs)}`; note.className = 'svp-startnote'; }
      else if (secs > -60) { note.textContent = ''; note.className = 'svp-startnote'; }   // times are to the minute
      else { note.textContent = 'already passed'; note.className = 'svp-startnote late'; }
      // A fixed time that has already passed won't start anything.
      allRows().forEach((r) => {
        const f = SP.clockToMs(r.querySelector('.svp-fixed').value, now);
        r.querySelector('.svp-fixed').classList.toggle('past', f != null && f < now);
      });
    };
    const clockTimer = setInterval(clockNote, 1000);
    wrap.addEventListener('input', clockNote);
    wrap.addEventListener('click', (e) => {
      if (e.target.id === 'svp-usenow') {
        const d = new Date();
        $('svp-start').value = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
        preview();
        clockNote();
      }
    });
    clockNote();
    tidy();
    preview();

    async function save() {
      const rows = allRows().map(read);
      const start = $('svp-start').value || '';
      const json = { 'Content-Type': 'application/json' };
      const named = rows.filter((r) => !r.el.classList.contains('gone')).every((r) => !r.isNew || r.name.trim());
      if (!named) { problem('Give each new segment a name.'); return; }
      try {
        const byClock = $('svp-clock').checked;
        await fetch(`${SERVER}/api/settings`, { method: 'POST', headers: json, body: JSON.stringify({ serviceStart: start, serviceRunByClock: byClock }) });
        serviceStart = start;
        runByClock = byClock;
        // Deleted, then added (so they have an id), then put in order and given their lengths and fixed times.
        for (const r of rows.filter((x) => x.el.classList.contains('gone') && x.id)) await fetch(`${SERVER}/api/segments/${r.id}`, { method: 'DELETE' });
        const kept = rows.filter((x) => !x.el.classList.contains('gone'));
        for (const r of kept.filter((x) => x.isNew)) {
          const res = await fetch(`${SERVER}/api/segments`, { method: 'POST', headers: json, body: JSON.stringify({ name: r.name.trim() }) });
          const j = await res.json();
          r.id = j.segment && j.segment.id;
        }
        await fetch(`${SERVER}/api/segments/reorder`, { method: 'POST', headers: json, body: JSON.stringify({ orderedIds: kept.map((r) => r.id).filter(Boolean) }) });
        for (const r of kept.filter((x) => x.id)) {
          const body = { fixedAt: r.fixed || null, included: r.include };
          if (r.len > 0) body.params = { mode: 'duration', durationSec: Math.round(r.len * 60) };
          await fetch(`${SERVER}/api/segments/${r.id}`, { method: 'PUT', headers: json, body: JSON.stringify(body) });
        }
        wrap.remove();
        await svc()?.loadSegments();
        svc()?.refreshTimerGrid?.();
      } catch { problem('The order of service could not be saved. Check that Kairo is running, then try again.'); }
    }
  }

  // ── wiring ────────────────────────────────────────────────────────────────────────────────────
  window.KairoServiceStrip = {
    segmentsChanged(list) { segments = list || []; if (!segments.some((s) => s.status === 'live')) keptGoingFor = null; render(); },
    tick(msg) {
      const p = msg && msg.payload;
      if (!p || p.cleared || typeof p.remainingMs !== 'number') return;
      lastTick = { triggerId: msg.triggerId, remainingMs: p.remainingMs, at: Date.now() };
      render();
    },
    openEditor,
  };

  // The strip is redrawn every second while a segment runs, which swallows a click whose press and release fall on
  // either side of a redraw. So its buttons act on the press (a keyboard press is handled below), once.
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const strip = $('svp-strip');
    if (strip && strip.contains(e.target)) {
      const b = e.target.closest('[data-act]');
      if (b) { e.preventDefault(); act(b.dataset.act); }
    }
  });
  document.addEventListener('click', (e) => {
    if (e.target.closest && e.target.closest('#segment-plan-btn')) openEditor();
  });
  document.addEventListener('keydown', (e) => {
    if ((e.key === 'Enter' || e.key === ' ') && e.target.closest) { const b = e.target.closest('#svp-strip [data-act]'); if (b) { e.preventDefault(); act(b.dataset.act); } }
  });

  async function boot() {
    await loadSetting();
    await svc()?.loadSegments();
    // Keeps overtime and the projected end moving between server ticks, and picks up changes made elsewhere.
    timer = setInterval(() => { clockTick(); if (segments.some((s) => s.status === 'live') || (planned() && segments.every((s) => s.status === 'pending'))) render(); }, 1000);
    setInterval(() => svc()?.loadSegments(), 15000);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', () => setTimeout(boot, 1500));
  else setTimeout(boot, 1500);
})();
