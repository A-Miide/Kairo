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

test('the deck: a title, the story, a slide per point with chips, the section and the closing', () => {
  const deck = S.build(S.readText(PASTED), { kit: 'midnight', style: 'gradient' });
  assert.deepEqual(deck.map(b => b.gen.kind), ['title', 'story', 'point', 'point', 'point', 'section', 'point', 'closing']);
  const p1 = deck[2].gen;
  assert.equal(p1.label, 'Point 1 of 4');
  assert.deepEqual(p1.refs, ['Romans 4:16–21', 'Hebrews 11:17–19']);
  assert.equal(p1.shortened, true);
  assert.equal(deck[2].layers.find(l => l.id === 'ss-headline').customText, p1.lines[0]);
  assert.match(deck[2].notes, /against hope/);
  assert.deepEqual(deck[0].gen.refs, ['Isaiah 8:18', 'John 3:8']);
  // The note is the presenter's, not a slide.
  assert.match(deck[deck.length - 1].presenterNote, /Next Sunday/);

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

test('words changed in the slide list go into the text layers, styling kept', () => {
  const deck = S.build(S.readText(PASTED), { kit: 'ember' });
  const b = deck[2];
  const head = b.layers.find(l => l.id === 'ss-headline');
  head.font.size = 90;          // the operator restyled it
  head.color = '#FFFFFF';
  const edited = S.withText(b, 'Point 1 of 4\nAbraham believed against all hope\nScriptures: Romans 4:18');
  const h2 = edited.layers.find(l => l.id === 'ss-headline');
  assert.equal(h2.customText, 'Abraham believed against all hope');
  assert.equal(h2.font.size, 90);
  assert.equal(h2.color, '#FFFFFF');
  assert.equal(edited.layers.find(l => l.id === 'ss-chip-0-text').customText, 'Romans 4:18');
});

test('every part of a slide is an ordinary, editable layer', () => {
  const deck = S.build(S.readText(PASTED), {});
  for (const b of deck) {
    for (const l of b.layers) {
      assert.ok(l.type === 'text' || l.type === 'background' || l.id === 'ss-grain' || l.id === 'ss-vignette', `${l.name} is ${l.type}`);
      if (l.type === 'text') assert.equal(l.binding, 'custom');
    }
  }
});

test('every look reads: text 4.5:1 and the accent 3:1 on both background tones', () => {
  for (const k of S.KITS) assert.deepEqual(S.kitWarnings(k), [], k.name);
});

test('a change to a shared part is found, and applied to every slide and later decks', () => {
  const item = S.newDeck(S.readText(PASTED), { kit: 'ember' });
  const before = item.blocks[2].layers;
  const after = JSON.parse(JSON.stringify(before));
  after.find(l => l.id === 'ss-footer').customText = 'Grace Chapel · November';
  after.find(l => l.id === 'ss-headline').customText = 'Only this slide';
  const changes = S.commonChanges(before, after);
  assert.deepEqual(changes.map(c => c.id), ['ss-footer']);
  item.blocks[2].layers = after;
  S.applyCommon(item, changes, 2);
  for (const b of item.blocks) {
    const f = b.layers.find(l => l.id === 'ss-footer');
    if (f) assert.equal(f.customText, 'Grace Chapel · November');
  }
  item.sermon.kit = 'graphite';
  const again = S.regenerate(item);
  assert.equal(again[0].layers.find(l => l.id === 'ss-footer')?.customText ?? 'Grace Chapel · November', 'Grace Chapel · November');
  assert.ok(again.some(b => b.layers.find(l => l.id === 'ss-footer')?.customText === 'Grace Chapel · November'));
});

test('the operator’s own colours go on every slide, and a look brings its own back', () => {
  const o = S.readText(PASTED);
  const item = S.newDeck(o, { kit: 'ember', colors: { bg: '#102030', accent: '#33CC88' } });
  const bg = item.blocks[2].layers.find(l => l.id === 'ss-bg');
  assert.ok(JSON.stringify(bg).includes('#102030'));
  assert.equal(item.blocks[2].layers.find(l => l.id === 'ss-label').color, '#33CC88');
  delete item.sermon.colors;
  item.blocks = S.regenerate(item);
  assert.ok(!JSON.stringify(item.blocks[2].layers).includes('#102030'));
});

test('three layouts, the lower third leaving the top of the slide clear', () => {
  const o = S.readText(PASTED);
  for (const l of S.LAYOUTS) assert.ok(S.newDeck(o, { layout: l.id }).blocks.length > 0, l.id);
  const lower = S.newDeck(o, { layout: 'lower' }).blocks.find(b => b.gen.kind === 'point');
  for (const layer of lower.layers) assert.ok(layer.pos.y >= 700, `${layer.name} sits in the band`);
});

test('each design is a look, a layout and a background that exist, and every look and layout is offered', () => {
  const o = S.readText(PASTED);
  for (const d of S.DESIGNS) {
    assert.ok(S.KITS.some(k => k.id === d.kit), d.kit);
    assert.ok(S.LAYOUTS.some(l => l.id === d.layout), d.layout);
    assert.ok(S.BACKGROUNDS.some(b => b.id === d.style), d.style);
    const deck = S.newDeck(o, d).sermon;
    assert.deepEqual([deck.kit, deck.layout, deck.style], [d.kit, d.layout, d.style]);
  }
  for (const k of S.KITS) assert.ok(S.DESIGNS.some(d => d.kit === k.id), `${k.id} has a design`);
  for (const l of S.LAYOUTS) assert.ok(S.DESIGNS.some(d => d.layout === l.id), `${l.id} is used`);
});

test('any part other slides have is shared: a card change goes to later slides only, when asked', () => {
  const item = S.newDeck(S.readText(PASTED), { kit: 'redcard', layout: 'badge' });
  const at = 3;
  const before = item.blocks[at].layers;
  const after = JSON.parse(JSON.stringify(before));
  const card = after.find(l => l.id === 'ss-card');
  card.corners = [10, 10, 10, 10];
  card.stroke = { enabled: true, color: '#FFFFFF', width: 6 };
  after.find(l => l.id === 'ss-headline').customText = 'Only this slide';
  const changes = S.commonChanges(before, after);
  assert.deepEqual(changes.map(c => c.id), ['ss-card'], 'the headline words are each slide\'s own');
  assert.ok(S.sharedWith(item, 'ss-card', at) > 0);
  item.blocks[at].layers = after;
  S.applyCommon(item, changes, at, 'after');
  const cardOf = (i) => item.blocks[i].layers.find(l => l.id === 'ss-card');
  assert.deepEqual(cardOf(at + 1).corners, [10, 10, 10, 10]);
  assert.equal(cardOf(at + 1).stroke.width, 6);
  assert.deepEqual(cardOf(1).corners, [70, 300, 80, 80], 'earlier slides keep theirs');
  assert.ok(!item.sermon.common || !item.sermon.common['ss-card'], '"later" is not the deck\'s own style');
});

test('a deck change asks to restyle hand-edited slides; when it does, their words stay', () => {
  const item = S.newDeck(S.readText(PASTED), { kit: 'ember', layout: 'left' });
  const b = item.blocks[2];
  const edited = S.withText(b, 'Point 1 of 4\nAbraham believed against all hope\nScriptures: Romans 4:18');
  edited.gen.edited = true;
  edited.layers.find(l => l.id === 'ss-headline').font.size = 90;
  item.blocks[2] = edited;
  item.sermon.layout = 'centred';
  const kept = S.regenerate(item);
  assert.equal(kept[2].layers.find(l => l.id === 'ss-headline').font.size, 90, 'kept as it was');
  const restyled = S.regenerate(item, { restyleEdited: true });
  const h = restyled[2].layers.find(l => l.id === 'ss-headline');
  assert.equal(h.customText, 'Abraham believed against all hope', 'its words stay');
  assert.equal(h.align, 'center', 'in the new layout');
});

// Layouts other than the one the pasted outline above uses: every one is read as an outline.
const LAYOUTS_READ = {
  'typed 1)': ['Grace Church', 'Theme: Faith', '1) Faith pleases God – Heb. 11:6', '2) Faith comes by hearing – Rom. 10:17', '3) Faith works by love – Gal. 5:6'],
  bullets: ['Grace Church', 'Theme: Faith', '• Faith pleases God – Heb. 11:6', '• Faith comes by hearing – Rom. 10:17', '• Faith works by love – Gal. 5:6'],
  dashes: ['Grace Church', 'Theme: Faith', '- Faith pleases God (Heb. 11:6)', '- Faith comes by hearing (Rom. 10:17)', '- Faith works by love (Gal. 5:6)'],
  roman: ['Grace Church', 'Theme: Faith', 'I. Faith pleases God – Heb. 11:6', 'II. Faith comes by hearing – Rom. 10:17', 'III. Faith works by love – Gal. 5:6'],
  letters: ['Grace Church', 'Theme: Faith', 'A. Faith pleases God – Heb. 11:6', 'B. Faith comes by hearing – Rom. 10:17', 'C. Faith works by love – Gal. 5:6'],
  'Point N:': ['Grace Church', 'Theme: Faith', 'Point 1: Faith pleases God – Heb. 11:6', 'Point 2: Faith comes by hearing – Rom. 10:17', 'Point 3: Faith works by love – Gal. 5:6'],
  'brackets': ['Grace Church', 'Theme: Faith', '1. Faith pleases God [Heb 11:6]', '2. Faith comes by hearing [Rom 10:17]', '3. Faith works by love [Gal 5:6]'],
  'scripture on the next line': ['Grace Church', 'Theme: Faith', '1. Faith pleases God', 'Heb. 11:6', '2. Faith comes by hearing', 'Rom. 10:17', '3. Faith works by love', 'Gal. 5:6'],
  'full book names': ['Grace Church', 'Theme: Faith', '1. Faith pleases God – Hebrews 11:6', '2. Faith comes by hearing – Romans 10:17', '3. Faith works by love – Galatians 5:6'],
  'two points': ['Grace Church', 'Theme: Faith', '1. Faith pleases God – Heb. 11:6', '2. Faith comes by hearing – Rom. 10:17'],
};
for (const [name, lines] of Object.entries(LAYOUTS_READ)) {
  test(`a sermon laid out with ${name} is read`, () => {
    const o = S.readText(lines.join('\n'));
    assert.ok(o, 'read');
    assert.equal(o.how.mode, 'points');
    const pts = o.items.filter(i => i.kind === 'point');
    assert.equal(pts.length, lines.filter(l => /faith (pleases|comes|works)/i.test(l) && !/^(Grace|Theme)/.test(l)).length);
    assert.equal(pts[0].text, 'Faith pleases God');
    assert.ok(pts.every(p => p.refs.length === 1), 'each point has its scripture');
    assert.ok(S.newDeck(o).blocks.length >= pts.length);
  });
}

test('points without any scripture are still points, and sub-points are not slides', () => {
  const none = S.readText(['Grace', 'Theme: X', '1. Faith pleases God', '2. Faith comes by hearing', '3. Faith works by love'].join('\n'));
  assert.equal(none.items.filter(i => i.kind === 'point').length, 3);
  const sub = S.readText(['Grace', 'Theme: X', '1. Faith pleases God – Heb. 11:6', 'a. without it impossible', 'b. believe He is', '2. Faith comes – Rom. 10:17', '3. Faith works – Gal. 5:6'].join('\n'));
  assert.equal(sub.items.filter(i => i.kind === 'point').length, 3);
  assert.ok(sub.other.includes('without it impossible'), 'a sub-point is offered, not made a slide');
});

test('headings with paragraphs under them: each heading a point, its scriptures found underneath', () => {
  const o = S.readText(['THE POWER OF FAITH', 'Pastor John', 'FAITH PLEASES GOD', 'Without faith it is impossible to please Him. Hebrews 11:6', 'FAITH COMES BY HEARING', 'So then faith comes by hearing. Romans 10:17', 'FAITH WORKS BY LOVE', 'For in Christ neither circumcision avails anything. Galatians 5:6'].join('\n'));
  assert.equal(o.how.mode, 'headings');
  assert.deepEqual(o.items.map(i => i.text), ['FAITH PLEASES GOD', 'FAITH COMES BY HEARING', 'FAITH WORKS BY LOVE']);
  assert.deepEqual(o.items.map(i => i.refs[0]), ['Hebrews 11:6', 'Romans 10:17', 'Galatians 5:6']);
  assert.equal(o.header[0].text, 'THE POWER OF FAITH');
  const deck = S.newDeck(o).blocks;
  assert.equal(deck[0].gen.kind, 'title');
  assert.ok(JSON.stringify(deck[0].layers).includes('THE POWER OF FAITH'), 'the first line is the title');
  assert.equal(deck[0].gen.footer, 'Pastor John');
});

test('prose: read as paragraphs when it names scriptures, never for lyrics or notices', () => {
  const prose = Array(8).fill('Faith is the substance of things hoped for, the evidence of things not seen, and Abraham believed God and it was counted to him as righteousness, as Romans 4:3 says.').join('\n');
  const o = S.readText(prose);
  assert.equal(o.how.mode, 'paragraphs');
  assert.ok(o.items.every(i => i.kind === 'story' && i.refs.includes('Romans 4:3')));
  assert.equal(S.readText('Amazing grace, how sweet the sound\nThat saved a wretch like me\nI once was lost, but now am found\nWas blind, but now I see'), null);
  assert.equal(S.readText('Welcome everyone\nOffering is at 10\nYouth meets Friday'), null);
});

test('a scripture after a dash keeps its stray dash off, and an unknown short book still splits the line', () => {
  const o = S.readText(['Grace', 'Theme: Isaiah 8:18 –', '1. A – Phx. 3:14', '2. B – Rom. 1:1', '3. C – Rom. 1:2'].join('\n'));
  assert.equal(o.items.filter(i => i.kind === 'point')[0].text, 'A');
  assert.equal(S.normaliseRef('Isa. 8:18 –'), 'Isaiah 8:18');
  assert.equal(S.normaliseRef('Phi. 3:14'), 'Philippians 3:14');
});

test('a line the operator re-labels becomes a point, a section or an introduction, and points renumber', () => {
  const item = S.newDeck(S.readText(PASTED), { kit: 'ember' });
  const sect = S.partsOf(item.sermon.outline).find(p => p.kind === 'section');
  item.sermon.include = [...new Set([...item.sermon.include, sect.id])];
  const before = S.regenerate(item).filter(b => b.gen.kind === 'point').length;
  item.sermon.kinds = { [sect.id]: 'point' };
  const after = S.regenerate(item);
  assert.equal(after.filter(b => b.gen.kind === 'point').length, before + 1);
  assert.ok(after.every(b => b.gen.kind !== 'point' || /^Point \d+ of \d+$/.test(b.gen.label)));
  item.sermon.kinds = { [sect.id]: 'story' };
  assert.equal(S.regenerate(item).find(b => b.gen.part === sect.id).gen.kind, 'story');
});
