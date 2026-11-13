// Tests for src/slide_finder.js — plain node:test, no DOM.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { SlideFinder } = require('./slide_finder.js');

const SLIDES = [
  { playlistId: 'p1', playlistName: 'Sunday', itemId: 'a', itemTitle: 'Announcements', index: 0, text: 'Welcome to church this morning' },
  { playlistId: 'p2', playlistName: 'Youth Night', itemId: 'k', itemTitle: 'Kingdom Law', index: 6, text: 'Obedience opens doors that prayer alone cannot open for the believer' },
  { playlistId: 'p2', playlistName: 'Youth Night', itemId: 'k', itemTitle: 'Kingdom Law', index: 7, text: 'Every seed sown in faith brings a harvest in its season' },
  { playlistId: 'p3', playlistName: 'Midweek', itemId: 'm', itemTitle: 'Faith Series', index: 2, text: 'Faith is the substance of things hoped for and the evidence of things not seen' },
];

function finder(cfg) {
  const offers = [];
  const f = new SlideFinder({ onOffer: (o) => offers.push(o), config: cfg });
  f.setSlides(SLIDES);
  return { f, offers };
}
// The speech arriving as it would: the same growing window, a few updates apart.
function speak(f, text, from = 0, step = 500, times = 5) {
  for (let i = 0; i < times; i++) f.ingest(text, { now: from + i * step });
}

test('a slide read from the middle of a deck in another playlist is offered once it holds', () => {
  const { f, offers } = finder();
  speak(f, 'and church I tell you obedience opens doors that prayer alone cannot open for the believer');
  assert.equal(offers.length, 1);
  assert.equal(offers[0].itemId, 'k');
  assert.equal(offers[0].index, 6);
  assert.equal(offers[0].playlistName, 'Youth Night');
});

test('one update is not enough: it must hold', () => {
  const { f, offers } = finder();
  f.ingest('obedience opens doors that prayer alone cannot open for the believer', { now: 0 });
  assert.equal(offers.length, 0);
});

test('a few shared common words offer nothing', () => {
  const { f, offers } = finder();
  speak(f, 'this morning we open the doors of the church to every believer');
  assert.equal(offers.length, 0);
});

test('the live deck is left to the slide follower', () => {
  const { f, offers } = finder();
  f.setExclude(['k']);
  speak(f, 'obedience opens doors that prayer alone cannot open for the believer');
  assert.equal(offers.length, 0);
});

test('the same slide is not offered again within the repeat window', () => {
  const { f, offers } = finder();
  const t = 'obedience opens doors that prayer alone cannot open for the believer';
  speak(f, t, 0);
  speak(f, t, 10000);
  assert.equal(offers.length, 1);
  speak(f, t, 200000);
  assert.equal(offers.length, 2);
});

test('a deck copied into two playlists is offered once, from the first place listed', () => {
  const offers = [];
  const f = new SlideFinder({ onOffer: (o) => offers.push(o) });
  const copy = { ...SLIDES[1], playlistId: 'p9', playlistName: 'Copy', itemId: 'k2' };
  f.setSlides([SLIDES[1], copy]);
  speak(f, 'obedience opens doors that prayer alone cannot open for the believer');
  assert.equal(offers.length, 1);
  assert.equal(offers[0].playlistId, 'p2');
});

test('a slide with too few words of its own is never offered', () => {
  const { f, offers } = finder();
  speak(f, 'welcome to church this morning');
  assert.equal(offers.length, 0);
});

// ── armed slides (the Auto toggle) and songs found by title ──
function armed(extra) {
  const offers = [];
  const f = new SlideFinder({ onOffer: (o) => offers.push(o) });
  f.setSlides([...SLIDES.map((s) => ({ ...s, auto: s.itemId === 'k' && s.index === 6 })), ...(extra || [])]);
  return { f, offers };
}
const OBEDIENCE = 'and church I tell you obedience opens doors that prayer alone cannot open for the believer';

test('an armed slide is marked to be sent when it is found; an unarmed one is only offered', () => {
  const { f, offers } = armed();
  speak(f, OBEDIENCE);
  assert.equal(offers.length, 1);
  assert.equal(offers[0].auto, true);
  const plain = finder();
  speak(plain.f, 'faith is the substance of things hoped for and the evidence of things not seen my friend');
  assert.equal(plain.offers.length, 1);
  assert.equal(plain.offers[0].auto, false);
});

test('two armed slides that read alike are offered together and neither is sent', () => {
  const offers = [];
  const f = new SlideFinder({ onOffer: (o) => offers.push(o) });
  const words = 'Obedience opens doors that prayer alone cannot open for the believer';
  f.setSlides([
    { playlistId: 'p1', playlistName: 'Sunday', itemId: 'a', itemTitle: 'Series A', index: 1, text: words + ' today', auto: true },
    { playlistId: 'p2', playlistName: 'Midweek', itemId: 'b', itemTitle: 'Series B', index: 4, text: words + ' tonight', auto: true },
  ]);
  speak(f, 'obedience opens doors that prayer alone cannot open for the believer');
  assert.equal(offers.length, 2);
  assert.ok(offers.every((o) => o.auto === false && o.ambiguous === true));
  assert.deepEqual(offers.map((o) => o.itemId).sort(), ['a', 'b']);
});

test('alike slides, none armed, stay silent as before', () => {
  const offers = [];
  const f = new SlideFinder({ onOffer: (o) => offers.push(o) });
  const words = 'Obedience opens doors that prayer alone cannot open for the believer';
  f.setSlides([
    { playlistId: 'p1', itemId: 'a', itemTitle: 'A', index: 1, text: words + ' today' },
    { playlistId: 'p2', itemId: 'b', itemTitle: 'B', index: 4, text: words + ' tonight' },
  ]);
  speak(f, 'obedience opens doors that prayer alone cannot open for the believer');
  assert.equal(offers.length, 0);
});

test('a song is found when its title is said, and offered unless it is armed', () => {
  const song = { playlistId: 'p1', playlistName: 'Sunday', itemId: 's1', itemTitle: 'Amazing Grace', index: 0, text: 'Amazing Grace', kind: 'song' };
  const { f, offers } = armed([song]);
  speak(f, 'now let us all stand and sing amazing grace together', 0);
  assert.equal(offers.length, 1);
  assert.equal(offers[0].itemId, 's1');
  assert.equal(offers[0].kind, 'song');
  assert.equal(offers[0].auto, false);
  const armedSong = armed([{ ...song, auto: true }]);
  speak(armedSong.f, 'now let us all stand and sing amazing grace together', 0);
  assert.equal(armedSong.offers.length, 1);
  assert.equal(armedSong.offers[0].auto, true);
});

test('one word of a title is not a song reference', () => {
  const { f, offers } = armed([{ playlistId: 'p1', itemId: 's1', itemTitle: 'Amazing Grace', index: 0, text: 'Amazing Grace', kind: 'song' }]);
  speak(f, 'it is only by grace that we stand before him this morning my friends', 0);
  assert.equal(offers.length, 0);
});
