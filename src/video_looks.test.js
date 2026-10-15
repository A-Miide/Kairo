// Tests for src/video_looks.js — the saved Layouts of Video director mode (pure parts only).
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { normalize, LABEL } = require('./video_looks.js');

test('the feature name lives in one constant', () => {
  assert.deepEqual(LABEL, { many: 'Layouts', one: 'Layout' });
});

test('normalize keeps a clean Layout and fills the gaps', () => {
  const list = normalize([{ id: 'a', name: '  Sunday   opener ', videoId: '@first', layers: { bible: true, video: false }, overlays: ['x', 5], themeId: 'screen-split-left' }]);
  assert.equal(list.length, 1);
  assert.equal(list[0].name, 'Sunday opener');
  assert.equal(list[0].videoId, '@first');
  assert.deepEqual(list[0].layers, { bible: true, slide: true, media: true, timer: true, video: false });
  assert.deepEqual(list[0].overlays, ['x']);
  assert.equal(list[0].themeId, 'screen-split-left');
});

test('normalize drops junk, de-duplicates ids and defaults safely', () => {
  const list = normalize([null, 'x', { id: 'a', name: 'One' }, { id: 'a', name: 'Two' }, { name: '' }]);
  assert.equal(list.length, 3);
  assert.equal(new Set(list.map(l => l.id)).size, 3);
  assert.equal(list[2].name, 'Layout');
  assert.equal(list[0].themeId, null);
  assert.equal(list[0].overlays, null);
  assert.equal(list[0].videoId, null);
  assert.equal(normalize('nope'), null);
});

test('normalize caps long names and ignores a non-string themeId', () => {
  const list = normalize([{ id: 'a', name: 'x'.repeat(100), themeId: 42 }]);
  assert.equal(list[0].name.length, 40);
  assert.equal(list[0].themeId, null);
});
