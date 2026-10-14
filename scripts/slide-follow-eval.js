#!/usr/bin/env node
// Slide-follow eval: replays a real sermon transcript, at real pace, against
// a slide deck whose every slide carries the moment it should come up, and
// scores how the slide followers (src/sermon_follow.js — "sermon" mode,
// tracks by distinctive vocabulary; src/lyrics_follow.js — "script" mode,
// exact wording) keep up.
//
// Decks live in server/eval/slide_decks/<sermon>.json:
//   { sermon, slides: [{ at: "mm:ss" | null, text }] }
// `at` is when an operator would put the slide up (the preacher starts on
// it); null means the slide is in the deck but the preacher never got to it.
// The transcript is server/eval/fixtures/<sermon>.json (Deepgram), or with
// --offline, parity/offline/<sermon>.json (the local model) when present.
//
// Usage:
//   node scripts/slide-follow-eval.js                      every deck, both modes
//   node scripts/slide-follow-eval.js enough-is-enough --mode sermon --verbose
//   node scripts/slide-follow-eval.js --offline
//
// Scoring, per slide that should come up:
//   caught  — the follower moved to it, at most 2 s before and before the
//             next slide was due (latency = how long after `at`)
//   missed  — it never showed (the follower stayed put, or jumped past it)
// and per advance:
//   early   — it moved to a slide the preacher hadn't reached yet (more
//             than 2 s ahead): what the congregation sees as a wrong slide.
// Plus the share of the sermon the screen showed the slide it should have.
'use strict';

const fs = require('fs');
const path = require('path');
const { LyricsFollower } = require('../src/lyrics_follow.js');
const { SermonFollower } = require('../src/sermon_follow.js');
const { SlideFollower } = require('../src/slide_follow.js');

const ROOT = path.join(__dirname, '..');
const DECKS = path.join(ROOT, 'server/eval/slide_decks');
const args = process.argv.slice(2);
const has = (f) => args.includes(f);
const val = (f, d) => { const i = args.indexOf(f); return i >= 0 && args[i + 1] ? args[i + 1] : d; };
const modes = val('--mode', 'new,sermon,script').split(',');
const VERBOSE = has('--verbose');
const OFFLINE = has('--offline');
const EARLY_GRACE_MS = 2000;
const only = args.filter((a, i) => !a.startsWith('--') && !['--mode'].includes(args[i - 1]));

