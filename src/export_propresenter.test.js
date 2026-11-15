'use strict';
const test = require('node:test');
const assert = require('node:assert');
const P = require('./export_pptx.js');
global.KairoPptx = P;
const R = require('./export_propresenter.js');

// A small protobuf reader: [{ n, w, v }] where v is a number, or bytes for length-delimited fields.
function fields(b) {
  const out = [];
  let i = 0;
  const vi = () => { let r = 0, m = 1, c; do { c = b[i++]; r += (c & 127) * m; m *= 128; } while (c & 128); return r; };
  while (i < b.length) {
    const k = vi(), n = Math.floor(k / 8), w = k % 8;
    if (!n) throw new Error('bad tag');
    if (w === 0) out.push({ n, w, v: vi() });
    else if (w === 1) { out.push({ n, w, v: new DataView(b.buffer, b.byteOffset + i, 8).getFloat64(0, true) }); i += 8; }
    else if (w === 5) { out.push({ n, w, v: new DataView(b.buffer, b.byteOffset + i, 4).getFloat32(0, true) }); i += 4; }
    else if (w === 2) { const l = vi(); out.push({ n, w, v: b.subarray(i, i + l) }); i += l; }
    else throw new Error('bad wire');
  }
  return out;
}
const all = (f, n) => f.filter(x => x.n === n);
const one = (f, n) => f.find(x => x.n === n);
const sub = (f, n) => { const x = one(f, n); return x ? fields(x.v) : null; };
const str = (f, n) => { const x = one(f, n); return x ? Buffer.from(x.v).toString('utf8') : null; };

const layers = [
  { id: 'bg', type: 'background', fill: 'gradient', color: '#ff0000', color2: '#0000ff', angle: 180, opacity: 100 },
  { id: 'v', type: 'text', binding: 'verse', font: { family: 'Georgia', size: 80, weight: 700, lineHeight: 1.2, transform: 'uppercase' }, color: '#ffffff', align: 'center', pos: { x: 100, y: 200, w: 1700, h: 500 }, accentColor: '#ffcc00' },
  { id: 'p', type: 'image', src: '/media/x.png', pos: { x: 0, y: 0, w: 100, h: 100 } },
];
const build = (texts) => R.buildPro(texts.map(t => P.itemsFromLayers(layers, {}, { verseText: t, referenceText: '' })).map(s => ({ items: s.items })), { title: 'My deck' });

test('a presentation has the header, a name, one group and one cue per slide', () => {
  const { bytes } = build(['First *word*', 'Second']);
  const root = fields(bytes);
  assert.equal(str(root, 3), 'My deck');
  assert.match(str(sub(root, 2), 1), /^[0-9A-F]{8}(-[0-9A-F]{4}){3}-[0-9A-F]{12}$/);
  const group = sub(root, 12);
  const cueIds = all(group, 2).map(c => str(fields(c.v), 1));
  const cues = all(root, 13);
  assert.equal(cues.length, 2);
  assert.deepEqual(cues.map(c => str(sub(fields(c.v), 1), 1)), cueIds);
});

test('each slide holds its elements top first, with a 1920 x 1080 size and a black canvas', () => {
  const { bytes } = build(['Hello']);
  const cue = fields(all(fields(bytes), 13)[0].v);
  const base = sub(sub(sub(sub(cue, 10), 23), 2), 1);
  const els = all(base, 1);
  assert.equal(els.length, 2);   // the gradient and the words (the picture is left out)
  const size = sub(base, 6);
  assert.deepEqual([one(size, 1).v, one(size, 2).v], [1920, 1080]);
  const top = sub(sub(fields(els[0].v), 1), 13);
  assert.ok(top, 'the words come first (they are on top)');
});

test('a gradient keeps its colours and ProPresenter\'s angle (counter-clockwise from right)', () => {
  const { bytes } = build(['x']);
  const cue = fields(all(fields(bytes), 13)[0].v);
  const els = all(sub(sub(sub(sub(cue, 10), 23), 2), 1), 1);
  const bg = sub(fields(els[1].v), 1);
  const fill = sub(bg, 9);
  assert.equal(one(fill, 4).v, 1);
  const g = sub(fill, 2);
  assert.equal(one(g, 2).v, 90 - 180 + 360);       // CSS 180° (down) = 270° here
  assert.equal(all(g, 4).length, 2);
});

