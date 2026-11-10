#!/usr/bin/env node
// Synthetic evaluation of the lyric follower (src/lyrics_follow.js).
//
// Builds songs from public-domain hymn text written out here and from Bible
// verses in databases/bibles/packs/*.json (es fr de pt sw yo ig ha), "sings"
// them through a degraded speech-recogniser model, feeds a LyricsFollower the
// way service.js does (a sliding window of 14 words, updated every two words) and scores it:
//
//   followed  slide changes the follower made on time, out of those the
//             arrangement needed
//   wrong     slide changes to a slide that was not being sung (or about to
//             be); a wrong advance on a live screen is the failure to avoid
//   delay     words after the new slide's first word was sung (negative =
//             the slide flipped early, as the last words of the old one landed)
//
// Usage:
//   node scripts/follow-eval.js                      current src/lyrics_follow.js
//   node scripts/follow-eval.js --lib path/to/old.js compare another build
//   node scripts/follow-eval.js --json               machine-readable
//
// The lyric text of any song other than the public-domain hymns is read from
// the Bible packs at run time; nothing copyrighted lives in this file.
'use strict';

const fs = require('fs');
const path = require('path');

const PACKS = path.join(__dirname, '..', 'databases', 'bibles', 'packs');

// ── fixtures ────────────────────────────────────────────────────────────
// Public-domain hymns (Newton 1779, Heber 1826, Toplady 1763), written out.
const HYMNS = {
  'Amazing Grace': [
    ['Verse 1', ['Amazing grace, how sweet the sound', 'That saved a wretch like me', 'I once was lost, but now am found', 'Was blind, but now I see']],
    ['Verse 2', ["'Twas grace that taught my heart to fear", 'And grace my fears relieved', 'How precious did that grace appear', 'The hour I first believed']],
    ['Verse 3', ['Through many dangers, toils and snares', 'I have already come', "'Tis grace hath brought me safe thus far", 'And grace will lead me home']],
  ],
  'Holy Holy Holy': [
    ['Verse 1', ['Holy, holy, holy, Lord God Almighty', 'Early in the morning our song shall rise to Thee', 'Holy, holy, holy, merciful and mighty', 'God in three Persons, blessed Trinity']],
    ['Verse 2', ['Holy, holy, holy, all the saints adore Thee', 'Casting down their golden crowns around the glassy sea', 'Cherubim and seraphim falling down before Thee', 'Which wert, and art, and evermore shalt be']],
  ],
  'Rock of Ages': [
    ['Verse 1', ['Rock of Ages, cleft for me', 'Let me hide myself in Thee', 'Let the water and the blood', 'From Thy wounded side which flowed', 'Be of sin the double cure', 'Save from wrath and make me pure']],
    ['Verse 2', ["Not the labors of my hands", "Can fulfill Thy law's demands", 'Could my zeal no respite know', 'Could my tears forever flow', 'All for sin could not atone', 'Thou must save, and Thou alone']],
  ],
};

const LANGS = ['en', 'es', 'fr', 'de', 'pt', 'sw', 'yo', 'ig', 'ha'];
const LANG_NAMES = { en: 'English', es: 'Spanish', fr: 'French', de: 'German', pt: 'Portuguese', sw: 'Swahili', yo: 'Yoruba', ig: 'Igbo', ha: 'Hausa' };

// New Testament passages every pack carries: [book, chapter, firstVerse, lastVerse]
const REFS = { v1: ['1 John', 4, 9, 10], v2: ['Romans', 8, 38, 39], chorus: ['John', 3, 16, 16], v3: ['Ephesians', 2, 8, 9] };

const packCache = {};
function packVerses(lang) {
  if (!packCache[lang]) {
    const raw = fs.readFileSync(path.join(PACKS, lang + '.json'), 'utf8').replace(/^﻿/, '');
    packCache[lang] = JSON.parse(raw).verses;
  }
  return packCache[lang];
}

// Split a verse into sung-sized lines: break at punctuation, merge scraps,
// halve anything long.
function chunkLines(text) {
  const parts = text.replace(/\{[^}]*\}/g, '').split(/(?<=[,;:.!?])\s+/).map((s) => s.trim()).filter(Boolean);
  const merged = [];
  for (const p of parts) {
    if (merged.length && (merged[merged.length - 1].split(/\s+/).length < 3)) merged[merged.length - 1] += ' ' + p;
    else merged.push(p);
  }
  if (merged.length > 1 && merged[merged.length - 1].split(/\s+/).length < 3) merged[merged.length - 2] += ' ' + merged.pop();
  const out = [];
  for (const p of merged) {
    const w = p.split(/\s+/);
    if (w.length > 10) { const h = Math.ceil(w.length / 2); out.push(w.slice(0, h).join(' '), w.slice(h).join(' ')); }
    else out.push(p);
  }
  return out;
}

