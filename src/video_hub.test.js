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
