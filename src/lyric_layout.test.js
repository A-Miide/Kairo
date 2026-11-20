// Tests for src/lyric_layout.js's layout rules (the drawing needs a browser; the rules don't), and for the hymn bank
// being read clean (hymns.js). A stand-in ruler measures text: every letter the same width, capitals with no
// descenders, so sizes and breaks can be checked by hand.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { planBlock, planSlide, slideRows } = require('./lyric_layout.js');
const { cleanBankLine } = require('./hymns.js');

const measure = (text) => ({ w: text.length * 0.62, l: 0, up: 0.72, down: /[gjpqy]/.test(text) ? 0.2 : 0, fA: 0.97, fD: 0.24 });
const box = { w: 1640, h: 860 };
const JOHN = 'For God so loved the world, that he gave his only begotten Son.';
const clean = (t) => t.replace(/[\u0001\u0002]/g, '');
const words = (rows) => rows.flatMap(r => r.kind === 'line' ? [r.text] : [...r.left, r.big]).join(' ');

test('every line is sized to the same width', () => {
  const res = planBlock(JOHN, box, measure, { caps: true });
  for (const p of res.g.placed.filter(p => p.kind === 'line')) assert.ok(Math.abs(p.s * p.k.w - res.g.W) < 1e-6, p.text);
});

test('the same words always lay out the same way', () => {
  const a = planBlock(JOHN, box, measure, { caps: true }), b = planBlock(JOHN, box, measure, { caps: true });
  assert.deepEqual(a.rows, b.rows);
});

test('every word is kept, in order, and no line ends on "the" or "and"', () => {
  const res = planBlock(JOHN, box, measure, { caps: true });
  assert.equal(words(res.rows).replace(/[,.]/g, ''), JOHN.toUpperCase().replace(/[,.]/g, ''));
  for (const r of res.rows.filter(r => r.kind === 'line').slice(0, -1)) assert.doesNotMatch(r.text, /\b(THE|AND|OF)$/);
});

test('lines break at the lyric\'s own line ends first', () => {
  const res = planBlock('When peace like a river attendeth my way\nWhen sorrows like sea billows roll', box, measure);
  const ends = res.rows.map(r => r.kind === 'line' ? r.text : r.left[1]);
  assert.ok(ends.some(t => /way$/.test(t)), JSON.stringify(ends));
});

test('a short closing word stands tall only in capitals', () => {
  const caps = planBlock('That he gave his only begotten Son', { w: 1640, h: 500 }, measure, { caps: true });
  const typed = planBlock('That he gave his only begotten Son', { w: 1640, h: 500 }, measure, { caps: false });
  assert.ok(!typed.rows.some(r => r.kind === 'tall'), 'never in as-typed text');
  if (caps.tall) assert.equal(caps.rows[caps.rows.length - 1].big, 'SON');
});

test('text too long to read as a poster falls back to one even size', () => {
  const esther = 'Then were the king\'s scribes called at that time in the third month, that is, the month Sivan, on the three and twentieth day thereof; and it was written according to all that Mordecai commanded unto the Jews, and to the lieutenants, and the deputies and rulers of the provinces which are from India unto Ethiopia, an hundred twenty and seven provinces.';
  assert.equal(planBlock(esther, box, measure, { caps: true }).plain, true);
});

test('the smallest letters of a block are never below the readable size', () => {
  for (const t of [JOHN, 'Amen', 'Be still, and know that I am God', 'O come, let us adore Him, Christ the Lord']) {
    const res = planBlock(t, box, measure, { caps: true });
    if (!res.plain) assert.ok(res.g.minCap >= 0.055 * box.h - 1e-6, t);
  }
});

test('slide-in rows: a slide\'s own lines, at most four, without their closing commas', () => {
  assert.deepEqual(slideRows('When peace like a river,\nattendeth my way,\nWhen sorrows like sea billows roll,\nit is well\nwith my soul').map(clean),
    ['When peace like a river', 'attendeth my way', 'When sorrows like sea billows roll', 'it is well']);
});

