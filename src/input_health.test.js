// Tests for src/input_health.js: when the on-screen clipping warning comes and goes.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { InputHealth, CLEAR_CLIP_MS } = require('./input_health.js');

// Reports ten a second from `from` for `ms`, each with `peak` and `clipped`; returns the last state.
function feed(h, from, ms, peak, clipped = false) {
  let s = 'ok';
  for (let t = from; t < from + ms; t += 100) s = h.note(peak, clipped, t);
  return s;
}

test('speech, pauses and silence never warn', () => {
  const h = new InputHealth(); h.reset();
  let t = 0;
  for (let i = 0; i < 10; i++) {
    assert.equal(feed(h, t, 3000, 0.3), 'ok'); t += 3000;
    assert.equal(feed(h, t, 3000, 0), 'ok'); t += 3000;
  }
  assert.equal(h.state(t + 60000), 'ok', 'reports that stop say nothing either: the No audio! pill covers silence');
});

test('one shout does not warn; a hot feed does, and clears once it has been fine for a while', () => {
  const h = new InputHealth(); h.reset();
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

test('a new session starts clean', () => {
  const h = new InputHealth(); h.reset();
  feed(h, 0, 2000, 1, true);
  assert.equal(h.state(2000), 'clipping');
  h.reset();
  assert.equal(h.state(2100), 'ok');
});