function lyricsFrom(lang, ref) {
  const [book, ch, a, b] = ref;
  const vs = packVerses(lang).filter((v) => v.book === book && v.chapter === ch && v.verse >= a && v.verse <= b);
  return vs.flatMap((v) => chunkLines(v.text));
}

// A four-block song: Verse 1, Verse 2, Chorus, Verse 3.
function bibleSong(lang) {
  return {
    title: LANG_NAMES[lang],
    blocks: [
      { label: 'Verse 1', lines: lyricsFrom(lang, REFS.v1) },
      { label: 'Verse 2', lines: lyricsFrom(lang, REFS.v2) },
      { label: 'Chorus', lines: lyricsFrom(lang, REFS.chorus) },
      { label: 'Verse 3', lines: lyricsFrom(lang, REFS.v3) },
    ],
  };
}

function hymnBlocks(name) { return HYMNS[name].map(([label, lines]) => ({ label: name + ' ' + label, lines })); }
function englishSong() { return { title: 'Amazing Grace', blocks: hymnBlocks('Amazing Grace') }; }

// A song whose lines switch language mid-line: the first half of each
// Yoruba line, then the English words of the same thought.
function mixedSong() {
  const yo = bibleSong('yo'), es = bibleSong('es');
  const mix = (a, b) => a.blocks.map((bl, i) => ({
    label: bl.label,
    lines: bl.lines.map((ln, j) => {
      const other = b.blocks[i].lines[j % b.blocks[i].lines.length];
      const w1 = ln.split(/\s+/), w2 = other.split(/\s+/);
      return j % 2 === 0 ? w1.slice(0, Math.ceil(w1.length / 2)).concat(w2.slice(Math.floor(w2.length / 2))).join(' ')
        : w2.slice(0, Math.ceil(w2.length / 2)).concat(w1.slice(Math.floor(w1.length / 2))).join(' ');
    }),
  }));
  return { title: 'Mixed', blocks: mix(yo, es) };
}

