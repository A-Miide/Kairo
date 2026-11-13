// Tests for src/export_pptx.js: plain node:test, no DOM.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const P = require('./export_pptx.js');
const S = require('./sermon_slides.js');

// Reads a stored zip back: its entries by name, each checked against its recorded CRC.
function unzip(bytes) {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let end = bytes.length - 22;
  while (end > 0 && dv.getUint32(end, true) !== 0x06054b50) end--;
  const count = dv.getUint16(end + 10, true);
  let at = dv.getUint32(end + 16, true);
  const out = {};
  for (let i = 0; i < count; i++) {
    assert.equal(dv.getUint32(at, true), 0x02014b50);
    const crc = dv.getUint32(at + 16, true), size = dv.getUint32(at + 24, true), nameLen = dv.getUint16(at + 28, true);
    const local = dv.getUint32(at + 42, true);
    const name = Buffer.from(bytes.subarray(at + 46, at + 46 + nameLen)).toString('utf8');
    const start = local + 30 + dv.getUint16(local + 26, true) + dv.getUint16(local + 28, true);
    const data = bytes.subarray(start, start + size);
    assert.equal(P.crc32(data), crc, `${name} checks out`);
    out[name] = Buffer.from(data);
    at += 46 + nameLen + dv.getUint16(at + 30, true) + dv.getUint16(at + 32, true);
  }
  return out;
}
const text = (files, name) => files[name].toString('utf8');
const solid = (hex, a = 1) => ({ kind: 'solid', color: { hex, a } });
const shape = (o) => ({ kind: 'shape', x: 0, y: 0, w: 400, h: 200, fill: solid('FF0000'), ...o });

test('the file is a valid zip with the content types first, and every reference points at a part', () => {
  const bytes = P.buildPptx([{ items: [shape({})] }, { items: [shape({})] }], {}, { title: 'Deck' });
  const files = unzip(bytes);
  assert.equal(Object.keys(files)[0], '[Content_Types].xml');
  const names = new Set(Object.keys(files));
  for (const n of names) {
    if (!n.endsWith('.rels')) continue;
    const dir = n.replace('_rels/', '').replace(/\.rels$/, '').split('/').slice(0, -1);
    const ids = [...text(files, n).matchAll(/Id="([^"]+)"[^>]*Target="([^"]+)"/g)];
    assert.equal(new Set(ids.map(m => m[1])).size, ids.length, `${n} has unique ids`);
    for (const [, , target] of ids) {
      const parts = [...dir];
      for (const seg of target.split('/')) { if (seg === '..') parts.pop(); else parts.push(seg); }
      assert.ok(names.has(parts.join('/')), `${n} → ${target}`);
    }
  }
  // two slides, each reachable from the presentation by its own id, and none reused for the theme
  const pres = text(files, 'ppt/presentation.xml'), prels = text(files, 'ppt/_rels/presentation.xml.rels');
  const slideIds = [...pres.matchAll(/<p:sldId [^>]*r:id="([^"]+)"/g)].map(m => m[1]);
  assert.equal(slideIds.length, 2);
  for (const id of slideIds) assert.match(prels, new RegExp(`Id="${id}"[^>]*slides/slide\\d\\.xml`));
  assert.match(prels, /theme\/theme1\.xml/);
  assert.equal(new Set([...slideIds, 'rId1']).size, 3);
  assert.match(text(files, '[Content_Types].xml'), /slide2\.xml/);
});

test('colours: shorthand, alpha, and rgba all read; a fill, a gradient and a shadow are written', () => {
  assert.deepEqual(P.parseColor('#f00'), { hex: 'FF0000', a: 1 });
  assert.deepEqual(P.parseColor('#11223380', 50), { hex: '112233', a: 0.5 * (128 / 255) });
  assert.deepEqual(P.parseColor('rgba(10, 20, 30, 0.5)'), { hex: '0A141E', a: 0.5 });
  assert.equal(P.parseColor('transparent'), null);
  const files = unzip(P.buildPptx([{ items: [
    shape({ fill: { kind: 'gradient', stops: [{ pos: 0, color: '#000000' }, { pos: 100, color: '#FFFFFF' }], angle: 180 } }),
    shape({ fill: solid('00FF00', 0.5), drop: { enabled: true, x: 0, y: 10, blur: 20, color: '#000000', opacity: 50 } }),
  ] }], {}));
  const xml = text(files, 'ppt/slides/slide1.xml');
  assert.match(xml, /<a:lin ang="5400000"/);                       // CSS "to bottom" is PowerPoint's 90 degrees
  assert.match(xml, /<a:alpha val="50000"\/>/);
  assert.match(xml, /<a:outerShdw blurRad="127000" dist="63500" dir="5400000"/);
});

