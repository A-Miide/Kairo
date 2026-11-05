// KAIRO — Slide finder: the slide being spoken, in ANY playlist.
//
// content_lookup.js brings up a deck in the live playlist when its OPENING is
// heard; slide_follow.js moves the live deck on. Neither catches a preacher
// (or a guest) who reads a slide from the middle of a deck in another
// playlist. This does: every slide of every 'slides' deck is indexed by its
// own words, and a slide whose words are clearly being spoken is OFFERED in
// Possible Matches (owner, 2026-10-17: "It should live in possible
// candidates"). Clicking the card opens the deck where it lives and sends
// that slide (service.js openFound). It never sends by itself.
//
// Same caution as content_lookup.js: enough of the slide's own distinctive
// words heard, a clear lead over the next-best slide, held for a moment, and
// the same slide offered again only after a while.
'use strict';

(function (root) {
  const LF = (typeof window !== 'undefined' && window.KairoLyricsFollow)
    || (typeof require === 'function' ? require('./lyrics_follow.js') : null);
  if (!LF) throw new Error('slide_finder.js requires lyrics_follow.js to be loaded first');
  const { tokenizeKeys, keyMatch, key } = LF;

  const DEFAULTS = {
    tailWords: 40,       // recent speech scored each update
    minKeys: 5,          // a slide needs this many distinctive words of its own to be findable
    minHits: 5,          // ...and this many of them heard
    confMatch: 0.6,      // ...making at least this fraction of its distinctive words
    leadMargin: 1.5,     // the best slide must beat the next-best by this multiplier
    sustainMs: 1500,     // and stay the best this long
    missGraceMs: 800,    // a single jittery update doesn't reset the clock
    repeatMs: 120000,    // the same slide isn't offered again within this
  };

  // Words that say nothing about WHICH slide: function words, and what every
  // sermon and song says anyway.
  const STOP = new Set(('the a an and or but of to in on at is are be am was were been '
    + 'i you he she it we they me my your his her our their us him them this that these those '
    + 'with for as by from not no so do did does done have has had will would shall can could '
    + 'should may might must if then than there here what which who whom when where why how '
    + 'all any some every one oh yeah now let lets just also very '
    + 'lord god jesus christ holy hallelujah hosanna amen').split(/\s+/).map(key));

  function slideKeys(text) {
    return [...new Set(tokenizeKeys(text).filter(k => k.length >= 3 && !STOP.has(k)))];
  }

  function SlideFinder(opts) {
    opts = opts || {};
    this.cfg = Object.assign({}, DEFAULTS, opts.config || {});
    this.onOffer = opts.onOffer || function () {};
    this.slides = [];
    this.byKey = new Map();
    this.exclude = new Set();
    this.offeredAt = new Map();
    this._bestKey = null;
    this._bestSince = 0;
    this._bestLastSeenAt = 0;
  }

  // slides: [{ playlistId, playlistName, itemId, itemTitle, index, text }], the
  // live playlist's first. The same words in several places (a deck copied into
  // two playlists) count once, where they come first.
  SlideFinder.prototype.setSlides = function (slides) {
    this.slides = [];
    this.byKey = new Map();
    const seen = new Set();
    for (const s of slides || []) {
      const keys = slideKeys(s.text);
      if (keys.length < this.cfg.minKeys) continue;
      const sig = [...keys].sort().join(' ');
      if (seen.has(sig)) continue;
      seen.add(sig);
      const at = this.slides.length;
      this.slides.push({ ...s, keys, id: `${s.itemId}:${s.index}` });
      for (const k of keys) {
        if (!this.byKey.has(k)) this.byKey.set(k, []);
        this.byKey.get(k).push(at);
      }
    }
  };

  // Items whose slides aren't offered (the deck that's live: slide_follow.js has it).
  SlideFinder.prototype.setExclude = function (itemIds) {
    this.exclude = new Set(itemIds || []);
  };

  SlideFinder.prototype.ingest = function (text, meta) {
    if (!this.slides.length) return;
    const now = meta && meta.now != null ? meta.now : Date.now();
    const heard = slideKeys(text).slice(-this.cfg.tailWords);
    if (!heard.length) return;

    // Only slides sharing at least two exact words with the speech are scored.
    const shared = new Map();
    for (const k of heard) for (const at of this.byKey.get(k) || []) shared.set(at, (shared.get(at) || 0) + 1);

    let best = null, secondScore = 0;
    for (const [at, n] of shared) {
      if (n < 2) continue;
      const s = this.slides[at];
      if (this.exclude.has(s.itemId)) continue;
      let hits = 0;
      for (const k of s.keys) if (heard.some(h => keyMatch(h, k))) hits++;
      if (hits < this.cfg.minHits) continue;
      const score = hits / s.keys.length;
      if (!best || score > best.score) {
        if (best) secondScore = Math.max(secondScore, best.score);
        best = { s, score };
      } else secondScore = Math.max(secondScore, score);
    }

    if (!best || best.score < this.cfg.confMatch || (secondScore > 0 && best.score < secondScore * this.cfg.leadMargin)) {
      if (this._bestKey && now - this._bestLastSeenAt > this.cfg.missGraceMs) this._bestKey = null;
      return;
    }
    if (this._bestKey !== best.s.id) {
      this._bestKey = best.s.id;
      this._bestSince = now;
      this._bestLastSeenAt = now;
      return;
    }
    this._bestLastSeenAt = now;
    if (now - this._bestSince < this.cfg.sustainMs) return;
    const last = this.offeredAt.get(best.s.id);
    if (last && now - last < this.cfg.repeatMs) return;
    this.offeredAt.set(best.s.id, now);
    this._bestKey = null;
    const { keys, id, ...slide } = best.s;
    this.onOffer({ ...slide, confidence: best.score });
  };

  const API = { SlideFinder, slideKeys, DEFAULTS };
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoSlideFinder = API;
})(typeof window !== 'undefined' ? window : null);
