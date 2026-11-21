// Tests for src/media_release.js: a removed video is stopped and emptied; a moved one is not.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { onMutations, mediaIn, release } = require('./media_release.js');

// Just enough of a media element: what release() calls, and whether it is in the page.
function video(connected = false) {
  const attrs = new Map([['src', 'clip.mp4']]);
  return {
    nodeType: 1, tagName: 'VIDEO', isConnected: connected, paused: false, loads: 0, srcObject: null,
    pause() { this.paused = true; },
    load() { this.loads++; },
    hasAttribute: (k) => attrs.has(k),
    removeAttribute: (k) => attrs.delete(k),
    get src() { return attrs.get('src') || ''; },
  };
}
const holder = (...children) => ({
  nodeType: 1, tagName: 'DIV',
  getElementsByTagName: (t) => (t === 'video' ? children : []),
});

test('a removed video is paused, emptied and reloaded so it lets go of its frames', () => {
  const v = video(false);
  onMutations([{ removedNodes: [v] }]);
  assert.equal(v.paused, true);
  assert.equal(v.src, '');
  assert.equal(v.loads, 1);
});

test('a video inside a removed container is released too', () => {
  const a = video(false), b = video(false);
  onMutations([{ removedNodes: [holder(a, b)] }]);
  assert.deepEqual([a.src, b.src], ['', '']);
});

test('a video moved elsewhere (back in the page) keeps playing', () => {
  const v = video(true);
  onMutations([{ removedNodes: [v] }]);
  assert.equal(v.paused, false);
  assert.equal(v.src, 'clip.mp4');
});

test('text and other nodes are ignored', () => {
  assert.deepEqual(mediaIn({ nodeType: 3 }), []);
  assert.deepEqual(mediaIn(null), []);
});

test('a live camera stream is detached as well', () => {
  const v = video(false); v.srcObject = {};
  release(v);
  assert.equal(v.srcObject, null);
});
