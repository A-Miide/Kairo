// Tests for src/slide_follow.js: plain node:test, no DOM.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { SlideFollower } = require('./slide_follow.js');

// A deck in no particular make: each slide repeats a "Slide N of M" line (any deck has some such chrome), has
// its own statement, and may list scripture references (which the server finds: `refs`).
const ref = (raw, book, chapter, verse) => ({ raw, book, chapter, verse, verseEnd: verse });
const DECK = [
  { text: 'Slide 1 of 5\nWelcome to the service' },
  { text: 'Slide 2 of 5\n7 Keys to Sustaining Divine Presence;' },
  { text: 'Slide 3 of 5\nThrough the fear of God as a lifestyle as we saw in Joseph\nRead: Genesis 39:3', refs: [ref('Genesis 39:3', 'Genesis', 39, 3)] },
  { text: 'Slide 4 of 5\nThrough Joy and rejoicing\nRead: Psalm 16:11 · Psalm 22:3', refs: [ref('Psalm 16:11', 'Psalms', 16, 11), ref('Psalm 22:3', 'Psalms', 22, 3)] },
  { text: 'Slide 5 of 5\nThrough meekness of spirit\nRead: Numbers 12:3', refs: [ref('Numbers 12:3', 'Numbers', 12, 3)] },
];

// Speaks `lines` at 380 ms a word, an interim every three words and a final at each line's end.
function advances(startAt, lines) {
  const out = [];
  const f = new SlideFollower({ slides: DECK }, { onAdvance: (e) => out.push(e.toIndex) });
  f.resync(startAt, -1e9);
  let t = 0;
  for (const line of lines) {
    const words = line.split(' ');
    for (let i = 3; i < words.length; i += 3) { t += 3 * 380; f.ingest(words.slice(0, i).join(' '), { isFinal: false, now: t }); }
    t += 380; f.ingest(line, { isFinal: true, now: t });
  }
  return out;
}

test('a statement said in full moves the deck even when its slide repeats a deck label and lists scripture references', () => {
  assert.deepEqual(advances(1, ['and now through the fear of God as a lifestyle as we saw in Joseph']), [2]);
});

test('a short heading said word for word moves it too', () => {
  assert.deepEqual(advances(2, ['next is through joy and rejoicing']), [3]);
});

test('the deck stays put on talk that is not a slide', () => {
  assert.deepEqual(advances(1, ['praise the Lord everybody we are going to continue and God is good all the time']), []);
});

test('the deck does not move for the reference list alone', () => {
  assert.deepEqual(advances(1, ['Genesis thirty nine verses three and four']), []);
});

test('the next slide moves on its two telling words when the speech engine garbles a third', () => {
  assert.deepEqual(advances(2, ['the next is tru joy and rejoicin']), [3]);
});

test('two telling words of a slide further on are not enough', () => {
  assert.deepEqual(advances(1, ['and meekness of spirit']), []);
});

test('a big number alone on a line of each slide is the deck\'s, not something the preacher says', () => {
  const deck = [1, 2, 3, 4].map((n) => ({ text: `0${n}\nPoint ${n} of 4\n${['Welcome to the service', 'The healing power of God', 'The restoration power of God', 'The oil for joy and rejoicing'][n - 1]}` }));
  const out = [];
  const f = new SlideFollower({ slides: deck }, { onAdvance: (e) => out.push(e.toIndex) });
  f.resync(0, -1e9);
  let t = 0;
  for (const line of ['the healing power of God']) {
    const words = line.split(' ');
    for (let i = 3; i < words.length; i += 3) { t += 1140; f.ingest(words.slice(0, i).join(' '), { isFinal: false, now: t }); }
    t += 400; f.ingest(line, { isFinal: true, now: t });
  }
  assert.deepEqual(out, [1]);
});
