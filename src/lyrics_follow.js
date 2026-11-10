// KAIRO — Lyric follower ("karaoke" auto-advance)
//
// Given the song that's live and a rolling speech transcript of what's being
// sung, keep a cursor on the current LINE of the lyrics and advance the
// slide when the singers cross into the next block.
//
// This is ALIGNMENT, not transcription. We already know the song — we're
// tracking a position in known text, not recognising open vocabulary. Bad
// transcript words don't sink it: the matcher only needs a few of the recent
// words to land on the right line.
//
// Runs client-side (service.js already knows the live song + block index,
// already receives {type:'transcript'} over the WS, and already has the
// slide-advance call). The same engine is import-able in Node for the replay
// harness (scripts/follow-replay.js) — see the dual export at the bottom.
//
// Design notes:
//  • LANGUAGE-NEUTRAL. Text is normalised with Unicode NFD, combining marks
//    stripped (Yoruba tones and dots, French and Portuguese accents, Arabic
//    vowel marks), lowercased, punctuation dropped; letters and digits of any
//    script survive. Chinese, Japanese and Thai have no spaces, so each
//    character is a token. English phonetics (thee/thou, ph→f, -ing) apply
//    ONLY to songs detected as English.
//  • CHARACTER-LEVEL MATCHING. Each lyric line becomes a set of character
//    3-grams of its space-free text (2-grams for CJK/Thai). The last few
//    heard words, joined without spaces, are scored against each candidate
//    line with a weighted Dice overlap; n-grams are weighted by rarity within
//    the song, and grams that only occur inside the song's own most frequent
//    words count little. Joining the words without spaces is what survives a
//    speech engine splitting or merging words. A normalised edit distance on
//    the winning line is the final gate.
//  • Songs may be one deck or many in one: the matcher sees a flat list of
//    lines; block boundaries only decide which slide to show.
//  • FORWARD first: the cursor tracks a window of lines ahead of it. A wrong
//    auto-advance on a live screen is worse than a miss, so crossing into
//    another block needs a LOCK (strong score, enough rare content, a clear
//    lead over the runner-up, low edit distance) and the dwell floor.
//  • REPEATS: when the forward window has no lock (a chorus sung out of
//    order, a leader repeating an earlier line), a bounded global re-search
//    runs over every line. A jump needs a much clearer lead and two
//    consecutive updates (or one final segment). A line whose text appears on
//    several slides resolves to the next not-yet-visited occurrence.
//  • "Last words of the block's final line" ARMS an advance (anticipation, so
//    the slide flips as the last word lands). A locked match on a line of the
//    next block is the CATCH-UP trigger.
//  • A partial can still revise earlier words depending on the STT engine,
//    so every ingest re-scores only the RECENT TAIL of the transcript.
//    Stateless per call apart from the cursor and the repeat confirmation.
'use strict';

