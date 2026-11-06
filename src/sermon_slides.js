// KAIRO — sermon slides: a sermon document made into a designed, editable deck (Phase 1, v1).
//
// docs/handover/slides-phase1-spec.md is the brief. In short:
//   - The design is made once, by hand, here: five layouts and six colour sets ("looks"). Kairo only
//     fills them with the sermon's words. It never invents a layout, a colour, a font or a word.
//   - The preacher's words, never Kairo's. A long point is cut at a natural break, marked
//     "shortened" for review, and its whole sentence kept in the slide's notes.
//   - Every word is a real text layer (pp_render.js draws them), so editing a word or changing the
//     look draws the slide again.
//   - Scripture appears as reference chips written out the same way ("Romans 4:16–21"); the verses
//     go up live from Kairo's Bible when they're read.
//   - Restraint: one accent word at most per slide, two fonts, 5% safe margins.
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

  // ── the looks: six colour sets made by hand ─────────────────────────────────────────────────────
  // Each is one colour world: a deep background and a second tone of the same hue for gradients,
  // one text colour and one accent, a display face and a reading face. Colours are chosen so text
  // clears 4.5:1 and the accent 3:1 on both background tones (checked by kitWarnings and the tests).
  const KITS = [
    { id: 'ember', name: 'Ember', bg: '#160A08', bg2: '#6B2313', text: '#FBF1E6', accent: '#F4A85F',
      display: { family: 'Montserrat', face: 'Montserrat-ExtraBold' }, body: { family: 'Inter', face: 'Inter-Regular' } },
    { id: 'midnight', name: 'Midnight', bg: '#090C21', bg2: '#2A2878', text: '#EEF0FF', accent: '#9CCBFF',
      display: { family: 'Playfair Display', face: 'PlayfairDisplay-Bold' }, body: { family: 'Inter', face: 'Inter-Regular' } },
    { id: 'evergreen', name: 'Evergreen', bg: '#0A1712', bg2: '#22503A', text: '#F2EFE3', accent: '#DCC27A',
      display: { family: 'EB Garamond', face: 'EBGaramond-SemiBold' }, body: { family: 'Manrope', face: 'Manrope-Regular' } },
    { id: 'lagoon', name: 'Lagoon', bg: '#041B21', bg2: '#0C5560', text: '#EAFAF7', accent: '#F3CA6E',
      display: { family: 'Raleway', face: 'Raleway-ExtraBold' }, body: { family: 'Inter', face: 'Inter-Regular' } },
    { id: 'graphite', name: 'Graphite', bg: '#0E0E10', bg2: '#2C2C31', text: '#FFFFFF', accent: '#FF8A65',
      display: { family: 'Archivo Black', face: 'ArchivoBlack-Regular' }, body: { family: 'Inter', face: 'Inter-Regular' } },
    { id: 'linen', name: 'Linen', bg: '#F5EEE4', bg2: '#E8CDB7', text: '#2A1C15', accent: '#A9432A',
      display: { family: 'Cormorant Garamond', face: 'CormorantGaramond-Bold' }, body: { family: 'Manrope', face: 'Manrope-Regular' } },
  ];
  // How the background is drawn from a look's two tones. All quiet; none adds anything to look at.
  const BACKGROUNDS = [
    { id: 'glow', name: 'Glow' },        // the second tone as a soft light from the top-left corner
    { id: 'gradient', name: 'Gradient' }, // the two tones, corner to corner
    { id: 'solid', name: 'Solid' },
  ];
  const kitById = (id) => KITS.find(k => k.id === id) || KITS[0];
  const FONT_FAMILIES = [...new Set(KITS.flatMap(k => [k.display.family, k.body.family]))];

  // ── colour ──────────────────────────────────────────────────────────────────────────────────────
  const HEX = /^#?([0-9a-f]{6})$/i;
  const rgb = (hex) => { const m = HEX.exec(String(hex || '')); const n = m ? parseInt(m[1], 16) : 0; return [(n >> 16 & 255) / 255, (n >> 8 & 255) / 255, (n & 255) / 255]; };
  const rgba = (hex, a = 1) => { const [r, g, b] = rgb(hex); return [r, g, b, a]; };
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
  // The one word shown in the accent colour: the point's most telling word (the longest that isn't
  // a common word or a name already in a heading). One per slide at most, so it keeps meaning.
  function accentWord(text) {
    // Not a name (capitalised) and not a shouted word: an ordinary word the point turns on.
    const words = (String(text || '').match(/[A-Za-z’']+/g) || []).filter(w => !STOP.has(w.toLowerCase()) && w.length >= 6 && /^[a-z]/.test(w));
    if (!words.length) return '';
    return words.sort((a, b) => b.length - a.length)[0];
  }

  // ── layers ──────────────────────────────────────────────────────────────────────────────────────
  let counter = 0;
  const lid = (p) => `${p}-${Date.now().toString(36)}${(counter++).toString(36)}`;
  function ppLayer(name, pos, extra) {
    return {
      type: 'pp', id: lid('ss'), name, binding: 'none', visible: true, opacity: 1, rotation: 0, flip: 'none', slideSize: { w: W, h: H },
      pos, shape: { type: 'rectangle' }, fill: { enable: false }, stroke: { enable: false }, shadow: { enable: false }, feather: { enable: false },
      ...extra,
    };
  }
  const run = (t, size, color, face, extra) => ({ t, size, color, font: face, ...extra });
  function textSpec(face, family, size, paragraphs, o = {}) {
    return {
      font: { name: face, family, size, bold: false, webFont: true, webFonts: o.webFonts || [] },
      paragraphs,
      margins: { l: 0, t: 0, r: 0, b: 0 },
      valign: o.valign || 'middle',
      scale: o.scale || 'down',
      caps: 'none', delimiter: '', kerning: 0, noLigatures: false, transform: 'none', superscript: 0,
      paragraph: { align: o.align || 'left', lineMult: o.lineMult || 1, balance: true },
      shadow: { enable: false },
    };
  }
  const para = (runs, align) => ({ align, runs });
  function roundedRect(w, h, r) {
    const rx = r / w, ry = r / h;
    const pt = (x, y, q0, q1) => ({ p: [x, y], q0: q0 || [x, y], q1: q1 || [x, y], curved: false });
    return { type: 'custom', closed: true, points: [
      pt(rx, 0), pt(1 - rx, 0, null, [1, 0]), pt(1, ry, [1, 0]), pt(1, 1 - ry, null, [1, 1]),
      pt(1 - rx, 1, [1, 1]), pt(rx, 1, null, [0, 1]), pt(0, 1 - ry, [0, 1]), pt(0, ry, null, [0, 0]),
    ] };
  }

  function backgroundLayers(kit, style) {
    const L = [{ type: 'background', id: lid('bg'), name: 'Canvas', color: kit.bg, fill: 'transparent', opacity: 100, visible: true },
      ppLayer('Background', { x: 0, y: 0, w: W, h: H }, { fill: { enable: true, color: rgba(kit.bg) } })];
    if (style === 'gradient') {
      L.push(ppLayer('Gradient', { x: 0, y: 0, w: W, h: H }, {
        fill: { enable: true, gradient: { type: 'linear', angle: -35, stops: [{ pos: 0, color: rgba(kit.bg2) }, { pos: 0.55, color: rgba(kit.bg, 0.6) }, { pos: 1, color: rgba(kit.bg) }] } },
      }));
    } else if (style === 'glow') {
      // A large soft circle of the second tone, its centre off the top-left corner.
      // Square and much larger than the slide, so the light has faded out well before any edge.
      L.push(ppLayer('Glow', { x: -1800, y: -1850, w: 3400, h: 3400 }, {
        fill: { enable: true, gradient: { type: 'radial', angle: 0, stops: [{ pos: 0, color: rgba(kit.bg2, 0.95) }, { pos: 0.22, color: rgba(kit.bg2, 0.55) }, { pos: 0.5, color: rgba(kit.bg2, 0) }, { pos: 1, color: rgba(kit.bg2, 0) }] } },
      }));
    }
    return L;
  }

  // The scripture chips: outlined pills in the accent, the reference in the reading face.
  function chipLayers(kit, refs, x, y, align) {
    const size = 32, h = 62, padX = 30, gap = 18;
    const widths = refs.map(r => Math.round(r.length * size * 0.54 + 2 * padX));
    const chips = [];
    let used = 0;
    for (let i = 0; i < refs.length; i++) {
      if (used + widths[i] > W - 2 * M) break;
      chips.push({ ref: refs[i], w: widths[i] });
      used += widths[i] + gap;
    }
    used -= gap;
    let cx = align === 'center' ? Math.round((W - used) / 2) : x;
    return chips.map(c => {
      const l = ppLayer(`Scripture ${c.ref}`, { x: cx, y, w: c.w, h }, {
        shape: roundedRect(c.w, h, h / 2),
        stroke: { enable: true, width: 2.5, color: rgba(kit.accent, 0.85), style: 'solid' },
        text: textSpec(kit.body.face, kit.body.family, size, [para([run(c.ref, size, rgba(kit.text), kit.body.face)], 'center')], { align: 'center', scale: 'none' }),
      });
      cx += c.w + gap;
      return l;
    });
  }
  const labelLayer = (kit, label, pos, align) => ppLayer('Label', pos, {
    text: textSpec(kit.body.face, kit.body.family, 30, [para([run(String(label).toUpperCase(), 30, rgba(kit.accent), kit.body.face.replace(/-Regular$/, '-SemiBold'), { kern: 6 })], align)], { align, scale: 'none' }),
  });
  const ruleLayer = (kit, x, y, w) => ppLayer('Rule', { x, y, w, h: 5 }, { fill: { enable: true, color: rgba(kit.accent) } });
  const footerLayer = (kit, footer) => !footer ? [] : [ppLayer('Footer', { x: M, y: 990, w: 1100, h: 36 }, {
    text: textSpec(kit.body.face, kit.body.family, 22, [para([run(String(footer).toUpperCase(), 22, rgba(kit.text, 0.55), kit.body.face, { kern: 5 })], 'left')], { scale: 'none' }),
  })];
  // A headline's runs: the slide's one accent word in the accent colour, the rest in the text colour.
  function headlineRuns(text, size, kit, accent) {
    if (!accent) return [run(text, size, rgba(kit.text), kit.display.face)];
    const at = text.indexOf(accent);
    if (at < 0) return [run(text, size, rgba(kit.text), kit.display.face)];
    return [run(text.slice(0, at), size, rgba(kit.text), kit.display.face), run(accent, size, rgba(kit.accent), kit.display.face), run(text.slice(at + accent.length), size, rgba(kit.text), kit.display.face)]
      .filter(r => r.t);
  }

  // ── the five layouts ────────────────────────────────────────────────────────────────────────────
  function layersFor(gen, kit, style, footer) {
    const L = backgroundLayers(kit, style);
    const refs = gen.refs || [];
    const lines = (gen.lines || []).filter(Boolean);
    const display = (size, align, accent) => lines.map(l => para(headlineRuns(l, size, kit, accent), align));
    if (gen.kind === 'title') {
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: 250, w: W - 2 * M, h: 50 }, 'center'));
      const [head, ...rest] = lines;
      L.push(ppLayer('Headline', { x: 200, y: 320, w: W - 400, h: 330 }, {
        text: textSpec(kit.display.face, kit.display.family, 132, [para(headlineRuns(head || '', 132, kit, ''), 'center')], { align: 'center', lineMult: 0.95 }),
      }));
      if (rest.length) L.push(ppLayer('Subtitle', { x: 200, y: 670, w: W - 400, h: 110 }, {
        text: textSpec(kit.body.face, kit.body.family, 44, rest.map(l => para([run(l, 44, rgba(kit.text, 0.82), kit.body.face)], 'center')), { align: 'center', valign: 'top' }),
      }));
      L.push(...chipLayers(kit, refs, 0, 830, 'center'));
    } else if (gen.kind === 'point') {
      // The point at the left; its number, very large and quiet, balances the right.
      if (gen.n) L.push(ppLayer('Number', { x: 1080, y: 160, w: 740, h: 760 }, {
        text: textSpec(kit.display.face, kit.display.family, 480, [para([run(String(gen.n).padStart(2, '0'), 480, rgba(kit.text, 0.07), kit.display.face)], 'right')], { align: 'right', scale: 'down' }),
      }));
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: 230, w: 900, h: 48 }, 'left'));
      L.push(ppLayer('Headline', { x: M, y: 300, w: 1180, h: 480 }, {
        text: textSpec(kit.display.face, kit.display.family, 104, display(104, 'left', gen.accent), { valign: 'top', lineMult: 1.0 }),
      }));
      L.push(...chipLayers(kit, refs, M, 840, 'left'));
    } else if (gen.kind === 'story') {
      L.push(ruleLayer(kit, M, 300, 96));
      if (gen.label) L.push(labelLayer(kit, gen.label, { x: M, y: 330, w: 900, h: 48 }, 'left'));
      L.push(ppLayer('Headline', { x: M, y: 400, w: 1380, h: 460 }, {
        text: textSpec(kit.display.face, kit.display.family, 76, display(76, 'left', ''), { valign: 'top', lineMult: 1.1 }),
      }));
    } else {
      // section / closing: one line, centred, with a short accent rule above
      const size = gen.kind === 'closing' ? 120 : 128;
      L.push(ruleLayer(kit, (W - 120) / 2, 340, 120));
      L.push(ppLayer('Headline', { x: 200, y: 380, w: W - 400, h: 400 }, {
        text: textSpec(kit.display.face, kit.display.family, size, lines.map(l => para([run(l, size, rgba(gen.kind === 'closing' ? kit.accent : kit.text), kit.display.face)], 'center')), { align: 'center', lineMult: 0.95 }),
      }));
    }
    L.push(...footerLayer(kit, footer));
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
    return { ...gen, label, lines: lines.length ? lines : gen.lines, refs, accent: gen.kind === 'point' ? accentWord(lines.join(' ')) : '', shortened: false };
  }
  // The slide's words checked against the rules (fit is checked after drawing: see checkFit).
  function issues(gen, kit) {
    const out = kitWarnings(kit);
    if (gen.shortened) out.push('Shortened: check the cut. The whole sentence is in the notes.');
    if ((gen.refs || []).some(r => !normaliseRef(r))) out.push('A scripture reference isn’t recognised.');
    return out;
  }
  // A slide drawn again keeps its layers' ids (by name), so what the slide editor stored for a layer
  // (a nudge, a resize) still applies after its words change.
  function keepIds(oldLayers, newLayers) {
    const byName = new Map((oldLayers || []).map(l => [l.name, l.id]));
    return newLayers.map(l => (byName.has(l.name) ? { ...l, id: byName.get(l.name) } : l));
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
    const kit = kitById(opts.kit);
    const style = BACKGROUNDS.some(b => b.id === opts.style) ? opts.style : 'glow';
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
      const make = (gen, extra) => blocks.push(makeBlock({ ...gen, part: p.id }, kit, style, footer, extra));
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
        make({ kind: 'point', n, label: `Point ${n} of ${total}`, lines: [text], refs: refsOf(p.refs), accent: accentWord(text), shortened }, { notes: p.text });
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
  // A slide with its words edited in the slide editor: its content from the text, drawn again.
  function withText(block, text) {
    if (!block || !block.gen) return block;
    const gen = parse(text, block.gen);
    return { ...block, gen, text: serialize(gen), label: gen.label || (gen.lines || [])[0] || block.label, layers: keepIds(block.layers, layersFor(gen, deckKit(gen), gen.style, gen.footer)) };
  }
  // Which part of a slide a layer shows ('label', 'lines', 'subtitle', 'ref:N'), or '' when it isn't words.
  function partOfLayer(block, layer) {
    if (!block || !block.gen || !layer) return '';
    if (layer.name === 'Label') return 'label';
    if (layer.name === 'Headline') return 'lines';
    if (layer.name === 'Subtitle') return 'subtitle';
    const m = /^Scripture (.+)$/.exec(layer.name || '');
    if (m) { const i = (block.gen.refs || []).indexOf(m[1]); return i >= 0 ? `ref:${i}` : ''; }
    return '';
  }
  // The words a part shows now (for the editor's field).
  function partText(block, part) {
    const g = (block && block.gen) || {};
    if (part === 'label') return g.label || '';
    if (part === 'lines') return (g.lines || [])[0] || '';
    if (part === 'subtitle') return (g.lines || [])[1] || '';
    const chip = /^ref:(\d+)$/.exec(part);
    return chip ? (g.refs || [])[+chip[1]] || '' : '';
  }
  // The same slide in another look, background or footer.
  // `own: true` sets this slide's background apart from the deck's; a deck-wide change leaves it be.
  function restyle(block, { kit, style, footer, own } = {}) {
    if (!block || !block.gen) return block;
    const keep = block.gen.ownStyle && own == null;
    const gen = { ...block.gen, ...(kit ? { kit } : {}), ...(style && !keep ? { style } : {}), ...(footer != null ? { footer } : {}), ...(own != null ? { ownStyle: own } : {}) };
    return { ...block, gen, layers: keepIds(block.layers, layersFor(gen, deckKit(gen), gen.style, gen.footer)) };
  }

  // One part of a slide edited in place: 'label', 'lines' (the headline, one line per entry),
  // 'subtitle' (a title slide's second line) or 'refs'.
  function withPart(block, part, value) {
    if (!block || !block.gen) return block;
    const gen = { ...block.gen };
    if (part === 'label') gen.label = String(value || '').trim();
    if (part === 'lines') gen.lines = [String(value || '').replace(/\s*\n\s*/g, ' ').trim()].filter(Boolean).concat(gen.kind === 'title' ? (gen.lines || []).slice(1) : []);
    if (part === 'subtitle') gen.lines = [(gen.lines || [])[0] || '', String(value || '').trim()].filter(Boolean);
    if (part === 'refs') gen.refs = (Array.isArray(value) ? value : String(value || '').split(/\s*[·;\n]\s*/)).map(r => normaliseRef(r) || String(r).trim()).filter(Boolean);
    // One chip ('ref:2'): that reference changed, or gone when emptied.
    const chip = /^ref:(\d+)$/.exec(part);
    if (chip) { const refs = [...(gen.refs || [])]; const v = String(value || '').trim(); if (v) refs[+chip[1]] = normaliseRef(v) || v; else refs.splice(+chip[1], 1); gen.refs = refs; }
    if (part === 'lines' && gen.kind === 'point') { gen.accent = accentWord(gen.lines[0] || ''); gen.shortened = false; }
    return { ...block, gen, text: serialize(gen), label: gen.label || (gen.lines || [])[0] || block.label, layers: keepIds(block.layers, layersFor(gen, deckKit(gen), gen.style, gen.footer)) };
  }

  const api = { W, H, KITS, BACKGROUNDS, FONT_FAMILIES, MIN_HEADLINE_PX, kitById, kitWarnings, contrast, bookName, normaliseRef,
    docxParagraphs, textParagraphs, outlineFrom, partsOf, readDocx, readText, shortenPoint, accentWord, build, withText, withPart, partOfLayer, partText, restyle, serialize, parse, issues };
  if (typeof window !== 'undefined') window.KairoSermonSlides = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
