// Tests for src/service_plan.js — plain node:test, fixed times, no DOM.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { buildPlan, clockText, timeOfDay } = require('./service_plan.js');

const DAY = new Date(2026, 10, 15, 0, 0, 0).getTime();   // a Sunday
const at = (h, m = 0, s = 0) => new Date(2026, 10, 15, h, m, s).getTime();
const seg = (id, name, order, min, extra = {}) => ({ id, name, order, status: 'pending',
  trigger: { params: { mode: 'duration', durationSec: min * 60 } }, ...extra });

const SERVICE = () => [seg('a', 'Preservice', 0, 20), seg('b', 'Opening prayer', 1, 5), seg('c', 'Worship', 2, 30), seg('d', 'The Word', 3, 45)];

test('planned times follow the order, from the service start', () => {
  const p = buildPlan(SERVICE(), at(8, 40), at(8, 0));
  assert.deepEqual(p.rows.map((r) => [timeOfDay(r.plannedStartMs), timeOfDay(r.plannedEndMs)]),
    [['8:40', '9:00'], ['9:00', '9:05'], ['9:05', '9:35'], ['9:35', '10:20']]);
  assert.equal(timeOfDay(p.plannedEndMs), '10:20');
  assert.equal(p.driftSec, 0);
  assert.equal(p.nextIndex, 0);
  assert.equal(p.liveIndex, -1);
});

test('segments are put in order by their order field, not their position in the list', () => {
  const s = SERVICE().reverse();
  assert.deepEqual(buildPlan(s, at(8, 40), at(8, 0)).rows.map((r) => r.seg.name), ['Preservice', 'Opening prayer', 'Worship', 'The Word']);
});

test('a segment that ran long pushes everything after it: drift and the projected end', () => {
  const s = SERVICE();
  s[0] = { ...s[0], status: 'done', startedAt: at(8, 40), endedAt: at(9, 0) };
  s[1] = { ...s[1], status: 'done', startedAt: at(9, 0), endedAt: at(9, 4, 50) };
  s[2] = { ...s[2], status: 'done', startedAt: at(9, 4, 50), endedAt: at(9, 37, 0) };   // 32:10 against 30:00
  s[3] = { ...s[3], status: 'live', startedAt: at(9, 37, 0) };
  const p = buildPlan(s, at(8, 40), at(9, 40));
  assert.equal(p.rows[2].actualSec, 32 * 60 + 10);
  assert.equal(p.rows[2].overSec, 130);
  assert.equal(p.liveIndex, 3);
  assert.equal(p.rows[3].remainingSec, 45 * 60 - 3 * 60);
  assert.equal(p.driftSec, 120);                           // worship ran 2:10 over, the prayer 0:10 short: the Word started 2:00 late
  assert.equal(timeOfDay(p.projectedEndMs), '10:22');
});

test('past its time the live segment counts overtime, and the projected end is no earlier than now', () => {
  const s = SERVICE();
  s[0] = { ...s[0], status: 'live', startedAt: at(8, 40) };
  const p = buildPlan(s, at(8, 40), at(9, 4, 10));         // 24:10 into a 20:00 segment
  assert.equal(p.rows[0].remainingSec, -250);
  assert.equal(p.rows[0].overSec, 250);
  assert.equal(p.driftSec, 250);
  assert.equal(timeOfDay(p.rows[1].projStartMs), '9:04');
});

test('"Add 5 min" gives the live segment more time, so it is no longer over', () => {
  const s = SERVICE();
  s[0] = { ...s[0], status: 'live', startedAt: at(8, 40), extendedSec: 300 };
  const p = buildPlan(s, at(8, 40), at(9, 4, 10));
  assert.equal(p.rows[0].overSec, 0);
  assert.equal(p.rows[0].remainingSec, 50);
  assert.equal(p.driftSec, 300);                           // five minutes later than planned, as chosen
});

test('a fixed segment keeps its clock time, and says how much it was squeezed', () => {
  const s = SERVICE();
  s[3] = { ...s[3], fixedAt: '9:30' };
  s[2] = { ...s[2], status: 'live', startedAt: at(9, 5) };
  const p = buildPlan(s, at(8, 40), at(9, 40));            // worship is 5 minutes over and the Word is fixed at 9:30
  assert.equal(timeOfDay(p.rows[3].projStartMs), '9:30');
  assert.equal(p.rows[3].squeezedSec, 10 * 60);
});

test('a segment with only an end time is planned to end then', () => {
  const s = [seg('a', 'Prayer', 0, 5), { id: 'b', name: 'Announcements', order: 1, status: 'pending', trigger: { params: { mode: 'endAt', endAtTime: '9:20' } } }];
  const p = buildPlan(s, at(9, 0), at(8, 0));
  assert.equal(timeOfDay(p.rows[1].plannedEndMs), '9:20');
});

test('a segment with no length is flagged and takes no time', () => {
  const s = [seg('a', 'Prayer', 0, 5), { id: 'b', name: 'Offering', order: 1, status: 'pending', trigger: { params: {} } }];
  const p = buildPlan(s, at(9, 0), at(8, 0));
  assert.equal(p.rows[1].unplanned, true);
  assert.equal(timeOfDay(p.plannedEndMs), '9:05');
});

test('clock text', () => {
  assert.equal(clockText(125), '2:05');
  assert.equal(clockText(-250), '4:10');
  assert.equal(clockText(3725), '1:02:05');
});

test('a segment left out of the service is not in the plan, and the times close up', () => {
  const s = SERVICE();
  s[1] = { ...s[1], included: false };
  const p = buildPlan(s, at(8, 40), at(8, 0));
  assert.deepEqual(p.rows.map((r) => r.seg.name), ['Preservice', 'Worship', 'The Word']);
  assert.equal(timeOfDay(p.plannedEndMs), '10:15');
});

test('a segment added while the service is running takes its place and moves the end time', () => {
  const s = SERVICE();
  s[0] = { ...s[0], status: 'done', startedAt: at(8, 40), endedAt: at(9, 0) };
  s[1] = { ...s[1], status: 'live', startedAt: at(9, 0) };
  const before = buildPlan(s, at(8, 40), at(9, 2));
  s.push(seg('e', 'Offering', 1.5, 10));                    // put between the prayer and worship
  const after = buildPlan(s, at(8, 40), at(9, 2));
  assert.deepEqual(after.rows.map((r) => r.seg.name), ['Preservice', 'Opening prayer', 'Offering', 'Worship', 'The Word']);
  assert.equal(after.projectedEndMs - before.projectedEndMs, 10 * 60 * 1000);
});
