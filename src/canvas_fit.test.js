// Tests for layer_geometry.js canvasFitRect: where a design of one shape sits
// on an output of another, and how much it is scaled, for each output's Fit.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const sandbox = { window: {} };
vm.runInNewContext(fs.readFileSync(path.join(__dirname, 'layer_geometry.js'), 'utf8'), sandbox);
const { canvasFitRect } = sandbox.window;

const near = (a, b, msg) => assert.ok(Math.abs(a - b) < 0.01, `${msg}: ${a} != ${b}`);
const HD = { w: 1920, h: 1080 }, PORTRAIT = { w: 1080, h: 1920 }, SQUARE = { w: 1080, h: 1080 };
const WIDE_LED = { w: 3840, h: 1080 };

test('a 16:9 design on a 16:9 output fills it at any resolution', () => {
  for (const [W, H] of [[1920, 1080], [1280, 720], [3840, 2160]]) {
    for (const fit of ['contain', 'cover']) {
      const r = canvasFitRect(W, H, HD, fit);
      assert.deepEqual([r.left, r.top, r.w, r.h], [0, 0, W, H]);
      near(r.k, H / 1080, `${W}x${H} scale`);
    }
  }
});

test('no canvas size is treated as 16:9', () => {
  assert.deepEqual(canvasFitRect(1920, 1080, undefined, 'contain'), canvasFitRect(1920, 1080, HD, 'contain'));
  assert.deepEqual(canvasFitRect(1920, 1080, { w: 0, h: 0 }, 'contain'), canvasFitRect(1920, 1080, HD, 'contain'));
});

test('text keeps its proportion to the canvas on a smaller screen (720p projector)', () => {
  const r = canvasFitRect(1280, 720, HD, 'contain');
  near(r.k, 2 / 3, '100 px text draws 66.7 px');
  assert.equal(r.uw, 1920); assert.equal(r.uh, 1080);
});

test('a portrait design on a landscape screen: Contain pillarboxes, Cover crops top and bottom', () => {
  const c = canvasFitRect(1920, 1080, PORTRAIT, 'contain');
  near(c.h, 1080, 'full height'); near(c.w, 607.5, 'narrow box'); near(c.left, 656.25, 'centred'); assert.equal(c.top, 0);
  const v = canvasFitRect(1920, 1080, PORTRAIT, 'cover');
  near(v.w, 1920, 'full width'); near(v.h, 3413.33, 'tall box'); assert.ok(v.top < 0, 'top cropped');
});

test('a 16:9 design on a tall LED wall: Contain letterboxes, Cover crops the sides', () => {
  const c = canvasFitRect(1080, 1920, HD, 'contain');
  near(c.w, 1080, 'full width'); near(c.h, 607.5, 'short box'); near(c.top, 656.25, 'centred'); near(c.k, 607.5 / 1080, 'scale');
  const v = canvasFitRect(1080, 1920, HD, 'cover');
  near(v.h, 1920, 'full height'); near(v.w, 3413.33, 'wide box'); assert.ok(v.left < 0, 'sides cropped');
});

test('a portrait design on a matching portrait LED fills it', () => {
  for (const fit of ['contain', 'cover']) {
    const r = canvasFitRect(1080, 1920, PORTRAIT, fit);
    assert.deepEqual([r.left, r.top, r.w, r.h], [0, 0, 1080, 1920]);
    near(r.k, 1920 / 1080, 'scale by height');
    near(r.uw, 607.5, 'laid out 607.5 x 1080');
  }
});

test('a 16:9 design on a 16:10 or 4:3 projector letterboxes rather than stretching', () => {
  const r = canvasFitRect(1920, 1200, HD, 'contain');
  near(r.w, 1920, 'full width'); near(r.h, 1080, 'design height'); near(r.top, 60, 'bands of 60');
  const q = canvasFitRect(1024, 768, HD, 'contain');
  near(q.w / q.h, 16 / 9, 'shape kept');
});

test('a square design and an ultra-wide LED', () => {
  const s = canvasFitRect(1920, 1080, SQUARE, 'contain');
  near(s.w, 1080, 'square'); near(s.left, 420, 'centred');
  const w = canvasFitRect(3840, 1080, HD, 'contain');
  near(w.w, 1920, 'pillarboxed'); near(w.left, 960, 'centred');
  const m = canvasFitRect(3840, 1080, WIDE_LED, 'contain');
  assert.deepEqual([m.left, m.top, m.w, m.h], [0, 0, 3840, 1080]);
});

test('the layout box always has the canvas shape, so a design looks the same everywhere', () => {
  for (const size of [HD, PORTRAIT, SQUARE, WIDE_LED]) {
    for (const [W, H] of [[1920, 1080], [1080, 1920], [1280, 1024], [3840, 1080]]) {
      for (const fit of ['contain', 'cover']) {
        const r = canvasFitRect(W, H, size, fit);
        near(r.uw / r.uh, size.w / size.h, 'layout shape');
        near(r.w / r.h, size.w / size.h, 'drawn shape');
        near(r.uw * r.k, r.w, 'scale matches width');
        if (fit === 'contain') assert.ok(r.w <= W + 0.01 && r.h <= H + 0.01, 'contained');
        else assert.ok(r.w >= W - 0.01 && r.h >= H - 0.01, 'covers');
      }
    }
  }
});

test('a zero-sized window falls back to 1920x1080 instead of dividing by zero', () => {
  const r = canvasFitRect(0, 0, HD, 'contain');
  assert.deepEqual([r.w, r.h], [1920, 1080]);
});
