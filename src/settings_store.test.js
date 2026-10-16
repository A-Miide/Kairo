// Tests for src/settings_store.js — saves leave in order, failures are reported and can roll back.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');

function load(fetchImpl) {
  const events = [];
  const sandbox = {
    console, setTimeout, clearTimeout, AbortController, structuredClone,
    SERVER: 'http://x', settings: { a: 1 },
    localStorage: { data: {}, setItem(k, v) { this.data[k] = v; }, getItem(k) { return this.data[k] ?? null; } },
    fetch: fetchImpl,
    CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init && init.detail; } },
    window: { dispatchEvent: (e) => { events.push(e); return true; }, addEventListener() {}, removeEventListener() {} },
  };
  vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(require.resolve('./settings_store.js'), 'utf8') + '\nthis.saveSettingsPatch = saveSettingsPatch; this.fetchServerSettings = fetchServerSettings; this.KairoCameras = KairoCameras; this.isDirectorOn = isDirectorOn;', sandbox);
  return { api: sandbox, events };
}

test('saves reach the server one after another, in the order they were made', async () => {
  const sent = [];
  let release;
  const first = new Promise(r => { release = r; });
  const { api } = load(async (url, init) => {
    sent.push(JSON.parse(init.body));
    if (sent.length === 1) await first;       // the first one is slow
    return { ok: true, status: 200 };
  });
  const p1 = api.saveSettingsPatch({ x: 1 });
  const p2 = api.saveSettingsPatch({ x: 2 });
  await new Promise(r => setTimeout(r, 5));
  assert.equal(sent.length, 1, 'the second waits for the first');
  release();
  assert.deepEqual(await Promise.all([p1, p2]), [true, true]);
  assert.deepEqual(JSON.parse(JSON.stringify(sent)), [{ x: 1 }, { x: 2 }]);
});

test('a refused or unreachable save resolves false, tells onError, and restores what was asked', async () => {
  const { api } = load(async () => ({ ok: false, status: 500 }));
  api.settings.mode = 'new';
  let why = null;
  assert.equal(await api.saveSettingsPatch({ mode: 'new' }, { rollback: { mode: 'old' }, onError: (e) => { why = e; } }), false);
  assert.equal(why.status, 500);
  assert.equal(api.settings.mode, 'old');

  const down = load(async () => { throw new Error('offline'); });
  let err = null;
  assert.equal(await down.api.saveSettingsPatch({ y: 1 }, { onError: (e) => { err = e; } }), false);
  assert.equal(err.message, 'offline');
});

test('everyone hears of a save right away', () => {
  const { api, events } = load(async () => ({ ok: true }));
  api.saveSettingsPatch({ stream: {} });
  assert.equal(events[0].type, 'kairo:settings');
  assert.deepEqual(JSON.parse(JSON.stringify(events[0].detail)), { reason: 'saved', keys: ['stream'] });
});

test('reads in flight are shared but each caller gets its own copy', async () => {
  let calls = 0;
  const { api } = load(async () => { calls++; return { ok: true, json: async () => ({ k: { n: 1 } }) }; });
  const [a, b] = await Promise.all([api.fetchServerSettings(), api.fetchServerSettings()]);
  assert.equal(calls, 1);
  a.k.n = 9;
  assert.equal(b.k.n, 1);
});

test('the camera list lives in settings; the display windows get a mirror written only by KairoCameras', async () => {
  const { api } = load(async () => ({ ok: true }));
  assert.deepEqual(JSON.parse(JSON.stringify(api.KairoCameras.list())), []);
  const list = [{ id: 'vid-1', name: 'Cam 1' }, null, { id: 'vid-2', name: 'Cam 2' }];
  api.KairoCameras.save(list, { 'display-1': 'vid-1' });
  assert.equal(api.settings.videoSources, list);
  assert.equal(api.KairoCameras.find('vid-2').name, 'Cam 2');
  assert.equal(api.KairoCameras.find('nope'), null);
  assert.equal(JSON.parse(api.localStorage.getItem('kairo-video-sources')).length, 3);
  assert.equal(JSON.parse(api.localStorage.getItem('kairo-video-live'))['display-1'], 'vid-1');
  api.KairoCameras.saveLive({ 'display-1': null });
  assert.equal(api.settings.videoLive['display-1'], null);
  assert.equal(JSON.parse(api.localStorage.getItem('kairo-video-live'))['display-1'], null);
});

test('isDirectorOn reads the one setting', () => {
  const { api } = load(async () => ({ ok: true }));
  assert.equal(api.isDirectorOn(), false);
  api.settings.videoDirector = true;
  assert.equal(api.isDirectorOn(), true);
});
