// KAIRO — Slide follower: moves a live slide deck along as the speaker
// gets to each slide.
//
// What a sermon or announcement deck looks like in practice: statements the
// preacher says close to word for word ("Your faith won't build itself"),
// scriptures they read (often naming the reference first), headings they
// talk around, and slides they never reach. So this listens for a slide's
// own words heard IN ORDER and close together in the last few seconds of
// speech — not a bag of shared words, which is what made a stray "build
// your faith" jump a deck twelve slides ahead.
//
// Rules, in the order they protect the screen:
//  • Forward only. The live slide never goes back on its own.
//  • The next slide needs a little proof; each slide further ahead needs
//    more (a skipped slide is common, a far jump on thin evidence is the
//    glaring failure). Several slides ahead needs most of the slide heard.
//  • Words the CURRENT slide explains as well never move it — a statement
//    repeated three times stays put.
//  • Words that already moved the deck can't move it again.
//  • Spoken numbers count as digits, so "Romans four twenty" lines up with
//    a slide that says "Romans 4:20".
//
// Runs client-side (service.js feeds it every transcript event); the same
// file loads in Node for the replay eval (scripts/slide-follow-eval.js).
'use strict';

(function (root) {

  const LF = (typeof window !== 'undefined' && window.KairoLyricsFollow)
    || (typeof require === 'function' ? require('./lyrics_follow.js') : null);
  if (!LF) throw new Error('slide_follow.js requires lyrics_follow.js to be loaded first');
  const { tokenizeKeys, keyMatch } = LF;

  // ── numbers: "twenty one" → 21, "first John" → 1 john, "4:20-21" → 4 20 21
  const UNITS = { zero: 0, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9,
    ten: 10, eleven: 11, twelve: 12, thirteen: 13, fourteen: 14, fifteen: 15, sixteen: 16, seventeen: 17,
    eighteen: 18, nineteen: 19 };
  const TENS = { twenty: 20, thirty: 30, forty: 40, fifty: 50, sixty: 60, seventy: 70, eighty: 80, ninety: 90 };
  const ORDINAL = { first: 1, second: 2, third: 3 };

  function words(text) {
    return String(text || '').toLowerCase()
      .replace(/[’']/g, '')
      .replace(/(\d)\s*[:.\-–—]\s*(?=\d)/g, '$1 ')
      .split(/[^a-z0-9]+/).filter(Boolean);
  }

  function numberize(ws) {
    const out = [];
    for (let i = 0; i < ws.length; i++) {
      const w = ws[i];
      if (TENS[w] != null) {
        const u = UNITS[ws[i + 1]];
        if (u != null && u > 0 && u < 10) { out.push(String(TENS[w] + u)); i++; } else out.push(String(TENS[w]));
      } else if (UNITS[w] != null) out.push(String(UNITS[w]));
      else if (ORDINAL[w] != null) out.push(String(ORDINAL[w]));
      else if (w === 'verse' || w === 'verses' || w === 'chapter') continue;
      else if (w === 'and' && /^\d+$/.test(out[out.length - 1] || '')) continue;
      else out.push(w);
    }
    return out;
  }

  function keysOf(text) {
    return numberize(words(text)).map((w) => (/^\d+$/.test(w) ? w : tokenizeKeys(w)[0])).filter(Boolean);
  }

  // Words that say nothing about WHICH slide is up.
  const STOP = new Set(keysOf(
    'the a an and or but of to in on at is are be am was were i you he she it we they me my your his her our '
    + 'their this that with for as so do did does done have has had will would can could should shall not no '
    + 'just now all if then than there here what who whom which when how why from by up out into over about '
    + 'him them us its also very say said says let thee thou thy ye unto hath lord god jesus christ amen yes'));

  function buildDeck(slides) {
    const list = slides.map((text, idx) => ({ idx, keys: keysOf(text) }));
    const df = new Map();
    list.forEach((s) => new Set(s.keys).forEach((k) => df.set(k, (df.get(k) || 0) + 1)));
    const n = Math.max(1, list.length);
    list.forEach((s) => {
      s.content = s.keys.map((k) => !STOP.has(k) && (k.length >= 3 || /^\d+$/.test(k)));
      // how informative each word is within THIS deck (a word on every
      // slide — "faith" in a faith sermon — still counts, just less)
      s.weight = s.keys.map((k, i) => (s.content[i] ? Math.min(1, 0.35 + Math.log(n / (df.get(k) || 1)) / Math.log(Math.max(2, n))) : 0));
      s.contentCount = s.content.filter(Boolean).length;
    });
    return list;
  }

  const DEFAULTS = {
    windowTokens: 40,      // recent speech scored against each slide
    recentTokens: 10,      // a match must end within the last N words heard (it's happening NOW)
    aheadWindow: 12,       // slides ahead considered at all
    minDwellMs: 1500,
  };

  // Evidence a slide `d` places ahead needs: distinct content words heard in
  // order, and the share of the slide's opening that covers. Short slides
  // ("Value is a function of cost") are judged on what they have.
  function needFor(d, slide) {
    const cap = Math.max(2, Math.min(slide.contentCount, 6));
    if (d === 1) return { hits: Math.min(3, cap), cover: 0.5 };
    if (d <= 3) return { hits: Math.min(4, cap), cover: 0.6 };
    return { hits: Math.min(5, cap), cover: 0.75 };
  }

  function SlideFollower(deck, opts) {
    opts = opts || {};
    this.cfg = Object.assign({}, DEFAULTS, opts.config || {});
    this.slides = buildDeck((deck && deck.slides) || []);
    this.index = 0;
    this.confidence = 0;
    this.onAdvance = opts.onAdvance || function () {};
    this.onPosition = opts.onPosition || function () {};
    this.enabled = true;
    this._final = [];        // keys from finished utterances (rolling)
    this._interim = [];      // the utterance still being heard
    this._consumed = 0;      // stream position up to which words already moved the deck
    this._finalBase = 0;     // stream position of _final[0]
    this._lastAdvanceAt = null;
    this._lastEmitAt = 0;
  }

  SlideFollower.prototype.resync = function (index, now) {
    if (!this.slides[index]) return;
    this.index = index;
    this._lastAdvanceAt = now || Date.now();
    this._consumed = this._finalBase + this._final.length + this._interim.length;
  };

  SlideFollower.prototype.snapshot = function () {
    return { index: this.index, confidence: Math.round(this.confidence * 100) / 100, frozen: false };
  };

  // In-order match of a slide's words against the recent speech: a longest
  // common subsequence where words "match" under the same fuzzy key rule
  // the lyric follower uses. Returns the content words matched, their
  // weight, where in the window the match ends, and how spread out it is.
  SlideFollower.prototype._match = function (slide, win) {
    const S = slide.keys, n = S.length, m = win.length;
    if (!n || !m) return null;
    // dp over (slide prefix, window prefix): best content weight
    const W = new Float32Array((n + 1) * (m + 1));
    const H = new Int16Array((n + 1) * (m + 1));
    for (let i = 1; i <= n; i++) {
      for (let j = 1; j <= m; j++) {
        const o = i * (m + 1) + j;
        let best = W[o - 1], hits = H[o - 1];
        const up = (i - 1) * (m + 1) + j;
        if (W[up] > best || (W[up] === best && H[up] > hits)) { best = W[up]; hits = H[up]; }
        if (keyMatch(S[i - 1], win[j - 1])) {
          const d = (i - 1) * (m + 1) + (j - 1);
          const w = W[d] + slide.weight[i - 1] + 0.0001;
          if (w > best) { best = w; hits = H[d] + (slide.content[i - 1] ? 1 : 0); }
        }
        W[o] = best; H[o] = hits;
      }
    }
    // walk back for the matched window positions
    let i = n, j = m;
    const pos = [];
    while (i > 0 && j > 0) {
      const o = i * (m + 1) + j;
      const d = (i - 1) * (m + 1) + (j - 1);
      if (keyMatch(S[i - 1], win[j - 1]) && Math.abs(W[o] - (W[d] + slide.weight[i - 1] + 0.0001)) < 1e-4) {
        if (slide.content[i - 1]) pos.push({ j: j - 1, w: slide.weight[i - 1], si: i - 1 });
        i--; j--;
      } else if (W[o] === W[o - 1] && H[o] === H[o - 1]) j--;
      else i--;
    }
    if (!pos.length) return null;
    pos.reverse();
    // keep the tightest cluster: drop leading matches far from the rest
    while (pos.length > 2 && pos[1].j - pos[0].j > 6) pos.shift();
    const hits = new Set(pos.map((p) => slide.keys[p.si])).size;
    const weight = pos.reduce((a, p) => a + p.w, 0);
    // coverage of the slide's opening (a long scripture is often only part-read)
    const head = slide.weight.filter((x, k) => slide.content[k]).slice(0, Math.max(6, pos[pos.length - 1].si + 1));
    const cover = weight / Math.max(0.0001, head.reduce((a, b) => a + b, 0));
    return { hits, weight, cover: Math.min(1, cover), first: pos[0].j, last: pos[pos.length - 1].j, span: pos[pos.length - 1].j - pos[0].j + 1 };
  };

  SlideFollower.prototype.ingest = function (text, meta) {
    if (!this.enabled) return;
    const now = (meta && meta.now) || Date.now();
    const keys = keysOf(text);
    if (meta && meta.isFinal) {
      this._final.push(...keys);
      this._interim = [];
      const extra = this._final.length - this.cfg.windowTokens * 2;
      if (extra > 0) { this._final.splice(0, extra); this._finalBase += extra; }
    } else {
      this._interim = keys;
    }
    this._decide(now);
    if (now - this._lastEmitAt >= 200) { this._lastEmitAt = now; this.onPosition(this.snapshot()); }
  };

  SlideFollower.prototype._decide = function (now) {
    if (this._lastAdvanceAt !== null && now - this._lastAdvanceAt < this.cfg.minDwellMs) return;
    const stream = this._final.concat(this._interim);
    const streamEnd = this._finalBase + stream.length;
    const from = Math.max(this._consumed, streamEnd - this.cfg.windowTokens);
    const win = stream.slice(from - this._finalBase);
    if (win.length < 2) return;
    const cur = this.slides[this.index];
    const curMatch = cur ? this._match(cur, win) : null;
    let pick = null;
    const hi = Math.min(this.slides.length - 1, this.index + this.cfg.aheadWindow);
    for (let k = this.index + 1; k <= hi; k++) {
      const s = this.slides[k];
      if (!s.contentCount) continue;
      const mt = this._match(s, win);
      if (!mt) continue;
      const need = needFor(k - this.index, s);
      if (mt.hits < need.hits || mt.cover < need.cover) continue;
      if (mt.last < win.length - this.cfg.recentTokens) continue;      // heard a while ago, not now
      if (mt.span > mt.hits * 3 + 4) continue;                          // scattered words, not the slide
      // the current slide explains these words at least as well → stay
      if (curMatch && curMatch.weight >= mt.weight && curMatch.last >= mt.first) continue;
      if (!pick || mt.weight > pick.mt.weight + 1) pick = { k, mt };      // nearer wins a near-tie
    }
    if (!pick) { this.confidence *= 0.9; return; }
    this.index = pick.k;
    this._lastAdvanceAt = now;
    this._consumed = from + pick.mt.last + 1;
    this.confidence = Math.min(1, pick.mt.cover);
    this.onAdvance({ toIndex: pick.k, confidence: this.confidence });
  };

  const API = { SlideFollower, keysOf, DEFAULTS };
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoSlideFollow = API;

})(typeof window !== 'undefined' ? window : null);