test('slide-in rows: one long line (a verse) is split into up to four rows', () => {
  const rows = slideRows(JOHN);
  assert.ok(rows.length >= 2 && rows.length <= 4, JSON.stringify(rows));
  assert.equal(rows.join(' ').replace(/[,.]/g, ''), JOHN.replace(/[,.]/g, ''));
});

test('slide in keeps a slide\'s own lines, all one size (the longest fits the width), and re-breaks them when they run small', () => {
  const res = planSlide('When peace like a river\nattendeth my way', box, measure, { caps: true });
  assert.deepEqual(res.rows.map(r => clean(r.text)), ['WHEN PEACE LIKE A RIVER', 'ATTENDETH MY WAY']);
  const sizes = res.g.placed.map(p => p.s);
  assert.ok(sizes.every(x => Math.abs(x - sizes[0]) < 1e-9), 'one size');
  assert.ok(Math.abs(Math.max(...res.g.placed.map(p => p.s * p.k.w)) - res.g.W) < 1e-6, 'the longest fills the width');
  const long = planSlide('And now we fight the battle but then shall wear the crown of full and everlasting and passionless renown', box, measure, { caps: true });
  assert.notDeepEqual(long.rows?.map(r => r.text), ['AND NOW WE FIGHT THE BATTLE BUT THEN SHALL WEAR THE CROWN OF FULL AND EVERLASTING AND PASSIONLESS RENOWN']);
});

test('words coloured in the slide editor (*like this*) keep their place, lose the asterisks, and carry a mark', () => {
  const res = planBlock('For God so *loved the* world', box, measure, { caps: true });
  const text = words(res.rows);
  assert.doesNotMatch(text, /\*/);
  assert.equal(text.replace(/\u0001/g, ''), 'FOR GOD SO LOVED THE WORLD');
  assert.equal((text.match(/\u0001/g) || []).length, 2, 'both coloured words marked');
});

test('two-tone: on a slide of two lyric lines, every word of the second line (as written) is marked, wherever it breaks', () => {
  const text = 'Amazing grace how sweet the sound\nthat saved a wretch like me';
  const all = words(planBlock(text, box, measure, { caps: true }).rows).split(' ');
  const last = all.filter(w => w.startsWith('\u0002')).map(clean);
  assert.deepEqual(last, ['THAT', 'SAVED', 'A', 'WRETCH', 'LIKE', 'ME']);
  const single = words(planBlock('For God so loved the world', box, measure, { caps: true }).rows);
  assert.doesNotMatch(single, /\u0002/, 'a one-line slide marks nothing');
});

test('balance: the weight never sits at the top (no one big line crowning smaller ones)', () => {
  for (const t of [JOHN, 'O come let us adore Him, Christ the Lord', 'Be still, and know that I am God', 'There is power in the name of Jesus']) {
    const res = planBlock(t, box, measure, { caps: true });
    if (res.plain || res.g.placed.length < 2) continue;
    const caps = res.g.placed.map(p => p.cap);
    assert.ok(!(caps[0] === Math.max(...caps) && caps[0] > 1.5 * caps[caps.length - 1]), `${t}: ${res.rows.map(r => r.text || r.big).join(' / ')}`);
  }
});

test('the hymn bank is read clean: sung syllables joined, broken apostrophes and run-together lines mended', () => {
  assert.equal(cleanBankLine('Born of Mar* y'), 'Born of Mary');
  assert.equal(cleanBankLine('those bless�d strains'), 'those bless’d strains');
  assert.equal(cleanBankLine('Nor our poor hymns disdainethMy soul'), 'Nor our poor hymns disdaineth My soul');
  assert.equal(cleanBankLine('McKinley and MacDonald'), 'McKinley and MacDonald');
});
