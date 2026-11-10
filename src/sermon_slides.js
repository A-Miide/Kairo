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
    // From the owner's reference palette cards (their exact codes, Nov 2026). Each is one tonal
    // ladder; `ramp` is the run of its steps that stays readable behind the text, which the
    // backgrounds blend through.
    { id: 'rose', name: 'Sunset', bg: '#1D1A39', bg2: '#662549', ramp: ['#1D1A39', '#451952', '#662549'], text: '#E8BCB9', accent: '#F39F5A',
      display: { family: 'Fraunces', weight: 700, width: 0.55 }, body: { family: 'Inter' } },
    { id: 'crimson', name: 'Crimson Night', bg: '#161E2F', bg2: '#384358', ramp: ['#161E2F', '#242F49', '#384358'], text: '#F6F1EE', accent: '#FFA586',
      display: { family: 'Bebas Neue', weight: 400, width: 0.42 }, body: { family: 'Inter' } },
    { id: 'mint', name: 'Forest Mint', bg: '#051F20', bg2: '#235347', ramp: ['#051F20', '#0B2B26', '#163832', '#235347'], text: '#DAF1DE', accent: '#8EB69B',
      display: { family: 'DM Serif Display', weight: 400, width: 0.5 }, body: { family: 'Manrope' } },
    { id: 'plum', name: 'Plum Peach', bg: '#3A0353', bg2: '#804A8A', ramp: ['#2A023C', '#3A0353', '#804A8A'], text: '#FFF4E8', accent: '#F8D299',
      display: { family: 'Poppins', weight: 800, width: 0.62 }, body: { family: 'Inter' } },
    { id: 'ocean', name: 'Deep Ocean', bg: '#03071A', bg2: '#0F3065', ramp: ['#03071A', '#06163A', '#0F3065'], text: '#EAF3FF', accent: '#6AAEEA',
      display: { family: 'Space Grotesk', weight: 700, width: 0.58 }, body: { family: 'Inter' } },
    { id: 'sky', name: 'Sky', bg: '#04202E', bg2: '#0A3C55', ramp: ['#04202E', '#0A3C55', '#0F5E83'], text: '#E1F2FA', accent: '#98D5EE',
      display: { family: 'Montserrat', weight: 800, width: 0.62 }, body: { family: 'Inter' } },
    { id: 'honey', name: 'Honey', bg: '#2E2507', bg2: '#5A4A0D', ramp: ['#2E2507', '#5A4A0D', '#7A5E12'], text: '#FFF3D8', accent: '#E8B84A',
      display: { family: 'Fraunces', weight: 600, width: 0.53 }, body: { family: 'Manrope' } },
    { id: 'lavender', name: 'Lavender', bg: '#F5EBFA', bg2: '#E7DBEF', ramp: ['#F5EBFA', '#E7DBEF'], text: '#49225B', accent: '#6E3482',
      display: { family: 'Poppins', weight: 700, width: 0.6 }, body: { family: 'Inter' } },
    // Sampled from the owner's reference slides, for the layouts made after them.
    { id: 'redcard', name: 'Red Card', bg: '#720B0C', bg2: '#8F0505', text: '#F7F7F7', accent: '#D10203', accentOn: ['#F7F7F7'],
      display: { family: 'Montserrat', weight: 800, width: 0.62 }, body: { family: 'Montserrat' } },
    { id: 'royal', name: 'Royal Blue', bg: '#0136A0', bg2: '#1B62B0', text: '#FFFFFF', accent: '#8FC5F3', accentOn: ['#0141AA'],
      display: { family: 'Anton', weight: 400, width: 0.5 }, body: { family: 'Montserrat' } },
    { id: 'night', name: 'Night Worship', bg: '#031A5A', bg2: '#071664', ramp: ['#020C3A', '#031A5A', '#0B2A8C'], text: '#FAFBFD', accent: '#9DB4FF',
      // The base stays dark; depth comes from one shaft of light and visible noise. The light's edges
      // run down to the right at about 36° (the owner's own lines over the reference): deepest navy
      // left of the lower line, the darker part of the light (#061853) between, light beyond the upper.
      sweep: { angle: 36, stops: [{ pos: 0, color: '#020B36' }, { pos: 40, color: '#041243' }, { pos: 54, color: '#061853' },
        { pos: 66, color: '#0B1D7A' }, { pos: 74, color: '#1E3C9E' }, { pos: 80, color: '#3157C1' }, { pos: 90, color: '#6B96E6' },
        { pos: 100, color: '#88B2EA' }] },
      grain: 'strong',
      glows: [],
      display: { family: 'Inter', weight: 800, width: 0.56 }, body: { family: 'Inter' } },
    { id: 'lime', name: 'Lime Black', bg: '#000000', bg2: '#0B0B0B', text: '#FFFFFF', accent: '#D5F228',
      display: { family: 'Anton', weight: 400, width: 0.5 }, body: { family: 'Inter' } },
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
    { id: 'statement', name: 'Statement' },
    { id: 'card', name: 'Card' },
    { id: 'quote', name: 'Quote' },
    { id: 'badge', name: 'Badge card' },
    { id: 'worthy', name: 'Worthy' },
    { id: 'glow', name: 'Light' },     // not "Glow": that's a background's name
  ];
  // The designs a deck is chosen from: each a look with the layout and background that suit it, so
  // one pick sets all three. Every look has one; the four from the owner's reference slides keep the
  // layout made for them.
  const DESIGNS = [
    { kit: 'ember', layout: 'left', style: 'glow' }, { kit: 'midnight', layout: 'centred', style: 'glow' },
    { kit: 'evergreen', layout: 'quote', style: 'gradient' }, { kit: 'lagoon', layout: 'lower', style: 'gradient' },
    { kit: 'graphite', layout: 'split', style: 'solid' }, { kit: 'linen', layout: 'editorial', style: 'solid' },
    { kit: 'rose', layout: 'card', style: 'gradient' }, { kit: 'crimson', layout: 'statement', style: 'glow' },
    { kit: 'mint', layout: 'left', style: 'gradient' }, { kit: 'plum', layout: 'centred', style: 'glow' },
    { kit: 'ocean', layout: 'split', style: 'gradient' }, { kit: 'sky', layout: 'lower', style: 'glow' },
    { kit: 'honey', layout: 'editorial', style: 'gradient' }, { kit: 'lavender', layout: 'card', style: 'solid' },
    { kit: 'redcard', layout: 'badge', style: 'solid' }, { kit: 'royal', layout: 'worthy', style: 'gradient' },
    { kit: 'night', layout: 'glow', style: 'glow' }, { kit: 'lime', layout: 'statement', style: 'solid' },
  ];
  const layoutOf = (id) => (LAYOUTS.some(l => l.id === id) ? id : 'left');
  const kitById = (id) => KITS.find(k => k.id === id) || KITS[0];
  // A look with the operator's own colours over it: { bg, bg2, text, accent }, any of them.
  function withColors(kit, colors) {
    const own = Object.fromEntries(Object.entries(colors || {}).filter(([k, v]) => ['bg', 'bg2', 'text', 'accent'].includes(k) && /^#[0-9a-f]{6}$/i.test(v)));
    if (!Object.keys(own).length) return kit;
    const mixed = { ...kit, ...own };
    if (own.bg || own.bg2) delete mixed.ramp;     // the look's own ladder no longer matches
    return mixed;
  }

  // ── colour ──────────────────────────────────────────────────────────────────────────────────────
  const HEX = /^#?([0-9a-f]{6})$/i;
  const rgb = (hex) => { const m = HEX.exec(String(hex || '')); const n = m ? parseInt(m[1], 16) : 0; return [(n >> 16 & 255) / 255, (n >> 8 & 255) / 255, (n & 255) / 255]; };
  const luminance = (hex) => { const f = (c) => (c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)); const [r, g, b] = rgb(hex).map(f); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const contrast = (a, b) => { const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

  function kitWarnings(kit) {
    const out = [];
    const tones = [kit.bg, kit.bg2, ...(kit.ramp || [])];
    for (const bg of tones) {
      if (contrast(kit.text, bg) < 4.5) { out.push('The text colour is hard to read on the background.'); break; }
    }
    // A look whose accent only ever sits on a card (Red Card) is checked against the card.
    for (const bg of kit.accentOn || tones) {
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
    chr: 'Chronicles', chron: 'Chronicles', phi: 'Philippians' };
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

  // ── paragraphs ─────────────────────────────────────────────────────────────────────────────────
  // A paragraph: { text, list, family, level, heading }. `family` is the kind of marker it began with
  // ('num' 1. 1) (1), 'roman' II., 'alpha' B), 'word' "Point 3:", 'bullet' • or a dash, or 'list' for
  // a Word list, whose marker isn't in the text); `level` is a Word list's depth; `heading` a line set
  // apart as a heading (a Word heading style, a short bold line, a short line in capitals).
  const NUMBER_NAMES = 'one two three four five six seven eight nine ten eleven twelve'.split(' ');
  function markerOf(line) {
    let m;
    if ((m = /^(?:main\s+|key\s+)?(?:point|pt|step|reason|lesson|principle|truth|key)\s*(?:no\.?\s*)?(\d{1,2}|[a-z]{3,6})\s*[:.\-–—)]?\s+(\S.*)$/i.exec(line)) && (/^\d/.test(m[1]) || NUMBER_NAMES.includes(m[1].toLowerCase()))) return { family: 'word', text: m[2] };
    if ((m = /^\(?(\d{1,2})(?:[.)]|:(?=\s))\s*(\S.*)$/.exec(line))) return { family: 'num', text: m[2] };
    if ((m = /^\(?((?:[IVXLC]{2,6})|I)[.)]\s+(\S.*)$/.exec(line))) return { family: 'roman', text: m[2] };
    if ((m = /^\(?([A-Za-z])[.)]\s+(\S.*)$/.exec(line))) return { family: 'alpha', text: m[2] };
    if ((m = /^(?:[•●▪◦‣*]|[–—-])\s+(\S.*)$/.exec(line))) return { family: 'bullet', text: m[1] };
    return null;
  }
  // A short line in capitals ("FAITH PLEASES GOD"): a heading, not a sentence.
  function capsHeading(text) {
    const letters = text.replace(/[^A-Za-z]/g, '');
    return letters.length >= 4 && wordCount(text) <= 12 && letters === letters.toUpperCase();
  }

  // The paragraphs of a Word document's XML: their words, and whether Word lists them.
  function docxParagraphs(xml) {
    const out = [];
    const P = /<w:p\b[^>]*?(?:\/>|>([\s\S]*?)<\/w:p>)/g;
    let m, gap = 0;
    while ((m = P.exec(xml))) {
      const body = m[1] || '';
      const style = (/<w:pStyle\b[^>]*w:val="([^"]+)"/.exec(body) || [])[1] || '';
      const text = body
        .replace(/<w:tab\/>/g, ' ').replace(/<w:br\/>/g, ' ')
        .replace(/<w:t(?:\s[^>]*)?>([\s\S]*?)<\/w:t>|<[^>]+>/g, (_, t) => (t != null ? decodeXml(t) : ''))
        .replace(/\s+/g, ' ').trim();
      if (!text) { gap += 1; continue; }
      const list = /^listparagraph/i.test(style) || body.includes('<w:numPr>');
      const level = +((/<w:ilvl\b[^>]*w:val="(\d+)"/.exec(body) || [])[1] || 0);
      // Every run bold: set apart as a heading when short.
      const runs = body.match(/<w:r\b[\s\S]*?<\/w:r>/g) || [];
      const bold = runs.length > 0 && runs.every(r => !/<w:t[ >]/.test(r) || /<w:b\/>|<w:b w:val="(?:1|true)"\/>/.test(r));
      const heading = /^(?:heading\d|title|subtitle)/i.test(style) || (!list && bold && wordCount(text) <= 12 && !/[.?!]$/.test(text));
      out.push({ text, list, family: list ? 'list' : null, level, heading, bold, gap });
      gap = 0;
    }
    return out;
  }

  // Pasted text: a line that starts with a marker ("1." "2)" "II." "B)" "•" "-" "Point 3:") is a
  // list paragraph (its marker taken off).
  function textParagraphs(text) {
    let gap = 0;
    const out = [];
    for (const raw of String(text || '').replace(/\r/g, '').split(/\n/)) {
      const l = raw.trim();
      if (!l) { gap += 1; continue; }
      const m = markerOf(l);
      out.push(m ? { text: m.text.trim(), list: true, family: m.family, level: 0, heading: false, gap }
        : { text: l, list: false, family: null, level: 0, heading: capsHeading(l), gap });
      gap = 0;
    }
    return out;
  }

  // ── scriptures in a line ───────────────────────────────────────────────────────────────────────
  // The references written after a point's words ("Rom. 4:16-21/Heb. 11:17-19 / Gen. 22:1-18"); a bare
  // verse number after a slash ("Mak. 16:15-16/20") is a verse of the same chapter.
  function refsIn(text) {
    const out = [];
    for (const raw of String(text || '').split(/[\/;]/)) {
      const part = raw.replace(/[\s–—-]+$/, '').trim();
      if (!part) continue;
      if (/^\d+(?:\s*[-–]\s*\d+)?$/.test(part) && out.length) out[out.length - 1] += ', ' + part;
      else out.push(part);
    }
    return out;
  }
  // A statement and the references after it, after a dash ("Daniel had to … – Dan. 6:10-23") or in
  // brackets at the end ("Daniel had to … (Dan. 6:10-23)").
  function splitStatement(text) {
    const dash = /\s+[–—-]\s*(?=(?:[1-3] ?)?[A-Z][a-z]{1,11}(?: of [A-Z][a-z]+)?\.? ?\d)/g;
    let m;
    while ((m = dash.exec(text))) {
      const rest = text.slice(m.index + m[0].length);
      const refs = refsIn(rest);
      if (refs.length && normaliseRef(refs[0])) return { statement: text.slice(0, m.index).trim(), refs };
    }
    // A dash and a short book name the reader doesn't know ("Phx. 3:14"): still the scriptures' start.
    const loose = /\s+[–-]\s*(?:[1-3] ?)?[A-Z][a-z]{1,4}\.? ?\d/.exec(text);
    if (loose) return { statement: text.slice(0, loose.index).trim(), refs: refsIn(text.slice(loose.index + loose[0].search(/[–-]/)).replace(/^[–\-\s]+/, '')) };
    const br = /\s*[(\[]([^()\[\]]{3,90})[)\]]\s*[.;]?$/.exec(text);
    if (br) {
      const refs = refsIn(br[1]);
      if (refs.length && refs.every(r => normaliseRef(r))) return { statement: text.slice(0, br.index).trim(), refs };
    }
    return { statement: text.trim(), refs: [] };
  }
  const isReferenceLine = (text) => wordCount(text) <= 12 && /^(?:[1-3] ?)?[A-Z][A-Za-z]{1,}\.? ?\d+(?::\d+)?/.test(text);
  // Every scripture named in running text ("As Romans 10:17 says"): a book and a chapter with a verse.
  function findRefs(text) {
    const out = [];
    const re = /\b((?:[1-3]\s?)?[A-Z][a-z]{1,11}(?:\s+of\s+[A-Z][a-z]+)?\.?)\s?(\d{1,3}:\d{1,3}(?:\s*[-–]\s*\d{1,3})?(?:\s*,\s*\d{1,3}(?:\s*[-–]\s*\d{1,3})?)*)/g;
    let m;
    while ((m = re.exec(String(text || '')))) {
      const ref = normaliseRef(`${m[1]} ${m[2]}`);
      if (ref && !out.includes(ref)) out.push(ref);
    }
    return out;
  }

  // ── reading paragraphs as an outline ───────────────────────────────────────────────────────────
  // Three ways, tried in order, for whatever a document is laid out as:
  //   points      numbered or bulleted points (any marker), the scriptures after each or on the line below
  //   headings    each heading is a point, the scriptures found in the paragraphs under it
  //   paragraphs  each paragraph a slide, its scriptures found in it
  // `how` says which, for the import dialog. Null when the text is not a sermon (a song's words, a
  // notice), unless `always`: a Word file the operator chose is read whatever it looks like.
  function outlineFrom(paras, { always = false } = {}) {
    // The kind of marker the points use: the one most lines begin with; any other is a sub-point.
    const counts = {};
    for (const p of paras) if (p.family && !(p.family === 'list' && p.level > 0)) counts[p.family] = (counts[p.family] || 0) + 1;
    const top = Object.keys(counts).sort((x, y) => counts[y] - counts[x])[0];
    const marked = top ? paras.map(p => ({ ...p, list: p.family === top && !(p.level > 0), sub: !!p.family && !(p.family === top && !(p.level > 0)) })) : paras;
    const byRoles = rolesOutline(paras);
    if (byRoles) return byRoles;
    const strict = top && counts[top] >= 2 ? pointsOutline(marked) : null;
    if (strict) return strict;
    return headingsOutline(paras) || paragraphsOutline(paras, always);
  }
  // Numbered or bulleted points.
  function pointsOutline(paras) {
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
      if (p.sub && seenPoint) { other.push(text); continue; }   // a point's own sub-points: offered, not slides
      if (p.list) {
        flush();
        seenPoint = true;
        const { statement, refs } = splitStatement(text);
        if (wordCount(statement) > 80) items.push({ kind: 'story', text: statement, refs: findRefs(statement) });
        else items.push({ kind: 'point', n: items.filter(i => i.kind === 'point').length + 1, text: statement, refs });
        continue;
      }
      if (!seenPoint) {
        if (/^week\s*\d+/i.test(text)) week = text;
        else if (/^introduction\b/i.test(text) || wordCount(text) > 30) {
          if (wordCount(text) > 30) items.push({ kind: 'story', text, refs: [] });
        } else if (header.length < 6) header.push(splitStatement(text));
        else other.push(text);
        continue;
      }
      // Scriptures alone on the line under a point that has none are that point's.
      const lastItem = items[items.length - 1];
      if (isReferenceLine(text) && !verses.length && lastItem && lastItem.kind === 'point' && !lastItem.refs.length) {
        const refs = refsIn(text);
        if (refs.every(r => normaliseRef(r))) { lastItem.refs = refs; continue; }
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
    const withRefs = points.filter(p => p.refs.length).length;
    // Three points, or two with a scripture: an outline. Fewer is a list of something else.
    if (!(points.length >= 3 || (points.length >= 2 && withRefs >= 1))) return null;
    return finish({ header: header.map(h => ({ text: h.statement, refs: h.refs })), week, items, other }, 'points');
  }
  // ── reading by roles ───────────────────────────────────────────────────────────────────────────
  // Documents come laid out in many ways (numbered points, bold statements, a ProPresenter slides file
  // with each verse typed under its own reference), so the reader doesn't look for a layout. It gives
  // each paragraph a role from what the paragraph is, and its neighbours:
  //   statement   a point or heading: numbered, bold, in capitals, or a Word heading
  //   scriptures  references and nothing else ("Isaiah 8:18 / John 3:8"), under the statement they belong to
  //   verseHead   a single reference with its words written out under it
  //   verseText   those words ("18. Behold, I and the children …")
  //   story       a long paragraph of prose        note   "NB: …" and what follows a "Note" heading
  //   short       a short plain line               sub    a point's own sub-point         line   anything else
  // and then one assembler builds the outline from the roles, whatever order they came in.
  const bareRef = (text) => String(text || '').replace(/\s*\[[^\]}]*[\]}]/g, '').trim();
  function refsOnly(text) {
    const first = bareRef(String(text || '').split('|')[0]);
    if (!first || wordCount(first) > 14 || !/^\(?(?:[1-3] ?)?[A-Z]/.test(first)) return null;
    const refs = refsIn(first);
    // Every part is a book and numbers, nothing else ("Isaiah 8:18", "Ps. 119:14, 111"): not a reference with words after it.
    const bare = (r) => /^(?:[1-3] ?)?[A-Za-z][A-Za-z. ]{1,24}?\s*\d{1,3}(?:\s*:\s*[\d\s,–—-]+)?$/.test(r.trim());
    return refs.length && refs.every(r => bare(r) && normaliseRef(r)) ? refs : null;
  }
  // A reference with its verse on the same line ("Prov 23:7 For as he thinketh …", "Matt 7:16-20 (16 Ye shall know …)").
  function inlineVerse(text) {
    const m = /^((?:[1-3] ?)?[A-Z][A-Za-z]{1,12}\.? ?\d{1,3}(?:\s*:\s*\d{1,3}(?:\s*[-–—]\s*\d{1,3})?(?:\s*[,/]\s*\d{1,3}(?:\s*[-–—]\s*\d{1,3})?)*)?)(?:\s*\[[^\]]*\])?\s*(\S[\s\S]*)$/.exec(String(text || '').trim());
    if (!m || !normaliseRef(m[1])) return null;
    const words = m[2].replace(/^\(\s*\d{0,3}\s*/, '').replace(/\)\s*$/, '').trim();
    return wordCount(words) >= 5 ? { ref: normaliseRef(m[1]), raw: m[1].trim(), text: words } : null;
  }
  const isBold = (p) => (p.bold !== undefined ? !!p.bold : !!p.heading);
  const startsVerse = (t) => /^\d{1,3}\s*[.)]\s*\S/.test(t) || (/^\d{1,3}\s+\S/.test(t) && !refsOnly(t));
  // Words written under a reference: they begin with a verse number or are a sentence, and aren't a heading.
  // In a numbered document the points are the numbered lines, so a bold line is free to be a verse; elsewhere a
  // bold line is a statement, and verse words are the plain ones.
  function verseBodyAt(paras, i, hasList) {
    const p = paras[i];
    if (!p || p.list || refsOnly(p.text) || capsHeading(p.text) || /^vs\b/i.test(p.text)) return false;
    return startsVerse(p.text) || ((hasList || !isBold(p)) && wordCount(p.text) >= 6);
  }
  function labelParagraphs(paras, hasList) {
    const roles = [];
    let inNotes = false;
    const isStatement = (p) => !p.sub && (hasList ? p.list : (p.list || isBold(p) || p.heading)) && wordCount(p.text) >= 2 && !refsOnly(p.text);
    let verseOpen = false;   // words are being read as a verse (a "VS:10" marker inside them doesn't end it)
    paras.forEach((p, i) => {
      const text = p.text.trim().replace(/^\d{6,}(?=[A-Za-z])/, '');
      const prev = roles[i - 1] || { role: '' };
      const seenStatement = roles.some(r => r.role === 'statement');
      let role = 'line';
      if (inNotes || (/^nb\b/i.test(text) && text.includes(':'))) role = 'note';
      else if (seenStatement && /^(?:note|notes|nb)\s*:?$/i.test(text)) { inNotes = true; role = 'skip'; }
      else if (p.sub && seenStatement) role = 'sub';
      else if (seenStatement && /^vs\s*:?\s*\d/i.test(text)) role = 'vs';
      else if (!p.list && seenStatement && !refsOnly(text) && inlineVerse(text)) role = 'verseInline';
      else if (refsOnly(text) && !p.list) {
        const prevOpen = prev.role === 'statement' && !prev.refs;
        role = prevOpen && !(isBold(p) && verseBodyAt(paras, i + 1, hasList)) ? 'scriptures' : (verseBodyAt(paras, i + 1, hasList) ? 'verseHead' : 'scriptures');
      } else if (verseOpen && (startsVerse(text) || wordCount(text) > 10 || (!isBold(p) && wordCount(text) >= 6)) && (hasList || !isBold(p) || startsVerse(text)) && !p.list && !capsHeading(text) && !/^vs\b/i.test(text)) role = 'verseText';
      else if (isStatement(p)) role = 'statement';
      else if (wordCount(text) > 30) role = 'story';
      else if (wordCount(text) <= 10 && !/^\d/.test(text)) role = 'short';
      if (role !== 'vs') verseOpen = role === 'verseHead' || role === 'verseText';
      const { statement, refs } = role === 'statement' ? splitStatement(text) : { statement: text, refs: [] };
      roles.push({ role, text, statement, refs: refs.length ? refs : null, p });
    });
    return roles;
  }

  // The outline from the roles. Returns null when it holds fewer than the points of a sermon.
  function rolesOutline(paras) {
    const counts = {};
    for (const p of paras) if (p.family && !(p.family === 'list' && p.level > 0)) counts[p.family] = (counts[p.family] || 0) + 1;
    const top = Object.keys(counts).sort((x, y) => counts[y] - counts[x])[0];
    const marked = top && counts[top] >= 2 ? paras.map(p => ({ ...p, list: p.family === top && !(p.level > 0), sub: !!p.family && !(p.family === top && !(p.level > 0)) })) : paras.map(p => ({ ...p, sub: false }));
    const hasList = marked.some(p => p.list);
    const roles = labelParagraphs(marked, hasList);
    const items = [], other = [], header = [];
    let cur = null, verse = null, week = null, pointSeen = false;
    const flushVerse = () => { verse = null; };
    // The document's own heading: the lines before its first point (numbered documents), or the capitals at its top.
    roles.forEach((r, i) => {
      const { role, p } = r;
      if (role === 'skip' || role === 'vs') return;
      if (role === 'note') { flushVerse(); items.push({ kind: 'note', text: r.text }); return; }
      if (role === 'sub') { other.push(r.text); return; }
      if (role === 'statement') {
        const isHeader = hasList ? (!pointSeen && !p.list && header.length < 6) : (header.length < 2 && !pointSeen && capsHeading(r.statement));
        if (isHeader) {
          header.push({ text: r.statement, refs: r.refs || [] });
          cur = { kind: 'headerline', refs: r.refs || [], verses: [], notes: [] };
          flushVerse();
          return;
        }
        pointSeen = true;
        if (wordCount(r.statement) > 80) { items.push({ kind: 'story', text: r.statement, refs: findRefs(r.statement) }); cur = null; flushVerse(); return; }
        cur = { kind: 'point', n: 0, text: r.statement.replace(/:$/, ''), refs: r.refs ? [...r.refs] : [], verses: [], notes: [] };
        items.push(cur); flushVerse();
        return;
      }
      if (role === 'scriptures') {
        const [first, ...rest] = r.text.split('|').map(t => t.trim());
        const refs = refsIn(bareRef(first));
        if (cur) { cur.refs.push(...refs); if (cur.kind === 'headerline') { const h = header[header.length - 1]; if (h) h.refs = cur.refs; } }
        const wk = rest.find(x => /^week\b/i.test(x));
        if (wk && !week) week = wk;
        if (rest.length && cur && cur.notes) cur.notes.push(rest.join(' | '));
        flushVerse();
        return;
      }
      if (role === 'verseHead') {
        verse = { ref: bareRef(r.text), text: [] };
        if (!cur || cur.kind === 'headerline') { cur = { kind: 'point', n: 0, text: '', refs: [], verses: [], notes: [] }; items.push(cur); }
        cur.verses.push(verse);
        return;
      }
      if (role === 'verseInline') {
        const v = inlineVerse(r.text);
        if (!cur || cur.kind === 'headerline') { cur = { kind: 'point', n: 0, text: '', refs: [], verses: [], notes: [] }; items.push(cur); }
        cur.verses.push({ ref: v.raw, text: [v.text] });
        flushVerse();
        return;
      }
      if (role === 'verseText') { if (verse) verse.text.push(r.text.replace(/^\d{1,3}\s*[.)]\s*(?=\S)/, '')); return; }
      if ((role === 'short' || role === 'line') && cur && cur.kind === 'point' && findRefs(r.text).length) {
        cur.notes.push(r.text);
        for (const ref of findRefs(r.text)) if (!cur.refs.includes(ref)) cur.refs.push(ref);
        flushVerse();
        return;
      }
      if (role === 'story') {
        flushVerse();
        // Prose before the first point is the introduction; prose among the points is that point's own note.
        items.push({ kind: 'story', text: r.text, refs: findRefs(r.text) }); cur = null;
        return;
      }
      if ((role === 'line' || role === 'short') && hasList && !pointSeen && wordCount(r.text) <= 30) {
        if (/^week\s*\d+/i.test(r.text)) week = r.text; else if (!/^introduction\b/i.test(r.text) && header.length < 6) { const h = splitStatement(r.text); header.push({ text: h.statement, refs: h.refs }); }
        return;
      }
      if (role === 'short') {
        flushVerse();
        if (!pointSeen) { if (/^week\s*\d+/i.test(r.text)) week = r.text; else if (!/^introduction\b/i.test(r.text) && header.length < 6) { const h = splitStatement(r.text); header.push({ text: h.statement, refs: h.refs }); } return; }
        if (/^vs\b/i.test(r.text) && /\d/.test(r.text)) { other.push(r.text); return; }
        items.push({ kind: 'section', text: r.text.replace(/:$/, '') }); cur = null;
        return;
      }
      other.push(r.text);
    });
    // Tidy: a point is its words, scriptures and verses; capitals with nothing under them divide the service.
    for (const it of items) {
      if (it.kind !== 'point') continue;
      it.refs = [...new Set(it.refs)];
      it.verses = it.verses.filter(v => v.text.length).map(v => ({ ref: v.ref, text: v.text.join(' ') }));
      it.notes = it.notes.join('\n'); if (!it.notes) delete it.notes;
      if (!it.verses.length) delete it.verses;
      if (!it.text && it.verses) it.text = it.verses[0].ref;
      if (capsHeading(it.text) && !it.refs.length && !it.verses) it.kind = 'section';
    }
    const last = [...items].reverse().find(i => i.kind !== 'note');
    if (last && last.kind === 'section') last.kind = 'closing';
    let n = 0;
    for (const it of items) { if (it.kind === 'point') it.n = ++n; else delete it.n; }
    const points = items.filter(i => i.kind === 'point');
    const withRefs = points.filter(p => p.refs.length || p.verses).length;
    if (!(points.length >= 3 || (points.length >= 2 && withRefs >= 1))) return null;
    const out = finish({ header, week, items, other, titleIndex: hasList ? undefined : 0 }, 'roles');
    const typed = items.reduce((c, i) => c + (i.verses ? i.verses.length : 0), 0);
    const named = new Set(items.flatMap(i => [...(i.refs || []), ...((i.verses || []).map(v => v.ref))])).size;
    out.how.verses = typed;
    // A document with no numbered points that writes out most of the scriptures it names is a slides document: its
    // verses are slides. A numbered outline's typed verses are the preacher's reading notes: offered, not on.
    out.how.versesOn = !hasList && typed >= 3 && typed >= 0.5 * named;
    return out;
  }
  // Each heading a point; the scriptures named in the paragraphs under it are its own.
  function headingsOutline(paras) {
    // The first heading with something written under it; short lines before it are the document's own heading.
    const first = paras.findIndex((p, i) => p.heading && !p.list && paras[i + 1] && !paras[i + 1].heading && (wordCount(paras[i + 1].text) > 6 || findRefs(paras[i + 1].text).length));
    const heads = paras.slice(Math.max(first, 0)).filter(p => p.heading);
    if (first < 0 || heads.length < 2) return null;
    const header = paras.slice(0, first).filter(p => wordCount(p.text) <= 30).slice(0, 6).map(p => ({ text: p.text, refs: [] }));
    const items = [];
    let cur = null;
    for (const p of paras.slice(first)) {
      if (p.heading) {
        cur = { kind: 'point', n: items.length + 1, text: p.text.replace(/:$/, ''), refs: [], notes: [] };
        items.push(cur);
      } else if (cur) { cur.notes.push(p.text); for (const r of findRefs(p.text)) if (!cur.refs.includes(r)) cur.refs.push(r); }
    }
    for (const it of items) { it.notes = it.notes.join('\n'); if (!it.notes) delete it.notes; }
    const words = paras.reduce((n, p) => n + wordCount(p.text), 0);
    if (!items.some(i => i.refs.length) && words < 120) return null;
    return finish({ header, week: null, items, other: [], titleIndex: 0 }, 'headings');
  }
  // Each paragraph a slide: short lines are sections, longer ones carry the scriptures they name.
  function paragraphsOutline(paras, always) {
    const words = paras.reduce((n, p) => n + wordCount(p.text), 0);
    const refs = paras.reduce((n, p) => n + findRefs(p.text).length, 0);
    if (!paras.length || (!always && !(words >= 150 && refs >= 1))) return null;
    const firstLong = paras.findIndex(p => wordCount(p.text) > 30);
    const lead = firstLong < 0 ? 0 : Math.min(firstLong, 4);
    const header = paras.slice(0, lead).map(p => ({ text: p.text, refs: [] }));
    const items = paras.slice(lead).map(p => (wordCount(p.text) <= 10
      ? { kind: 'section', text: p.text.replace(/:$/, '') }
      : { kind: 'story', text: p.text, refs: findRefs(p.text) }));
    return finish({ header, week: null, items, other: [], titleIndex: 0 }, 'paragraphs');
  }
  // The outline with how it was read, for the import dialog.
  function finish(o, mode) {
    const points = o.items.filter(i => i.kind === 'point');
    o.how = { mode, points: points.length, slides: o.items.filter(i => i.kind !== 'note').length, sections: o.items.filter(i => i.kind === 'section').length,
      withRefs: o.items.filter(i => i.refs && i.refs.length).length };
    return o;
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
      // A Word file the operator chose is always offered: read as points, as headings, or paragraph by paragraph.
      return xml ? outlineFrom(docxParagraphs(new TextDecoder().decode(xml)), { always: true }) : null;
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
      ...(o.rotation ? { rotation: o.rotation } : {}),
      ...(o.decor ? { decor: true } : {}),   // cropped on purpose (a ghost word): not "cut off"
    };
  }
  function shapeLayer(id, name, pos, color, o = {}) {
    return { id, type: 'background', name, visible: true, fill: 'solid', color, color2: color, opacity: o.opacity || 100, angle: 0, radius: o.radius || 0, pos };
  }
  function backgroundLayers(kit, style) {
    // A full-size Background layer (not the canvas itself): its fill, colours and gradient are set
    // under Style like any slide's background. A look with a tonal ladder (`ramp`) blends through its
    // steps, as its palette card does; then a fine grain over it all, which ties the tones together
    // and keeps the gradient from banding on a projector.
    const canvas = { id: 'ss-bg', type: 'background', name: 'Background', visible: true, opacity: 100, angle: 135, radius: 0, pos: { x: 0, y: 0, w: W, h: H } };
    const ramp = kit.ramp && kit.ramp.length > 1 ? kit.ramp : [kit.bg, kit.bg2];
    const stops = ramp.map((c, i) => ({ pos: Math.round(i / (ramp.length - 1) * 100), color: c }));
    let bg;
    if (style === 'gradient') {
      bg = { ...canvas, fill: 'gradient', color: ramp[ramp.length - 1], color2: ramp[0], gradientStops: [...stops].reverse() };
    } else if (style === 'glow') {
      // The deepest step as the ground; the next steps as soft light from the top-left corner.
      bg = { ...canvas, fill: 'gradient', color: ramp[0], color2: ramp[0], gradientStops: [{ pos: 0, color: ramp[0] }, { pos: 100, color: ramp[0] }],
        gradientGlows: [{ x: 4, y: 0, rx: 70, ry: 100, color: ramp[ramp.length - 1] },
          ...(ramp.length > 2 ? [{ x: 0, y: 0, rx: 34, ry: 50, color: ramp[ramp.length - 2] }] : [])] };
    } else {
      bg = { ...canvas, fill: 'solid', color: kit.bg, color2: kit.bg };
    }
    // Edges falling off into shadow, for depth (strength set per layout; see vignette()).
    const shade = { id: 'ss-vignette', type: 'image', name: 'Vignette', visible: true, src: 'backgrounds/vignette.svg', fit: 'fill',
      opacity: 45, pos: { x: 0, y: 0, w: W, h: H } };
    const grain = { id: 'ss-grain', type: 'image', name: 'Grain', visible: true, src: kit.grain === 'strong' ? 'backgrounds/grain-strong.svg' : 'backgrounds/grain.svg', fit: 'cover',
      opacity: 100, blendMode: 'Overlay', pos: { x: 0, y: 0, w: W, h: H } };
    return [bg, shade, grain];
  }
  // The background layers of a slide with its vignette at `opacity` (the reference layouts are darker).
  const vignette = (layers, opacity) => layers.map(l => (l.id === 'ss-vignette' ? { ...l, opacity } : l));

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
    // Body copy (kit.display.floor) may go smaller than a headline would.
    const floor = Math.min(kit.display.floor || MIN_HEADLINE_PX, size);
    while (s > floor && lines(s) * s * lineHeight > h) s -= 2;
    return Math.max(floor, s);   // never bigger than the slide asked for
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
    if (layout === 'statement') return statement(gen, kit, style, footer);
    if (layout === 'card') return card(gen, kit, style, footer);
    if (layout === 'quote') return quote(gen, kit, style, footer);
    if (layout === 'badge') return badge(gen, kit, style, footer);
    if (layout === 'worthy') return worthy(gen, kit, style, footer);
    if (layout === 'glow') return glow(gen, kit, style, footer);
    const L = backgroundLayers(kit, style);
    const refs = gen.refs || [];
    const lines = (gen.lines || []).filter(Boolean);
    if (layout === 'centred' && (gen.kind === 'point' || gen.kind === 'story')) {
      if (gen.kind === 'point' && gen.n) L.push(textLayer('ss-number', 'Number', { x: (W - 740) / 2, y: 190, w: 740, h: 640 }, String(gen.n).padStart(2, '0'),
        { ...display(kit, 520, kit.text, 'center', { lineHeight: 1.0 }), opacity: 7, decor: true }));
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
        { ...display(kit, 480, kit.text, 'right', { lineHeight: 1.0 }), opacity: 7, decor: true }));
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

  // ── poster styles: type as texture, two voices, one loud accent ─────────────────────────────────
  // Heavy condensed caps for the point, a script for the label, both whatever the look's own faces.
  const CONDENSED = { family: 'Anton', weight: 400, width: 0.56 };
  const SCRIPT = { family: 'Yellowtail', weight: 400 };
  const NUMBER_WORDS = ['', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten',
    'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen', 'twenty'];
  // The word that becomes the ghost behind a slide: its first word of four letters or more.
  const ghostWord = (text) => (String(text || '').match(/[A-Za-zÀ-ÿ']{4,}/) || [''])[0].replace(/'s$/i, '').toUpperCase();
  const scriptLabel = (gen) => gen.kind === 'point' && gen.n ? `Point ${NUMBER_WORDS[gen.n] || gen.n}` : gen.kind === 'title' ? (gen.label || '') : gen.kind === 'story' ? 'Introduction' : '';
  // The scriptures as one quiet tracked line (the chip layers, with their pills left clear).
  function refLine(kit, refs, x, y, align, color) {
    return chipLayers(kit, refs.slice(0, 3), x, y, align).map(l => l.type === 'background'
      ? { ...l, opacity: 0 }
      : { ...l, color: color || kit.accent, font: { ...l.font, size: 28, weight: 600, letterSpacing: 3, transform: 'uppercase' } });
  }
  const condensedKit = (kit) => ({ ...kit, display: CONDENSED });
  // A colour part way from a to b (0..1).
  const mix = (a, b, t) => '#' + [1, 3, 5].map(i => Math.max(0, Math.min(255, Math.round(parseInt(a.slice(i, i + 2), 16) * (1 - t) + parseInt(b.slice(i, i + 2), 16) * t)))
    .toString(16).padStart(2, '0')).join('').toUpperCase();

  // Statement: the point in huge condensed caps, its first word ghosted enormous behind it, the label
  // in script over its corner, the church's name running up the left edge.
  function statement(gen, kit, style, footer) {
    const L = backgroundLayers(kit, style);
    const lines = (gen.lines || []).filter(Boolean);
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const ghost = ghostWord(gen.kind === 'title' ? text : text);
    if (ghost) L.push(textLayer('ss-ghost', 'Ghost word', { x: -60, y: 170, w: W + 600, h: 760 }, ghost,
      { decor: true, family: CONDENSED.family, size: 600, color: kit.text, opacity: 5, lineHeight: 1.0, spacing: -6 }));
    if (footer) L.push(textLayer('ss-footer', 'Footer', { x: 70 - 420, y: 540 - 18, w: 840, h: 36 }, String(footer),
      { family: kit.body.family, size: 20, color: kit.text, opacity: 55, spacing: 8, transform: 'uppercase', align: 'center', rotation: -90 }));
    const X = 250;
    const label = scriptLabel(gen);
    if (label) L.push(textLayer('ss-label', 'Label', { x: X - 10, y: 250, w: 1200, h: 130 }, label,
      { family: SCRIPT.family, size: 104, color: kit.accent, rotation: -4, lineHeight: 1.0 }));
    const big = gen.kind === 'story' ? 66 : 150;
    const h = headline(gen.kind === 'story' ? kit : condensedKit(kit), gen, { x: X, y: 360, w: W - X - 120, h: 470 }, text, big,
      gen.kind === 'closing' ? kit.accent : kit.text, 'left', gen.kind === 'story' ? 1.2 : 0.98);
    if (gen.kind !== 'story') h.font.transform = 'uppercase';
    L.push(h);
    if (gen.kind === 'title' && lines[1]) L.push(textLayer('ss-subtitle', 'Subtitle', { x: X, y: 830, w: 1300, h: 56 }, lines.slice(1).join(' '),
      { family: kit.body.family, size: 34, color: kit.text, opacity: 80, spacing: 6, transform: 'uppercase' }));
    else L.push(...refLine(kit, gen.refs || [], X, 850, 'left'));
    return L;
  }

  // Card: the look's colour behind, the ghost word stacked down it; a white card with the point in the
  // look's darkest tone, its label in an accent tag, the number in a badge breaking the card's top.
  function card(gen, kit, style, footer) {
    const L = backgroundLayers(kit, style);
    const lines = (gen.lines || []).filter(Boolean);
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const ghost = ghostWord(text);
    if (ghost) for (let i = 0; i < 3; i++) L.push(textLayer(`ss-ghost-${i}`, `Ghost word ${i + 1}`, { x: -40, y: -60 + i * 380, w: W + 400, h: 420 }, ghost,
      { decor: true, family: CONDENSED.family, size: 420, color: kit.bg2, opacity: 55, lineHeight: 1.0, spacing: -4 }));
    const cx = 400, cy = 190, cw = W - 800, ch = 720;
    // Liquid glass: the ghost words and the light show softly through it; the words on it in the
    // look's own text colour (the glass is only a faint tint, so the background still decides).
    const ink = kit.text;
    L.push({ ...shapeLayer('ss-card', 'Card', { x: cx, y: cy, w: cw, h: ch }, '#FFFFFF', { radius: 64, opacity: 10 }), glass: true });
    if (gen.kind === 'point' && gen.n) {
      L.push(shapeLayer('ss-badge', 'Badge', { x: W / 2 - 80, y: cy - 80, w: 160, h: 160 }, kit.accent, { radius: 80 }));
      L.push(textLayer('ss-number', 'Number', { x: W / 2 - 80, y: cy - 52, w: 160, h: 110 }, String(gen.n),
        { family: CONDENSED.family, size: 92, color: kit.bg, align: 'center', lineHeight: 1.0 }));
    }
    const tag = gen.kind === 'point' ? 'Point' : gen.kind === 'title' ? (gen.label || 'Series') : gen.kind === 'story' ? 'Introduction' : '';
    if (tag) {
      const tw = Math.round(tag.length * 30 * 0.62 + 56);
      L.push(shapeLayer('ss-tag', 'Tag', { x: W / 2 - tw / 2, y: cy + 130, w: tw, h: 58 }, kit.accent, { radius: 10 }));
      L.push(textLayer('ss-label', 'Label', { x: W / 2 - tw / 2, y: cy + 140, w: tw, h: 44 }, tag,
        { family: kit.body.family, size: 30, weight: 700, color: kit.bg, align: 'center', spacing: 2, transform: 'uppercase' }));
    }
    const h = headline(gen.kind === 'story' ? kit : condensedKit(kit), gen, { x: cx + 80, y: cy + 220, w: cw - 160, h: 380 }, text, gen.kind === 'story' ? 58 : 124, ink, 'center', gen.kind === 'story' ? 1.15 : 0.98);
    if (gen.kind !== 'story') h.font.transform = 'uppercase';
    L.push(h);
    L.push(...chipLayers(kit, (gen.refs || []).slice(0, 3), 0, cy + ch - 120, 'center').map(l => l.type === 'background'
      ? { ...l, color: kit.accent, opacity: 26 } : { ...l, color: ink }));
    if (footer) L.push(...footerLayer(kit, footer, { x: M, y: 1000, w: W - 2 * M, h: 34 }, 'center'));
    return L;
  }

  // Quote: oversized quote marks in the accent, cropped at the corners; the point in big condensed
  // caps between them; the label knocked out of an accent bar.
  function quote(gen, kit, style, footer) {
    const L = backgroundLayers(kit, style);
    const lines = (gen.lines || []).filter(Boolean);
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    L.push(textLayer('ss-quote-open', 'Opening quote', { x: 250, y: 40, w: 400, h: 420 }, '\u201C',
      { decor: true, family: CONDENSED.family, size: 420, color: kit.accent, opacity: 100, lineHeight: 1.0 }));
    L.push(textLayer('ss-quote-close', 'Closing quote', { x: W - 560, y: 700, w: 400, h: 420 }, '\u201D',
      { decor: true, family: CONDENSED.family, size: 420, color: kit.accent, opacity: 100, lineHeight: 1.0, align: 'right' }));
    const X = 300, TW = W - 2 * X;
    const tag = gen.label || (gen.kind === 'story' ? 'Introduction' : '');
    if (tag) {
      const tw = Math.round(tag.length * 28 * 0.66 + 48);
      L.push(shapeLayer('ss-tag', 'Tag', { x: X, y: 300, w: tw, h: 54 }, kit.accent));
      L.push(textLayer('ss-label', 'Label', { x: X, y: 309, w: tw, h: 40 }, tag,
        { family: kit.body.family, size: 28, weight: 800, color: kit.bg, align: 'center', spacing: 3, transform: 'uppercase' }));
    }
    const h = headline(gen.kind === 'story' ? kit : condensedKit(kit), gen, { x: X, y: 380, w: TW, h: 420 }, text,
      gen.kind === 'story' ? 64 : 150, kit.text, 'left', gen.kind === 'story' ? 1.2 : 0.95);
    if (gen.kind !== 'story') h.font.transform = 'uppercase';
    L.push(h);
    L.push(...refLine(kit, gen.refs || [], X, 840, 'left', kit.text));
    L.push(...footerLayer(kit, footer, { x: X, y: 990, w: 1100, h: 36 }));
    return L;
  }

  // ── after the owner's references (their composition, Kairo's fonts and the deck's own words) ──
  const lastWord = (text) => {
    const words = String(text || '').trim().split(/\s+/);
    if (words.length < 3) return [words.join(' '), ''];
    return [words.slice(0, -1).join(' '), words[words.length - 1].replace(/[.,;:!?]+$/, '')];
  };

  // Badge card: a solid colour field with the ghost word stacked down it in a deeper shade; a white
  // card, a ringed number badge on its top edge, the label in a coloured tag, and the scriptures
  // under a short rule ending in a dot.
  function badge(gen, kit, style, footer) {
    // The field: the look's deepest tone, lit a little from the middle; the ghost word in its brighter
    // shade (as in the reference: brighter than the field, not darker).
    const L = [{ ...backgroundLayers(kit, 'solid')[0], fill: 'gradient', color: kit.bg, color2: kit.bg,
      gradientStops: [{ pos: 0, color: kit.bg }, { pos: 100, color: kit.bg }],
      gradientGlows: [] }, ...vignette(backgroundLayers(kit, style).slice(1), 70)];
    const lines = (gen.lines || []).filter(Boolean);
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const ghost = ghostWord(text).slice(0, 5);
    if (ghost) for (let i = 0; i < 4; i++) L.push(textLayer(`ss-ghost-${i}`, `Ghost word ${i + 1}`, { x: 40, y: -120 + i * 330, w: W, h: 380 }, ghost,
      { decor: true, family: 'Montserrat', weight: 900, size: 380, color: mix(kit.bg2, '#FF0000', 0.12), opacity: 100, lineHeight: 1.0, spacing: -6 }));
    const cx = 500, cy = 140, cw = W - 1000, ch = 860;
    const paper = '#F7F7F7', ink = '#2B2B2B', body = '#3A3A3A';
    // One sweeping curve at the top-right; the other corners gently rounded, as in the reference.
    L.push({ ...shapeLayer('ss-card', 'Card', { x: cx, y: cy, w: cw, h: ch }, paper, { radius: 70 }), corners: [70, 300, 80, 80], lift: true });
    // A pale ghost letter inside the card, as the reference's big "O".
    // Wide enough for the widest capital, so the letter is never cut and the editor never resizes it.
    if (ghost) L.push(textLayer('ss-card-ghost', 'Card ghost', { x: cx + cw - 760, y: cy + 210, w: 760, h: 600 }, ghost[0],
      { decor: true, family: 'Montserrat', weight: 900, size: 640, color: '#EDEDED', lineHeight: 0.9, align: 'right' }));
    const mid = W / 2;
    if (gen.kind === 'point' && gen.n) {
      L.push(shapeLayer('ss-badge-ring', 'Badge ring', { x: mid - 100, y: cy - 100, w: 200, h: 200 }, kit.bg, { radius: 100 }));
      L.push(shapeLayer('ss-badge', 'Badge', { x: mid - 74, y: cy - 74, w: 148, h: 148 }, '#FFFFFF', { radius: 74 }));
      L.push(textLayer('ss-number', 'Number', { x: mid - 74, y: cy - 50, w: 148, h: 100 }, String(gen.n),
        { family: 'Montserrat', weight: 900, size: 84, color: kit.bg, align: 'center', lineHeight: 1.0 }));
    }
    const X = cx + 130, TW = cw - 260;
    // The title line: a dark bold word and its pair knocked out of an accent tag ("Ombi | langu").
    const [lead, tagWord] = gen.kind === 'point' ? ['Point', NUMBER_WORDS[gen.n] || String(gen.n || '')]
      : gen.kind === 'title' ? [(gen.label || 'This').split(' ')[0], (gen.label || 'series').split(' ').slice(1).join(' ') || 'series']
      : gen.kind === 'story' ? ['The', 'story'] : ['', ''];
    if (lead) {
      const lw = Math.round(lead.length * 70 * 0.68);
      L.push(textLayer('ss-label', 'Label', { x: X, y: cy + 180, w: lw + 20, h: 92 }, lead,
        { family: 'Montserrat', size: 70, weight: 800, color: ink, lineHeight: 1.1, spacing: -1 }));
      const tw = Math.round(tagWord.length * 62 * 0.62 + 44);
      L.push(shapeLayer('ss-tag', 'Tag', { x: X + lw + 30, y: cy + 182, w: tw, h: 88 }, kit.accent, { radius: 8 }));
      L.push(textLayer('ss-tag-text', 'Tag word', { x: X + lw + 30, y: cy + 190, w: tw, h: 76 }, tagWord,
        { family: 'Montserrat', size: 62, weight: 800, color: '#FFFFFF', align: 'center', lineHeight: 1.1 }));
    }
    L.push(headline({ ...kit, display: { family: 'Montserrat', weight: 500, width: 0.66, floor: 34 } }, gen, { x: X, y: cy + 300, w: TW - 120, h: 360 }, text,
      gen.kind === 'story' ? 48 : 58, body, 'left', 1.25));
    L.push(shapeLayer('ss-rule', 'Rule', { x: X, y: cy + ch - 168, w: 210, h: 4 }, kit.accent));
    L.push(shapeLayer('ss-dot', 'Dot', { x: X + 216, y: cy + ch - 175, w: 18, h: 18 }, kit.accent, { radius: 9 }));
    const refs = gen.refs || [];
    L.push(textLayer('ss-refs-label', 'Scriptures label', { x: X, y: cy + ch - 140, w: TW, h: 40 }, refs.length ? 'Scriptures' : (footer || ''),
      { family: 'Montserrat', size: 32, weight: 800, color: kit.accent }));
    if (refs.length) L.push(...refLine(kit, refs.slice(0, 2), X, cy + ch - 100, 'left', ink)
      .map(l => l.type === 'text' ? { ...l, font: { ...l.font, family: 'Montserrat', weight: 700, size: 32, letterSpacing: 0, transform: 'none' } } : l));
    return L;
  }

  // Worthy: the words in heavy slanted condensed caps, the last word in a big script across their
  // corner, the words ghosted large behind, and the church's name running up both edges.
  function worthy(gen, kit, style, footer) {
    const field = (kit.ramp && kit.ramp[kit.ramp.length - 1]) || kit.bg2;
    const L = [{ ...backgroundLayers(kit, 'solid')[0], fill: 'gradient', color: field, color2: kit.bg,
      gradientStops: [{ pos: 0, color: field }, { pos: 100, color: kit.bg }], angle: 150 }, ...vignette(backgroundLayers(kit, style).slice(1), 88)];
    const lines = (gen.lines || []).filter(Boolean);
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const [rest, last] = lastWord(text);
    const ghost = ghostWord(text);
    if (ghost) L.push(textLayer('ss-ghost', 'Ghost word', { x: 60, y: 200, w: W + 200, h: 800 }, ghost.charAt(0) + ghost.slice(1).toLowerCase(),
      { decor: true, family: SCRIPT.family, size: 560, color: kit.bg, opacity: 50, lineHeight: 1.0, rotation: -4 }));
    if (footer) {
      L.push(textLayer('ss-footer', 'Footer', { x: 80 - 420, y: 540 - 18, w: 840, h: 36 }, String(footer),
        { family: kit.body.family, size: 22, weight: 600, color: kit.text, opacity: 70, spacing: 6, transform: 'uppercase', align: 'center', rotation: -90 }));
      L.push(textLayer('ss-footer-right', 'Footer right', { x: W - 80 - 420, y: 540 - 18, w: 840, h: 36 }, String(footer),
        { family: kit.body.family, size: 22, weight: 600, color: kit.text, opacity: 70, spacing: 6, transform: 'uppercase', align: 'center', rotation: 90 }));
    }
    const h = headline(condensedKit(kit), gen, { x: 260, y: 200, w: W - 520, h: 470 }, gen.kind === 'story' ? text : rest, gen.kind === 'story' ? 76 : 150, gen.kind === 'story' ? kit.text : kit.accent, 'center', 0.95);
    h.font.italic = true;
    if (gen.kind !== 'story') h.font.transform = 'uppercase';
    L.push(h);
    const scriptWord = last ? last.charAt(0).toUpperCase() + last.slice(1).toLowerCase() : '';
    if (gen.kind !== 'story' && last) L.push(textLayer('ss-script', 'Script word', { x: 300, y: 560, w: W - 600, h: 260 }, scriptWord,
      { family: SCRIPT.family, size: 230, color: kit.text, align: 'center', lineHeight: 1.0, rotation: -6 }));
    const refs = gen.refs || [];
    const sub = gen.kind === 'title' ? lines.slice(1).join(' ') : refs.slice(0, 2).join('  ·  ');
    if (sub) L.push(textLayer('ss-subtitle', 'Subtitle', { x: 300, y: 860, w: W - 600, h: 50 }, sub,
      { family: kit.body.family, size: 34, weight: 600, color: kit.text, opacity: 90, align: 'center' }));
    return L;
  }

  // Glow: a deep ladder with a diagonal sweep of light and grain; the words tight, heavy and centred,
  // one small line under them.
  function glow(gen, kit, style, footer) {
    const ramp = kit.ramp && kit.ramp.length > 1 ? kit.ramp : [kit.bg, kit.bg2];
    const top = ramp[ramp.length - 1];
    const L = [{ ...backgroundLayers(kit, 'solid')[0], fill: 'gradient', color: ramp[0], color2: ramp[0],
      angle: kit.sweep ? kit.sweep.angle : 135,
      gradientStops: kit.sweep ? kit.sweep.stops : [{ pos: 0, color: ramp[0] }, { pos: 100, color: ramp[0] }],
      gradientGlows: kit.glows || [{ x: 100, y: 0, rx: 55, ry: 70, color: top }, { x: 62, y: 38, rx: 30, ry: 18, color: top }, { x: 0, y: 100, rx: 40, ry: 50, color: ramp[1] || top }] },
      ...vignette(backgroundLayers(kit, style).slice(1), kit.sweep ? 0 : 70)]  // light at the edge stays bright;
    const lines = (gen.lines || []).filter(Boolean);
    const text = gen.kind === 'title' ? lines[0] || '' : lines.join(' ');
    const tight = { ...kit, display: { family: 'Inter', weight: 800, width: 0.56 } };
    const h = headline(tight, gen, { x: 260, y: 300, w: W - 520, h: 420 }, text, gen.kind === 'story' ? 70 : 128, kit.text, 'center', 1.0);
    h.font.letterSpacing = gen.kind === 'story' ? -1 : -4;
    h.shadow = { enabled: true, color: kit.accent, opacity: 45, blur: 34, x: 0, y: 0 };   // the words glow a little, as in the reference
    // The look's own folds of light, over the background and under the grain.
    // Folds marked `top` go over the vignette (a plane of light isn't shaded by it), the rest under.
    (kit.folds || []).forEach((f) => L.splice(f.top ? L.findIndex(l => l.id === 'ss-grain') : 1, 0, { id: f.id, type: 'background', name: f.name, visible: true, fill: 'gradient',
      color: f.stops[0].color, color2: f.stops[f.stops.length - 1].color, gradientStops: f.stops, angle: f.angle, opacity: f.opacity,
      radius: 0, clip: f.clip, pos: { x: 0, y: 0, w: W, h: H } }));
    L.push(h);
    const refs = gen.refs || [];
    const sub = gen.kind === 'title' ? lines.slice(1).join(' ') : gen.label && gen.kind !== 'story' ? `${gen.label}${refs.length ? '  ·  ' + refs[0] : ''}` : refs[0] || '';
    if (sub) L.push(textLayer('ss-subtitle', 'Subtitle', { x: 300, y: 760, w: W - 600, h: 44 }, sub,
      { family: 'Inter', size: 30, weight: 600, color: kit.text, opacity: 85, align: 'center' }));
    L.push(...footerLayer(kit, footer, { x: M, y: 990, w: W - 2 * M, h: 36 }, 'center'));
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
    const themeLine = header.find(h => /theme/i.test(h.text)) || header[outline.titleIndex != null ? outline.titleIndex : 1] || header[0];
    const seriesLine = header.find(h => /series|topic|title/i.test(h.text) && h !== themeLine);
    const parts = [];
    header.forEach((h, i) => {
      if (h === themeLine) parts.push({ id: 'title', kind: 'title', text: h.text, refs: [], on: true });
      else if (h !== seriesLine) parts.push({ id: `h${i}`, kind: 'line', text: h.text, refs: h.refs || [], on: false });
    });
    (outline.items || []).forEach((it, i) => {
      parts.push({ id: `i${i}`, kind: it.kind, text: it.text, refs: it.refs || [], n: it.n, notes: it.notes, on: it.kind !== 'note' });
      (it.verses || []).forEach((v, j) => parts.push({ id: `i${i}v${j}`, kind: 'verse', text: v.text, refs: [v.ref], on: !!(outline.how && outline.how.versesOn) }));
    });
    (outline.other || []).forEach((t, i) => parts.push({ id: `o${i}`, kind: 'line', text: t, refs: [], on: false }));
    return parts;
  }

  // The deck for an outline: the parts that are on, each in its layout. Typed verses are left out
  // unless picked (the verses go up live from Kairo's Bible); the notes go to the last slide's notes.
  //   opts: { kit, style, footer, include: Set of part ids (default: the parts on by default),
  //           kinds: { part id: 'point' | 'section' | 'story' } for lines the operator re-labelled }
  function build(outline, opts = {}) {
    const kit = withColors(kitById(opts.kit), opts.colors);
    const style = BACKGROUNDS.some(b => b.id === opts.style) ? opts.style : 'glow';
    const layout = layoutOf(opts.layout);
    const header = (outline.header || []).filter(h => h && h.text);
    const footerLine = outline.titleIndex === 0 ? header[1] : header[0];
    const footer = opts.footer != null ? opts.footer : (footerLine ? footerLine.text.replace(/\.?\s*inc\.?$/i, '').trim() : '');
    // What the operator said a line is, over what the reader made of it.
    const parts = partsOf(outline).map(p => (opts.kinds && opts.kinds[p.id] ? { ...p, kind: opts.kinds[p.id] } : p));
    const on = (p) => (opts.include ? opts.include.has(p.id) : p.on);
    const blocks = [];
    const afterColon = (t) => String(t || '').replace(/^[^:]{1,40}:\s*/, '').trim();
    const themeLine = header.find(h => /theme/i.test(h.text)) || header[outline.titleIndex != null ? outline.titleIndex : 1] || header[0];
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
        make({ kind: 'story', label: 'Introduction', lines: [cut], refs: refsOf(p.refs), shortened: cut !== p.text }, { notes: p.text });
      } else if (p.kind === 'point') {
        n += 1;
        const { text, shortened } = shortenPoint(p.text);
        make({ kind: 'point', n, label: `Point ${n} of ${total}`, lines: [text], refs: refsOf(p.refs), shortened }, { notes: p.notes || p.text });
      } else if (p.kind === 'section' || p.kind === 'closing') {
        make({ kind: p.kind, label: '', lines: [p.text], refs: [] });
      } else {
        // A line, note or typed verse the operator chose: the quieter left-hand layout.
        const whole = p.kind === 'verse' && outline.how && outline.how.versesOn;   // a verse the document wrote out is read whole
        const cut = whole ? p.text : opening(p.text, 28);
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
  // The deck made again from its choices. A slide changed by hand is kept as it is, unless
  // `restyleEdited`: then it takes the new look and layout too, keeping its own words.
  function regenerate(item, { restyleEdited = false } = {}) {
    const d = item.sermon;
    const fresh = build(d.outline, { kit: d.kit, style: d.style, layout: d.layout, colors: d.colors, footer: d.footer, include: new Set(d.include), kinds: d.kinds });
    const kept = new Map((item.blocks || []).filter(b => b.gen && b.gen.edited).map(b => [b.gen.part, b]));
    const kit = withColors(kitById(d.kit), d.colors);
    return fresh.map(b => {
      const k = kept.get(b.gen.part);
      if (k && !restyleEdited) return k;
      if (k) {
        const gen = { ...b.gen, label: k.gen.label, lines: k.gen.lines, refs: k.gen.refs, shortened: k.gen.shortened, tooLong: false };
        b = makeBlock(gen, kit, d.style, d.footer, { notes: k.notes ?? b.notes, presenterNote: k.presenterNote ?? b.presenterNote });
      }
      // What the operator applied to every slide stays on every new slide too.
      for (const [id, props] of Object.entries(d.common || {})) {
        const l = b.layers.find(x => x.id === id);
        if (l) for (const [k, v] of Object.entries(props)) { if (v === null) delete l[k]; else l[k] = JSON.parse(JSON.stringify(v)); }
      }
      return b;
    });
  }

  // What a change to a part carries to the same part on other slides. A part is shared when the same
  // layer (by id) is on other slides too: the footer, the label, the card, a ghost word, a scripture
  // chip... Its words stay each slide's own (only the footer's words are the same everywhere); its look
  // and place carry.
  const TEXT_STYLE = ['font', 'color', 'opacity', 'align', 'pos', 'shadow', 'outline', 'rotation', 'gradient', 'fill', 'color2', 'gradientStops', 'angle'];
  const FILL = ['fill', 'color', 'color2', 'gradientStops', 'gradientGlows', 'angle', 'opacity', 'radius', 'src', 'corners', 'clip',
    'stroke', 'drop', 'glass', 'glassBlur', 'lift', 'shape', 'rotation', 'dim'];
  const OWN_WORDS = { 'ss-footer': true };
  function carried(layer) {
    if (!layer) return [];
    if (layer.type === 'text') return OWN_WORDS[layer.id] ? ['customText', ...TEXT_STYLE] : TEXT_STYLE;
    if (layer.type === 'background') return [...FILL, ...(layer.pos ? ['pos'] : [])];
    return ['opacity', 'pos', 'fit', 'rotation', 'blendMode'];
  }
  // A slide's parts that changed: [{ id, name, props }] with the new values to carry.
  function commonChanges(before, after) {
    const out = [];
    for (const b of after || []) {
      const a = (before || []).find(l => l.id === b.id);
      if (!a || !b.id) continue;
      const props = {};
      for (const k of carried(b)) if (JSON.stringify(a[k]) !== JSON.stringify(b[k])) props[k] = b[k] === undefined ? null : JSON.parse(JSON.stringify(b[k]));
      if (Object.keys(props).length) out.push({ id: b.id, name: String(b.name || 'part').toLowerCase(), props });
    }
    return out;
  }
  // How many other slides of the deck have this part.
  const sharedWith = (item, id, fromIndex) => (item.blocks || []).filter((b, i) => i !== fromIndex && (b.layers || []).some(l => l.id === id)).length;
  // Those changes onto the same part of other slides: every slide ('all'), or only the slides after
  // this one ('after'). Applied to all, a change also becomes the deck's own, so slides made later
  // (a look or contents change) keep it.
  function applyCommon(item, changes, fromIndex, scope = 'all') {
    const d = item.sermon;
    if (scope === 'all') d.common = { ...(d.common || {}) };
    for (const c of changes) {
      if (scope === 'all') d.common[c.id] = { ...(d.common[c.id] || {}), ...c.props };
      (item.blocks || []).forEach((b, i) => {
        if (i === fromIndex || (scope === 'after' && i < fromIndex)) return;
        const l = (b.layers || []).find(x => x.id === c.id);
        if (!l) return;
        for (const [k, v] of Object.entries(c.props)) { if (v === null) delete l[k]; else l[k] = JSON.parse(JSON.stringify(v)); }
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
    // A layout that sets the last word apart in script (Worthy) keeps the split.
    if (block.layers.some(l => l.id === 'ss-script')) {
      const [rest, last] = lastWord(gen.kind === 'title' ? (gen.lines || [])[0] : (gen.lines || []).join(' '));
      set('ss-headline', rest); set('ss-script', last);
    }
    (gen.refs || []).forEach((r, i) => set(`ss-chip-${i}-text`, r));
    return { ...block, gen, text: serialize(gen), label: gen.label || (gen.lines || [])[0] || block.label };
  }

  const api = { W, H, KITS, BACKGROUNDS, LAYOUTS, DESIGNS, kitById, withColors, kitWarnings, contrast, bookName, normaliseRef,
    docxParagraphs, textParagraphs, labelParagraphs, outlineFrom, partsOf, readDocx, readText, shortenPoint, build, newDeck, regenerate, commonChanges, applyCommon, sharedWith, withText, serialize, parse, issues };
  if (typeof window !== 'undefined') window.KairoSermonSlides = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})();
