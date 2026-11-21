// Tests for src/input_health.js: when the on-screen input warning comes and goes.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { InputHealth, SILENT_MS, CLEAR_CLIP_MS } = require('./input_health.js');

// Reports ten a second from `from` for `ms`, each with `peak` and `clipped`; returns the last state.
function feed(h, from, ms, peak, clipped = false) {
  let s = 'ok';
  for (let t = from; t < from + ms; t += 100) s = h.note(peak, clipped, t);
  return s;
}

test('speech with ordinary pauses never warns', () => {
  const h = new InputHealth(); h.reset(0);
  let t = 0;
  for (let i = 0; i < 20; i++) {
    assert.equal(feed(h, t, 3000, 0.3), 'ok'); t += 3000;
    assert.equal(feed(h, t, 3000, 0.0005), 'ok'); t += 3000;   // a 3 s pause
  }
});

test('a feed that falls to hiss warns after several seconds and clears when sound returns', () => {
  const h = new InputHealth(); h.reset(0);
  feed(h, 0, 5000, 0.4);
  assert.equal(feed(h, 5000, SILENT_MS - 200, 0.001), 'ok');
  assert.equal(feed(h, 5000 + SILENT_MS - 200, 400, 0.001), 'silent');
  assert.equal(h.note(0.2, false, 20000), 'ok');
});

test('reports that stop altogether count as silence', () => {
  const h = new InputHealth(); h.reset(0);
  feed(h, 0, 2000, 0.3);
  assert.equal(h.state(2000 + SILENT_MS + 100), 'silent');
});

test('a silent start warns too, but not before the input had its seconds', () => {
  const h = new InputHealth(); h.reset(1000);
  assert.equal(h.state(1000 + SILENT_MS - 1), 'ok');
  assert.equal(h.state(1000 + SILENT_MS), 'silent');
});

test('one shout does not warn; a hot feed does, and clears once it has been fine for a while', () => {
  const h = new InputHealth(); h.reset(0);
  feed(h, 0, 2000, 0.5);
  assert.equal(feed(h, 2000, 300, 1, true), 'ok');           // three clipped reports
  assert.equal(feed(h, 2300, 3000, 0.5), 'ok');
  // the church PC: a third of the readings at full scale
  let s;
  for (let t = 10000; t < 15000; t += 100) s = h.note(t % 300 === 0 ? 1 : 0.6, t % 300 === 0, t);
  assert.equal(s, 'clipping');
  assert.equal(feed(h, 15000, CLEAR_CLIP_MS - 500, 0.5), 'clipping');
  assert.equal(feed(h, 15000 + CLEAR_CLIP_MS - 500, 1000, 0.5), 'ok');
});

test('silence wins over clipping', () => {
  const h = new InputHealth(); h.reset(0);
  feed(h, 0, 3000, 1, true);
  assert.equal(feed(h, 3000, SILENT_MS + 200, 0), 'silent');
});
