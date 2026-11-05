// "Try it" on the site (owner, 2026-10-31): say a verse and see it come up, the way KAIRO does
// in a service. The browser's own speech recognition does the listening; the matching runs on
// this page against the KJV (kjv.json). A taste of the app, not its engine: KAIRO itself hears
// much more (ranges read on, a verse when only its chapter was named, paraphrases).
(function () {
  const root = document.getElementById('try');
  if (!root) return;
  const micBtn = root.querySelector('#try-mic');
  const micLabel = root.querySelector('#try-mic-label');
  const heardEl = root.querySelector('#try-heard');
  const screenText = root.querySelector('#try-screen-text');
  const screenRef = root.querySelector('#try-screen-ref');
  const howEl = root.querySelector('#try-how');
  const typed = root.querySelector('#try-typed');
  const noteEl = root.querySelector('#try-note');

  // ── The Bible ────────────────────────────────────────────────────────
  let bible = null;          // [[name, [[verse, …], …]], …]
  let loading = null;
  const index = new Map();   // word → [verse key, …]
  const verses = [];         // [{ b, c, v, words }]
  const STOP = new Set('the and of to that in he shall unto i his a for they be is him not them it with all thou thy was god which my me said but ye their have will thee from as are when this out were by you up there hath then had people on an or so who at if now one us her what came our no she into been also may upon your these let any even we let'.split(' '));

  function words(s) {
    return s.toLowerCase().replace(/[’']/g, '').replace(/[^a-z0-9\s]/g, ' ').split(/\s+/).filter(Boolean);
  }

  function load() {
    if (loading) return loading;
    loading = fetch('demo/kjv.json').then(r => r.json()).then(data => {
      bible = data;
      data.forEach(([, chapters], b) => chapters.forEach((vs, c) => vs.forEach((text, v) => {
        const key = verses.length;
        const w = words(text);
        verses.push({ b, c, v, w });
        for (const word of new Set(w)) {
          if (STOP.has(word)) continue;
          let list = index.get(word);
          if (!list) index.set(word, list = []);
          list.push(key);
        }
      })));
    });
    return loading;
  }

  // ── Spoken numbers ───────────────────────────────────────────────────
  const ONES = { zero: 0, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9, ten: 10, eleven: 11, twelve: 12, thirteen: 13, fourteen: 14, fifteen: 15, sixteen: 16, seventeen: 17, eighteen: 18, nineteen: 19 };
  const TENS = { twenty: 20, thirty: 30, forty: 40, fifty: 50, sixty: 60, seventy: 70, eighty: 80, ninety: 90 };
  // "one hundred and nineteen" → 119, "twenty three" → 23, and "three sixteen" → 3 16 (a new
  // number starts where the last can't go on). "Psalm one ten" stays 1 10: the reference decides.
  function numbersToDigits(text) {
    const out = [];
    const ws = text.split(/\s+/);
    let i = 0;
    while (i < ws.length) {
      const first = ws[i];
      if (!(first in ONES) && !(first in TENS) && first !== 'hundred') { out.push(first); i++; continue; }
      let cur = 0;
      for (; i < ws.length; i++) {
        const x = ws[i];
        if (x === 'hundred') cur = (cur || 1) * 100;
        else if (x === 'and' && cur >= 100 && cur % 100 === 0 && (ws[i + 1] in ONES || ws[i + 1] in TENS)) continue;
        else if (x in TENS) {
          if (cur % 100 === 0) cur += TENS[x];
          else break;
        } else if (x in ONES) {
          const n = ONES[x];
          if (cur === 0 && !(i > 0 && ws[i - 1] === 'zero')) cur = n;
          else if (cur % 100 === 0 && cur > 0) cur += n;
          else if (cur % 10 === 0 && cur % 100 >= 20 && n < 10) cur += n;
          else break;
        } else break;
      }
      out.push(String(cur));
    }
    return out.join(' ');
  }

  // ── Book names ───────────────────────────────────────────────────────
  let books = null;  // [[pattern words, book index]], longest first
  function bookNames() {
    if (books) return books;
    const ord = { '1': ['1', 'first', '1st'], '2': ['2', 'second', '2nd'], '3': ['3', 'third', '3rd'] };
    const list = [];
    bible.forEach(([name], b) => {
      const m = name.match(/^([123]) (.+)$/);
      const base = (m ? m[2] : name).toLowerCase();
      const forms = [base];
      if (base === 'psalms') forms.push('psalm');
      if (base === 'song of solomon') forms.push('song of songs', 'songs');
      if (base === 'revelation') forms.push('revelations');
      for (const f of forms) {
        if (m) ord[m[1]].forEach(o => list.push([`${o} ${f}`, b]));
        else list.push([f, b]);
      }
    });
    books = list.sort((a, b) => b[0].length - a[0].length);
    return books;
  }

  // The last "Book chapter (verse (to verse))" said in `text`.
  function lastReference(text) {
    const t = numbersToDigits(words(text).join(' ')).replace(/\b(\d+) (\d+) (\d+)\b/g, '$1 $2 $3');
    const alt = bookNames().map(([n]) => n.replace(/ /g, '\\s+')).join('|');
    const re = new RegExp(`\\b(${alt})\\s+(?:chapter\\s+)?(\\d+)(?:\\s*(?:verses?|v|and verse|from verse)?\\s+(\\d+)(?:\\s+(?:to|through|and|till|until)\\s+(?:verse\\s+)?(\\d+))?)?`, 'g');
    let found = null, m;
    while ((m = re.exec(t))) found = m;
    if (!found) return null;
    const name = found[1].replace(/\s+/g, ' ');
    const b = bookNames().find(([n]) => n === name)[1];
    const chapters = bible[b][1];
    let c = +found[2] - 1;
    let verseFrom = found[3], verseTo = found[4];
    // "Psalm one ten, one to three": a psalm past 100 said as "one" and the rest.
    if (chapters.length > 100 && c === 0 && verseFrom && +verseFrom > chapters[0].length && chapters[100 + +verseFrom - 1]) {
      c = 100 + +verseFrom - 1;
      const more = t.slice(found.index + found[0].length).match(/^\s+(?:verses?\s+)?(\d+)(?:\s+(?:to|through|and)\s+(?:verse\s+)?(\d+))?/);
      verseFrom = more ? more[1] : verseTo;
      verseTo = more ? more[2] : undefined;
    }
    if (!chapters[c]) return null;
    const from = verseFrom ? +verseFrom - 1 : 0;
    if (from < 0 || from >= chapters[c].length) return null;
    let to = verseTo ? +verseTo - 1 : from;
    if (to < from || to >= chapters[c].length) to = from;
    return { b, c, from, to: Math.min(to, from + 2), end: found.index + found[0].length, len: t.length };
  }

  // ── Quoted words ─────────────────────────────────────────────────────
  // The verse whose wording the last words said follow closely: three-word runs in common,
  // candidates from the rarest words said.
  function quoted(text) {
    const said = words(text).slice(-24);
    if (said.length < 5) return null;
    const tri = s => { const out = new Set(); for (let i = 0; i + 2 < s.length; i++) out.add(s[i] + ' ' + s[i + 1] + ' ' + s[i + 2]); return out; };
    const saidTri = tri(said);
    const rare = [...new Set(said)].filter(w => index.has(w)).sort((a, b) => index.get(a).length - index.get(b).length).slice(0, 4);
    const seen = new Set();
    let best = null;
    for (const w of rare) {
      for (const key of index.get(w).slice(0, 3000)) {
        if (seen.has(key)) continue;
        seen.add(key);
        const vt = tri(verses[key].w);
        let hit = 0;
        for (const t of vt) if (saidTri.has(t)) hit++;
        const share = hit / Math.min(vt.size, saidTri.size || 1);
        if (hit >= 3 && share >= 0.45 && (!best || hit > best.hit)) best = { key, hit };
      }
    }
    return best && verses[best.key];
  }

  // ── Showing it ───────────────────────────────────────────────────────
  let shown = '';
  function show(b, c, from, to, how) {
    const key = `${b}:${c}:${from}-${to}`;
    if (key === shown) return;
    shown = key;
    const [name, chapters] = bible[b];
    const vs = chapters[c].slice(from, to + 1);
    const length = vs.join(' ').length;
    screenText.classList.toggle('long', length > 200 && length <= 360);
    screenText.classList.toggle('longer', length > 360);
    screenText.innerHTML = vs.map((t, i) => (vs.length > 1 ? `<sup>${from + i + 1}</sup>` : '') + escapeHtml(t)).join(' ');
    screenRef.textContent = `${name} ${c + 1}:${from + 1}${to > from ? '–' + (to + 1) : ''} (KJV)`;
    howEl.textContent = how;
    root.querySelector('.try-screen').classList.remove('try-pop');
    void root.offsetWidth;
    root.querySelector('.try-screen').classList.add('try-pop');
  }

  function escapeHtml(s) {
    return s.replace(/[&<>"]/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]);
  }

  // What was heard so far: a reference wins when it's the newest thing said; else the words.
  function detect(text) {
    if (!bible) return;
    const ref = lastReference(text);
    if (ref && ref.len - ref.end < 40) {
      show(ref.b, ref.c, ref.from, ref.to, 'Found by the reference');
      return;
    }
    const after = ref ? numbersToDigits(words(text).join(' ')).slice(ref.end) : text;
    const q = quoted(after);
    if (q) show(q.b, q.c, q.v, q.v, 'Found by its words, no reference');
  }

  // ── Listening ────────────────────────────────────────────────────────
  const Recognition = window.SpeechRecognition || window.webkitSpeechRecognition;
  let rec = null, listening = false, finalText = '';

  function setListening(on) {
    listening = on;
    micBtn.classList.toggle('on', on);
    micLabel.textContent = on ? 'Listening… tap to stop' : 'Tap and say a verse';
  }

  if (!Recognition) {
    micBtn.disabled = true;
    micLabel.textContent = 'Speaking needs Chrome, Edge or Safari';
    noteEl.textContent = 'This browser can’t listen. Type instead, as a preacher would say it.';
  }

  // Listening starts the moment the button is tapped: a browser may refuse the microphone to a
  // page that starts it later (after the Bible downloads). The Bible loads alongside, and what
  // was heard meanwhile is matched once it's in.
  let lastHeard = '';
  micBtn.addEventListener('click', () => {
    if (!Recognition) return;
    if (listening) { setListening(false); rec && rec.stop(); return; }
    load().then(() => lastHeard && detect(lastHeard));
    rec = new Recognition();
    rec.lang = 'en-US';
    rec.continuous = true;
    rec.interimResults = true;
    finalText = '';
    rec.onresult = e => {
      let interim = '';
      for (let i = e.resultIndex; i < e.results.length; i++) {
        if (e.results[i].isFinal) finalText += ' ' + e.results[i][0].transcript;
        else interim += ' ' + e.results[i][0].transcript;
      }
      const all = (finalText + ' ' + interim).trim();
      heardEl.textContent = all.split(/\s+/).slice(-40).join(' ');
      lastHeard = all.split(/\s+/).slice(-60).join(' ');
      detect(lastHeard);
    };
    // Anything but a pause ends listening and says why, rather than failing silently.
    rec.onerror = e => {
      if (e.error === 'no-speech' || e.error === 'aborted') return;
      setListening(false);
      micLabel.textContent = 'Tap to try again';
      noteEl.textContent = {
        'not-allowed': 'The microphone wasn’t allowed. Allow it in the address bar and tap again, or type instead.',
        'service-not-allowed': 'This browser won’t let the page listen. Try Chrome or Safari, or type instead.',
        'audio-capture': 'No microphone was found. Plug one in and tap again, or type instead.',
        'network': 'This browser couldn’t reach its speech service. Try Chrome or Safari, or type instead.',
      }[e.error] || `Listening stopped (${e.error}). Tap to try again, or type instead.`;
    };
    // Browsers stop after a pause: carry on while the visitor hasn't stopped it.
    rec.onend = () => { if (listening) try { rec.start(); } catch (_) {} };
    rec.onstart = () => { if (!lastHeard) heardEl.textContent = 'Listening… say a verse.'; };
    try { rec.start(); setListening(true); } catch (err) {
      setListening(false);
      noteEl.textContent = `Listening couldn\u2019t start (${err.message || err}). Type instead.`;
    }
  });

  let typedTimer = null;
  typed.addEventListener('input', () => {
    clearTimeout(typedTimer);
    typedTimer = setTimeout(async () => {
      await load();
      detect(typed.value);
    }, 250);
  });

  root.querySelectorAll('[data-try]').forEach(chip => chip.addEventListener('click', async () => {
    await load();
    typed.value = chip.dataset.try;
    detect(chip.dataset.try);
  }));
})();
