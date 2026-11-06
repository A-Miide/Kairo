// Tests for src/sermon_slides.js: plain node:test, no DOM.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const S = require('./sermon_slides.js');

// A pasted outline laid out as pastors' files are: header lines, a story, numbered points with
// references after a dash, typed verses under them, a section line, a closing, then a note.
const PASTED = [
  'GRACE CHAPEL INTERNATIONAL',
  'Prophetic Theme: I AM REDEEMED – Isa. 8:18',
  'Teaching series: UNLOCKING THE SUPERNATURAL – Jhn. 3:8',
  'There is this story of a lion cub that grew up among the sheep and never knew it could roar until one day an old lion came down to the river and showed it its reflection in the water.',
  '1. Abraham had to suspend his natural senses to obtain the promise, against hope believing in hope – Rom. 4:16-21/Heb. 11:17-19',
  'Rom 4:16-21',
  '16 Therefore it is of faith, that it might be by grace; to the end the promise might be sure to all the seed.',
  '2. Gideon had to suspend his natural senses to deliver Israel – Judg.6:1-16 / Judg. 7:1-20',
  '3. Daniel had to suspend his natural senses to survive among hungry lions – Dan. 6:10-23',
  'But who am I?',
  '4. I am redeemed, a spirit being in human flesh – Jhn. 3:5/Act. 14:11',
  'Jesus is Lord!',
  'Note:',
  '5. Next Sunday is our second service in the series.',
].join('\n');

test('a pasted outline is read from its layout', () => {
  const o = S.readText(PASTED);
  assert.ok(o, 'read as an outline');
  const kinds = o.items.map(i => i.kind);
  assert.deepEqual(kinds, ['story', 'point', 'point', 'point', 'section', 'point', 'closing', 'note']);
  assert.deepEqual(o.items[1].refs, ['Rom. 4:16-21', 'Heb. 11:17-19']);
  assert.equal(o.items[1].verses[0].ref, 'Rom 4:16-21');
});

test('a document that is not an outline is left alone', () => {
  assert.equal(S.readText('Amazing grace, how sweet the sound\nThat saved a wretch like me\nI once was lost'), null);
});

test('every reference is written out the same way', () => {
  assert.equal(S.normaliseRef('Rom. 4:16-21'), 'Romans 4:16–21');
  assert.equal(S.normaliseRef('Judg.6:1-16'), 'Judges 6:1–16');
  assert.equal(S.normaliseRef('Jhn. 3:8'), 'John 3:8');
  assert.equal(S.normaliseRef('1 Cor. 2:10-11'), '1 Corinthians 2:10–11');
  assert.equal(S.normaliseRef('Isa. 8:18 –'), 'Isaiah 8:18');
  assert.equal(S.normaliseRef('Mak. 16:15-16, 20'), 'Mark 16:15–16, 20');
  assert.equal(S.normaliseRef('Ps 23:1'), 'Psalm 23:1');
  assert.equal(S.normaliseRef('Phil 4:13'), 'Philippians 4:13');
  assert.equal(S.normaliseRef('the next point'), null);
});

test('a long point is cut at a natural break, never mid-thought, and marked', () => {
  const long = S.shortenPoint('Abraham had to suspend his natural senses to obtain the promise, against hope believing in hope');
  assert.equal(long.text, 'Abraham had to suspend his natural senses to obtain the promise');
  assert.equal(long.shortened, true);
  const short = S.shortenPoint('I am redeemed, a spirit being in human flesh');
  assert.equal(short.shortened, false);
  // No cut that would end on a small word ("of"): the point stays whole.
  assert.equal(S.shortenPoint('But the knowledge of who we are is what determines what we can do in life today').shortened, false);
});

test('one accent word, never a name', () => {
  assert.equal(S.accentWord('Abraham had to suspend his natural senses'), 'suspend');
  assert.equal(S.accentWord('Not just any Word, but the right Word'), '');
});

test('the deck: a title, the story, a slide per point with chips, the section and the closing', () => {
  const deck = S.build(S.readText(PASTED), { kit: 'midnight', style: 'gradient' });
  assert.deepEqual(deck.map(b => b.gen.kind), ['title', 'story', 'point', 'point', 'point', 'section', 'point', 'closing']);
  const p1 = deck[2].gen;
  assert.equal(p1.label, 'Point 1 of 4');
  assert.deepEqual(p1.refs, ['Romans 4:16–21', 'Hebrews 11:17–19']);
  assert.equal(p1.shortened, true);
  assert.match(deck[2].notes, /against hope/);
  assert.deepEqual(deck[0].gen.refs, ['Isaiah 8:18', 'John 3:8']);
  // The note is the presenter's, not a slide.
  assert.match(deck[deck.length - 1].presenterNote, /Next Sunday/);
  // Every word is a real text layer.
  assert.ok(deck[2].layers.some(l => l.name === 'Headline' && l.text));
});

test('the operator chooses the parts: leave a point out, put a typed verse in', () => {
  const o = S.readText(PASTED);
  const parts = S.partsOf(o);
  assert.ok(parts.some(p => p.kind === 'verse' && !p.on), 'typed verses are listed, off');
  assert.ok(parts.some(p => p.kind === 'line' && /GRACE CHAPEL/.test(p.text)), 'set-aside lines are listed');
  const include = new Set(parts.filter(p => p.on).map(p => p.id));
  const second = parts.filter(p => p.kind === 'point')[1];
  const verse = parts.find(p => p.kind === 'verse');
  include.delete(second.id);
  include.add(verse.id);
  const deck = S.build(o, { include });
  const points = deck.filter(b => b.gen.kind === 'point');
  assert.equal(points.length, 3);
  assert.equal(points[1].gen.label, 'Point 2 of 3');
  assert.ok(deck.some(b => b.gen.part === verse.id && b.gen.label === 'Romans 4:16–21'));
});

test('a part changed in place redraws the slide and keeps its layers', () => {
  const deck = S.build(S.readText(PASTED), {});
  const b = deck[2];
  const edited = S.withPart(b, 'lines', 'Abraham believed against all hope');
  assert.equal(edited.gen.lines[0], 'Abraham believed against all hope');
  assert.equal(edited.gen.shortened, false);
  const headline = (blk) => blk.layers.find(l => l.name === 'Headline');
  assert.equal(headline(edited).id, headline(b).id, 'the layer keeps its id, so a move made in the editor stays');
  const chip = S.withPart(b, 'ref:1', 'Heb 11:1');
  assert.deepEqual(chip.gen.refs, ['Romans 4:16–21', 'Hebrews 11:1']);
  assert.equal(S.partOfLayer(b, headline(b)), 'lines');
});

test('every look reads: text 4.5:1 and the accent 3:1 on both background tones', () => {
  for (const k of S.KITS) assert.deepEqual(S.kitWarnings(k), [], k.name);
});