(function (root) {

  // ── normalisation ──────────────────────────────────────────────────────
  // Letters that do not decompose under NFD but that speech engines write
  // with their plain Latin neighbour (Hausa hooked letters, German ß, ...).
  const FOLD = {
    'ɓ': 'b', 'ɗ': 'd', 'ƙ': 'k', 'ß': 'ss', 'æ': 'ae', 'œ': 'oe', 'ø': 'o',
    'đ': 'd', 'ł': 'l', 'ı': 'i', 'ɛ': 'e', 'ɔ': 'o', 'ŋ': 'ng',
  };
  const FOLD_RE = /[ɓɗƙßæœøđłıɛɔŋ]/g;
  const MARKS_RE = /\p{M}/gu;
  const NON_WORD_RE = /[^\p{L}\p{N}]/gu;
  // Scripts written without spaces between words: one character per token.
  const CHAR_SCRIPT_RE = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Thai}\p{Script=Lao}\p{Script=Khmer}\p{Script=Myanmar}]/u;

  // Language-neutral word normal form.
  function norm(w) {
    let s = String(w || '').normalize('NFD').replace(MARKS_RE, '').toLowerCase();
    s = s.replace(FOLD_RE, (c) => FOLD[c]);
    return s.replace(NON_WORD_RE, '');
  }

  // Common sung-worship / STT confusions — folded so "thee"/"the",
  // "o"/"oh", "'tis"/"it is" don't cost a match. English only. Kept small on
  // purpose: over-folding erases the distinctive words the aligner leans on.
  const STT_SWAP = {
    thee: 'the', thou: 'the', thy: 'the', thine: 'the', ye: 'you',
    o: 'oh', ah: 'oh', tis: 'itis', twas: 'itwas', ere: 'before',
    unto: 'to', upon: 'on', art: 'are', hath: 'has', doth: 'does',
    'gonna': 'goingto', 'wanna': 'wantto',
  };

  // Cheap phonetic-ish key for English: fold the swap table, collapse doubled
  // letters, a few digraph reductions, drop a trailing "e", "-ing" → "-in".
  function englishRules(w) {
    if (STT_SWAP[w]) w = STT_SWAP[w];
    w = w.replace(/(.)\1+/g, '$1');
    w = w.replace(/ph/g, 'f').replace(/wr/g, 'r').replace(/kn/g, 'n').replace(/gh/g, '');
    w = w.replace(/ing$/, 'in');
    if (w.length > 3) w = w.replace(/e$/, '');
    return w;
  }

  // key(raw[, lang]): the word as the matcher sees it. English rules by
  // default (every existing caller is English); pass any other lang code
  // ('es', 'yo', 'x', ...) for the plain language-neutral form.
  function key(raw, lang) {
    const w = norm(raw);
    if (!w) return '';
    return (lang && lang !== 'en') ? w : englishRules(w);
  }

  // Split on whitespace; spaceless scripts are split into single characters.
  function splitWords(text) {
    const out = [];
    for (const piece of String(text || '').split(/\s+/)) {
      if (!piece) continue;
      if (!CHAR_SCRIPT_RE.test(piece)) { out.push(piece); continue; }
      let buf = '';
      for (const ch of Array.from(piece)) {
        if (CHAR_SCRIPT_RE.test(ch)) { if (buf) { out.push(buf); buf = ''; } out.push(ch); }
        else buf += ch;
      }
      if (buf) out.push(buf);
    }
    return out;
  }

  function tokenizeKeys(text, lang) {
    return splitWords(text).map((w) => key(w, lang)).filter(Boolean);
  }

  // Same folding as tokenizeKeys, but keeps each surviving key's real
  // audio-relative start/end (seconds) alongside it. Fed from Deepgram's own
  // `words` array (always present on a transcript event, no config needed),
  // threaded through the server -> WS -> onTranscript's meta.words. Built
  // directly from the source `words` array (not a separate map-then-filter
  // pass), so a dropped empty-key word can never desync a key from the wrong
  // timestamp. A spaceless-script word split into characters shares the
  // word's times.
  function tokenizeKeysWithTimes(words, lang) {
    const out = [];
    for (const w of (words || [])) {
      for (const piece of splitWords(w && w.word)) {
        const k = key(piece, lang);
        if (k) out.push({ key: k, start: w.start, end: w.end });
      }
    }
    return out;
  }

  // Two keys "match" if equal, or one is a prefix of the other and the
  // shorter is at least 4 chars (catches "believ"/"believed", "sing"/
  // "singing" that survived the -ing fold, etc.) — never for short keys,
  // where a prefix collision is usually coincidence.
  function keyMatch(a, b) {
    if (!a || !b) return false;
    if (a === b) return true;
    const [s, l] = a.length <= b.length ? [a, b] : [b, a];
    return s.length >= 4 && l.startsWith(s);
  }

  // A song is English when enough of its words are English function words.
  // Words that are also common in other languages (was, me, no, in, he, we,
  // a, i) are left out on purpose.
  const EN_FUNCTION = new Set(('the and of you your my is are with for that this thy thou thee '
    + 'his our who shall from when will not have lord god').split(' '));
  function detectLang(tokens) {
    if (!tokens.length) return 'x';
    let hits = 0;
    for (const t of tokens) if (EN_FUNCTION.has(t)) hits++;
    return hits / tokens.length >= 0.18 ? 'en' : 'x';
  }

  // ── the song model ─────────────────────────────────────────────────────
  // song.blocks: [{ label, lines: [string] }]  (Kairo's hymn/song shape).
  // Optional song.lang forces the language ('en' or anything else).
  function charGrams(s, n) {
    const out = new Set();
    if (!s) return out;
    if (s.length <= n) { out.add(s); return out; }
    for (let i = 0; i + n <= s.length; i++) out.add(s.substr(i, n));
    return out;
  }

  function flattenSong(song, cfg) {
    cfg = cfg || {};
    const rawLines = [];  // [{ blockIdx, text }]
    const blockDefs = [];
    (song && song.blocks || []).forEach((b, bi) => {
      const first = rawLines.length;
      (b.lines || []).forEach((l) => {
        String(l == null ? '' : l).split(/\n+/).forEach((part) => { if (part.trim()) rawLines.push({ blockIdx: bi, text: part }); });
      });
      blockDefs.push({ bi, label: b.label, first });
    });

    // Language: explicit, else detected from the song's own words.
    const probe = [];
    rawLines.forEach((r) => splitWords(r.text).forEach((w) => { const k = norm(w); if (k) probe.push(k); }));
    const lang = (song && song.lang) || cfg.lang || detectLang(probe);

    const lines = [];
    const blocks = [];
    let unit = 0;
    blockDefs.forEach((bd, i) => {
      const l0 = lines.length;
      const start = unit;
      const stop = i + 1 < blockDefs.length ? blockDefs[i + 1].first : rawLines.length;
      for (let r = bd.first; r < stop; r++) {
        const toks = tokenizeKeys(rawLines[r].text, lang);
        if (!toks.length) continue;
        lines.push({ idx: lines.length, blockIdx: bd.bi, toks, s: toks.join(''), wstart: unit, wlen: toks.length, t: null });
        unit += toks.length;
      }
      blocks.push({ idx: bd.bi, label: bd.label || ('Part ' + (bd.bi + 1)), start, end: unit, len: unit - start, l0, l1: lines.length });
    });

    // Distinct line texts (a chorus on three slides is one text, three
    // occurrences) — rarity is counted over distinct texts.
    const byText = new Map();
    const texts = [];
    lines.forEach((ln) => {
      let t = byText.get(ln.s);
      if (!t) { t = { tid: texts.length, s: ln.s, L: ln.s.length, toks: ln.toks, occ: [], gmap: null, cum: null, total: 0 }; byText.set(ln.s, t); texts.push(t); }
      t.occ.push(ln.idx);
      ln.t = t;
    });
    const T = texts.length;

    let spaceless = 0, allChars = 0;
    texts.forEach((t) => { allChars += t.L; for (const ch of t.s) if (CHAR_SCRIPT_RE.test(ch)) spaceless++; });
    const cjk = allChars > 0 && spaceless / allChars >= 0.5;
    const n = cjk ? 2 : 3;

    // The song's own most frequent words are its stopwords.
    const wordDf = new Map();
    texts.forEach((t) => new Set(t.toks).forEach((k) => wordDf.set(k, (wordDf.get(k) || 0) + 1)));
    const stopMin = Math.max(3, Math.ceil(0.2 * T));
    const isStop = (k) => wordDf.get(k) >= stopMin || (!cjk && k.length <= 1);

    // Per text: gram -> end offset of its first occurrence; plus which grams
    // ever sit on a non-stopword character.
    const df = new Map();
    const content = new Set();
    texts.forEach((t) => {
      const stopMask = [];
      t.toks.forEach((k) => { const st = isStop(k); for (let c = 0; c < k.length; c++) stopMask.push(st); });
      t.gmap = new Map();
      if (t.L <= n) {
        t.gmap.set(t.s, t.L);
        if (stopMask.some((x) => !x)) content.add(t.s);
      } else {
        for (let i = 0; i + n <= t.L; i++) {
          const g = t.s.substr(i, n);
          if (!t.gmap.has(g)) t.gmap.set(g, i + n);
          for (let c = i; c < i + n; c++) if (!stopMask[c]) { content.add(g); break; }
        }
      }
      t.gmap.forEach((_, g) => df.set(g, (df.get(g) || 0) + 1));
    });
    const logT = Math.log(1 + T);
    const w = new Map();
    df.forEach((d, g) => w.set(g, (Math.log(1 + T / d) / logT) * (content.has(g) ? 1 : 0.35)));

    // Cumulative weight by prefix length, for scoring a half-sung line.
    texts.forEach((t) => {
      t.cum = new Float64Array(t.L + 1);
      t.gmap.forEach((end, g) => { t.cum[Math.min(end, t.L)] += w.get(g); });
      for (let i = 1; i <= t.L; i++) t.cum[i] += t.cum[i - 1];
      t.total = t.cum[t.L];
    });

    return { lang, n, cjk, lines, blocks, texts, w };
  }

  // Score one heard window against one lyric text. Dice overlap of the
  // rarity-weighted grams, against the whole line and against the line's
  // opening (a line only half sung); the better of the two wins.
  function scoreText(model, t, win) {
    let I = 0, Ip = 0;
    const P = Math.min(t.L, Math.round(win.len * 1.1) + 3);
    for (const g of win.grams) {
      const end = t.gmap.get(g);
      if (end === undefined) continue;
      const wt = model.w.get(g);
      I += wt;
      if (end <= P) Ip += wt;
    }
    const full = (2 * I) / (win.W + t.total || 1);
    const pre = (2 * Ip) / (win.W + t.cum[P] || 1);
    return pre > full ? { score: pre, mass: Ip } : { score: full, mass: I };
  }

  // Semi-global edit distance: all of `h` (heard) against the best-fitting
  // stretch of `t` (lyric). Returns the cost, and where that stretch ends.
  function align(h, t) {
    const n = h.length, L = t.length;
    let prev = new Array(L + 1).fill(0);
    let cur = new Array(L + 1).fill(0);
    for (let i = 1; i <= n; i++) {
      cur[0] = i;
      const hc = h.charCodeAt(i - 1);
      for (let j = 1; j <= L; j++) {
        const sub = prev[j - 1] + (hc === t.charCodeAt(j - 1) ? 0 : 1);
        const del = prev[j] + 1;
        const ins = cur[j - 1] + 1;
        cur[j] = sub <= del ? (sub <= ins ? sub : ins) : (del <= ins ? del : ins);
      }
      const tmp = prev; prev = cur; cur = tmp;
    }
    let D = Infinity, e = 0;
    for (let j = 0; j <= L; j++) if (prev[j] < D) { D = prev[j]; e = j; }
    return { D, e };
  }

  // ── the follower ───────────────────────────────────────────────────────
  const DEFAULTS = {
    tailWords: 10,       // how many recent transcript words to score each ingest
    minDwellMs: 2500,    // floor; the real dwell scales with block length (see _dwell)
    dwellFrac: 0.35,     // ...to at least this fraction of the block's expected duration
    anticipate: false,   // true flips as the last words of a block's final line land; false waits to hear the first words of the next block
    confAdvance: 0.42,   // min confidence for an anticipation advance
    confHold: 0.25,      // below this, freeze — do not advance at all
    softScore: 0.5,      // line score that lets the cursor track along inside the block
    lockScore: 0.5,      // line score for a LOCK (needed to cross into another block)
    edSoft: 0.5,         // max normalised edit distance for a soft match
    edLock: 0.45,        // ...and for a lock
    minMass: 3.5,          // a lock needs this much rare-gram weight (~7 distinctive characters)
    leadMin: 0.05,       // a lock needs this lead over the runner-up line text
    recScore: 0.7,       // a repeat/out-of-order jump needs a much stronger match...
    recEd: 0.35,
    recMass: 6,
    recLead: 0.15,       // ...with a clearer lead...
    recMargin: 0.12,     // ...and a lead over whatever the forward window found
    recConfirm: 2,       // consecutive updates that must agree (one final segment is enough)
    globalMaxLines: 600, // bound on the global re-search
    minSpanLines: 6,     // forward window is at least this many lines
    emitThrottleMs: 200,
    // How many blocks ahead of the immediate next one a catch-up may skip
    // past in one jump. 1 is right for songs: a verse genuinely dropped live
    // is the rare case. A caller tracking a deck presented in whatever order
    // the presenter chooses sets this higher to search further ahead.
    maxBlockSkip: 1,
  };

  function LyricsFollower(song, opts) {
    opts = opts || {};
    const cfg = Object.assign({}, DEFAULTS, opts.config || {});
    const model = flattenSong(song, cfg);

    this.cfg = cfg;
    this.model = model;
    this.lang = model.lang;
    this.lines = model.lines;
    this.blocks = model.blocks;
    // Heard-window sizes in tokens; spaceless scripts carry less per token.
    const scale = model.cjk ? 2 : 1;
    this.tailTokens = cfg.tailWords * scale;
    this.winSizes = [2, 3, 4, 5, 6, 8, 10].map((x) => x * scale).filter((x) => x <= this.tailTokens);
    this.nextLine = 0;       // index of the next line expected; nextLine-1 is the line being sung
    this.visited = new Array(this.lines.length).fill(false);
    this.pos = 0;            // cursor in word units across the whole song
    this.blockIdx = 0;       // the block currently DISPLAYED (only moves via _advanceTo)
    this.confidence = 0;     // 0..1, EMA of recent alignment quality
    this.rate = 2.3;         // tokens/sec, EMA
    this.armed = false;
    this._pending = null;    // repeat/out-of-order jump awaiting confirmation
    this._heard = [];        // the heard tail at the previous ingest
    this._clock = 0;         // tokens heard so far (counted by how far the tail slid)
    this._movedAt = -Infinity; // _clock when the cursor last moved
    this._flip = null;       // the last anticipated flip: { to, at }
    this._noAnticipate = false;
    this._lastMatchAt = 0;
    // null, not 0 — this class is fed either real Date.now() or (see
    // scripts/follow-replay.js) a synthetic clock starting near 0, and
    // there's no absolute timestamp that's safely "in the past" for both
    // domains. null means the dwell floor doesn't apply until a real
    // advance/resync has set this once, in whatever clock domain ingest()
    // is being called with.
    this._lastAdvanceAt = null;
    this._lastEmitAt = 0;
    this.onPosition = opts.onPosition || function () {};
    this.onAdvance = opts.onAdvance || function () {};
    this.enabled = true;
  }

  // Operator override — snap the cursor to a block (manual next/prev, or a
  // direct slide click). Auto-follow continues from there.
  LyricsFollower.prototype.resync = function (blockIdx, now) {
    const b = this.blocks[blockIdx];
    if (!b) return;
    this.pos = b.start;
    this.blockIdx = blockIdx;
    this.nextLine = b.l0;
    for (let i = 0; i < this.visited.length; i++) this.visited[i] = i < b.l0;
    this.armed = false;
    this._pending = null;
    this._movedAt = this._clock;
    this._lastAdvanceAt = now || Date.now();
    this.confidence = Math.max(this.confidence, 0.5); // trust the human
  };

  LyricsFollower.prototype.snapshot = function () {
    const b = this.blocks[this.blockIdx];
    return {
      blockIdx: this.blockIdx,
      blockLabel: b ? b.label : null,
      lineIdx: Math.max(0, this.nextLine - 1),
      confidence: Math.round(this.confidence * 100) / 100,
      rate: Math.round(this.rate * 100) / 100,
      posInBlock: b ? Math.max(0, Math.min(1, (this.pos - b.start) / Math.max(1, b.len))) : 0,
      armed: this.armed,
      frozen: this.confidence < this.cfg.confHold,
    };
  };

  // The recent heard tail as scorable windows: the last m tokens joined
  // WITHOUT spaces, for several m. The right m depends on how much of the
  // current line has been sung, so every size competes.
  LyricsFollower.prototype._windows = function (heard) {
    const w = this.model.w, n = this.model.n;
    const sizes = this.winSizes.filter((m) => m <= heard.length);
    if (heard.length && !sizes.includes(heard.length) && heard.length <= this.tailTokens) sizes.push(heard.length);
    const wins = [];
    for (const m of sizes) {
      const str = heard.slice(-m).join('');
      if (str.length < n) continue;
      const grams = charGrams(str, n);
      let W = 0;
      grams.forEach((g) => { W += w.has(g) ? w.get(g) : 1; });  // a gram the song never has is pure noise
      wins.push({ m, str, len: str.length, grams, W });
    }
    return wins;
  };

  // Score every distinct line text that occurs in lines [lo, hi]; best first.
  LyricsFollower.prototype._rank = function (lo, hi, wins) {
    const seen = new Set();
    const out = [];
    for (let i = lo; i <= hi; i++) {
      const t = this.lines[i].t;
      if (seen.has(t.tid)) continue;
      seen.add(t.tid);
      let best = null;
      for (const win of wins) {
        const r = scoreText(this.model, t, win);
        if (!best || r.score > best.score) best = { t, score: r.score, mass: r.mass, win };
      }
      if (best) out.push(best);
    }
    out.sort((a, b) => b.score - a.score);
    return out;
  };

  // Lead over the runner-up, and the edit-distance check on the winner.
  LyricsFollower.prototype._evaluate = function (ranked) {
    const a = ranked[0];
    if (!a) return null;
    const lead = a.score - (ranked[1] ? ranked[1].score : 0);
    const al = align(a.win.str, a.t.s);
    return { a, lead, ed: al.D / a.win.str.length, e: al.e };
  };

  // Which occurrence of a repeated text is meant. The line being sung stays
  // the one; otherwise the next occurrence not yet visited, in singing order;
  // failing that the nearest.
  LyricsFollower.prototype._pickOcc = function (t, lo, hi) {
    const cur = this.nextLine - 1;
    let firstUnvisited = -1, nearest = -1, nd = Infinity;
    for (const i of t.occ) {
      if (i < lo || i > hi) continue;
      if (i === cur) return i;
      if (firstUnvisited < 0 && i >= this.nextLine && !this.visited[i]) firstUnvisited = i;
      const d = Math.abs(i - cur);
      if (d < nd) { nd = d; nearest = i; }
    }
    return firstUnvisited >= 0 ? firstUnvisited : nearest;
  };

  LyricsFollower.prototype._moveCursor = function (idx, frac) {
    const ln = this.lines[idx];
    // Lines the cursor passes over count as visited (a jump back leaves what
    // was already sung marked, so the arrangement's expected order survives).
    for (let k = Math.max(0, this.nextLine - 1); k < idx; k++) this.visited[k] = true;
    this.visited[idx] = true;
    this.nextLine = idx + 1;
    this.pos = ln.wstart + Math.max(0, Math.min(1, frac)) * ln.wlen;
  };

  LyricsFollower.prototype._dwell = function () {
    const b = this.blocks[this.blockIdx];
    const expMs = b ? (b.len / Math.max(0.6, this.rate)) * 1000 : 0;
    return Math.max(this.cfg.minDwellMs, expMs * this.cfg.dwellFrac);
  };

  LyricsFollower.prototype._dwellOk = function (now) {
    return this._lastAdvanceAt === null || now - this._lastAdvanceAt >= this._dwell();
  };

  LyricsFollower.prototype._advanceTo = function (target, now, reason) {
    // One strike: if the singers went somewhere else right after an
    // anticipated flip, this arrangement does not follow the deck's order
    // (a skipped verse, a chorus out of turn) — stop guessing, wait to hear.
    if (reason !== 'anticipate' && this._flip && target !== this._flip.to && now - this._flip.at < 20000) this._noAnticipate = true;
    this._flip = reason === 'anticipate' ? { to: target, at: now } : null;
    this.blockIdx = target;
    this.armed = false;
    this._pending = null;
    this._lastAdvanceAt = now;
    this.confidence = Math.max(this.confidence, 0.5); // trust it, like resync()
    this.onAdvance({ toBlockIdx: target, label: this.blocks[target].label, confidence: this.confidence, reason });
  };

  LyricsFollower.prototype._emit = function (now) {
    if (now - this._lastEmitAt < this.cfg.emitThrottleMs) return;
    this._lastEmitAt = now;
    this.onPosition(this.snapshot());
  };

  // Search the lines ahead of the cursor. Returns the verdict for the best
  // line: where it is, whether it is a soft match or a LOCK, and how far
  // along the line the singers are.
  LyricsFollower.prototype._searchForward = function (wins) {
    const cfg = this.cfg, N = this.lines.length;
    const lo = Math.max(0, this.nextLine - 1);
    const lastBlock = this.blocks[Math.min(this.blocks.length - 1, this.blockIdx + 1 + cfg.maxBlockSkip)];
    const hi = Math.min(N - 1, Math.max(lastBlock.l1 - 1, lo + cfg.minSpanLines));
    if (lo > hi) return null;
    const ranked = this._rank(lo, hi, wins);
    const r = this._evaluate(ranked);
    if (!r) return null;
    const occ = this._pickOcc(r.a.t, lo, hi);
    if (occ < 0) return null;
    r.occ = occ;
    r.soft = r.a.score >= cfg.softScore && r.ed <= cfg.edSoft;
    r.locked = r.a.score >= cfg.lockScore && r.ed <= cfg.edLock && r.a.mass >= cfg.minMass && r.lead >= cfg.leadMin;
    return r;
  };

  // How many tokens the heard tail slid by since the previous ingest: the
  // smallest k for which the older part of the tail is the previous tail's end.
  function slideBy(prev, cur) {
    for (let k = 0; k < cur.length; k++) {
      const rest = cur.length - k;
      if (rest > prev.length) continue;
      let same = true;
      for (let i = 0; i < rest; i++) if (cur[i] !== prev[prev.length - rest + i]) { same = false; break; }
      if (same) return k;
    }
    return cur.length;
  }

  // Bounded global re-search: the cursor is frozen or lost (a chorus sung
  // out of order, an earlier line repeated). Accepts a jump only on a clear
  // lead, confirmed by two consecutive updates or one final segment.
  LyricsFollower.prototype._recover = function (wins, fwd, isFinal, now) {
    const cfg = this.cfg, N = this.lines.length;
    if (!N) return;
    let lo = 0, hi = N - 1;
    if (N > cfg.globalMaxLines) {
      lo = Math.max(0, Math.min(N - cfg.globalMaxLines, this.nextLine - Math.floor(cfg.globalMaxLines / 2)));
      hi = lo + cfg.globalMaxLines - 1;
    }
    const r = this._evaluate(this._rank(lo, hi, wins));
    const fwdScore = fwd ? fwd.a.score : 0;
    if (!r || r.a.score < cfg.recScore || r.ed > cfg.recEd || r.a.mass < cfg.recMass
      || r.lead < cfg.recLead || r.a.score < fwdScore + cfg.recMargin) { this._pending = null; return; }
    const occ = this._pickOcc(r.a.t, lo, hi);
    if (occ < 0 || occ === this.nextLine - 1) { this._pending = null; return; }  // already there

    const p = this._pending;
    // A second look only counts once at least two more words have arrived.
    if (p && p.line === occ) { if (this._clock >= p.clock + 2) { p.n++; p.clock = this._clock; } }
    else this._pending = { line: occ, n: 1, clock: this._clock };
    if (this._pending.n < cfg.recConfirm && !isFinal) return;

    const bi = this.lines[occ].blockIdx;
    if (bi !== this.blockIdx && !this._dwellOk(now)) return;     // keep the pending jump, wait out the dwell
    this._moveCursor(occ, Math.min(1, r.e / Math.max(1, this.lines[occ].s.length)));
    this._pending = null;
    this.confidence = Math.max(this.confidence, 0.5);
    this._lastMatchAt = now;
    if (bi !== this.blockIdx) this._advanceTo(bi, now, 'repeat');
  };

  // text: the transcript segment (full text of the current window is fine —
  // we only look at its tail). meta.isFinal marks a finished segment (one is
  // enough to confirm a repeat jump).
  // meta.words (optional): [{word, start, end}] real per-word timestamps
  // (seconds) — Deepgram always includes these. When present, `rate` is
  // updated from the actual AUDIO time of the matched words instead of
  // wall-clock arrival timing, which is a poor proxy since ASR delivers text
  // in irregular bursts. Falls back to the wall-clock estimate when absent.
  LyricsFollower.prototype.ingest = function (text, meta) {
    if (!this.enabled) return;
    const cfg = this.cfg;
    const now = (meta && meta.now) || Date.now();
    const timed = meta && Array.isArray(meta.words) && meta.words.length
      ? tokenizeKeysWithTimes(meta.words, this.lang) : null;
    const keys = timed ? timed.map(t => t.key) : tokenizeKeys(text, this.lang);
    if (!keys.length || !this.lines.length) { this._emit(now); return; }
    const heard = keys.slice(-this.tailTokens);
    this._clock += slideBy(this._heard, heard);
    this._heard = heard;
    const heardTimes = timed ? timed.slice(-this.tailTokens) : null;
    const wins = this._windows(heard);
    const isFinal = !!(meta && meta.isFinal);

    const fwd = wins.length ? this._searchForward(wins) : null;
    const prevPos = this.pos;

    // Where along its line the best match says the singers are.
    const progress = (occ, e) => Math.min(1, e / Math.max(1, this.lines[occ].s.length));
    if (fwd && fwd.locked) {
      // Crossing into another block is _maybeAdvance's job (dwell, skip limit);
      // inside the block the cursor just follows.
      if (this.lines[fwd.occ].blockIdx === this.blockIdx && fwd.occ >= this.nextLine) this._moveCursor(fwd.occ, progress(fwd.occ, fwd.e));
      else if (fwd.occ === this.nextLine - 1) this.pos = Math.max(this.pos, this.lines[fwd.occ].wstart + progress(fwd.occ, fwd.e) * this.lines[fwd.occ].wlen);
      this.confidence = this.confidence * 0.5 + 0.92 * 0.5;
    } else if (fwd && fwd.soft) {
      if (this.lines[fwd.occ].blockIdx === this.blockIdx && fwd.occ >= this.nextLine) this._moveCursor(fwd.occ, progress(fwd.occ, fwd.e));
      this.confidence = this.confidence * 0.72 + 0.5 * 0.28;
    } else {
      this.confidence *= 0.8; // nothing lined up — decay toward freeze
    }

    // Rate: from the audio time the matched words spanned, else wall clock.
    if (this.pos > prevPos && fwd) {
      let inst = 0;
      const m = fwd.a.win.m;
      if (heardTimes && heardTimes.length >= m) {
        const startT = heardTimes[heardTimes.length - m].start;
        const endT = heardTimes[heardTimes.length - 1].end;
        if (startT != null && endT != null && endT - startT > 0.15) inst = m / (endT - startT);
      } else if (this._lastMatchAt) {
        const dt = (now - this._lastMatchAt) / 1000;
        if (dt > 0.15) inst = Math.min(this.pos - prevPos, m) / dt;
      }
      if (inst > 0 && inst < 12) this.rate = this.rate * 0.7 + inst * 0.3;
      this._lastMatchAt = now;
    }

    const moved0 = this.nextLine, block0 = this.blockIdx;
    this._maybeAdvance(now, fwd);
    if (!(fwd && fwd.locked)) {
      // A jump needs words heard SINCE the cursor last moved: the tail still
      // holds the line just sung, which must not pull the cursor back to it.
      const fresh = Math.min(heard.length, this._clock - this._movedAt);
      const recWins = fresh >= 3 ? this._windows(heard.slice(-fresh)) : [];
      if (recWins.length) this._recover(recWins, fwd, isFinal, now);
    }
    if (this.nextLine !== moved0 || this.blockIdx !== block0 || (fwd && this.pos !== prevPos)) this._movedAt = this._clock;
    this._emit(now);
  };

  // Block-level decisions from the verdict on the best forward line.
  LyricsFollower.prototype._maybeAdvance = function (now, fwd) {
    const cfg = this.cfg;
    const b = this.blocks[this.blockIdx];
    const next = this.blocks[this.blockIdx + 1];
    if (!b || !next || !fwd || !fwd.locked) return;
    if (!this._dwellOk(now)) return;

    // CATCH-UP: a locked match on a line of a later block (the next one, or
    // as many further as maxBlockSkip allows). Independent, self-contained
    // evidence — it can override a frozen confidence, like resync() does.
    const ln = this.lines[fwd.occ];
    if (ln.blockIdx > this.blockIdx && ln.blockIdx <= this.blockIdx + 1 + cfg.maxBlockSkip) {
      this._moveCursor(fwd.occ, fwd.e / Math.max(1, ln.s.length));
      this._lastMatchAt = now;
      this._advanceTo(ln.blockIdx, now, 'catchup');
      return;
    }

    // ANTICIPATION: the final line of the block is locked and its last words
    // have just been heard — flip as the last word lands.
    if (!cfg.anticipate || this._noAnticipate) return;
    const lastLine = b.l1 - 1;
    if (ln.idx !== lastLine || lastLine < b.l0) return;
    // Only into a block not yet sung: after a repeat (a chorus sung again) the
    // deck's next slide is not necessarily the next thing sung, so wait to hear it.
    if (this.visited.slice(next.l0, next.l1).some(Boolean)) return;
    const L = ln.s.length;
    const lastTok = ln.toks[ln.toks.length - 1].length + (ln.toks.length > 1 ? ln.toks[ln.toks.length - 2].length : 0);
    const tailChars = Math.max(3, Math.min(lastTok, Math.floor(L * 0.4)));
    if (fwd.e < L - tailChars) return;
    this.armed = true;
    if (this.confidence < cfg.confAdvance) return;
    this._advanceTo(this.blockIdx + 1, now, 'anticipate');
  };

  const API = { LyricsFollower, flattenSong, key, keyMatch, tokenizeKeys, tokenizeKeysWithTimes, DEFAULTS };

  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoLyricsFollow = API;

})(typeof window !== 'undefined' ? window : null);