test('text: size, spacing, line height, case, weight, alignment and an accent word', () => {
  const layers = [{ id: 't', type: 'text', visible: true, binding: 'custom', customText: 'Hello *world*\nSecond line', pos: { x: 100, y: 100, w: 800, h: 300 },
    font: { family: 'Playfair Display', size: 100, weight: 700, italic: true, lineHeight: 1.2, letterSpacing: 4, transform: 'uppercase' }, color: '#FFFFFF', opacity: 100, align: 'center', accentColor: '#FF8800' }];
  const { items, fonts } = P.itemsFromLayers(layers, {}, {}, { fillKeys: [], layerKeys: [] });
  assert.deepEqual(fonts, ['Playfair Display']);
  const xml = text(unzip(P.buildPptx([{ items }], {})), 'ppt/slides/slide1.xml');
  assert.match(xml, /sz="5000"/);                                   // 100 canvas px = 50 pt
  assert.match(xml, /spc="200"/);
  assert.match(xml, /cap="all"/);
  assert.match(xml, /b="1" i="1"/);
  assert.match(xml, /algn="ctr"/);
  assert.match(xml, /<a:spcPts val="6000"\/>/);                     // 1.2 x 100 px = 60 pt
  assert.match(xml, /typeface="Playfair Display"/);
  assert.match(xml, /srgbClr val="FF8800"/);
  assert.equal((xml.match(/<a:p>/g) || []).length, 2);
  assert.ok(!xml.includes('*world*'), 'the asterisks are markup, not words');
});

test('faint text is mixed into the background colour, so it is faint in any viewer', () => {
  const layers = [
    { id: 'bg', type: 'background', visible: true, fill: 'solid', color: '#000000', opacity: 100, pos: { x: 0, y: 0, w: 1920, h: 1080 } },
    { id: 'n', type: 'text', visible: true, binding: 'custom', customText: '01', pos: { x: 0, y: 0, w: 900, h: 600 },
      font: { family: 'Inter', size: 500, weight: 800, lineHeight: 1, letterSpacing: 0, transform: 'none' }, color: '#FFFFFF', opacity: 10, align: 'right' },
  ];
  const { items } = P.itemsFromLayers(layers, {}, {}, { fillKeys: [], layerKeys: [] });
  const t = items.find(i => i.kind === 'text');
  assert.deepEqual(t.fill.color, { hex: '1A1A1A', a: 1 });
});

test('what a slide asks for per slide wins: hidden layers, moved boxes, other colours', () => {
  const layers = [
    { id: 'a', type: 'text', visible: true, binding: 'verse', pos: { x: 0, y: 0, w: 500, h: 100 }, font: { family: 'Inter', size: 40, weight: 400, lineHeight: 1.2, letterSpacing: 0, transform: 'none' }, color: '#FFFFFF', opacity: 100, align: 'left' },
    { id: 'b', type: 'text', visible: true, binding: 'reference', pos: { x: 0, y: 200, w: 500, h: 50 }, font: { family: 'Inter', size: 30, weight: 400, lineHeight: 1.2, letterSpacing: 0, transform: 'none' }, color: '#FFFFFF', opacity: 100, align: 'left' },
  ];
  const content = { verseText: 'For God so loved', referenceText: 'John 3:16' };
  const plain = P.itemsFromLayers(layers, {}, content, { fillKeys: [], layerKeys: [] });
  assert.deepEqual(plain.items.map(i => i.text), ['For God so loved', 'John 3:16']);
  const own = P.itemsFromLayers(layers, { a: { pos: { x: 50, y: 60, w: 700, h: 120 }, color: '#00FF00' }, b: { visible: false } }, content, { fillKeys: [], layerKeys: [] });
  assert.equal(own.items.length, 1);
  assert.equal(own.items[0].x, 50);
  assert.equal(own.items[0].fill.color.hex, '00FF00');
  const empty = P.itemsFromLayers(layers, {}, { verseText: '', referenceText: 'John 3:16' }, { fillKeys: [], layerKeys: [] });
  assert.equal(empty.items.length, 0, 'a reference with no verse to caption is left out, as on the output');
});

