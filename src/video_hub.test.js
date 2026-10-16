// Tests for src/video_hub.js — one capture per camera, and no camera left running after releaseAll.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');

function fakeStream() {
  const track = { stopped: false, stop() { this.stopped = true; } };
  return { track, getTracks: () => [track] };
}

function loadHub(getUserMedia) {
  global.window = {};
  Object.defineProperty(global, 'navigator', { value: { mediaDevices: { getUserMedia, enumerateDevices: async () => [] } }, configurable: true });
  delete require.cache[require.resolve('./video_hub.js')];
  require('./video_hub.js');
  return global.window.KairoVideoHub;
}

test('releaseAll while a camera is still opening stops it as soon as it arrives', async () => {
  let resolveOpen;
  const s = fakeStream();
  const hub = loadHub(() => new Promise(r => { resolveOpen = r; }));
  let delivered = 0;
  hub.lease('cam1', 'thumb', () => { delivered++; }, () => {});
  await new Promise(r => setTimeout(r, 5));      // the open is pending
  hub.releaseAll();
  resolveOpen(s);
  await new Promise(r => setTimeout(r, 5));
  assert.equal(s.track.stopped, true, 'the late stream is closed');
  assert.equal(delivered, 0, 'nobody is handed it');
  assert.equal(hub.openCount(), 0);
});

test('one capture serves every holder, and the last release lets it go after the linger', async () => {
  const opened = [];
  const hub = loadHub(async () => { const s = fakeStream(); opened.push(s); return s; });
  const got = [];
  const r1 = hub.lease('cam1', 'thumb', (st) => got.push(st), () => {});
  const r2 = hub.lease('cam1', 'thumb', (st) => got.push(st), () => {});
  await new Promise(r => setTimeout(r, 5));
  assert.equal(opened.length, 1);
  assert.equal(got.length, 2);
  r1(); r2();
  hub.releaseAll();
  assert.equal(opened[0].track.stopped, true);
});

test('instant replay shares the camera at 720p30 and the capture drops back when it lets go', async () => {
  const asked = [];
  const hub = loadHub(async (c) => { const s = fakeStream(); asked.push({ c, s }); return s; });
  let thumb = null, replay = null;
  hub.lease('cam1', 'thumb', (st) => { thumb = st; }, () => {});
  await new Promise(r => setTimeout(r, 5));
  assert.equal(asked.length, 1);
  const release = hub.lease('cam1', 'replay', (st) => { replay = st; }, () => {});
  await new Promise(r => setTimeout(r, 5));
  assert.equal(asked.length, 2, 'one more open, at the bigger size');
  const v = asked[1].c.video;
  assert.equal(v.width.ideal, 1280); assert.equal(v.height.ideal, 720); assert.equal(v.frameRate.ideal, 30);
  assert.equal(replay, asked[1].s);
  assert.equal(thumb, asked[1].s, 'the thumbnail takes the same capture');
  assert.equal(asked[0].s.track.stopped, true, 'the small one is closed, not kept beside it');
  assert.equal(hub.openCount(), 1);
  release();
  await new Promise(r => setTimeout(r, 1700));
  assert.equal(asked.length, 3, 'back to the thumbnail size once replay has let go');
  hub.releaseAll();
});
