// KAIRO — the order of service as a plan: each segment has a length (or an end time) in a set order, the service
// has a start time, and this works out the planned clock times, what has really happened, where the service is
// projected to end, and how far behind (or ahead) of plan it is. No UI and no clock of its own: everything is
// passed in, so it can be tested with fixed times (service_plan.test.js).
//
// A segment, as the server keeps it:
//   { id, name, order, status: 'pending'|'live'|'done', startedAt, endedAt (ms), extendedSec,
//     fixedAt: 'HH:MM'|null,   // starts at this clock time whatever came before
//     trigger: { params: { mode: 'duration'|'endAt', durationSec, endAtTime: 'HH:MM' } } }
'use strict';

(function (root) {
  // 'HH:MM' on the day of `dayMs` -> ms; null for anything else.
  function clockToMs(hhmm, dayMs) {
    const m = /^(\d{1,2}):(\d{2})$/.exec(String(hhmm || '').trim());
    if (!m) return null;
    const d = new Date(dayMs);
    d.setHours(Number(m[1]), Number(m[2]), 0, 0);
    return d.getTime();
  }

  // Planned length in seconds, or null when the segment only has an end time (or nothing).
  function lengthSec(seg) {
    const p = (seg && seg.trigger && seg.trigger.params) || {};
    if (p.mode === 'duration' && Number(p.durationSec) > 0) return Number(p.durationSec);
    return null;
  }

  // segments (in any order), serviceStartMs (ms), nowMs (ms) ->
  //   { rows, plannedEndMs, projectedEndMs, driftSec, liveIndex, nextIndex }
  // A row: { seg, lengthSec, plannedStartMs, plannedEndMs, projStartMs, projEndMs, squeezedSec,
  //          status, elapsedSec, overSec, remainingSec, actualSec, unplanned }
  function buildPlan(segments, serviceStartMs, nowMs) {
    // A segment left out of the service (included: false) isn't in the plan, unless it is the one running.
    const list = [...(segments || [])].filter((s) => s.included !== false || s.status === 'live').sort((a, b) => (a.order || 0) - (b.order || 0));
    const rows = [];
    let plannedCursor = serviceStartMs;
    let projCursor = serviceStartMs;
    for (const seg of list) {
      const len = lengthSec(seg);
      const fixedMs = clockToMs(seg.fixedAt, serviceStartMs);
      const endAtMs = len == null && seg.trigger && seg.trigger.params && seg.trigger.params.mode !== 'duration'
        ? clockToMs(seg.trigger.params.endAtTime, serviceStartMs) : null;

      const plannedStartMs = fixedMs != null ? fixedMs : plannedCursor;
      const plannedEndMs = len != null ? plannedStartMs + len * 1000 : (endAtMs != null && endAtMs > plannedStartMs ? endAtMs : plannedStartMs);
      plannedCursor = plannedEndMs;

      const planLenMs = plannedEndMs - plannedStartMs;
      const extendedMs = (Number(seg.extendedSec) || 0) * 1000;
      const row = { seg, lengthSec: len, plannedStartMs, plannedEndMs, status: seg.status || 'pending',
        unplanned: planLenMs <= 0, squeezedSec: 0, elapsedSec: 0, overSec: 0, remainingSec: null, actualSec: null };

      if (row.status === 'done' && seg.startedAt) {
        row.projStartMs = seg.startedAt;
        row.projEndMs = seg.endedAt || seg.startedAt;
        row.actualSec = Math.round((row.projEndMs - row.projStartMs) / 1000);
        row.overSec = Math.max(0, row.actualSec - Math.round((planLenMs + extendedMs) / 1000));
        projCursor = row.projEndMs;
      } else if (row.status === 'live' && seg.startedAt) {
        row.projStartMs = seg.startedAt;
        row.elapsedSec = Math.max(0, Math.round((nowMs - seg.startedAt) / 1000));
        const allowedMs = planLenMs + extendedMs;
        const leftMs = seg.startedAt + allowedMs - nowMs;
        row.remainingSec = Math.round(leftMs / 1000);
        row.overSec = leftMs < 0 ? Math.round(-leftMs / 1000) : 0;
        // Still going: it ends at its time, or, once over, no earlier than now.
        row.projEndMs = Math.max(nowMs, seg.startedAt + allowedMs);
        projCursor = row.projEndMs;
      } else {
        // Waiting: it follows the one before, unless it is fixed to a clock time.
        let start = projCursor;
        if (fixedMs != null) {
          if (projCursor > fixedMs) row.squeezedSec = Math.round((projCursor - fixedMs) / 1000);
          start = fixedMs;
        }
        row.projStartMs = start;
        row.projEndMs = start + planLenMs;
        projCursor = row.projEndMs;
      }
      rows.push(row);
    }
    const last = rows[rows.length - 1];
    const plannedEndMs = last ? last.plannedEndMs : serviceStartMs;
    const projectedEndMs = last ? last.projEndMs : serviceStartMs;
    return {
      rows,
      plannedEndMs,
      projectedEndMs,
      driftSec: Math.round((projectedEndMs - plannedEndMs) / 1000),
      liveIndex: rows.findIndex((r) => r.status === 'live'),
      nextIndex: rows.findIndex((r) => r.status === 'pending'),
    };
  }

  // 125 -> "2:05", 3725 -> "1:02:05"
  function clockText(sec) {
    const s = Math.max(0, Math.round(Math.abs(sec)));
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
    const pad = (n) => String(n).padStart(2, '0');
    return h ? `${h}:${pad(m)}:${pad(r)}` : `${m}:${pad(r)}`;
  }
  function timeOfDay(ms) {
    const d = new Date(ms);
    return `${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')}`;
  }

  const API = { buildPlan, lengthSec, clockToMs, clockText, timeOfDay };
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoServicePlan = API;
})(typeof window !== 'undefined' ? window : null);