test('the words are RTF with the font, size, colours, case and an accent run', () => {
  const { bytes, notes } = build(['For *God* so loved']);
  assert.ok(notes.includes('Pictures that couldn’t be loaded'));   // no picture bytes given
  const cue = fields(all(fields(bytes), 13)[0].v);
  const els = all(sub(sub(sub(sub(cue, 10), 23), 2), 1), 1);
  const text = sub(sub(fields(els[0].v), 1), 13);
  const rtf = str(text, 5);
  assert.match(rtf, /^\{\\rtf1\\ansi/);
  assert.match(rtf, /\\fonttbl\\f0\\fswiss\\fcharset0 Georgia-Bold;/);
  assert.match(rtf, /\\fs160 /);
  assert.match(rtf, /\\red255\\green255\\blue255;\\red255\\green204\\blue0;/);
  assert.match(rtf, /\\cf1 For \\cf2 God\\cf1  so loved\}$/);
  const attrs = sub(text, 3);
  assert.equal(one(attrs, 2).v, 1);                  // all capitals
  assert.equal(one(sub(attrs, 1), 2).v, 80);
});

test('non-Latin and special characters are escaped in the RTF', () => {
  const rtf = R.buildRtf({ text: 'a{b}\\ “q” \n2', font: { size: 40, weight: 400, lineHeight: 1 }, align: 'left' }, 'Arial', { hex: 'FFFFFF', a: 1 }, null);
  assert.match(rtf, /a\\\{b\\\}\\\\ /);
  assert.match(rtf, /\\u8220 q\\uc0\\u8221 /);
  assert.match(rtf, /\\\n2\}$/);
});

test('a shape carries an empty text block, as ProPresenter writes one (no "Text" placeholder)', () => {
  const { bytes } = build(['x']);
  const cue = fields(all(fields(bytes), 13)[0].v);
  const els = all(sub(sub(sub(sub(cue, 10), 23), 2), 1), 1);
  const bg = sub(fields(els[1].v), 1);
  const text = sub(bg, 13);
  assert.ok(text, 'the background shape has a text block');
  assert.match(str(text, 5), /^\{\\rtf1/);
  assert.doesNotMatch(str(text, 5), /\\cf1/, 'with no words in it');
});

test('a glow becomes a radial gradient fading to clear, not a dropped picture', () => {
  const items = [{ kind: 'image', x: 100, y: 50, w: 800, h: 600, fit: 'fill', src: 'glow:6B2313:80' }];
  const { bytes, notes } = R.buildPro([{ items }], { title: 't' });
  assert.ok(!notes.some(n => /Picture/.test(n)));
  const cue = fields(all(fields(bytes), 13)[0].v);
  const el = sub(fields(all(sub(sub(sub(sub(cue, 10), 23), 2), 1), 1)[0].v), 1);
  const g = sub(sub(el, 9), 2);
  assert.equal(one(g, 1).v, 1);                       // radial
  const stops = all(g, 4).map(x => fields(x.v));
  const alpha = (st) => one(sub(st, 1), 4).v;
  assert.ok(Math.abs(alpha(stops[0]) - 0.8) < 1e-6);
  assert.equal(alpha(stops[1]), 0);                  // clear at the edge
});

test('a picture becomes a media fill, and its file is handed back to pack in a .probundle', () => {
  const png = Uint8Array.from([137, 80, 78, 71]);
  const items = [{ kind: 'image', x: 0, y: 0, w: 1920, h: 1080, fit: 'cover', src: '/media/bg.png' }];
  const out = R.buildPro([{ items }], { title: 't', images: { '/media/bg.png': { bytes: png, ext: 'png', w: 1920, h: 1080 } }, mediaDir: '/Users/x/Documents/ProPresenter/Media/Assets' });
  assert.equal(out.media.length, 1);
  assert.match(out.media[0].rel, /^Media\/Assets\/[0-9A-F-]+\.png$/);
  const cue = fields(all(fields(out.bytes), 13)[0].v);
  const el = sub(fields(all(sub(sub(sub(sub(cue, 10), 23), 2), 1), 1)[0].v), 1);
  const m = sub(sub(el, 9), 3);
  const url = sub(m, 2);
  assert.equal(str(url, 1), 'file:///Users/x/Documents/ProPresenter/Media/Assets/' + out.media[0].rel.split('/').pop());
  assert.equal(str(sub(url, 4), 2), out.media[0].rel);
  assert.equal(one(sub(sub(m, 5), 1), 1).v, 1);       // cover = ProPresenter "fill"
});
