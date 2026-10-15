// Tests for src/video_sources.js — the pure logic behind the Sources list.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const V = require('./video_sources.js');

const cam = (n) => ({ deviceId: 'dev' + n, label: 'Camera ' + n, kind: 'videoinput' });

test('normalizeSources drops junk, duplicate ids and unnamed devices', () => {
  const list = V.normalizeSources([
    { id: 'a', name: ' Front  cam ', deviceId: 'd1', label: 'FaceTime', width: 1920, height: 1080, fps: 30 },
    { id: 'a', name: 'dup', deviceId: 'd2' },
    { id: '', deviceId: 'd3' },
    { id: 'b' },
    null,
    { id: 'c', deviceId: 'd4', width: 'x', fps: 9999 },
  ]);
  assert.deepEqual(list.map(s => s.id), ['a', 'c']);
  assert.equal(list[0].name, 'Front cam');
  assert.equal(list[1].width, 0);
  assert.equal(list[1].fps, 0);
  assert.deepEqual(V.normalizeSources('nope'), []);
});

test('addSource names from the device label and keeps names unique', () => {
  let r = V.addSource([], cam(1));
  assert.equal(r.added.name, 'Camera 1');
  r = V.addSource(r.list, { deviceId: 'x', label: 'Camera 1' });
  assert.equal(r.added.name, 'Camera 1 2');
  r = V.addSource(r.list, { deviceId: 'y', label: '' });
  assert.equal(r.added.name, 'Camera');
  assert.equal(r.list.length, 3);
  assert.equal(new Set(r.list.map(s => s.id)).size, 3);
  assert.equal(V.addSource([], {}).added, null);
});

test('renameSource trims, ignores empty and avoids collisions', () => {
  const a = V.addSource([], cam(1));
  const b = V.addSource(a.list, cam(2));
  let list = V.renameSource(b.list, b.added.id, '  Stage   left ');
  assert.equal(list[1].name, 'Stage left');
  list = V.renameSource(list, b.added.id, '   ');
  assert.equal(list[1].name, 'Stage left');
  list = V.renameSource(list, b.added.id, 'camera 1');
  assert.equal(list[1].name, 'camera 1 2');
  list = V.renameSource(list, b.added.id, 'camera 1 2');
  assert.equal(list[1].name, 'camera 1 2');
});

test('moveSource reorders and clamps at the ends', () => {
  let list = ['a', 'b', 'c'].map(id => ({ id, name: id }));
  assert.deepEqual(V.moveSource(list, 'c', -1).map(s => s.id), ['a', 'c', 'b']);
  assert.deepEqual(V.moveSource(list, 'a', -1).map(s => s.id), ['a', 'b', 'c']);
  assert.deepEqual(V.moveSource(list, 'a', 5).map(s => s.id), ['b', 'c', 'a']);
  assert.equal(V.moveSource(list, 'zzz', 1), list);
});

test('removeSource and updateSource', () => {
  const a = V.addSource([], cam(1));
  const u = V.updateSource(a.list, a.added.id, { width: 1280, height: 720, fps: 60, label: ' New ' });
  assert.deepEqual([u[0].width, u[0].height, u[0].fps, u[0].label], [1280, 720, 60, 'New']);
  assert.equal(V.updateSource(a.list, 'nope', { fps: 24 })[0].fps, 0);
  assert.deepEqual(V.removeSource(a.list, a.added.id), []);
});

test('resolveDevice prefers the id, falls back to the label, else null', () => {
  const devices = [cam(1), cam(2), { deviceId: 'mic', label: 'Mic', kind: 'audioinput' }];
  assert.equal(V.resolveDevice({ deviceId: 'dev2', label: 'Camera 1' }, devices), 'dev2');
  assert.equal(V.resolveDevice({ deviceId: 'gone', label: 'camera 1' }, devices), 'dev1');
  assert.equal(V.resolveDevice({ deviceId: 'gone', label: 'Nope' }, devices), null);
  assert.equal(V.resolveDevice({ deviceId: 'mic', label: '' }, devices), null);
  assert.equal(V.resolveDevice({ deviceId: 'dev1' }, []), null);
});

test('buildConstraints uses ideals and never asks for audio', () => {
  assert.deepEqual(V.buildConstraints({ width: 1920, height: 1080, fps: 30 }, 'd'), {
    video: { deviceId: { exact: 'd' }, width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 30 } },
    audio: false,
  });
  assert.deepEqual(V.buildConstraints({ width: 0, height: 0, fps: 0 }, ''), { video: true, audio: false });
});

test('describeCaptureError gives operator words', () => {
  assert.match(V.describeCaptureError({ name: 'NotAllowedError' }), /blocked/);
  assert.match(V.describeCaptureError({ name: 'NotFoundError' }), /not connected/);
  assert.match(V.describeCaptureError({ name: 'NotReadableError' }), /Another app/);
  assert.match(V.describeCaptureError(null), /could not be opened/);
});

test('live map: normalize, set, and the Video layer gate', () => {
  const sources = [{ id: 's1' }, { id: 's2' }];
  const live = V.normalizeLive({ main: 's1', gone: 's2', mon: 'removed' }, sources, ['main', 'mon', 'new']);
  assert.deepEqual(live, { main: 's1', mon: null, new: null });
  assert.deepEqual(V.setLive(live, ['main', 'mon'], 's2'), { main: 's2', mon: 's2', new: null });
  assert.equal(V.setLive(live, ['main'], '').main, null);
  assert.equal(V.liveSourceFor('main', { video: true }, live, sources).id, 's1');
  assert.equal(V.liveSourceFor('main', { video: false }, live, sources), null);
  assert.equal(V.liveSourceFor('main', {}, live, sources), null);
  assert.equal(V.liveSourceFor('mon', { video: true }, live, sources), null);
});

test('switchPlan cuts for Cut or a first source, fades with the output speed', () => {
  assert.deepEqual(V.switchPlan('cut', 1, true), { mode: 'cut', ms: 0 });
  assert.deepEqual(V.switchPlan('fade', 1, false), { mode: 'cut', ms: 0 });
  assert.deepEqual(V.switchPlan('fade', 2, true), { mode: 'fade', ms: 600 });
  assert.deepEqual(V.switchPlan('slide-up', undefined, true), { mode: 'fade', ms: 300 });
});
