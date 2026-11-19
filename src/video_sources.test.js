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
    { id: 'b', name: 'Pulpit camera' },
    null,
    { id: 'c', deviceId: 'd4', width: 'x', fps: 9999 },
  ]);
  assert.deepEqual(list.map(s => s.id), ['a', 'b', 'c']);
  assert.equal(list[1].name, 'Pulpit camera');   // a placeholder keeps its name
  assert.equal(list[1].enabled, true);
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
  assert.equal(r.added.name, 'Input');
  assert.equal(r.list.length, 3);
  assert.equal(new Set(r.list.map(s => s.id)).size, 3);
  const ph = V.addSource([], null, { name: 'Pulpit camera' });
  assert.equal(ph.added.name, 'Pulpit camera');
  assert.equal(ph.added.deviceId, '');
  assert.equal(V.addSource(ph.list, null).added.name, 'Input');
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

test('buildConstraints asks no more than the screen shows, keeping the shape', () => {
  const c = V.buildConstraints({ width: 3840, height: 2160, fps: 30 }, 'd', { width: 1920, height: 1200 });
  assert.equal(c.video.width.ideal, 1920);
  assert.equal(c.video.height.ideal, 1080);
  const small = V.buildConstraints({ width: 1280, height: 720, fps: 30 }, 'd', { width: 1920, height: 1080 });
  assert.equal(small.video.width.ideal, 1280, 'a smaller source is left as it is');
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
  const sources = [{ id: 's1', deviceId: 'd1' }, { id: 's2', deviceId: 'd2' }];
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

test('placeholders: state, assigning a device keeps name and settings', () => {
  const devices = [cam(1), cam(2)];
  let r = V.addSource([], null, { name: 'Pulpit camera', width: 1280, height: 720, fps: 30 });
  assert.equal(V.stateOf(r.added, devices), 'placeholder');
  let list = V.assignDevice(r.list, r.added.id, devices[1]);
  assert.equal(list[0].name, 'Pulpit camera');
  assert.deepEqual([list[0].width, list[0].fps, list[0].deviceId], [1280, 30, 'dev2']);
  assert.equal(V.stateOf(list[0], devices), 'ready');
  // The device goes away: the input stays, as 'missing', with its name.
  assert.equal(V.stateOf(list[0], [cam(1)]), 'missing');
  assert.equal(V.stateOf(list[0], devices), 'ready');   // and is back when it returns
});

test('freeDevices and addAll skip devices already used', () => {
  const devices = [cam(1), cam(2), cam(3)];
  let list = V.addSource([], devices[0]).list;
  assert.deepEqual(V.freeDevices(list, devices).map(d => d.deviceId), ['dev2', 'dev3']);
  list = V.addAll(list, devices);
  assert.equal(list.length, 3);
  assert.equal(V.freeDevices(list, devices).length, 0);
  assert.equal(V.addAll(list, devices).length, 3);
});

test('enabled flag, and a placeholder or disabled input is never taken to air', () => {
  const devices = [cam(1)];
  let list = V.addSource([], devices[0]).list;
  const id = list[0].id;
  const live = { main: id };
  assert.ok(V.liveSourceFor('main', { video: true }, live, list));
  list = V.setEnabled(list, id, false);
  assert.equal(V.liveSourceFor('main', { video: true }, live, list), null);
  const ph = V.addSource([], null, { name: 'Lobby' });
  assert.equal(V.liveSourceFor('main', { video: true }, { main: ph.added.id }, ph.list), null);
});

test('two cameras of one model: a name match never takes the other input\'s device', () => {
  const dev = (id) => ({ kind: 'videoinput', deviceId: id, label: 'USB Camera' });
  const devices = [dev('d1'), dev('d2')];
  const A = { id: 'a', deviceId: 'd1', label: 'USB Camera' };
  const B = { id: 'b', deviceId: 'gone', label: 'USB Camera' };
  // B lost its id, A holds d1: B gets d2, never d1.
  assert.equal(V.resolveDevice(B, devices, [A, B]), 'd2');
  // d2 unplugged: B is missing, not a second view of A's camera.
  assert.equal(V.resolveDevice(B, [dev('d1')], [A, B]), null);
  assert.equal(V.stateOf(B, [dev('d1')], [A, B]), 'missing');
  // A unplugged while B holds d2 (A's id is stale): A is missing, not B's camera.
  const A2 = { id: 'a', deviceId: 'gone-a', label: 'USB Camera' };
  const B2 = { id: 'b', deviceId: 'd2', label: 'USB Camera' };
  assert.equal(V.resolveDevice(A2, [dev('d2')], [A2, B2]), null);
  assert.equal(V.resolveDevice(A2, devices, [A2, B2]), 'd1');
  // Both lost their ids: each takes its own, in list order.
  const C = { id: 'c', deviceId: 'x', label: 'USB Camera' }, D = { id: 'd', deviceId: 'y', label: 'USB Camera' };
  assert.equal(V.resolveDevice(C, devices, [C, D]), 'd1');
  assert.equal(V.resolveDevice(D, devices, [C, D]), 'd2');
  // A single camera by name still resolves, and with no list the old behaviour holds.
  assert.equal(V.resolveDevice(B, [dev('d9')], [B]), 'd9');
  assert.equal(V.resolveDevice(B, devices), 'd1');
});

test('an input can carry its own key: a letter, a digit or F1-F12, never a key with another job', () => {
  assert.equal(V.keyName('Q'), 'q');
  assert.equal(V.keyName('7'), '7');
  assert.equal(V.keyName('f2'), 'F2');
  assert.equal(V.keyName(' '), '');
  assert.equal(V.keyName('Escape'), '');
  assert.equal(V.keyName('ArrowLeft'), '');
  assert.equal(V.keyName('ab'), '');
  const list = V.normalizeSources([{ id: 'a', name: 'Pulpit', label: 'Cam', key: 'Q' }, { id: 'b', name: 'Choir', label: 'Cam2', key: 'Enter' }]);
  assert.equal(list[0].key, 'q');
  assert.equal(list[1].key, '');
  assert.equal(V.updateSource(list, 'b', { key: 'F3' })[1].key, 'F3');
});
