// Language-neutral lyric following: normalisation, character n-gram matching,
// degraded speech, repeats and out-of-order choruses, several songs in one
// deck. The songs are public-domain hymns written out in scripts/follow-eval.js
// and Bible verses read from databases/bibles/packs (never copyrighted lyrics).
// Plain node:test, no server.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const lib = require('./lyrics_follow.js');
const ev = require('../scripts/follow-eval.js');

const { LyricsFollower, key, keyMatch, tokenizeKeys } = lib;

// The same follower with anticipation (flip as the last words land) off.
function withoutAnticipation() {
  function F(song, opts) {
    opts = Object.assign({}, opts);
    opts.config = Object.assign({ anticipate: false }, opts.config);
    return new LyricsFollower(song, opts);
  }
  F.prototype = LyricsFollower.prototype;
  return { LyricsFollower: F };
}

// Each configuration runs the whole synthetic set once.
const memo = new Map();
const suiteRows = (name, impl) => { if (!memo.has(name)) memo.set(name, ev.runSuites(impl)); return memo.get(name); };

const noAnt = withoutAnticipation();
const total = (rows) => rows.reduce((a, r) => ({ need: a.need + r.need, got: a.got + r.got, wrong: a.wrong + r.wrong }), { need: 0, got: 0, wrong: 0 });

test('normalisation keeps letters of every script and strips marks', () => {
  assert.equal(key('Ọlọ́run', 'yo'), 'olorun');                 // Yoruba tones and dots
  assert.equal(key('àgùntàn', 'yo'), 'aguntan');
  assert.equal(key('Éternel', 'fr'), 'eternel');
  assert.equal(key('coração', 'pt'), 'coracao');
  assert.equal(key('ƙauna', 'ha'), 'kauna');                    // Hausa hooked k
  assert.equal(key('Господь', 'ru'), 'господь');               // Cyrillic survives
  assert.equal(key('الله', 'ar'), 'الله');                      // Arabic survives
  assert.ok(key('Ịhụnanya', 'ig').length > 0);
  assert.deepEqual(tokenizeKeys('神爱世人，甚至', 'zh'), ['神', '爱', '世', '人', '甚', '至']);   // one token per character
  assert.deepEqual(tokenizeKeys('บ้านของฉัน', 'th').length, 'บ้านของฉัน'.replace(/\p{M}/gu, '').length);
});

test('English phonetics apply to English only', () => {
  assert.equal(key('thee'), 'the');                              // default: English, as every existing caller expects
  assert.equal(key('thee', 'en'), 'the');
  assert.equal(key('thee', 'es'), 'thee');
  assert.equal(key('singing'), 'singin');
  assert.equal(key('singing', 'yo'), 'singing');
  assert.ok(keyMatch('believ', 'believed'));
  assert.ok(!keyMatch('is', 'isle'));
});

test('a Spanish song is not given English rules, an English one is', () => {
  const es = new LyricsFollower({ blocks: [{ label: 'A', lines: ['El Señor es mi pastor', 'nada me faltará'] }] });
  const en = new LyricsFollower({ blocks: [{ label: 'A', lines: ['The Lord is my shepherd', 'I shall not want'] }] });
  assert.notEqual(es.lang, 'en');
  assert.equal(en.lang, 'en');
});

test('in order, in every language, under every degradation: no wrong advance, nearly every slide followed', () => {
  const rows = suiteRows('default', lib).filter((r) => r.group === 'language' || r.group === 'mixed');
  const t = total(rows);
  assert.equal(t.wrong, 0, 'a wrong slide change on a live screen is the failure to avoid');
  assert.ok(t.got / t.need >= 0.97, `followed ${t.got}/${t.need}`);
  for (const r of rows.filter((x) => x.degrade !== 'all')) assert.equal(r.got, r.need, `${r.name} ${r.degrade} seed ${r.seed}: ${r.got}/${r.need}`);
});

test('Yoruba, Igbo, Hausa and Swahili are followed in full (an a-z-only matcher loses most of their letters)', () => {
  const rows = suiteRows('default', lib).filter((r) => r.group === 'language' && ['Yoruba', 'Igbo', 'Hausa', 'Swahili'].includes(r.name) && r.degrade === 'clean');
  const t = total(rows);
  assert.equal(t.got, t.need);
});

