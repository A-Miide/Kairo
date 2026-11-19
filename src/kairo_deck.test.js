'use strict';
const test = require('node:test');
const assert = require('node:assert');
const D = require('./kairo_deck.js');

const item = {
  id: 'slides-abc', libraryId: 'lib-1', type: 'slides', title: 'Week 3',
  blocks: [
    { label: 'Title', layers: [
      { id: 'bg', type: 'image', src: '/api/media/bin/file/church%20logo.png' },
      { id: 'grain', type: 'image', src: 'backgrounds/grain.svg' },
      { id: 'again', type: 'image', src: 'http://127.0.0.1:7788/api/media/bin/file/church%20logo.png?token=secret' },
    ] },
    { label: 'Point', image: '/api/pp-assets/x/photo.jpg', text: 'It is mentality that defines destiny' },
  ],
  sermon: { outline: { items: [{ kind: 'point', n: 1, text: 'Prov 23:7' }] } },
};
const files = {
  '/api/media/bin/file/church%20logo.png': { type: 'image/png', dataBase64: 'UE5H' },
  '/api/pp-assets/x/photo.jpg': { type: 'image/jpeg', dataBase64: 'SlBH' },
};

test('the local pictures a deck uses are found once each, however their address was written', () => {
  assert.deepEqual(D.mediaAddresses(item), ['/api/media/bin/file/church%20logo.png', '/api/pp-assets/x/photo.jpg']);
});

test('a packed deck keeps every slide, the outline and the pictures, without ids or tokens', () => {
  const text = D.pack(item, files, new Date('2026-11-18T12:00:00Z'));
  assert.doesNotMatch(text, /secret/);
  const d = JSON.parse(text);
  assert.equal(d.kairoDeck, true);
  assert.equal(d.formatVersion, D.FORMAT_VERSION);
  assert.equal(d.exportedAt, '2026-11-18T12:00:00.000Z');
  assert.equal(d.item.id, undefined);
  assert.equal(d.item.libraryId, undefined);
  assert.equal(d.item.blocks[0].layers[0].src, 'kairo-media:0');
  assert.equal(d.item.blocks[0].layers[2].src, 'kairo-media:0');
  assert.equal(d.item.blocks[0].layers[1].src, 'backgrounds/grain.svg', 'a built-in picture is left as it is');
  assert.equal(d.item.blocks[1].image, 'kairo-media:1');
  assert.deepEqual(d.media, [{ name: 'church logo.png', type: 'image/png', dataBase64: 'UE5H' }, { name: 'photo.jpg', type: 'image/jpeg', dataBase64: 'SlBH' }]);
  assert.equal(d.item.sermon.outline.items[0].text, 'Prov 23:7');
});

test('a picture that could not be read keeps its address', () => {
  const d = JSON.parse(D.pack(item, { '/api/pp-assets/x/photo.jpg': files['/api/pp-assets/x/photo.jpg'] }));
  assert.equal(d.item.blocks[0].layers[0].src, '/api/media/bin/file/church%20logo.png');
  assert.equal(d.item.blocks[1].image, 'kairo-media:0');
  assert.equal(d.media.length, 1);
});

test('opening a deck puts each picture back at its new address', () => {
  const { item: opened, media } = D.unpack(D.pack(item, files));
  assert.equal(media.length, 2);
  const linked = D.relink(opened, ['/api/media/bin/file/church%20logo-2.png', '/api/media/bin/file/photo.jpg']);
  assert.equal(linked.blocks[0].layers[0].src, '/api/media/bin/file/church%20logo-2.png');
  assert.equal(linked.blocks[1].image, '/api/media/bin/file/photo.jpg');
  assert.equal(D.relink(opened, []).blocks[1].image, '', 'a picture that could not be stored is left empty');
});

test('a file that is not a Kairo deck, or is from a newer Kairo, is refused with a reason', () => {
  assert.throws(() => D.unpack('not json'), /isn’t a Kairo deck/);
  assert.throws(() => D.unpack(JSON.stringify({ kairoTheme: true, theme: {} })), /isn’t a Kairo deck/);
  assert.throws(() => D.unpack(JSON.stringify({ kairoDeck: true, formatVersion: 99, item: { type: 'slides', blocks: [] } })), /newer Kairo/);
  assert.throws(() => D.unpack(JSON.stringify({ kairoDeck: true, formatVersion: 1, item: { type: 'other' } })), /no slides/);
});
