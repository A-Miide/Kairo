// KAIRO — sermon slides: a sermon document made into a designed, editable deck (Phase 1, v1).
//
// docs/handover/slides-phase1-spec.md is the brief. In short:
//   - The design is made once, by hand, here: five layouts and six colour sets ("looks"). Kairo only
//     fills them with the sermon's words. It never invents a layout, a colour, a font or a word.
//   - The preacher's words, never Kairo's. A long point is cut at a natural break, marked
//     "shortened" for review, and its whole sentence kept in the slide's notes.
//   - Scripture appears as reference chips written out the same way ("Romans 4:16–21"); the verses
//     go up live from Kairo's Bible when they're read.
//   - Restraint: one accent colour (label, rule, chips), two fonts, 5% safe margins.
//   - Every slide is made of the slide editor's own layers (text and shapes), so it is edited exactly
//     like one built by hand: retype, restyle, move.
//
// Reading: a Word file (.docx is a zip of XML; read here, in the page) or pasted text becomes an
// outline: header lines, points (list paragraphs) with the references after their dash, the typed
// verses under them (kept out of the slides), short lines between parts (sections), a last short
// line (the closing). Recognised from layout, not from any one document's words.
(function () {
  'use strict';

  const W = 1920, H = 1080;
  const M = 120;                       // left/right margin (over the 5% safe area)
  const MIN_HEADLINE_PX = 60;          // readable from the back row on a 1080p slide
  const POINT_WORDS = 12;              // a point line aims for this many words or fewer

  // ── the looks: colour sets made by hand ─────────────────────────────────────────────────────────
  // Each is one colour world: a deep background and a second tone of the same hue for gradients,
  // one text colour and one accent, a display face and a reading face. Colours are chosen so text
  // clears 4.5:1 and the accent 3:1 on both background tones (checked by kitWarnings and the tests).
  const KITS = [
    { id: 'ember', name: 'Ember', bg: '#160A08', bg2: '#6B2313', text: '#FBF1E6', accent: '#F4A85F',
      display: { family: 'Montserrat', weight: 800, width: 0.62 }, body: { family: 'Inter' } },
    { id: 'midnight', name: 'Midnight', bg: '#090C21', bg2: '#2A2878', text: '#EEF0FF', accent: '#9CCBFF',
      display: { family: 'Playfair Display', weight: 700, width: 0.52 }, body: { family: 'Inter' } },
    { id: 'evergreen', name: 'Evergreen', bg: '#0A1712', bg2: '#22503A', text: '#F2EFE3', accent: '#DCC27A',
      display: { family: 'EB Garamond', weight: 600, width: 0.46 }, body: { family: 'Manrope' } },
    { id: 'lagoon', name: 'Lagoon', bg: '#041B21', bg2: '#0C5560', text: '#EAFAF7', accent: '#F3CA6E',
      display: { family: 'Raleway', weight: 800, width: 0.58 }, body: { family: 'Inter' } },
    { id: 'graphite', name: 'Graphite', bg: '#0E0E10', bg2: '#2C2C31', text: '#FFFFFF', accent: '#FF8A65',
      display: { family: 'Archivo Black', weight: 400, width: 0.66 }, body: { family: 'Inter' } },
    { id: 'linen', name: 'Linen', bg: '#F5EEE4', bg2: '#E8CDB7', text: '#2A1C15', accent: '#A9432A',
      display: { family: 'Cormorant Garamond', weight: 700, width: 0.44 }, body: { family: 'Manrope' } },
    // From the owner's reference boards (their colour stories, Nov 2026).
    { id: 'rose', name: 'Rose Dusk', bg: '#1C0920', bg2: '#70294A', text: '#FFF1EC', accent: '#EEAB85',
      display: { family: 'Fraunces', weight: 700, width: 0.55 }, body: { family: 'Inter' } },
    { id: 'crimson', name: 'Crimson Night', bg: '#0F1424', bg2: '#5E1730', text: '#FFF4F1', accent: '#F09A84',
      display: { family: 'Bebas Neue', weight: 400, width: 0.42 }, body: { family: 'Inter' } },
    { id: 'mint', name: 'Forest Mint', bg: '#06201C', bg2: '#204B40', text: '#ECF7EE', accent: '#9FD1B0',
      display: { family: 'DM Serif Display', weight: 400, width: 0.5 }, body: { family: 'Manrope' } },
    { id: 'plum', name: 'Plum Peach', bg: '#140828', bg2: '#4F266A', text: '#FBF0FF', accent: '#ECAD70',
      display: { family: 'Poppins', weight: 800, width: 0.62 }, body: { family: 'Inter' } },
    { id: 'ocean', name: 'Deep Ocean', bg: '#03071A', bg2: '#0F3065', text: '#EAF3FF', accent: '#6AAEEA',
      display: { family: 'Space Grotesk', weight: 700, width: 0.58 }, body: { family: 'Inter' } },
    { id: 'sky', name: 'Sky', bg: '#061D29', bg2: '#1C495D', text: '#F1FAFE', accent: '#7CC6E6',
      display: { family: 'Montserrat', weight: 800, width: 0.62 }, body: { family: 'Inter' } },
    { id: 'honey', name: 'Honey', bg: '#1F160B', bg2: '#6A4E1B', text: '#FFF6E6', accent: '#F1C76A',
      display: { family: 'Fraunces', weight: 600, width: 0.53 }, body: { family: 'Manrope' } },
    { id: 'lavender', name: 'Lavender', bg: '#F1E5F6', bg2: '#D9BFE6', text: '#2A1238', accent: '#7B3A93',
      display: { family: 'Poppins', weight: 700, width: 0.6 }, body: { family: 'Inter' } },
  ];
  // How the background is drawn from a look's two tones. All quiet; none adds anything to look at.
  const BACKGROUNDS = [
    { id: 'glow', name: 'Glow' },        // the second tone as a soft light from the top-left corner
    { id: 'gradient', name: 'Gradient' }, // the two tones, corner to corner
    { id: 'solid', name: 'Solid' },
  ];
  // Where the words sit: at the left with the point's number quiet on the right; centred; or in a
  // band along the bottom over whatever is behind (a camera, a video).
  const LAYOUTS = [
    { id: 'left', name: 'Left' },
    { id: 'centred', name: 'Centred' },
    { id: 'lower', name: 'Lower third' },
    { id: 'split', name: 'Split' },
    { id: 'editorial', name: 'Editorial' },
  ];
  const layoutOf = (id) => (LAYOUTS.some(l => l.id === id) ? id : 'left');
  const kitById = (id) => KITS.find(k => k.id === id) || KITS[0];
  // A look with the operator's own colours over it: { bg, bg2, text, accent }, any of them.
  function withColors(kit, colors) {
    const own = Object.fromEntries(Object.entries(colors || {}).filter(([k, v]) => ['bg', 'bg2', 'text', 'accent'].includes(k) && /^#[0-9a-f]{6}$/i.test(v)));
    return Object.keys(own).length ? { ...kit, ...own } : kit;
  }

  // ── colour ──────────────────────────────────────────────────────────────────────────────────────
  const HEX = /^#?([0-9a-f]{6})$/i;
  const rgb = (hex) => { const m = HEX.exec(String(hex || '')); const n = m ? parseInt(m[1], 16) : 0; return [(n >> 16 & 255) / 255, (n >> 8 & 255) / 255, (n & 255) / 255]; };
  const luminance = (hex) => { const f = (c) => (c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)); const [r, g, b] = rgb(hex).map(f); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const contrast = (a, b) => { const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

  function kitWarnings(kit) {
    const out = [];
    for (const bg of [kit.bg, kit.bg2]) {
      if (contrast(kit.text, bg) < 4.5) { out.push('The text colour is hard to read on the background.'); break; }
    }
    for (const bg of [kit.bg, kit.bg2]) {
      if (contrast(kit.accent, bg) < 3) { out.push('The accent colour is hard to see on the background.'); break; }
    }
    return out;
  }

  // ── books and references ────────────────────────────────────────────────────────────────────────
  const BOOKS = ['Genesis', 'Exodus', 'Leviticus', 'Numbers', 'Deuteronomy', 'Joshua', 'Judges', 'Ruth', '1 Samuel', '2 Samuel',
    '1 Kings', '2 Kings', '1 Chronicles', '2 Chronicles', 'Ezra', 'Nehemiah', 'Esther', 'Job', 'Psalms', 'Proverbs', 'Ecclesiastes',
    'Song of Solomon', 'Isaiah', 'Jeremiah', 'Lamentations', 'Ezekiel', 'Daniel', 'Hosea', 'Joel', 'Amos', 'Obadiah', 'Jonah', 'Micah',
    'Nahum', 'Habakkuk', 'Zephaniah', 'Haggai', 'Zechariah', 'Malachi', 'Matthew', 'Mark', 'Luke', 'John', 'Acts', 'Romans',
    '1 Corinthians', '2 Corinthians', 'Galatians', 'Ephesians', 'Philippians', 'Colossians', '1 Thessalonians', '2 Thessalonians',
    '1 Timothy', '2 Timothy', 'Titus', 'Philemon', 'Hebrews', 'James', '1 Peter', '2 Peter', '1 John', '2 John', '3 John', 'Jude', 'Revelation'];
  // Short forms a prefix or the consonants can't settle on their own.
  const ALIASES = { jn: 'John', jhn: 'John', joh: 'John', mk: 'Mark', mak: 'Mark', mrk: 'Mark', lk: 'Luke', luk: 'Luke', mt: 'Matthew',
    mat: 'Matthew', ps: 'Psalms', psa: 'Psalms', pss: 'Psalms', psm: 'Psalms', pr: 'Proverbs', prv: 'Proverbs', pro: 'Proverbs', jud: 'Judges',
    jdg: 'Judges', judg: 'Judges', phil: 'Philippians', php: 'Philippians', phl: 'Philippians', phm: 'Philemon', philem: 'Philemon',
    song: 'Song of Solomon', sos: 'Song of Solomon', sng: 'Song of Solomon', ss: 'Song of Solomon', ecc: 'Ecclesiastes', eccl: 'Ecclesiastes',
    qoh: 'Ecclesiastes', jas: 'James', jam: 'James', jos: 'Joshua', jsh: 'Joshua', jon: 'Jonah', jnh: 'Jonah', jer: 'Jeremiah', jr: 'Jeremiah',
    ez: 'Ezekiel', eze: 'Ezekiel', ezk: 'Ezekiel', hab: 'Habakkuk', zep: 'Zephaniah', zeph: 'Zephaniah', zec: 'Zechariah', zech: 'Zechariah',
    mal: 'Malachi', am: 'Amos', ob: 'Obadiah', na: 'Nahum', nah: 'Nahum', hag: 'Haggai', ge: 'Genesis', gn: 'Genesis', ex: 'Exodus',
    lv: 'Leviticus', nu: 'Numbers', nm: 'Numbers', dt: 'Deuteronomy', ru: 'Ruth', ne: 'Nehemiah', es: 'Esther', est: 'Esther',
    is: 'Isaiah', isa: 'Isaiah', la: 'Lamentations', lam: 'Lamentations', da: 'Daniel', dn: 'Daniel', ho: 'Hosea', jl: 'Joel',
    mi: 'Micah', mic: 'Micah', ac: 'Acts', act: 'Acts', ro: 'Romans', rm: 'Romans', rom: 'Romans', ga: 'Galatians', gal: 'Galatians',
    eph: 'Ephesians', col: 'Colossians', ti: 'Titus', tit: 'Titus', he: 'Hebrews', heb: 'Hebrews', re: 'Revelation', rev: 'Revelation',
    rv: 'Revelation', co: 'Corinthians', cor: 'Corinthians', th: 'Thessalonians', thes: 'Thessalonians', thess: 'Thessalonians',
    ti_: 'Timothy', tim: 'Timothy', pe: 'Peter', pet: 'Peter', sa: 'Samuel', sam: 'Samuel', ki: 'Kings', kgs: 'Kings', ch: 'Chronicles',
    chr: 'Chronicles', chron: 'Chronicles' };
  const consonants = (s) => s.replace(/[aeiou]/g, '');
  // A book as written in a document ("Rom.", "Jhn", "1 Cor", "Judg") to its full name, or null.
  function bookName(raw) {
    const m = /^([1-3])?\s*([A-Za-z]+(?:\s+of\s+[A-Za-z]+)?)\.?$/.exec(String(raw || '').trim());
    if (!m) return null;
    const num = m[1] || '';
    const word = m[2].toLowerCase().replace(/\s+/g, ' ');
    const named = (name) => {
      if (!name) return null;
      const full = num ? `${num} ${name}` : name;
      return BOOKS.includes(full) ? full : (!num && BOOKS.includes(name) ? name : null);
    };
    if (ALIASES[word]) return named(ALIASES[word]);
    const pool = BOOKS.filter(b => /^[1-3] /.test(b) === !!num).map(b => b.replace(/^[1-3] /, ''));
    const lower = [...new Set(pool)];
    const exact = lower.find(b => b.toLowerCase() === word);
    if (exact) return named(exact);
    const prefix = lower.filter(b => b.toLowerCase().startsWith(word));
    if (word.length >= 2 && prefix.length === 1) return named(prefix[0]);
    const skel = lower.filter(b => consonants(b.toLowerCase().replace(/ /g, '')).startsWith(consonants(word)));
    if (word.length >= 3 && skel.length === 1) return named(skel[0]);
    return null;
  }
  // One reference as written ("Rom. 4:16-21", "Judg.6:1-16", "Isa. 8:18 –", "Mak. 16:15-16, 20") to
  // the way every chip is written ("Romans 4:16–21"); null when it isn't one.
  function normaliseRef(raw) {
    const s = String(raw || '').replace(/\[[^\]]*\]/g, '').replace(/[–—-]\s*$/, '').trim();
    const m = /^([1-3]?\s*[A-Za-z]+(?:\s+of\s+[A-Za-z]+)?)\.?\s*(\d+)(?:\s*:\s*(\d+(?:\s*[-–]\s*\d+)?(?:\s*,\s*\d+(?:\s*[-–]\s*\d+)?)*))?/.exec(s);
    if (!m) return null;
    let book = bookName(m[1]);
    if (!book) return null;
    const chapter = m[2];
    const verses = m[3] ? m[3].replace(/\s*[-–]\s*/g, '–').replace(/\s*,\s*/g, ', ') : '';
    if (book === 'Psalms' && !/[–,]/.test(verses) && verses) book = 'Psalm';
    if (book === 'Psalms' && !verses) book = 'Psalm';
    return verses ? `${book} ${chapter}:${verses}` : `${book} ${chapter}`;
  }

  // ── reading a document ──────────────────────────────────────────────────────────────────────────
  const wordCount = (t) => String(t || '').trim().split(/\s+/).filter(Boolean).length;
  const decodeXml = (s) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'")
    .replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(+n)).replace(/&#x([0-9a-f]+);/gi, (_, n) => String.fromCodePoint(parseInt(n, 16))).replace(/&amp;/g, '&');

  // The paragraphs of a Word document's XML: their words, and whether Word lists them.
  function docxParagraphs(xml) {
    const out = [];
    const P = /<w:p\b[^>]*?(?:\/>|>([\s\S]*?)<\/w:p>)/g;
    let m;
    while ((m = P.exec(xml))) {
      const body = m[1] || '';
      const style = (/<w:pStyle\b[^>]*w:val="([^"]+)"/.exec(body) || [])[1] || '';
      const text = body
        .replace(/<w:tab\/>/g, ' ').replace(/<w:br\/>/g, ' ')
        .replace(/<w:t(?:\s[^>]*)?>([\s\S]*?)<\/w:t>|<[^>]+>/g, (_, t) => (t != null ? decodeXml(t) : ''))
        .replace(/\s+/g, ' ').trim();
      if (text) out.push({ text, list: /^listparagraph/i.test(style) || body.includes('<w:numPr>') });
    }
    return out;
  }

  // Pasted text: a line starting with "1." "2)" "•" "-" is a list paragraph (its marker taken off).
  function textParagraphs(text) {
    return String(text || '').replace(/\r/g, '').split(/\n/).map(l => l.trim()).filter(Boolean).map(l => {
      const m = /^(?:\d{1,2}[.)]|[•●▪◦*–-])\s+(.*)$/.exec(l);
      return m ? { text: m[1].trim(), list: true } : { text: l, list: false };
    });
  }

  // The references written after a point's words ("Rom. 4:16-21/Heb. 11:17-19 / Gen. 22:1-18"); a bare
  // verse number after a slash ("Mak. 16:15-16/20") is a verse of the same chapter.
  function refsIn(text) {
    const out = [];
    for (const part of String(text || '').split(/[\/;]/).map(p => p.trim()).filter(Boolean)) {
      if (/^\d+(?:\s*[-–]\s*\d+)?$/.test(part) && out.length) out[out.length - 1] += ', ' + part;
      else out.push(part);
    }
    return out;
  }
  // A statement and the references after its dash: "Daniel had to … – Dan. 6:10-23".
  function splitStatement(text) {
    const m = /\s+[–-]\s*(?:[1-3] ?)?[A-Z][a-z]{1,4}\.? ?\d/.exec(text);
    if (!m) return { statement: text.trim(), refs: [] };
    const dashAt = m.index + m[0].search(/[–-]/);
    return { statement: text.slice(0, m.index).trim(), refs: refsIn(text.slice(dashAt).replace(/^[–\-\s]+/, '')) };
  }
  const isReferenceLine = (text) => wordCount(text) <= 12 && /^(?:[1-3] ?)?[A-Z][A-Za-z]{1,}\.? ?\d+(?::\d+)?/.test(text);

  // The outline of a document's paragraphs, or null when it isn't laid out like one (fewer than
  // three points, or most points without a reference).
  function outlineFrom(paras) {
    const header = [];
    let week = null;
    const items = [];
    let verses = [];
    let seenPoint = false;
    let inNotes = false;
    const other = [];   // lines the reader set aside: still offered, so nothing in the document is lost
    const flush = () => {
      if (!verses.length) return;
      const last = [...items].reverse().find(i => i.kind === 'point');
      if (last) last.verses = verses.map(([ref, t]) => ({ ref, text: t.join(' ') }));
      verses = [];
    };
    for (const p of paras) {
      // A run of digits stuck to the start of a line is a Word field's leftover ("1313180192405Prophetic").
      const text = p.text.trim().replace(/^\d{6,}(?=[A-Za-z])/, '');
      // "NB: …", or a "Note" heading and everything after it: for the preacher, not the screen.
      if (inNotes || (/^nb\b/i.test(text) && text.includes(':'))) { flush(); items.push({ kind: 'note', text }); continue; }
      if (seenPoint && /^(?:note|notes|nb)\s*:?$/i.test(text)) { flush(); inNotes = true; continue; }
      if (!text) continue;
      if (p.list) {
        flush();
        seenPoint = true;
        const { statement, refs } = splitStatement(text);
        if (wordCount(statement) > 80) items.push({ kind: 'story', text: statement });
        else items.push({ kind: 'point', n: items.filter(i => i.kind === 'point').length + 1, text: statement, refs });
        continue;
      }
      if (!seenPoint) {
        if (/^week\s*\d+/i.test(text)) week = text;
        else if (/^introduction\b/i.test(text) || wordCount(text) > 30) {
          if (wordCount(text) > 30) items.push({ kind: 'story', text });
        } else if (header.length < 6) header.push(splitStatement(text));
        else other.push(text);
        continue;
      }
      if (isReferenceLine(text)) verses.push([text, []]);
      else if (/^vs\b/i.test(text) && /\d/.test(text)) other.push(text);   // "VS:10": a verse the preacher stresses
      else if (verses.length && (wordCount(text) > 10 || /^\d/.test(text))) verses[verses.length - 1][1].push(text);
      else if (wordCount(text) <= 10 && !/^\d/.test(text)) { flush(); items.push({ kind: 'section', text: text.replace(/:$/, '') }); }
      else if (verses.length) verses[verses.length - 1][1].push(text);
    }
    flush();
    const last = [...items].reverse().find(i => i.kind !== 'note');
    if (last && last.kind === 'section') last.kind = 'closing';
    const points = items.filter(i => i.kind === 'point');
    if (points.length < 3 || points.filter(p => p.refs.length).length * 2 < points.length) return null;
    return { header: header.map(h => ({ text: h.statement, refs: h.refs })), week, items, other };
  }

  // ── a .docx, read in the page: a zip's word/document.xml ────────────────────────────────────────
  async function inflateRaw(bytes) {
    if (typeof DecompressionStream === 'function') {
      const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
      return new Uint8Array(await new Response(stream).arrayBuffer());
    }
    // eslint-disable-next-line global-require
    return new Uint8Array(require('zlib').inflateRawSync(bytes));
  }
  async function zipEntry(buf, name) {
    const u8 = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
    const dv = new DataView(u8.buffer, u8.byteOffset, u8.byteLength);
    let eocd = -1;
    for (let i = u8.length - 22; i >= Math.max(0, u8.length - 66000); i--) if (dv.getUint32(i, true) === 0x06054b50) { eocd = i; break; }
    if (eocd < 0) return null;
    let at = dv.getUint32(eocd + 16, true);
    const count = dv.getUint16(eocd + 10, true);
    for (let n = 0; n < count && dv.getUint32(at, true) === 0x02014b50; n++) {
      const method = dv.getUint16(at + 10, true), size = dv.getUint32(at + 20, true);
      const nameLen = dv.getUint16(at + 28, true), extraLen = dv.getUint16(at + 30, true), commentLen = dv.getUint16(at + 32, true);
      const local = dv.getUint32(at + 42, true);
      const entry = new TextDecoder().decode(u8.subarray(at + 46, at + 46 + nameLen));
      if (entry === name) {
        const start = local + 30 + dv.getUint16(local + 26, true) + dv.getUint16(local + 28, true);
        const data = u8.subarray(start, start + size);
        return method === 0 ? data : inflateRaw(data);
      }
      at += 46 + nameLen + extraLen + commentLen;
    }
    return null;
  }
  async function readDocx(buf) {
    try {
      const xml = await zipEntry(buf, 'word/document.xml');
      return xml ? outlineFrom(docxParagraphs(new TextDecoder().decode(xml))) : null;
    } catch { return null; }
  }
  const readText = (text) => outlineFrom(textParagraphs(text));

  // ── words on a slide ────────────────────────────────────────────────────────────────────────────
  const STOP = new Set('the a an of to and as with from for that who which in on at is are was were be been had have has his her their our your my i we you they it this these those not only also but or so than then when while into by up out over all one every each whole even most there here just very really some what will would could should must can may through about after before because being does done him them its itself himself herself themselves us me'.split(' '));
  // A long point cut at its first natural break that leaves a whole thought of 12 words or fewer.
  // Never reworded: the words kept are the preacher's, in order. Returns { text, shortened }.
  function shortenPoint(text) {
    const t = String(text || '').trim();
    if (wordCount(t) <= POINT_WORDS) return { text: t, shortened: false };
    // A cut must leave a whole thought: six words or more, not ending on a small word ("of", "the").
    const whole = (i) => {
      const kept = t.slice(0, i).replace(/[,;:–—\s]+$/, '');
      const n = wordCount(kept);
      const last = (kept.match(/[A-Za-z’']+$/) || [''])[0].toLowerCase();
      return n >= 6 && n <= POINT_WORDS && !STOP.has(last);
    };
    const at = (re) => { const found = []; let m; while ((m = re.exec(t))) if (whole(m.index)) found.push(m.index); return found.length ? found[found.length - 1] : -1; };
    // Punctuation first; then before a joining word.
    let cut = at(/[,;:–—](?=\s)/g);
    if (cut < 0) cut = at(/\s(?=(?:to|as|with|among|and|but|from|that|who|which|including|because|while|when|so) )/g);
    if (cut < 0) return { text: t, shortened: false };
    return { text: t.slice(0, cut).replace(/[,;:–—\s]+$/, ''), shortened: true };
  }

  // ── layers ──────────────────────────────────────────────────────────────────────────────────────
  // The slide editor's own layers (the ones a theme is made of): a text layer of custom text with its
  // own font, size and colour, and shapes (background layers with a box). So a sermon slide is
  // edited exactly like one built by hand: retype, restyle, move; nothing is drawn again over it.
  const hex2 = (n) => Math.round(n).toString(16).padStart(2, '0');
  const withAlpha = (hex, a) => `#${String(hex).replace('#', '').slice(0, 6)}${hex2(a * 255)}`;
  function textLayer(id, name, pos, text, o) {
    return {
      id, type: 'text', name, visible: true, binding: 'custom', customText: text, pos,
      font: { family: o.family, size: o.size, weight: o.weight || 400, italic: false, lineHeight: o.lineHeight || 1.15, letterSpacing: o.spacing || 0, transform: o.transform || 'none' },
      color: o.color, opacity: o.opacity || 100, align: o.align || 'left',
      shadow: { enabled: false, color: '#000000', opacity: 0, blur: 0, x: 0, y: 0 },
      outline: { enabled: false, color: '#000000', width: 0 },
      webFont: true,
    };
  }
  function shapeLayer(id, name, pos, color, o = {}) {
    return { id, type: 'background', name, visible: true, fill: 'solid', color, color2: color, opacity: o.opacity || 100, angle: 0, radius: o.radius || 0, pos };
  }
  function backgroundLayers(kit, style) {
    // A full-size Background layer (not the canvas itself): its fill, colours and gradient are set
    // under Style like any slide's background.
    const canvas = { id: 'ss-bg', type: 'background', name: 'Background', visible: true, opacity: 100, angle: 135, radius: 0, pos: { x: 0, y: 0, w: W, h: H } };
    if (style === 'gradient') {
      return [{ ...canvas, fill: 'gradient', color: kit.bg2, color2: kit.bg, gradientStops: [{ pos: 0, color: kit.bg2 }, { pos: 55, color: kit.bg }, { pos: 100, color: kit.bg }] }];
    }
    if (style === 'glow') {
      // The second tone as a soft light from the top-left corner: the theme gradient's own glow.
      return [{ ...canvas, fill: 'gradient', color: kit.bg, color2: kit.bg, gradientStops: [{ pos: 0, color: kit.bg }, { pos: 100, color: kit.bg }],
        gradientGlows: [{ x: 4, y: 0, rx: 62, ry: 95, color: kit.bg2 }] }];
    }
    return [{ ...canvas, fill: 'solid', color: kit.bg, color2: kit.bg }];
  }

  // The scripture chips: a tinted pill each, the reference on it in the reading face.
  function chipLayers(kit, refs, x, y, align) {
    const size = 32, h = 62, padX = 30, gap = 18;
    const chips = [];
    let used = 0;
    for (const ref of refs) {
      const w = Math.round(ref.length * size * 0.54 + 2 * padX);
      if (used + w > W - 2 * M) break;
      chips.push({ ref, w });
      used += w + gap;
    }
    used -= gap;
    let cx = align === 'center' ? Math.round((W - used) / 2) : align === 'right' ? W - M - used : x;
    const out = [];
    chips.forEach((c, i) => {
      out.push(shapeLayer(`ss-chip-${i}`, `Scripture ${i + 1}`, { x: cx, y, w: c.w, h }, kit.accent, { radius: h / 2, opacity: 22 }));
      out.push(textLayer(`ss-chip-${i}-text`, `Scripture ${i + 1} text`, { x: cx, y: y + 12, w: c.w, h: h - 18 }, c.ref,
        { family: kit.body.family, size, weight: 500, color: kit.text, align: 'center', lineHeight: 1.1 }));
      cx += c.w + gap;
    });
    return out;
  }
  const labelLayer = (kit, label, pos, align) => textLayer('ss-label', 'Label', pos, String(label),
    { family: kit.body.family, size: 30, weight: 600, color: kit.accent, align, spacing: 6, transform: 'uppercase' });
  const ruleLayer = (kit, x, y, w) => shapeLayer('ss-rule', 'Rule', { x, y, w, h: 5 }, kit.accent);
  const footerLayer = (kit, footer, pos = { x: M, y: 990, w: 1100, h: 36 }, align = 'left') => !footer ? [] : [textLayer('ss-footer', 'Footer', pos, String(footer),
    { family: kit.body.family, size: 22, color: kit.text, opacity: 55, spacing: 5, transform: 'uppercase', align })];
  const display = (kit, size, color, align, o = {}) => ({ family: kit.display.family, size, weight: kit.display.weight, color, align, lineHeight: o.lineHeight || 1.08 });
  // The largest size up to `size` at which `text` fits a box `w` × `h` in the look's display face
  // (an estimate from the face's average letter width; capitals run wider). A plain text layer
  // doesn't shrink itself, so the slide is made at a size that fits. Never below the readable
  // minimum: past that the slide is flagged instead (issues).
  function fitSize(kit, text, w, h, size, lineHeight) {
    const t = String(text || '');
    const caps = t.replace(/[^A-Za-z]/g, '').length && t === t.toUpperCase() ? 1.22 : 1;
    const em = (kit.display.width || 0.55) * caps;
    let s = size;
    const lines = (px) => {
      const perLine = Math.max(1, Math.floor(w / (px * em)));
      let n = 0, cur = 0;
      for (const word of t.split(/\s+/)) { const len = word.length + (cur ? 1 : 0); if (cur && cur + len > perLine) { n += 1; cur = word.length; } else cur += len; }
      return n + 1;
    };
    while (s > MIN_HEADLINE_PX && lines(s) * s * lineHeight > h) s -= 4;
    return Math.max(MIN_HEADLINE_PX, s);
  }
  // A headline layer at the size that fits its box; `gen.tooLong` when even the minimum doesn't.
  function headline(kit, gen, pos, text, size, color, align, lineHeight) {
    const px = fitSize(kit, text, pos.w, pos.h, size, lineHeight);
    if (px === MIN_HEADLINE_PX && fitSize(kit, text, pos.w, pos.h, MIN_HEADLINE_PX + 4, lineHeight) === MIN_HEADLINE_PX) {
      const em = (kit.display.width || 0.55);
      const perLine = Math.floor(pos.w / (px * em));
      if (Math.ceil(text.length / Math.max(1, perLine)) * px * lineHeight > pos.h) gen.tooLong = true;
    }
    return textLayer('ss-headline', 'Headline', pos, text, display(kit, px, color, align, { lineHeight }));
  }

  // ── the five layouts ────────────────────────────────────────────────────────────────────────────
  function layersFor(gen, kit, style, footer) {
    const layout = layoutOf(gen.layout);
    if (layout === 'lower') return lowerThird(gen, kit, style, footer);
    if (layout === 'split') return split(gen, kit, style, footer);
    if (layout === 'editorial') return editorial(gen, kit, style, footer);
    const L = backgroundLayers(kit, style);
    const refs = gen.refs || [];
    const lines = (gen.lines || []).filter(Boolean);
    if (layout === 'centred' && (gen.kind === 'point' || gen.kind === 'story')) {
      if (gen.kind === 'point' && gen.n) L.push(textLayer('ss-number', 'Number', { x: (W - 740) / 2, y: 190, w: 740, h: 640 }, String(gen.n).padStart(2, '0'),
        { ...display(kit, 520, kit.text, 'center', { lineHeight: 1.0 }), opacity: 7 }));
      if (gen.kind === 'story') L.push(ruleLayer(kit, (W - 96) / 2, 300, 96));
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: gen.kind === 'story' ? 330 : 230, w: W - 2 * M, h: 48 }, 'center'));
      if (gen.kind === 'point') {
        L.push(headline(kit, gen, { x: 200, y: 300, w: W - 400, h: 500 }, lines.join(' '), 104, kit.text, 'center', 1.08));
        L.push(...chipLayers(kit, refs, 0, 840, 'center'));
      } else {
        L.push(headline(kit, gen, { x: 240, y: 400, w: W - 480, h: 460 }, lines.join(' '), 76, kit.text, 'center', 1.2));
      }
      L.push(...footerLayer(kit, footer, { x: M, y: 990, w: W - 2 * M, h: 36 }, 'center'));
      return L;
    }
    if (gen.kind === 'title') {
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: 250, w: W - 2 * M, h: 50 }, 'center'));
      const [head, ...rest] = lines;
      L.push(headline(kit, gen, { x: 200, y: 320, w: W - 400, h: 330 }, head || '', 132, kit.text, 'center', 1.0));
      if (rest.length) L.push(textLayer('ss-subtitle', 'Subtitle', { x: 200, y: 670, w: W - 400, h: 110 }, rest.join(' '),
        { family: kit.body.family, size: 44, color: kit.text, opacity: 82, align: 'center' }));
      L.push(...chipLayers(kit, refs, 0, 830, 'center'));
    } else if (gen.kind === 'point') {
      // The point at the left; its number, very large and quiet, balances the right.
      if (gen.n) L.push(textLayer('ss-number', 'Number', { x: 1080, y: 250, w: 740, h: 580 }, String(gen.n).padStart(2, '0'),
        { ...display(kit, 480, kit.text, 'right', { lineHeight: 1.0 }), opacity: 7 }));
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: 230, w: 900, h: 48 }, 'left'));
      L.push(headline(kit, gen, { x: M, y: 300, w: 1180, h: 500 }, lines.join(' '), 104, kit.text, 'left', 1.08));
      L.push(...chipLayers(kit, refs, M, 840, 'left'));
    } else if (gen.kind === 'story') {
      L.push(ruleLayer(kit, M, 300, 96));
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: 330, w: 900, h: 48 }, 'left'));
      L.push(headline(kit, gen, { x: M, y: 400, w: 1380, h: 460 }, lines.join(' '), 76, kit.text, 'left', 1.2));
    } else {
      // section / closing: one line, centred, with a short accent rule above
      L.push(ruleLayer(kit, (W - 120) / 2, 340, 120));
      L.push(headline(kit, gen, { x: 200, y: 380, w: W - 400, h: 400 }, lines.join(' '), gen.kind === 'closing' ? 120 : 128, gen.kind === 'closing' ? kit.accent : kit.text, 'center', 1.0));
    }
    L.push(...footerLayer(kit, footer));
    return L;
  }

  // The words in a band along the bottom third; above it the slide is clear, so what is behind
  // (the camera, a video) shows. Every kind of slide takes the same band.
  function lowerThird(gen, kit, style, footer) {
    const top = 700;
    const L = backgroundLayers(kit, style).map(b => ({ ...b, pos: { x: 0, y: top, w: W, h: H - top }, opacity: 94 }));
    const refs = gen.refs || [];
    const lines = (gen.lines || []).filter(Boolean);
    if (gen.kind === 'section' || gen.kind === 'closing') L.push(ruleLayer(kit, M, top + 60, 96));
    else if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: top + 52, w: 900, h: 44 }, 'left'));
    L.push(...chipLayers(kit, refs.slice(0, 3), 0, top + 42, 'right'));
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    L.push(headline(kit, gen, { x: M, y: top + 112, w: W - 2 * M, h: 196 }, text, gen.kind === 'story' ? 60 : 76,
      gen.kind === 'closing' ? kit.accent : kit.text, 'left', 1.1));
    L.push(...footerLayer(kit, footer, { x: M, y: H - 52, w: 1100, h: 32 }));
    return L;
  }

  // A panel of the second tone down the left third holds the number (or the label); the words sit
  // beside it on the background.
  function split(gen, kit, style, footer) {
    const P = 640;                                   // the panel's width
    const L = backgroundLayers(kit, style);
    L.push(shapeLayer('ss-panel', 'Panel', { x: 0, y: 0, w: P, h: H }, kit.bg2));
    const refs = gen.refs || [];
    const lines = (gen.lines || []).filter(Boolean);
    const X = P + 110, TW = W - X - M;
    if (gen.kind === 'point' && gen.n) {
      L.push(textLayer('ss-number', 'Number', { x: 0, y: 300, w: P, h: 420 }, String(gen.n).padStart(2, '0'),
        { ...display(kit, 360, kit.text, 'center', { lineHeight: 1.0 }), opacity: 92 }));
    } else if (gen.label) {
      L.push(labelLayer(kit, gen.label, { x: 80, y: 500, w: P - 160, h: 48 }, 'center'));
    }
    if (gen.kind === 'point' && gen.label) L.push(labelLayer(kit, gen.label, { x: X, y: 250, w: TW, h: 48 }, 'left'));
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const size = gen.kind === 'story' ? 64 : 96;
    L.push(headline(kit, gen, { x: X, y: 320, w: TW, h: 440 }, text, size, gen.kind === 'closing' ? kit.accent : kit.text, 'left', 1.08));
    if (gen.kind === 'title' && lines[1]) L.push(textLayer('ss-subtitle', 'Subtitle', { x: X, y: 780, w: TW, h: 60 }, lines.slice(1).join(' '),
      { family: kit.body.family, size: 40, color: kit.text, opacity: 82, align: 'left' }));
    L.push(...chipLayers(kit, refs.slice(0, 2), X, 850, 'left'));
    L.push(...footerLayer(kit, footer, { x: X, y: 990, w: TW, h: 36 }));
    return L;
  }

  // A magazine page: the number large in the top corner in the accent, the label opposite, then the
  // headline set low and wide over a long fine rule, the scriptures under it.
  function editorial(gen, kit, style, footer) {
    const L = backgroundLayers(kit, style);
    const refs = gen.refs || [];
    const lines = (gen.lines || []).filter(Boolean);
    if (gen.kind === 'point' && gen.n) L.push(textLayer('ss-number', 'Number', { x: M, y: 70, w: 520, h: 250 }, String(gen.n).padStart(2, '0'),
      { ...display(kit, 230, kit.accent, 'left', { lineHeight: 1.0 }), opacity: 100 }));
    if (gen.label) L.push(labelLayer(kit, gen.label, { x: W - M - 900, y: 110, w: 900, h: 48 }, 'right'));
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const size = gen.kind === 'story' ? 64 : 104;
    L.push(headline(kit, gen, { x: M, y: 420, w: W - 2 * M - 160, h: 360 }, text, size, gen.kind === 'closing' ? kit.accent : kit.text, 'left', 1.02));
    L.push({ ...ruleLayer(kit, M, 812, W - 2 * M), opacity: 70, pos: { x: M, y: 812, w: W - 2 * M, h: 3 } });
    if (gen.kind === 'title' && lines[1]) L.push(textLayer('ss-subtitle', 'Subtitle', { x: M, y: 846, w: 1200, h: 56 }, lines.slice(1).join(' '),
      { family: kit.body.family, size: 38, color: kit.text, opacity: 82, align: 'left' }));
    else L.push(...chipLayers(kit, refs.slice(0, 3), M, 846, 'left'));
    L.push(...footerLayer(kit, footer, { x: M, y: 990, w: 1100, h: 36 }));
    return L;
  }

  // ── slides ──────────────────────────────────────────────────────────────────────────────────────
  // What the slide editor shows: the label, then the headline lines, then a 'Scriptures:' line.
  function serialize(gen) {
    return [gen.label, ...(gen.lines || []), (gen.refs || []).length ? 'Scriptures: ' + gen.refs.join(' · ') : ''].filter(Boolean).join('\n');
  }
  function parse(text, gen) {
    const lines = String(text || '').replace(/\r/g, '').split('\n').map(l => l.trim()).filter(Boolean);
    let refs = [];
    if (lines.length && /^scriptures?\s*:/i.test(lines[lines.length - 1])) {
      refs = lines.pop().replace(/^scriptures?\s*:/i, '').split(/\s*[·|;]\s*/).map(r => normaliseRef(r) || r.trim()).filter(Boolean);
    }
    const label = gen.label && lines.length > 1 ? lines.shift() : (gen.label ? gen.label : '');
    return { ...gen, label, lines: lines.length ? lines : gen.lines, refs, shortened: false };
  }
  // The slide's words checked against the rules (fit is checked after drawing: see checkFit).
  function issues(gen, kit) {
    const out = kitWarnings(kit);
    if (gen.shortened) out.push('Shortened: check the cut. The whole sentence is in the notes.');
    if (gen.tooLong) out.push('Too long to fit at a readable size: shorten the words.');
    if ((gen.refs || []).some(r => !normaliseRef(r))) out.push('A scripture reference isn’t recognised.');
    return out;
  }
  function makeBlock(gen, kit, style, footer, extra = {}) {
    const g = { ...gen, kit: kit.id, style, footer };
    return { label: gen.label || (gen.lines || [])[0] || 'Slide', canvasSize: { w: W, h: H }, layers: layersFor(g, kit, style, footer), text: serialize(g), gen: g, ...extra };
  }
  const deckKit = (gen) => kitById(gen && gen.kit);

  // Everything the document holds, in its order, each a part the operator keeps or leaves out.
  // What makes a slide by default is on; notes, typed verses and other lines are off but listed,
  // so whatever the reader set aside can still be put on a slide.
  //   { id, kind: 'title'|'story'|'point'|'section'|'closing'|'note'|'verse'|'line', text, refs, on }
  function partsOf(outline) {
    const header = (outline.header || []).filter(h => h && h.text);
    const themeLine = header.find(h => /theme/i.test(h.text)) || header[1] || header[0];
    const seriesLine = header.find(h => /series|topic|title/i.test(h.text) && h !== themeLine);
    const parts = [];
    header.forEach((h, i) => {
      if (h === themeLine) parts.push({ id: 'title', kind: 'title', text: h.text, refs: [], on: true });
      else if (h !== seriesLine) parts.push({ id: `h${i}`, kind: 'line', text: h.text, refs: h.refs || [], on: false });
    });
    (outline.items || []).forEach((it, i) => {
      parts.push({ id: `i${i}`, kind: it.kind, text: it.text, refs: it.refs || [], n: it.n, on: it.kind !== 'note' });
      (it.verses || []).forEach((v, j) => parts.push({ id: `i${i}v${j}`, kind: 'verse', text: v.text, refs: [v.ref], on: false }));
    });
    (outline.other || []).forEach((t, i) => parts.push({ id: `o${i}`, kind: 'line', text: t, refs: [], on: false }));
    return parts;
  }

  // The deck for an outline: the parts that are on, each in its layout. Typed verses are left out
  // unless picked (the verses go up live from Kairo's Bible); the notes go to the last slide's notes.
  //   opts: { kit, style, footer, include: Set of part ids (default: the parts on by default) }
  function build(outline, opts = {}) {
    const kit = withColors(kitById(opts.kit), opts.colors);
    const style = BACKGROUNDS.some(b => b.id === opts.style) ? opts.style : 'glow';
    const layout = layoutOf(opts.layout);
    const header = (outline.header || []).filter(h => h && h.text);
    const footer = opts.footer != null ? opts.footer : (header[0] ? header[0].text.replace(/\.?\s*inc\.?$/i, '').trim() : '');
    const parts = partsOf(outline);
    const on = (p) => (opts.include ? opts.include.has(p.id) : p.on);
    const blocks = [];
    const afterColon = (t) => String(t || '').replace(/^[^:]{1,40}:\s*/, '').trim();
    const themeLine = header.find(h => /theme/i.test(h.text)) || header[1] || header[0];
    const seriesLine = header.find(h => /series|topic|title/i.test(h.text) && h !== themeLine);
    const refsOf = (list) => [...new Set(list.map(r => normaliseRef(r) || null).filter(Boolean))];
    const opening = (text, words) => { const first = String(text).split(/(?<=[.!?])\s+/)[0]; return wordCount(first) > words ? first.split(/\s+/).slice(0, words).join(' ') + '…' : first; };
    const total = parts.filter(p => p.kind === 'point' && on(p)).length;
    let n = 0;
    for (const p of parts) {
      if (!on(p)) continue;
      const make = (gen, extra) => blocks.push(makeBlock({ ...gen, part: p.id, layout }, kit, style, footer, extra));
      if (p.kind === 'title') {
        const head = /:/.test(themeLine.text) ? afterColon(themeLine.text) : themeLine.text;
        const sub = seriesLine ? afterColon(seriesLine.text) || seriesLine.text : '';
        const week = String(outline.week || '').split(/[(:]/)[0].trim();
        const label = week || (/:/.test(themeLine.text) ? themeLine.text.split(':')[0].trim() : '');
        make({ kind: 'title', label, lines: [head, sub].filter(Boolean), refs: refsOf([...(themeLine.refs || []), ...((seriesLine && seriesLine.refs) || [])]) });
      } else if (p.kind === 'story') {
        const cut = opening(p.text, 28);
        make({ kind: 'story', label: 'Introduction', lines: [cut], refs: [], shortened: cut !== p.text }, { notes: p.text });
      } else if (p.kind === 'point') {
        n += 1;
        const { text, shortened } = shortenPoint(p.text);
        make({ kind: 'point', n, label: `Point ${n} of ${total}`, lines: [text], refs: refsOf(p.refs), shortened }, { notes: p.text });
      } else if (p.kind === 'section' || p.kind === 'closing') {
        make({ kind: p.kind, label: '', lines: [p.text], refs: [] });
      } else {
        // A line, note or typed verse the operator chose: the quieter left-hand layout.
        const cut = opening(p.text, 28);
        const ref = p.kind === 'verse' ? refsOf(p.refs)[0] || '' : '';
        make({ kind: 'story', label: ref, lines: [cut], refs: [], shortened: cut !== p.text }, { notes: p.text });
      }
    }
    const notes = parts.filter(p => p.kind === 'note' && !on(p)).map(p => p.text);
    if (notes.length && blocks.length) blocks[blocks.length - 1].presenterNote = notes.join('\n');
    return blocks;
  }
  // A sermon deck made new from its choices (item.sermon: outline, include, kit, style, footer).
  // A slide the operator edited by hand (gen.edited) is kept as it is, so a look or contents change
  // never undoes their work; every other slide is drawn afresh.
  function newDeck(outline, { kit, style, layout, colors, footer } = {}) {
    const include = partsOf(outline).filter(p => p.on).map(p => p.id);
    const k = kitById(kit).id;
    const st = BACKGROUNDS.some(b => b.id === style) ? style : 'glow';
    const lay = layoutOf(layout);
    const own = colors && Object.keys(colors).length ? { ...colors } : undefined;
    const blocks = build(outline, { kit: k, style: st, layout: lay, colors: own, footer, include: new Set(include) });
    return { sermon: { outline, include, kit: k, style: st, layout: lay, colors: own, footer: blocks[0] ? blocks[0].gen.footer : (footer || '') }, blocks };
  }
  function regenerate(item) {
    const d = item.sermon;
    const fresh = build(d.outline, { kit: d.kit, style: d.style, layout: d.layout, colors: d.colors, footer: d.footer, include: new Set(d.include) });
    const kept = new Map((item.blocks || []).filter(b => b.gen && b.gen.edited).map(b => [b.gen.part, b]));
    return fresh.map(b => {
      if (kept.has(b.gen.part)) return kept.get(b.gen.part);
      // What the operator applied to every slide stays on every new slide too.
      for (const [id, props] of Object.entries(d.common || {})) {
        const l = b.layers.find(x => x.id === id);
        if (l) Object.assign(l, JSON.parse(JSON.stringify(props)));
      }
      return b;
    });
  }

  // The parts every slide shares, and what of each a change carries to the others: the footer's
  // words and style; the label's and number's style (their words differ per slide); the rule's and
  // background's fill.
  const TEXT_STYLE = ['font', 'color', 'opacity', 'align', 'pos', 'shadow', 'outline'];
  const FILL = ['fill', 'color', 'color2', 'gradientStops', 'gradientGlows', 'angle', 'opacity', 'radius', 'src'];
  const COMMON = {
    'ss-footer': { name: 'footer', props: ['customText', ...TEXT_STYLE] },
    'ss-label': { name: 'label', props: TEXT_STYLE },
    'ss-number': { name: 'point number', props: TEXT_STYLE },
    'ss-rule': { name: 'accent rule', props: [...FILL, 'pos'] },
    'ss-bg': { name: 'background', props: FILL },
    'ss-panel': { name: 'side panel', props: FILL },
  };
  // A slide's shared parts that changed: [{ id, name, props }] with the new values to carry.
  function commonChanges(before, after) {
    const out = [];
    for (const [id, spec] of Object.entries(COMMON)) {
      const a = (before || []).find(l => l.id === id), b = (after || []).find(l => l.id === id);
      if (!a || !b) continue;
      const props = {};
      for (const k of spec.props) if (JSON.stringify(a[k]) !== JSON.stringify(b[k]) && b[k] !== undefined) props[k] = JSON.parse(JSON.stringify(b[k]));
      if (Object.keys(props).length) out.push({ id, name: spec.name, props });
    }
    return out;
  }
  // Those changes onto every other slide of the deck, and into the deck's own style.
  function applyCommon(item, changes, fromIndex) {
    const d = item.sermon;
    d.common = { ...(d.common || {}) };
    for (const c of changes) {
      d.common[c.id] = { ...(d.common[c.id] || {}), ...c.props };
      (item.blocks || []).forEach((b, i) => {
        if (i === fromIndex) return;
        const l = (b.layers || []).find(x => x.id === c.id);
        if (l) Object.assign(l, JSON.parse(JSON.stringify(c.props)));
      });
    }
  }

  // A slide's words changed in the playlist's slide list: written into its text layers in place,
  // so any font, size or colour the operator set stays as it is.
  function withText(block, text) {
    if (!block || !block.gen) return block;
    const gen = parse(text, block.gen);
    const set = (id, value) => { const l = block.layers.find(x => x.id === id); if (l && value != null) l.customText = value; };
    set('ss-label', gen.label);
    if (gen.kind === 'title') { set('ss-headline', (gen.lines || [])[0] || ''); set('ss-subtitle', (gen.lines || []).slice(1).join(' ')); }
    else set('ss-headline', (gen.lines || []).join(' '));
    (gen.refs || []).forEach((r, i) => set(`ss-chip-${i}-text`, r));
    return { ...block, gen, text: serialize(gen), label: gen.label || (gen.lines || [])[0] || block.label };
  }

  const api = { W, H, KITS, BACKGROUNDS, LAYOUTS, kitById, withColors, kitWarnings, contrast, bookName, normaliseRef,
    docxParagraphs, textParagraphs, outlineFrom, partsOf, readDocx, readText, shortenPoint, build, newDeck, regenerate, commonChanges, applyCommon, withText, serialize, parse, issues };
  if (typeof window !== 'undefined') window.KairoSermonSlides = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
