// Tests for src/audio_lane.js, the audio lane. Stubs the browser pieces
// (Audio element, localStorage, window) and checks the playlist, loop, fade and
// ducking behaviour that decides what is heard.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

function load() {
  const mem = {};
  const els = [];
  class FakeAudio {
    constructor() { this.volume = 1; this.paused = true; this.ended = false; this.currentTime = 0; this.duration = 100; this.l = {}; els.push(this); }
    addEventListener(n, f) { this.l[n] = f; }
    play() { this.paused = false; return Promise.resolve(); }
    pause() { this.paused = true; }
    setSinkId() { return Promise.resolve(); }
  }
  const win = {};
  const ctx = {
    window: win, Audio: FakeAudio, performance, setInterval, clearInterval, console, SERVER: 'http://x',
    localStorage: { getItem: (k) => (k in mem ? mem[k] : null), setItem: (k, v) => { mem[k] = String(v); } },
  };
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(__dirname + '/audio_lane.js', 'utf8'), ctx);
  const lane = win.KairoAudioLane;
  lane.tracks = ['a', 'b', 'c'].map(n => ({ name: n, label: n, url: '/' + n }));
  lane.setFade(0);
  return { lane, els, mem };
}

test('play starts the first track, next and prev wrap around', () => {
  const { lane } = load();
  lane.play();
  assert.equal(lane.state().index, 0);
  lane.next(); lane.next(); lane.next();
  assert.equal(lane.state().index, 0);
  lane.prev();
  assert.equal(lane.state().index, 2);
});

test('a finished track moves on, repeats, or stops by the loop mode', () => {
  const { lane, els } = load();
  lane.play(2);
  els[0].l.ended();
  assert.equal(lane.state().index, 2, 'last track, loop off: stays');
  lane.setLoop('all'); els[0].l.ended();
  assert.equal(lane.state().index, 0, 'repeat playlist wraps');
  lane.setLoop('one'); els[0].l.ended();
  assert.equal(lane.state().index, 0, 'repeat one stays on the same track');
});

test('volume is the element level, and is clamped', () => {
  const { lane, els } = load();
  lane.play(0);
  lane.setVolume(50);
  assert.equal(els[0].volume, 0.5);
  lane.setVolume(300);
  assert.equal(els[0].volume, 1);
});

test('a fade-in starts silent and a fade-out stop ends paused at the start', async () => {
  const { lane, els } = load();
  lane.setFade(0.2);
  lane.play(0);
  assert.ok(els[0].volume < 0.05, 'starts silent');
  await new Promise(r => setTimeout(r, 400));
  assert.equal(els[0].volume, lane.state().volume / 100);
  lane.fadeOut();
  await new Promise(r => setTimeout(r, 400));
  assert.equal(els[0].paused, true);
  assert.equal(els[0].currentTime, 0);
});

test('ducking lowers the level under a video and restores it, and can be turned off', async () => {
  const { lane, els } = load();
  lane.setVolume(80);
  lane.play(0);
  lane.duckFor(true);
  await new Promise(r => setTimeout(r, 600));
  assert.ok(els[0].volume < 0.25, 'lowered');
  lane.duckFor(false);
  await new Promise(r => setTimeout(r, 600));
  assert.equal(els[0].volume, 0.8);
  lane.setDuckOn(false);
  lane.duckFor(true);
  await new Promise(r => setTimeout(r, 100));
  assert.equal(els[0].volume, 0.8, 'ducking off: unchanged');
});

test('autostart plays the last track on launch only when asked', async () => {
  const { lane, els, mem } = load();
  lane.refresh = async () => lane.tracks;
  await lane.init();
  assert.equal(els.length, 0, 'off by default: no audio element at all');
  lane.play(1); lane.stop({ fade: false });
  lane.setAutostart(true);
  await lane.init();
  assert.equal(lane.state().index, 1);
  assert.equal(mem['kairo-audio-lane-autostart'], '1');
});
