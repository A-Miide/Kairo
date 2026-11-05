'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const SM = require('./slide_markers.js');
const { SlideFollower } = require('./slide_follow.js');

const slideText = (n, body) => `${body}\n${n}.\nJOHN 6:48-58`;

test('a number-only box is the verse number, the citation box the reference, the rest the words', () => {
  const p = SM.parse(slideText(48, 'I am that bread of life.'));
  assert.equal(p.marker, 48);
  assert.equal(p.kind, 'verse');
  assert.equal(p.reference, 'JOHN 6:48-58');
  assert.equal(p.body, 'I am that bread of life.');
  assert.deepEqual(SM.display(slideText(49, 'Your fathers did eat manna in the wilderness, and are dead.')), { text: '49. Your fathers did eat manna in the wilderness, and are dead.', reference: 'JOHN 6:48-58' });
});

test('a number that is not inside the named passage, or with no passage, is an index', () => {
  assert.equal(SM.parse('2.\nWhat we will do').kind, 'index');
  assert.equal(SM.parse('2.\nWhat we will do').marker, 2);
  assert.equal(SM.parse('99.\nJohn 6:48-58\nbody').kind, 'index');
  assert.equal(SM.parse('Be kind to one another').marker, null);
});

test('words that only look like a citation are not one', () => {
  assert.equal(SM.parse('Page 3:15\nsomething').reference, null);
  assert.equal(SM.parse('Meet at 6:30\nthen').reference, null);
  // A service time on an announcement slide
  assert.equal(SM.parse('Join us Sunday 9:30\n2.\nWorship').reference, null);
  assert.equal(SM.parse('Join us Sunday 9:30\n2.\nWorship').kind, 'index');
  assert.equal(SM.parse('October 12:00\nLunch').reference, null);
  // ...and a real book is still one
  assert.equal(SM.parse('48.\nJohn 6:48-58\nI am that bread of life.').reference, 'John 6:48-58');
});

test('plain slides are untouched', () => {
  assert.deepEqual(SM.display('Welcome to church'), { text: 'Welcome to church', reference: '' });
});

test('a slide marked as one verse carries only that verse; an unmarked one carries its whole passage', () => {
  const refs = [{ book: 'John', chapter: 6, verse: 48, verseEnd: 58 }];
  const v = (n) => ({ book: 'John', chapter: 6, verse: n });
  assert.equal(SM.carries({ refs, marker: 49, markerEnd: 49, markerKind: 'verse' }, v(49)), true);
  assert.equal(SM.carries({ refs, marker: 49, markerEnd: 49, markerKind: 'verse' }, v(50)), false);
  assert.equal(SM.carries({ refs, marker: 2, markerKind: 'index' }, v(50)), true);
  assert.equal(SM.carries({ refs }, v(50)), true);
  assert.equal(SM.carries({ refs }, { book: 'John', chapter: 7, verse: 50 }), false);
});

test('a spoken "verse 50" moves the deck to the slide marked 50, not just any slide of the passage', () => {
  const texts = [48, 49, 50, 51].map(n => slideText(n, `words of verse ${n}`));
  const moves = [];
  const f = new SlideFollower({ slides: texts }, { onAdvance: (e) => moves.push(e.toIndex), onPosition: () => {} });
  const range = [{ book: 'John', chapter: 6, verse: 48, verseEnd: 58, raw: 'JOHN 6:48-58' }];
  f.setRefs(texts.map(() => range));
  f.resync(0);
  assert.equal(f.noteVerse({ book: 'John', chapter: 6, verse: 50 }), true);
  assert.deepEqual(moves, [2]);
  // already on the slide that carries it: nothing moves
  assert.equal(f.noteVerse({ book: 'John', chapter: 6, verse: 50 }), false);
});

const { indexCall } = require('./slide_follow.js');

test('a numbered point is heard; a citation, a book or a bare number is not', () => {
  assert.equal(indexCall('so point three is that God is faithful'), 3);
  assert.equal(indexCall('step 4 is to pray'), 4);
  assert.equal(indexCall('reason twenty one'), 21);
  assert.equal(indexCall('number six verse 24'), null);          // the book of Numbers
  assert.equal(indexCall('Numbers 6 says'), null);
  assert.equal(indexCall('a number of people came'), null);
  assert.equal(indexCall('three people came'), null);
  assert.equal(indexCall('slide three of chapter five'), null);   // "of": not a point call
});

test('ordinary talk with a number in it never moves the deck', () => {
  assert.equal(indexCall('He had a key, one that opened the door'), null);
  assert.equal(indexCall('The lesson one learns is patience'), null);
  assert.equal(indexCall('number one priority is prayer'), null);
  assert.equal(indexCall('the reason one prays'), null);
  assert.equal(indexCall('thing one and thing two'), null);
  // ...while a point said as one still is
  assert.equal(indexCall('Number one: pray'), 1);
  assert.equal(indexCall('reason one, God loves you'), 1);
});

test('"point three" moves the deck to the slide numbered 3, forward only, and never while scripture is read', () => {
  const texts = ['Welcome', '1.\nGod is good', '2.\nGod is near', '3.\nGod is faithful', '4.\nGod is able'];
  const moves = [];
  const mk = () => { const f = new SlideFollower({ slides: texts }, { onAdvance: (e) => moves.push([e.toIndex, e.by]), onPosition: () => {} }); f.resync(0); return f; };
  let f = mk();
  f.ingest('so the third thing, point three, is', { isFinal: true, now: 1000 });
  assert.deepEqual(moves, [[3, 'index']]);
  // not backwards
  f.ingest('remember point one', { isFinal: true, now: 20000 });
  assert.equal(moves.length, 1);
  // scripture in progress: a number is a verse
  moves.length = 0; f = mk();
  f.ingest('point three', { isFinal: true, now: 1000, scriptureActive: true });
  assert.equal(moves.length, 0);
  // not on a partial transcript
  f.ingest('point three', { isFinal: false, now: 2000 });
  assert.equal(moves.length, 0);
});
