// Regression tests for src/queue_order.js — plain node:test, no DOM.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const Q = require('./queue_order.js');

test('a passage read verse by verse stays in reading order', () => {
  // 2 Corinthians 4:16, then 17, then 18 (the 2026-10-14 screenshot showed 18, 17, 16).
  let refs = [];
  for (const ref of ['2 Corinthians 4:16', '2 Corinthians 4:17', '2 Corinthians 4:18']) {
    const p = Q.placeInQueue(refs, ref);
    if (p.action === 'top') refs = [ref, ...refs];
    else if (p.action === 'after') refs.splice(p.index + 1, 0, ref);
  }
  assert.deepEqual(refs, ['2 Corinthians 4:16', '2 Corinthians 4:17', '2 Corinthians 4:18']);
});

test('the last verse of a finished range stays last', () => {
  const refs = ['Psalms 63:1', 'Psalms 63:2', 'Psalms 63:3', 'John 3:16'];
  assert.deepEqual(Q.placeInQueue(refs, 'Psalms 63:3'), { action: 'stay' });
  assert.deepEqual(Q.placeInQueue(refs, 'Psalms 63:2'), { action: 'stay' });
});

test('reading on past a range joins the end of it', () => {
  const refs = ['Psalms 63:1', 'Psalms 63:2', 'Psalms 63:3', 'John 3:16'];
  assert.deepEqual(Q.placeInQueue(refs, 'Psalms 63:4'), { action: 'after', index: 2 });
});

test('anything else is a new send at the top', () => {
  const refs = ['Psalms 63:1', 'Psalms 63:2', 'John 3:16'];
  assert.deepEqual(Q.placeInQueue(refs, 'Psalms 63:5'), { action: 'top' });
  assert.deepEqual(Q.placeInQueue(refs, 'Psalms 64:3'), { action: 'top' });
  assert.deepEqual(Q.placeInQueue(refs, 'John 3:16'), { action: 'top' }); // below the passage: comes back up
  assert.deepEqual(Q.placeInQueue(refs, 'Romans 8:28'), { action: 'top' });
  assert.deepEqual(Q.placeInQueue([], 'Romans 8:28'), { action: 'top' });
});

test('the top passage ends at a gap, a different chapter or a different book', () => {
  assert.equal(Q.topPassageLength(['John 1:1', 'John 1:2', 'John 1:4']), 2);
  assert.equal(Q.topPassageLength(['John 1:1', 'John 2:2']), 1);
  assert.equal(Q.topPassageLength(['1 John 1:1', 'John 1:2']), 1);
  assert.equal(Q.topPassageLength(['John 1:2', 'John 1:1']), 1); // newest-first is not a passage
});

test('numbered sibling books count as the same verse', () => {
  assert.ok(Q.sameVerseAnyBookNumber('2 Corinthians 4:16', '1 Corinthians 4:16'));
  assert.ok(Q.sameVerseAnyBookNumber('1 John 3:16', 'John 3:16'));
  assert.ok(Q.sameVerseAnyBookNumber('II Kings 2:11', '1 Kings 2:11'));
  assert.ok(!Q.sameVerseAnyBookNumber('2 Corinthians 4:16', '1 Corinthians 4:17'));
  assert.ok(!Q.sameVerseAnyBookNumber('2 Corinthians 4:16', '2 Timothy 4:16'));
  assert.ok(!Q.sameVerseAnyBookNumber('Song of Solomon 1:1', 'Solomon 1:1'));
});

test('a queued verse keeps its look-alikes out of Possible Matches for the window', () => {
  const now = 1_000_000_000;
  const queued = [{ ref: '2 Corinthians 4:16', at: now - 60_000 }];
  assert.ok(Q.coveredByQueue('1 Corinthians 4:16', queued, now));
  assert.ok(Q.coveredByQueue('2 Corinthians 4:16', queued, now));
  assert.ok(!Q.coveredByQueue('1 Corinthians 4:15', queued, now));
  const old = [{ ref: '2 Corinthians 4:16', at: now - Q.OVERLAP_WINDOW_MS - 1 }];
  assert.ok(!Q.coveredByQueue('1 Corinthians 4:16', old, now));  // window passed
  assert.ok(Q.coveredByQueue('2 Corinthians 4:16', old, now));   // the exact verse: while it's queued
});