// ── the speech-recogniser model ─────────────────────────────────────────
function rng(seed) {
  let a = seed >>> 0;
  return () => { a = (a + 0x6D2B79F5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
}

const CONFUSE = { a: 'o', e: 'i', i: 'e', o: 'u', u: 'o', b: 'p', p: 'b', d: 't', t: 'd', s: 'z', z: 's', k: 'g', g: 'k', m: 'n', n: 'm', v: 'f', f: 'v' };
const stripMarks = (s) => s.normalize('NFD').replace(/\p{M}/gu, '').replace(/ɓ/g, 'b').replace(/ɗ/g, 'd').replace(/ƙ/g, 'k');

const DEGRADE = {
  clean: (words) => words,
  diacritics: (words) => words.map((w) => ({ ...w, text: stripMarks(w.text) })),
  drop25: (words, r) => words.filter(() => r() >= 0.25),
  splitjoin: (words, r) => {
    const out = [];
    for (let i = 0; i < words.length; i++) {
      const w = words[i], x = r();
      if (x < 0.15 && w.text.length >= 6) {
        const c = 2 + Math.floor(r() * (w.text.length - 4));
        out.push({ ...w, text: w.text.slice(0, c) }, { ...w, text: w.text.slice(c) });
      } else if (x < 0.3 && i + 1 < words.length) {
        out.push({ text: w.text + words[i + 1].text, sung: words[i + 1].sung }); i++;
      } else out.push(w);
    }
    return out;
  },
  swap: (words, r) => words.map((w) => {
    if (r() >= 0.2 || w.text.length < 3) return w;
    const chars = Array.from(w.text);
    const at = Math.floor(r() * chars.length);
    const lower = chars[at].toLowerCase();
    chars[at] = CONFUSE[lower] || (lower === chars[at] ? 'e' : 'E');
    return { ...w, text: chars.join('') };
  }),
};
DEGRADE.all = (words, r) => DEGRADE.swap(DEGRADE.splitjoin(DEGRADE.drop25(DEGRADE.diacritics(words, r), (() => { const q = r; return () => q() * 0.8; })()), r), r);

// ── arrangements ────────────────────────────────────────────────────────
// A plan lists what is sung: [{ block, lines?: [lineIndexes] }, ...]
function sungWords(song, plan) {
  const words = [];
  const segs = [];
  plan.forEach((seg) => {
    const b = song.blocks[seg.block];
    const idxs = seg.lines || b.lines.map((_, i) => i);
    const start = words.length;
    idxs.forEach((li) => b.lines[li].split(/\s+/).filter(Boolean).forEach((text) => words.push({ text, block: seg.block })));
    segs.push({ block: seg.block, start, end: words.length });
  });
  words.forEach((w, i) => { w.sung = i; });
  return { words, segs };
}

// Run one performance through a follower; return what happened.
function perform(lib, song, plan, { degrade = 'clean', seed = 1, startBlock, wpm = 130, tick = 2, window = 14 } = {}) {
  const { words, segs } = sungWords(song, plan);
  const heard = DEGRADE[degrade](words, rng(seed * 7919 + 13));
  const advances = [];
  let clock = 1000000, fed = 0;
  const f = new lib.LyricsFollower(song, { config: JSON.parse(process.env.FOLLOW_CFG || '{}'), onAdvance: (e) => advances.push({ to: e.toBlockIdx, fed, conf: e.confidence, reason: e.reason }) });
  f.resync(startBlock != null ? startBlock : plan[0].block, clock);
  const msPerWord = 60000 / wpm;
  let tickNo = 0;
  for (let i = 0; i < heard.length; i += tick) {
    const upto = Math.min(heard.length, i + tick);
    clock += msPerWord * (upto - i);
    fed = heard[upto - 1].sung + 1;
    tickNo++;
    f.ingest(heard.slice(Math.max(0, upto - window), upto).map((h) => h.text).join(' '), { now: clock, isFinal: upto === heard.length || tickNo % 8 === 0 });
  }

  // Score.
  const blockAt = (n) => (words[Math.min(words.length - 1, Math.max(0, n))] || {}).block;
  const transitions = [];
  segs.forEach((s, i) => { if (i > 0 && s.block !== segs[i - 1].block) transitions.push({ block: s.block, start: s.start, end: s.end, got: null }); });
  let wrong = 0;
  for (const a of advances) {
    const near = [blockAt(a.fed - 1), blockAt(a.fed), blockAt(a.fed + 1), blockAt(a.fed + 2)];
    // the block that the singers are leaving does not count as "about to be sung"
    const cur = blockAt(a.fed - 1);
    const ok = a.to === cur || near.slice(1).includes(a.to);
    const tr = transitions.find((t) => t.block === a.to && t.got === null && a.fed >= t.start - 3 && a.fed <= t.end);
    if (ok && tr) tr.got = a.fed - tr.start;
    else if (!ok) wrong++;
  }
  return { transitions, wrong, advances: advances.length };
}

// ── suites ──────────────────────────────────────────────────────────────
const SEEDS = [1, 2, 3];
const DEGRADES = ['clean', 'diacritics', 'drop25', 'splitjoin', 'swap', 'all'];

function suites() {
  const out = [];
  // 1. every language, every degradation, V1 V2 Chorus V3 in order
  for (const lang of LANGS) {
    const song = lang === 'en' ? { title: 'English', blocks: [...hymnBlocks('Amazing Grace'), ...hymnBlocks('Holy Holy Holy').slice(0, 1)] } : bibleSong(lang);
    const order = song.blocks.map((_, i) => ({ block: i }));
    out.push({ group: 'language', name: LANG_NAMES[lang], song, plan: order });
  }
  // 2. mixed-language lines
  const mixed = mixedSong();
  out.push({ group: 'mixed', name: 'Mixed Yoruba/Spanish lines', song: mixed, plan: mixed.blocks.map((_, i) => ({ block: i })) });
  // 3. chorus three times, out of the deck's order (one Chorus slide only)
  for (const lang of ['en', 'es', 'yo', 'sw']) {
    const song = lang === 'en' ? englishChorusSong() : reorderChorus(bibleSong(lang));   // [V1, Chorus, V2, V3]
    out.push({ group: 'chorus x3', name: LANG_NAMES[lang], song, plan: [0, 1, 2, 1, 3, 1].map((b) => ({ block: b })) });
  }
  // 4. chorus repeated back to back, with its own slide each time
  {
    const song = englishChorusSong();
    const dup = { title: song.title, blocks: [song.blocks[0], song.blocks[1], { ...song.blocks[1], label: 'Chorus (2)' }, song.blocks[2]] };
    out.push({ group: 'chorus slides', name: 'English, chorus twice, 2 slides', song: dup, plan: [0, 1, 2, 3].map((block) => ({ block })) });
  }
  // 5. the leader repeats one line (and then another three times)
  for (const lang of ['en', 'fr', 'ig']) {
    const song = lang === 'en' ? englishChorusSong() : bibleSong(lang);
    const b0 = song.blocks[0].lines.length;
    out.push({ group: 'repeat a line', name: LANG_NAMES[lang], song, plan: [
      { block: 0, lines: Array.from({ length: b0 }, (_, i) => i).flatMap((i) => (i === 1 ? [1, 1] : [i])) },
      { block: 1, lines: Array.from({ length: song.blocks[1].lines.length }, (_, i) => i).flatMap((i) => (i === 0 ? [0, 0, 0] : [i])) },
      { block: 2 }, { block: 3 },
    ] });
  }
  // 6. a verse skipped
  for (const lang of ['en', 'de', 'ha']) {
    const song = lang === 'en' ? englishChorusSong() : bibleSong(lang);
    const n = song.blocks.length;
    out.push({ group: 'skip a verse', name: LANG_NAMES[lang], song, plan: [{ block: 0 }, { block: 2 }, { block: n - 1 }] });
  }
  // 7. one deck, several songs
  {
    const deck = { title: 'Deck', blocks: [...hymnBlocks('Amazing Grace'), ...hymnBlocks('Holy Holy Holy'), ...hymnBlocks('Rock of Ages')] };
    out.push({ group: 'deck', name: 'Holy Holy Holy then Rock of Ages', song: deck, plan: [{ block: 3 }, { block: 4 }, { block: 5 }, { block: 6 }] });
    out.push({ group: 'deck', name: 'whole deck in order', song: deck, plan: deck.blocks.map((_, i) => ({ block: i })) });
    const mixedDeck = { title: 'Deck', blocks: [...bibleSong('pt').blocks, ...bibleSong('yo').blocks, ...hymnBlocks('Rock of Ages')] };
    out.push({ group: 'deck', name: 'Portuguese, Yoruba, English in one deck', song: mixedDeck, plan: mixedDeck.blocks.map((_, i) => ({ block: i })) });
  }
  return out;
}

function englishChorusSong() {
  const ag = hymnBlocks('Amazing Grace');
  return { title: 'English', blocks: [ag[0], { label: 'Chorus', lines: HYMNS['Holy Holy Holy'][0][1] }, ag[1], ag[2]] };
}
function reorderChorus(song) { const [v1, v2, c, v3] = song.blocks; return { title: song.title, blocks: [v1, c, v2, v3] }; }

function runSuites(lib, opts) {
  opts = opts || {};
  const degrades = opts.degrades || DEGRADES;
  const rows = [];
  for (const s of suites()) {
    const dg = (s.group === 'language' || s.group === 'mixed') ? degrades : ['clean', 'all'];
    for (const d of dg) {
      for (const seed of SEEDS) {
        const r = perform(lib, s.song, s.plan, { degrade: d, seed });
        rows.push({ group: s.group, name: s.name, degrade: d, seed, need: r.transitions.length, got: r.transitions.filter((t) => t.got !== null).length, wrong: r.wrong, delays: r.transitions.filter((t) => t.got !== null).map((t) => t.got) });
      }
    }
  }
  return rows;
}

function summarise(rows, keyFn) {
  const by = new Map();
  for (const r of rows) {
    const k = keyFn(r);
    const s = by.get(k) || { need: 0, got: 0, wrong: 0, delays: [] };
    s.need += r.need; s.got += r.got; s.wrong += r.wrong; s.delays.push(...r.delays);
    by.set(k, s);
  }
  return by;
}
const median = (a) => { if (!a.length) return NaN; const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
const mean = (a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : NaN);

module.exports = { runSuites, summarise, perform, suites, bibleSong, englishChorusSong, hymnBlocks, HYMNS, DEGRADE, median, mean, DEGRADES, LANGS, LANG_NAMES };

if (require.main === module) {
  const args = process.argv.slice(2);
  const i = args.indexOf('--lib');
  const lib = require(path.resolve(i >= 0 ? args[i + 1] : path.join(__dirname, '..', 'src', 'lyrics_follow.js')));
  const rows = runSuites(lib);
  if (args.includes('--json')) { console.log(JSON.stringify(rows)); process.exit(0); }
  const fmt = (s) => `${String(s.got).padStart(4)}/${String(s.need).padEnd(4)} followed  wrong ${String(s.wrong).padStart(2)}  delay mean ${mean(s.delays).toFixed(1)} median ${median(s.delays)}`;
  for (const [title, fn] of [['by group', (r) => r.group], ['by degradation (language + mixed)', (r) => (r.group === 'language' || r.group === 'mixed' ? r.degrade : null)], ['all', () => 'ALL']]) {
    console.log('\n' + title);
    for (const [k, s] of summarise(rows, fn)) if (k) console.log('  ' + String(k).padEnd(16) + fmt(s));
  }
}