test('shapes: rounded, each corner, cut-out, ellipse, stroke; glass and blends are reported', () => {
  const bg = (o) => ({ id: 's', type: 'background', visible: true, fill: 'solid', color: '#336699', opacity: 100, pos: { x: 10, y: 20, w: 300, h: 200 }, ...o });
  const run = (o) => text(unzip(P.buildPptx([{ items: P.itemsFromLayers([bg(o)], {}, {}, { fillKeys: [], layerKeys: [] }).items }], {})), 'ppt/slides/slide1.xml');
  assert.match(run({ radius: 20 }), /prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 10000"\/>/);
  assert.match(run({ shape: 'ellipse' }), /prst="ellipse"/);
  assert.match(run({ corners: [40, 0, 40, 0] }), /<a:custGeom>.*<a:arcTo wR="254000"/s);
  assert.match(run({ clip: [[0, 0], [100, 10], [90, 100]] }), /<a:custGeom>.*<a:lnTo>.*<a:close\/>/s);
  assert.match(run({ stroke: { enabled: true, width: 4, color: '#FFFFFF' } }), /<a:ln w="25400">/);
  const glass = P.itemsFromLayers([bg({ glass: true })], {}, {}, { fillKeys: [], layerKeys: [] });
  assert.ok(glass.notes.some(n => /glass/i.test(n)));
  const grain = P.itemsFromLayers([{ id: 'g', type: 'image', visible: true, src: 'backgrounds/grain.svg', fit: 'cover', opacity: 100, blendMode: 'Overlay', pos: { x: 0, y: 0, w: 1920, h: 1080 } }], {}, {}, { fillKeys: [], layerKeys: [] });
  assert.equal(grain.items.length, 0);
  assert.ok(grain.notes.some(n => /blend/i.test(n)));
});

test('a gradient\'s soft glow becomes one small picture, shared by every slide that has it', () => {
  const layers = [{ id: 'bg', type: 'background', visible: true, fill: 'gradient', color: '#220000', color2: '#220000', opacity: 100, angle: 135, pos: { x: 0, y: 0, w: 1920, h: 1080 },
    gradientStops: [{ pos: 0, color: '#220000' }, { pos: 100, color: '#220000' }], gradientGlows: [{ x: 4, y: 0, rx: 70, ry: 100, color: '#AA3311' }] }];
  const { items } = P.itemsFromLayers(layers, {}, {}, { fillKeys: [], layerKeys: [] });
  const glow = items.find(i => i.kind === 'image');
  assert.match(glow.src, /^glow:AA3311:100$/);
  const files = unzip(P.buildPptx([{ items }, { items }], {}));
  const media = Object.keys(files).filter(n => n.startsWith('ppt/media/'));
  assert.equal(media.length, 1);
  assert.deepEqual([...files[media[0]].subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
  assert.match(text(files, '[Content_Types].xml'), /Extension="png" ContentType="image\/png"/);
  const rels = text(files, 'ppt/slides/_rels/slide2.xml.rels');
  assert.match(rels, /\.\.\/media\/image1\.png/);
});

test('pictures: cover crops to the box, contain shrinks inside it, a missing one is skipped', () => {
  const img = { bytes: Uint8Array.from([1, 2, 3]), ext: 'jpeg', w: 2000, h: 1000 };
  const slide = (fit) => text(unzip(P.buildPptx([{ items: [{ kind: 'image', x: 0, y: 0, w: 1000, h: 1000, src: 'a.jpg', fit }, { kind: 'image', x: 0, y: 0, w: 10, h: 10, src: 'gone.jpg', fit: 'fill' }] }], { 'a.jpg': img })), 'ppt/slides/slide1.xml');
  assert.match(slide('cover'), /<a:srcRect l="25000" r="25000"\/>/);
  const contain = slide('contain');
  assert.match(contain, /cy="3175000"/);                           // 1000 x 500 px, centred in a 1000 px box
  assert.equal((contain.match(/<p:pic>/g) || []).length, 1);
});

test('a real sermon deck becomes slides whose words are all there', () => {
  const outline = S.readText(['Grace Church', 'Theme: Faith', '1. Faith pleases God – Heb. 11:6', '2. Faith comes by hearing – Rom. 10:17', '3. Faith works by love – Gal. 5:6'].join('\n'));
  for (const [kit, layout] of [['ember', 'left'], ['royal', 'worthy'], ['redcard', 'badge'], ['lime', 'statement'], ['midnight', 'card']]) {
    const deck = S.newDeck(outline, { kit, layout });
    const slides = deck.blocks.map(b => ({ items: P.itemsFromLayers(b.layers, {}, {}, { fillKeys: [], layerKeys: [] }).items }));
    const files = unzip(P.buildPptx(slides, {}));
    const all = Object.keys(files).filter(n => /^ppt\/slides\/slide\d+\.xml$/.test(n)).map(n => text(files, n)).join(' ');
    assert.equal(Object.keys(files).filter(n => /^ppt\/slides\/slide\d+\.xml$/.test(n)).length, deck.blocks.length, `${layout}: a slide each`);
    for (const word of ['Faith pleases God', 'Faith comes by hearing', 'Faith works by love'].map(w => w.split(' ')[1])) assert.ok(all.toLowerCase().includes(word.toLowerCase()), `${layout} keeps "${word}"`);
    assert.match(all, /Hebrews 11:6/);
  }
});
