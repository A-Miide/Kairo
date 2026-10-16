// Tests for src/instant_replay.js — it follows the camera on air through the hub's capture
// and never opens a camera of its own.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');

function setup() {
  const leases = [];      // { deviceId, profile, released }
  const listeners = {};
  const recorders = [];
  const elStub = () => ({ appendChild() {}, append() {}, addEventListener() {}, classList: { toggle() {} }, setAttribute() {}, style: {}, parentNode: { insertBefore() {} } });
  const win = {
    KairoUI: { el: elStub, popover: () => ({ close() {} }) },
    KairoVideoDirector: { state: () => ({ active: true, program: { id: 'vid-1', mixed: false } }) },
    KairoVideoHub: {
      resolve: async (src) => 'dev-' + src.id,
      lease(deviceId, profile, onStream) {
        const l = { deviceId, profile, released: false };
        leases.push(l);
        const stream = { active: true, getTracks: () => [{ stop() { throw new Error('the hub owns the tracks'); } }] };
        queueMicrotask(() => onStream(stream));
        return () => { l.released = true; };
      },
    },
    MediaRecorder: class { static isTypeSupported() { return true; } constructor() { this.state = 'inactive'; recorders.push(this); } start() { this.state = 'recording'; } stop() { this.state = 'inactive'; if (this.onstop) this.onstop(); } },
  };
  const sandbox = {
    window: win, console, clearTimeout, Date,
    // Real timers that never keep the test process alive (replay arms 8 s and 16 s ones).
    setTimeout: (fn, ms) => { const t = setTimeout(fn, ms); t.unref(); return t; },
    settings: { videoDirector: true, videoReplay: true, videoSources: [{ id: 'vid-1' }, { id: 'vid-2' }] },
    document: { getElementById: () => null, createElement: () => ({ style: {}, appendChild() {} }), head: { appendChild() {} } },
    navigator: { mediaDevices: { getUserMedia() { throw new Error('replay must not open a camera itself'); } } },
    isDirectorOn: () => !!sandbox.settings.videoDirector,
    onDirectorChange: (fn) => { listeners.dir = fn; },
    onSettingsChange: (fn) => { listeners.settings = fn; },
    KairoCameras: { find: (id) => sandbox.settings.videoSources.find(s => s.id === id) || null },
    FileReader: class {}, Blob: class {}, fetch: async () => ({ json: async () => ({}) }),
  };
  sandbox.MediaRecorder = win.MediaRecorder;
  vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(require.resolve('./instant_replay.js'), 'utf8'), sandbox);
  return { sandbox, win, leases, listeners, recorders };
}
const tick = () => new Promise(r => setTimeout(r, 5));

test('replay takes a replay lease on the hub for the camera on air, and starts two recorders on it', async () => {
  const { leases, listeners, recorders } = setup();
  listeners.dir({ on: true, reason: 'started' });
  await tick();
  assert.equal(leases.length, 1);
  assert.equal(leases[0].profile, 'replay');
  assert.equal(leases[0].deviceId, 'dev-vid-1');
  assert.equal(recorders.length, 1, 'the second recorder starts half a period later');
});

test('the camera on air changing moves the lease; Director mode turning off lets it go', async () => {
  const { sandbox, win, leases, listeners } = setup();
  listeners.dir({ on: true, reason: 'started' });
  await tick();
  win.KairoVideoDirector.state = () => ({ active: true, program: { id: 'vid-2', mixed: false } });
  listeners.dir({ on: true, reason: 'program' });
  await tick();
  assert.equal(leases.length, 2);
  assert.equal(leases[0].released, true);
  assert.equal(leases[1].deviceId, 'dev-vid-2');
  sandbox.settings.videoDirector = false;
  listeners.dir({ on: false, reason: 'stopped' });
  await tick();
  assert.equal(leases[1].released, true);
});

test('a change that arrives while the camera is being found is not lost', async () => {
  const { win, leases, listeners } = setup();
  let program = 'vid-1';
  win.KairoVideoDirector.state = () => ({ active: true, program: { id: program, mixed: false } });
  listeners.dir({ on: true, reason: 'started' });
  program = 'vid-2';
  listeners.dir({ on: true, reason: 'program' });     // while the first open is still resolving
  await tick(); await tick();
  const live = leases.filter(l => !l.released);
  assert.equal(live.length, 1);
  assert.equal(live[0].deviceId, 'dev-vid-2');
});
