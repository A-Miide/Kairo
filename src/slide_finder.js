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
// A slide of a file the operator has switched on (its Auto-Deploy toggle) is SENT when it is found, with no click: that is
// the operator's consent given ahead of time, for that slide. When two or more armed slides are about equally
// likely (the same chorus in two decks) nothing is sent: both are offered instead. A song or hymn is found by
// its TITLE being said (two or more distinctive words of it, all heard) and is only offered unless it too has
// its Auto-Deploy toggle on.
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
    minTitleKeys: 2,     // a song or hymn is found by its title: this many distinctive words, all of them heard
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
      if (keys.length < (s.kind === 'song' ? this.cfg.minTitleKeys : this.cfg.minKeys)) continue;
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

    let best = null, second = null;
    for (const [at, n] of shared) {
      if (n < 2) continue;
      const s = this.slides[at];
      if (this.exclude.has(s.itemId)) continue;
      let hits = 0;
      for (const k of s.keys) if (heard.some(h => keyMatch(h, k))) hits++;
      // A song's title must be heard whole; a slide needs enough of its own words.
      if (hits < (s.kind === 'song' ? s.keys.length : this.cfg.minHits)) continue;
      const score = hits / s.keys.length;
      if (!best || score > best.score) {
        second = best;
        best = { s, score };
      } else if (!second || score > second.score) second = { s, score };
    }
    const secondScore = second ? second.score : 0;

    const clear = best && best.score >= this.cfg.confMatch && !(secondScore > 0 && best.score < secondScore * this.cfg.leadMargin);
    // Armed slides that can't be told apart are offered together, not sent.
    const ambiguous = best && second && best.score >= this.cfg.confMatch && !clear && (best.s.auto || second.s.auto);
    if (!clear && !ambiguous) {
      if (this._bestKey && now - this._bestLastSeenAt > this.cfg.missGraceMs) this._bestKey = null;
      return;
    }
    const group = ambiguous ? [best, second] : [best];
    const groupKey = group.map(g => g.s.id).join('|');
    if (this._bestKey !== groupKey) {
      this._bestKey = groupKey;
      this._bestSince = now;
      this._bestLastSeenAt = now;
      return;
    }
    this._bestLastSeenAt = now;
    if (now - this._bestSince < this.cfg.sustainMs) return;
    const fresh = group.filter(g => { const last = this.offeredAt.get(g.s.id); return !(last && now - last < this.cfg.repeatMs); });
    if (!fresh.length) return;
    for (const g of fresh) this.offeredAt.set(g.s.id, now);
    this._bestKey = null;
    for (const g of fresh) {
      const { keys, id, auto, ...slide } = g.s;
      // Sent by itself only when it is the one armed slide that clearly won.
      this.onOffer({ ...slide, auto: !!auto && !ambiguous, ambiguous: !!ambiguous, confidence: g.score });
    }
  };

  const API = { SlideFinder, slideKeys, DEFAULTS };
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoSlideFinder = API;
})(typeof window !== 'undefined' ? window : null);