test('a chorus sung three times out of the deck order is found again', () => {
  const rows = suiteRows('noant', noAnt).filter((r) => r.group === 'chorus x3' && r.degrade === 'clean');
  assert.ok(rows.length >= 12);
  for (const r of rows) {
    assert.equal(r.wrong, 0);
    assert.equal(r.got, r.need, `${r.name} seed ${r.seed}: ${r.got}/${r.need}`);
  }
  // Anticipation guesses the deck's next slide; when the arrangement goes back to the
  // chorus instead, that guess shows a wrong slide for a moment (see the notes in
  // lyrics_follow.js) and the dwell floor slows the correction. It is still found.
  const guessed = suiteRows('default', lib).filter((x) => x.group === 'chorus x3' && x.degrade === 'clean');
  const t = total(guessed);
  assert.ok(t.got / t.need >= 0.8, `${t.got}/${t.need}`);
  assert.ok(guessed.every((r) => r.wrong <= 2), 'at most the first guess and one late correction per song');
});

test('a leader repeating one line, or three times, never moves the slide', () => {
  const rows = suiteRows('default', lib).filter((r) => r.group === 'repeat a line');
  assert.ok(rows.length >= 6);
  assert.equal(total(rows).wrong, 0);
  assert.ok(total(rows).got / total(rows).need >= 0.95);
});

test('a skipped verse is caught up to; without anticipation nothing is shown early', () => {
  const rows = suiteRows('noant', noAnt).filter((r) => r.group === 'skip a verse');
  assert.ok(rows.length >= 18);
  const t = total(rows);
  assert.equal(t.wrong, 0);
  assert.ok(t.got / t.need >= 0.95, `${t.got}/${t.need}`);
});

test('after one anticipated flip the singers contradict, anticipation stops for the rest of the song', () => {
  const song = ev.englishChorusSong();          // [Verse 1, Chorus, Verse 2, Verse 3]
  const adv = [];
  const f = new LyricsFollower(song, { config: { anticipate: true }, onAdvance: (e) => adv.push(e) });
  let clock = 1000000;
  f.resync(0, clock);
  const feed = (text) => { text.split(' ').forEach((w, i, a) => { clock += 450; f.ingest(a.slice(Math.max(0, i - 9), i + 1).join(' '), { now: clock }); }); };
  feed(song.blocks[0].lines.join(' '));           // end of verse 1: flips to the chorus
  assert.equal(adv[0].toBlockIdx, 1);
  assert.equal(adv[0].reason, 'anticipate');
  clock += 6000;
  feed(song.blocks[2].lines.join(' '));           // ...but they sing verse 2: caught up, and the guess is dropped
  assert.ok(adv.some((e) => e.toBlockIdx === 2 && e.reason !== 'anticipate'));
  assert.equal(f._noAnticipate, true);
});

test('several songs in one deck: followed across song boundaries, no wrong advance', () => {
  const rows = suiteRows('default', lib).filter((r) => r.group === 'deck');
  assert.ok(rows.length >= 15);
  const t = total(rows);
  assert.ok(t.wrong <= 1, `wrong ${t.wrong}`);            // one late flip under the heaviest degradation, never in clean speech
  assert.equal(total(rows.filter((r) => r.degrade === 'clean')).wrong, 0);
  assert.ok(t.got / t.need >= 0.95, `${t.got}/${t.need}`);
});

test('a repeat jump needs two updates with new words, or one final segment', () => {
  const song = ev.englishChorusSong();
  const chorus = 'Holy holy holy Lord God Almighty early in the morning our song shall rise'.split(' ');
  const run = (final) => {
    const adv = [];
    const f = new LyricsFollower(song, { onAdvance: (e) => adv.push(e) });
    f.resync(2, 1000000);                                  // showing verse 2; the chorus comes back
    const out = [];
    const t0 = 1010000;
    out.push(f.ingest(chorus.slice(0, 10).join(' '), { now: t0, isFinal: final }) || f.blockIdx);
    out.push(f.ingest(chorus.slice(0, 10).join(' '), { now: t0 + 100 }) || f.blockIdx);   // nothing new
    out.push(f.ingest(chorus.slice(0, 12).join(' '), { now: t0 + 600 }) || f.blockIdx);   // two more words
    return { blocks: out, adv };
  };
  const slow = run(false);
  assert.equal(slow.blocks[0], 2, 'one update is not enough');
  assert.equal(slow.blocks[1], 2, 'the same words again are not a second look');
  assert.equal(slow.blocks[2], 1, 'two more words confirm it');
  assert.equal(slow.adv[0].reason, 'repeat');
  const fast = run(true);
  assert.equal(fast.blocks[0], 1, 'a final segment is enough on its own');
});