// A scripture slide opens with its reference ("Romans 4:20-21 …", "1 John 5:4 …").
const isScripture = (text) => /^(?:[1-3]\s)?[A-Z][a-z]+(?:\s(?:of\s)?[A-Z][a-z]+)?\s\d+:\d+/.test(text);
const toMs = (s) => { if (s == null) return null; const [m, sec] = String(s).split(':').map(Number); return (m * 60 + sec) * 1000; };
const fmt = (ms) => { const s = Math.round(ms / 1000); return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`; };

function loadTranscript(name, deck) {
  // A deck recorded with the offline model replays that model's own stream:
  // every interim and final result at the moment it arrived.
  if (OFFLINE || (deck && deck.transcript === 'offline')) {
    const file = path.join(ROOT, 'parity/offline', `${name}.json`);
    if (!fs.existsSync(file)) return null;
    const d = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (Array.isArray(d.inputs) && d.inputs.length) return { live: d.inputs.map((x) => ({ now: x.t || x.audioMs, text: x.text || '', isFinal: !!x.isFinal })) };
    return null;
  }
  const file = path.join(ROOT, 'server/eval/fixtures', `${name}.json`);
  if (!fs.existsSync(file)) return null;
  const d = JSON.parse(fs.readFileSync(file, 'utf8'));
  return (d.transcript || []).map((s) => ({ startMs: s.startMs, text: s.text || '' }));
}

// The transcript as the app receives it: each utterance grows as interim
// results (two words at a time, at speaking pace) and then finalises.
function* deliveries(segs) {
  if (segs.live) { yield* segs.live; return; }
  for (let i = 0; i < segs.length; i++) {
    const words = segs[i].text.split(/\s+/).filter(Boolean);
    if (!words.length) continue;
    const start = segs[i].startMs;
    const next = i + 1 < segs.length ? segs[i + 1].startMs : start + words.length * 400;
    const per = Math.min(400, Math.max(120, (next - start) / Math.max(1, words.length)));
    for (let n = 2; n < words.length; n += 2) yield { now: start + n * per, text: words.slice(0, n).join(' '), isFinal: false };
    yield { now: start + words.length * per, text: words.join(' '), isFinal: true };
  }
}

function run(deck, segs, mode) {
  const slides = deck.slides;
  const advances = [];
  let follower;
  if (mode === 'new') {
    follower = new SlideFollower({ slides: slides.map((s) => s.text) }, { onAdvance: (e) => advances.push({ to: e.toIndex }) });
  } else if (mode === 'sermon') {
    follower = new SermonFollower({ slides: slides.map((s) => s.text) }, {
      onAdvance: (e) => advances.push({ to: e.toIndex }),
    });
  } else {
    const song = { title: deck.sermon, blocks: slides.map((s, i) => ({ label: `Slide ${i + 1}`, lines: [s.text] })) };
    follower = new LyricsFollower(song, {
      config: { maxBlockSkip: Math.max(1, slides.length - 1) },
      onAdvance: (e) => advances.push({ to: e.toBlockIdx }),
    });
  }
  follower.resync(0, 1); // (a 0 clock reads as "use Date.now()")
  let clock = 0;
  for (const d of deliveries(segs)) {
    clock = d.now;
    const before = advances.length;
    follower.ingest(d.text, { now: d.now, isFinal: d.isFinal });
    for (let k = before; k < advances.length; k++) { advances[k].at = d.now; advances[k].heard = d.text; }
  }
  return { advances, end: clock };
}

function score(deck, result) {
  const due = deck.slides.map((s, i) => ({ i, at: toMs(s.at) })).filter((s) => s.at != null).sort((a, b) => a.at - b.at);
  const shouldShow = (t) => { let cur = due[0].i; for (const d of due) if (d.at <= t) cur = d.i; return cur; };
  const rows = [];
  let caught = 0, missed = 0, early = 0;
  const kind = { statement: [0, 0], scripture: [0, 0] };
  const lat = [];
  for (let k = 1; k < due.length; k++) {
    const d = due[k];
    const nextAt = k + 1 < due.length ? due[k + 1].at : Infinity;
    const adv = result.advances.find((a) => a.to === d.i && a.at >= d.at - EARLY_GRACE_MS && a.at < nextAt);
    const kk = kind[isScripture(deck.slides[d.i].text) ? 'scripture' : 'statement'];
    kk[1]++;
    if (adv) { caught++; kk[0]++; lat.push(adv.at - d.at); rows.push({ slide: d.i, at: d.at, got: adv.at }); }
    else { missed++; rows.push({ slide: d.i, at: d.at, got: null }); }
  }
  for (const a of result.advances) {
    const want = shouldShow(a.at + EARLY_GRACE_MS);
    a.early = due.findIndex((d) => d.i === a.to) > due.findIndex((d) => d.i === want) || !due.some((d) => d.i === a.to);
    if (a.early) early++;
  }
  // share of time (first slide due → end) the screen showed the right slide
  let shown = 0, right = 0, t = 0, showing = due[0].i;
  const evts = result.advances.map((a) => ({ at: a.at, to: a.to }));
  const end = result.end;
  for (let ms = 0; ms <= end; ms += 1000) {
    while (evts.length && evts[0].at <= ms) showing = evts.shift().to;
    shown++; if (showing === shouldShow(ms)) right++;
    t = ms;
  }
  lat.sort((a, b) => a - b);
  const med = lat.length ? lat[Math.floor(lat.length / 2)] : null;
  return { caught, missed, early, total: due.length - 1, medianLatency: med, rightShare: right / Math.max(1, shown), rows, kind };
}

const files = fs.readdirSync(DECKS).filter((f) => f.endsWith('.json')).map((f) => f.replace(/\.json$/, ''));
const names = only.length ? only : files;
const totals = {};
for (const name of names) {
  const deck = JSON.parse(fs.readFileSync(path.join(DECKS, `${name}.json`), 'utf8'));
  const segs = loadTranscript(deck.sermon || name, deck);
  if (!segs) { console.log(`${name}: no ${OFFLINE ? 'offline ' : ''}transcript — skipped`); continue; }
  for (const mode of modes) {
    const r = run(deck, segs, mode);
    const s = score(deck, r);
    const group = `${mode}${deck.heldOut ? ' held-out' : ''}`;
    const t = totals[group] || (totals[group] = { caught: 0, missed: 0, early: 0, total: 0, right: 0, n: 0, st: [0, 0], sc: [0, 0] });
    t.st[0] += s.kind.statement[0]; t.st[1] += s.kind.statement[1]; t.sc[0] += s.kind.scripture[0]; t.sc[1] += s.kind.scripture[1];
    t.caught += s.caught; t.missed += s.missed; t.early += s.early; t.total += s.total; t.right += s.rightShare; t.n++;
    console.log(`${name}${deck.heldOut ? ' (held out)' : ''} [${mode}]  caught ${s.caught}/${s.total} (statements ${s.kind.statement[0]}/${s.kind.statement[1]}, scripture ${s.kind.scripture[0]}/${s.kind.scripture[1]})  early ${s.early}  median latency ${s.medianLatency == null ? '—' : (s.medianLatency / 1000).toFixed(1) + 's'}  right slide ${(s.rightShare * 100).toFixed(0)}% of the time`);
    if (VERBOSE) {
      for (const row of s.rows) console.log(`    ${fmt(row.at)}  #${row.slide} ${row.got == null ? 'MISSED' : '+' + ((row.got - row.at) / 1000).toFixed(1) + 's'}  ${deck.slides[row.slide].text.slice(0, 60)}`);
      for (const a of r.advances.filter((x) => x.early)) console.log(`    EARLY ${fmt(a.at)} → #${a.to} ${deck.slides[a.to].text.slice(0, 50)}  ← heard "${a.heard.slice(-70)}"`);
    }
  }
}
for (const [mode, t] of Object.entries(totals)) {
  console.log(`ALL [${mode}]  caught ${t.caught}/${t.total} (statements ${t.st[0]}/${t.st[1]}, scripture ${t.sc[0]}/${t.sc[1]})  early ${t.early}  right slide ${(t.right / t.n * 100).toFixed(0)}% of the time`);
}
