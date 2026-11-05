// Tests for src/output_transitions.js — each transition lands where display.html reads it.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');

function load() {
  const store = {}, saved = [];
  const sandbox = {
    localStorage: { setItem: (k, v) => { store[k] = v; }, removeItem: (k) => { delete store[k]; } },
    saveSettingsPatch: (p) => { saved.push(p); return Promise.resolve(true); },
  };
  vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(require.resolve('./output_transitions.js'), 'utf8') + '\nthis.T = KairoTransitions;', sandbox);
  return { T: sandbox.T, store, saved };
}

test('words go through the settings save, so the server broadcasts them', () => {
  const { T, saved, store } = load();
  T.words({ animation: 'slide-up' });
  T.words({ speed: 2 });
  assert.equal(JSON.stringify(saved), JSON.stringify([{ outputAnimation: 'slide-up' }, { outputAnimationSpeed: 2 }]));
  assert.equal(Object.keys(store).length, 0);
});

test('media and video are written under the keys the output windows read, and null takes them away', () => {
  const { T, store } = load();
  T.media('cut'); T.video('fade'); T.videoFade(450);
  assert.equal(store['kairo-media-transition'], '{"animation":"cut"}');
  assert.equal(store['kairo-video-transition'], '"fade"');
  assert.equal(store['kairo-video-fade'], '450');
  T.media(''); T.video(null); T.videoFade(null);
  assert.deepEqual(Object.keys(store), []);
});
