// KAIRO — a deck as Kairo's own file (.kairo).
//
// The item exactly as Kairo keeps it (every slide's layers, the sermon outline it came from, its theme), with
// the pictures it uses from this computer packed in, so another Kairo opens it looking the same. Pure: the
// caller fetches the pictures (packing) and stores them again (opening).
//
// The file is JSON: { kairoDeck: true, formatVersion, app, exportedAt, item, media: [{ name, type, dataBase64 }] }.
// In `item`, each packed picture's address is replaced by "kairo-media:<n>", its place in `media`.
(function (root) {
  'use strict';

  const FORMAT_VERSION = 1;
  const REF = 'kairo-media:';
  // A picture served by this computer's Kairo: the Media Bin, a media folder, imported ProPresenter pictures.
  // Pictures that ship with Kairo (backgrounds/grain.svg) and data: addresses travel as they are.
  const isLocalMedia = (s) => typeof s === 'string' && /^(?:https?:\/\/(?:127\.0\.0\.1|localhost)(?::\d+)?)?\/api\//.test(s);
  // A local address without its host or its token: the same file however it was written down.
  const plainAddress = (s) => s.replace(/^https?:\/\/(?:127\.0\.0\.1|localhost)(?::\d+)?/, '').replace(/([?&])token=[^&]*&?/, '$1').replace(/[?&]$/, '');

  // Every string in a value, rewritten by `fn` (deep copy).
  function mapStrings(v, fn) {
    if (typeof v === 'string') return fn(v);
    if (Array.isArray(v)) return v.map(x => mapStrings(x, fn));
    if (v && typeof v === 'object') {
      const out = {};
      for (const k of Object.keys(v)) out[k] = mapStrings(v[k], fn);
      return out;
    }
    return v;
  }

  // The local pictures an item uses, each once.
  function mediaAddresses(item) {
    const found = new Set();
    mapStrings(item, (s) => { if (isLocalMedia(s)) found.add(plainAddress(s)); return s; });
    return [...found];
  }

  const fileName = (address) => decodeURIComponent(address.split('?')[0].split('/').pop() || 'picture');

  // item: the playlist item. files: address → { type, dataBase64 } for the pictures that could be read (a
  // missing one keeps its address and simply won't show elsewhere). Returns the file's text.
  function pack(item, files, now) {
    const addresses = mediaAddresses(item).filter(a => files && files[a]);
    const index = new Map(addresses.map((a, i) => [a, i]));
    const copy = mapStrings(item, (s) => (isLocalMedia(s) && index.has(plainAddress(s)) ? REF + index.get(plainAddress(s)) : s));
    delete copy.id;
    delete copy.libraryId;
    return JSON.stringify({
      kairoDeck: true,
      formatVersion: FORMAT_VERSION,
      app: 'KAIRO',
      exportedAt: (now || new Date()).toISOString(),
      item: copy,
      media: addresses.map(a => ({ name: fileName(a), type: files[a].type || 'application/octet-stream', dataBase64: files[a].dataBase64 })),
    });
  }

  // The file's text → { item, media }. Throws a sentence the operator can read when it isn't a Kairo deck.
  function unpack(text) {
    let d;
    try { d = JSON.parse(text); } catch { throw new Error('This file isn’t a Kairo deck.'); }
    if (!d || d.kairoDeck !== true || !d.item || typeof d.item !== 'object') throw new Error('This file isn’t a Kairo deck.');
    if (Number(d.formatVersion) > FORMAT_VERSION) throw new Error('This deck was made by a newer Kairo. Update Kairo to open it.');
    if (!['slides', 'song', 'scripture', 'image'].includes(d.item.type) || !Array.isArray(d.item.blocks)) throw new Error('This Kairo deck has no slides in it.');
    const media = Array.isArray(d.media) ? d.media.filter(m => m && typeof m.dataBase64 === 'string') : [];
    return { item: d.item, media };
  }

  // The item with each "kairo-media:<n>" put back as addresses[n] (where the picture now lives); one that
  // couldn't be stored is dropped to an empty address rather than left pointing nowhere.
  function relink(item, addresses) {
    return mapStrings(item, (s) => {
      if (!s.startsWith(REF)) return s;
      const a = addresses[Number(s.slice(REF.length))];
      return typeof a === 'string' ? a : '';
    });
  }

  const api = { pack, unpack, relink, mediaAddresses, FORMAT_VERSION };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (root) root.KairoDeck = api;
})(typeof window !== 'undefined' ? window : null);