test('the line just sung does not pull the slide back after a flip (stale words)', () => {
  const song = ev.englishChorusSong();
  const adv = [];
  const f = new LyricsFollower(song, { onAdvance: (e) => adv.push(e) });
  let clock = 1000000;
  f.resync(0, clock);
  const words = song.blocks[0].lines.join(' ').split(' ').concat(song.blocks[1].lines.join(' ').split(' '));
  words.forEach((w, i, a) => { clock += 450; f.ingest(a.slice(Math.max(0, i - 9), i + 1).join(' '), { now: clock }); });
  assert.deepEqual(adv.map((e) => e.toBlockIdx), [1, 2].slice(0, adv.length));
  assert.ok(!adv.some((e) => e.reason === 'repeat'));
});

test('noise does not move anything', () => {
  const song = ev.englishChorusSong();
  const adv = [];
  const f = new LyricsFollower(song, { onAdvance: (e) => adv.push(e) });
  f.resync(1, 1000000);
  const noise = 'so today we are going to talk about the importance of giving and how the church has grown over the years and I want to thank everyone for coming this morning'.split(' ');
  let clock = 1000000;
  noise.forEach((w, i, a) => { clock += 400; f.ingest(a.slice(Math.max(0, i - 9), i + 1).join(' '), { now: clock }); });
  assert.equal(adv.length, 0);
});

test('Chinese lines follow by character (public-domain Union Version)', () => {
  const song = { blocks: [
    { label: 'A', lines: ['耶和华是我的牧者', '我必不致缺乏', '他使我躺卧在青草地上', '领我在可安歇的水边'] },
    { label: 'B', lines: ['他使我的灵魂苏醒', '为自己的名引导我走义路'] },
  ] };
  const adv = [];
  const f = new LyricsFollower(song, { onAdvance: (e) => adv.push(e) });
  let clock = 1000000;
  f.resync(0, clock);
  const chars = Array.from(song.blocks[0].lines.join('') + song.blocks[1].lines.join(''));
  chars.forEach((c, i, a) => {
    clock += 250;
    // a speech engine drops about one character in five
    if (i % 5 === 3) return;
    f.ingest(a.slice(Math.max(0, i - 14), i + 1).filter((_, j) => (i - 14 + j) % 5 !== 3).join(' '), { now: clock });
  });
  assert.deepEqual(adv.map((e) => e.toBlockIdx), [1]);
});

test('Cyrillic and Arabic text is tracked, not discarded', () => {
  for (const lines of [
    ['Господь мой пастырь', 'я ни в чём не буду нуждаться', 'Он покоит меня на злачных пажитях', 'водит меня к водам тихим'],
    ['الرب راعي', 'فلا يعوزني شيء', 'في مراع خضر يربضني', 'إلى مياه الراحة يوردني'],
  ]) {
    const song = { blocks: [{ label: 'A', lines: lines.slice(0, 2) }, { label: 'B', lines: lines.slice(2) }] };
    const adv = [];
    const f = new LyricsFollower(song, { onAdvance: (e) => adv.push(e) });
    let clock = 1000000;
    f.resync(0, clock);
    const words = lines.join(' ').split(' ');
    words.forEach((w, i, a) => { clock += 450; f.ingest(a.slice(Math.max(0, i - 9), i + 1).join(' '), { now: clock }); });
    assert.deepEqual(adv.map((e) => e.toBlockIdx), [1]);
  }
});

test('the helper modules that reuse key/keyMatch/tokenizeKeys still get what they expect', () => {
  assert.equal(typeof lib.key, 'function');
  assert.equal(typeof lib.keyMatch, 'function');
  assert.equal(typeof lib.tokenizeKeys, 'function');
  assert.equal(typeof lib.tokenizeKeysWithTimes, 'function');
  assert.deepEqual(tokenizeKeys('Thee, O Lord, singing!'), ['the', 'oh', 'lord', 'singin']);
  assert.deepEqual(lib.tokenizeKeysWithTimes([{ word: 'Thou', start: 0, end: 1 }, { word: '--', start: 1, end: 2 }]), [{ key: 'the', start: 0, end: 1 }]);
});
