// Settings → Bible → Your Bibles: Bibles the church has the right to use, from a file (read and
// checked by the engine, bible_import.rs) or from its own API.Bible account (api_bible.rs). Each
// joins the Bible version lists under "Your Bibles". API.Bible leads: it is how a church gets its
// Bibles free and legally, instead of a file found online.
(() => {
  const $ = (id) => document.getElementById(id);
  const list = $('bible-list');
  if (!list) return;
  const base = () => (typeof SERVER !== 'undefined' ? SERVER : '');
  const get = (path) => fetch(`${base()}${path}`).then((r) => r.json());
  const post = (path, body, raw) => fetch(`${base()}${path}`, raw ? { method: 'POST', body } : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).then((r) => r.json());
  const status = $('bible-import-status');
  const say = (t, bad) => { status.textContent = t || ''; status.classList.toggle('bi-bad', !!bad); };
  // ── The Bible version lists (Settings and the toolbar) ──
  function yours(sel) {
    let g = sel.querySelector('optgroup[data-yours]');
    if (!g) {
      g = document.createElement('optgroup');
      g.label = 'Your Bibles';
      g.dataset.yours = '';
      sel.append(g);
    }
    return g;
  }
  function syncVersions(codes) {
    ['translation-select-settings', 'translation-select'].map($).filter(Boolean).forEach((sel) => {
      const g = yours(sel);
      [...g.querySelectorAll('option')].filter((o) => !codes.includes(o.value)).forEach((o) => {
        if (sel.value === o.value) sel.value = 'KJV';
        o.remove();
      });
      codes.forEach((c) => { if (![...sel.options].some((o) => o.value === c)) g.append(new Option(c, c)); });
      g.hidden = !codes.length;
    });
  }

  // ── The list of added Bibles ──
  let polling = null;
  let apiConnected = false;
  let keyHint = '';
  async function refresh() {
    const [files, api] = await Promise.all([get('/api/bibles/mine').catch(() => ({})), get('/api/api-bible/mine').catch(() => ({}))]);
    const rows = [
      ...(files.bibles || []).map((b) => ({ ...b, kind: 'file', source: `File · ${Number(b.verses || 0).toLocaleString()} verses` })),
      ...(api.bibles || []).map((b) => {
        const done = b.total && b.chapters >= b.total;
        return { ...b, kind: 'api', downloading: !done, source: done ? `API.Bible${b.abbreviation && b.abbreviation !== b.code ? ` · shows as ${b.abbreviation}` : ''} · works offline` : `API.Bible · downloading ${Math.min(99, Math.floor((100 * (b.chapters || 0)) / (b.total || 1)))}%` };
      }),
    ].sort((a, b) => a.code.localeCompare(b.code));
    list.replaceChildren(...rows.map(row));
    renderCredits(rows);
    $('bible-list-empty').hidden = rows.length > 0;
    syncVersions(rows.map((r) => r.code));
    apiConnected = !!api.connected;
    keyHint = api.keyHint || '';
    showApiState();
    clearTimeout(polling);
    if (rows.some((r) => r.downloading)) polling = setTimeout(refresh, 3000);
  }
  function row(b) {
    const li = document.createElement('li');
    const code = document.createElement('span');
    code.className = 'bi-code-tag';
    code.textContent = b.code;
    const name = document.createElement('span');
    name.className = 'bi-name-text';
    name.textContent = b.name && b.name !== b.code ? b.name : '';
    const src = document.createElement('span');
    src.className = 'bi-source';
    src.textContent = b.source;
    const rm = document.createElement('button');
    rm.type = 'button';
    rm.className = 'bi-trash';
    rm.title = `Remove ${b.code}`;
    rm.setAttribute('aria-label', `Remove ${b.code}`);
    rm.innerHTML = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><line x1="10" y1="11" x2="10" y2="17"/><line x1="14" y1="11" x2="14" y2="17"/></svg>';
    rm.addEventListener('click', async () => {
      const r = await post(b.kind === 'api' ? '/api/api-bible/remove' : '/api/bibles/remove', { code: b.code }).catch(() => ({}));
      if (r.ok) { say(`${b.code} removed.`); refresh(); }
    });
    li.append(code, name, src, rm);
    // API.Bible asks every Bible it serves to carry its copyright notice and a link back to it.
    if (b.kind === 'api') {
      const credit = document.createElement('span');
      credit.className = 'bi-credit';
      if (b.copyright) credit.append(`${b.copyright} `);
      const a = document.createElement('a');
      a.href = 'https://api.bible';
      a.target = '_blank';
      a.rel = 'noopener';
      a.textContent = 'api.bible';
      credit.append('Provided by ', a);
      li.append(credit);
    }
    return li;
  }


  // ── About → Bible copyrights: every Bible Kairo can show, with its notice ──
  const BUNDLED = [
    ['KJV', 'King James Version. Public domain outside the United Kingdom, where the rights are vested in the Crown.'],
    ['NLT', 'Holy Bible, New Living Translation, copyright © 1996, 2004, 2015 by Tyndale House Foundation. All rights reserved.'],
    ['NIV', 'The Holy Bible, New International Version®, NIV® Copyright © 1973, 1978, 1984, 2011 by Biblica, Inc.® All rights reserved worldwide.'],
    ['ESV', 'The Holy Bible, English Standard Version® (ESV®), copyright © 2001 by Crossway, a publishing ministry of Good News Publishers. All rights reserved.'],
    ['NASB', 'New American Standard Bible®, copyright © 1960, 1971, 1977, 1995, 2020 by The Lockman Foundation. All rights reserved.'],
  ];
  function renderCredits(rows) {
    const dl = $('about-credits');
    if (!dl) return;
    const items = [...BUNDLED];
    if (typeof LANG_PACKS !== 'undefined') LANG_PACKS.filter((p) => p.code !== 'en').forEach((p) => items.push([p.name, p.translations]));
    rows.forEach((b) => items.push([b.kind === 'api' ? (b.abbreviation || b.code) : b.code, b.kind === 'api' ? { text: b.copyright || b.name || '', api: true } : `${b.name || b.code}. Added by your church from its own file.`]));
    dl.replaceChildren(...items.flatMap(([k, v]) => {
      const dt = document.createElement('dt');
      dt.textContent = k;
      const dd = document.createElement('dd');
      if (typeof v === 'string') dd.textContent = v;
      else {
        dd.append(`${v.text} Provided by `);
        const a = document.createElement('a');
        a.href = 'https://api.bible';
        a.target = '_blank';
        a.rel = 'noopener';
        a.textContent = 'API.Bible';
        dd.append(a, '.');
      }
      return [dt, dd];
    }));
  }

  // ── From a file ──
  const fileIn = $('bible-import-file');
  const filePanel = $('bible-file-panel');
  let picked = null;
  function showPanel(p) {
    filePanel.hidden = p !== filePanel;
    $('bible-file-start').hidden = p === filePanel;
  }
  $('bible-import-btn').addEventListener('click', () => fileIn.click());
  fileIn.addEventListener('change', () => {
    picked = fileIn.files[0] || null;
    fileIn.value = '';
    if (!picked) return;
    showPanel(filePanel);
    $('bible-file-name').textContent = picked.name;
    say('');
  });
  document.querySelectorAll('#bible-import [data-close]').forEach((x) => x.addEventListener('click', () => { showPanel(null); picked = null; }));
  $('bible-file-add').addEventListener('click', async () => {
    if (!picked) return;
    const add = $('bible-file-add');
    add.disabled = true;
    say('Reading…');
    const url = `/api/bibles/import?filename=${encodeURIComponent(picked.name)}`;
    const r = await post(url, picked, true).catch(() => ({ error: 'Kairo could not reach its engine.' }));
    add.disabled = false;
    if (!r.ok) return say(r.error || 'Kairo could not read this file.', true);
    showPanel(null);
    picked = null;
    say(`${r.code} added: ${r.verses.toLocaleString()} verses, ${r.books} books.${r.skipped ? ` ${r.skipped} didn't fit the usual layout and were left out.` : ''}`);
    refresh();
  });

  // ── From the church's API.Bible account ──
  let bibles = null;
  let chosen = null;
  // Not connected: the invitation and the key field. Connected: which key, Replace or Remove it, and
  // the finder to add a Bible. Replacing shows the key field again over the connected account.
  let replacing = false;
  function showApiState() {
    $('api-bible-signin').hidden = apiConnected && !replacing;
    $('api-bible-account').hidden = !apiConnected || replacing;
    $('api-bible-cancel-replace').hidden = !replacing;
    $('api-bible-hint').textContent = keyHint ? `Connected · key …${keyHint}` : 'Connected';
    if (apiConnected && !bibles) loadBibles();
  }
  $('api-bible-replace').addEventListener('click', () => { replacing = true; showApiState(); $('api-bible-key').focus(); });
  $('api-bible-cancel-replace').addEventListener('click', () => { replacing = false; $('api-bible-key').value = ''; showApiState(); });
  async function loadBibles() {
    search.placeholder = 'Loading your Bibles…';
    const r = await get('/api/api-bible/bibles').catch(() => ({}));
    search.placeholder = 'Add a Bible: NIV, Message, Catholic…';
    if (!r.ok) return say(r.error || 'Kairo could not reach API.Bible.', true);
    bibles = r.bibles || [];
    if (!options.hidden) render();
  }
  $('api-bible-connect').addEventListener('click', async () => {
    const btn = $('api-bible-connect');
    btn.disabled = true;
    say('Checking the key and which Bibles it can read…');
    const r = await post('/api/api-bible/connect', { key: $('api-bible-key').value }).catch(() => ({ error: 'Kairo could not reach its engine.' }));
    btn.disabled = false;
    if (!r.ok) return say(r.error || 'That key was not accepted.', true);
    $('api-bible-key').value = '';
    bibles = r.bibles || [];
    replacing = false;
    say(`Connected. ${bibles.length} Bibles are available to add.`);
    await refresh();
    search.focus();
  });
  const disconnect = $('api-bible-disconnect');
  disconnect.addEventListener('click', async () => {
    if (!disconnect.dataset.sure) {
      disconnect.dataset.sure = '1';
      // A second press confirms: the key goes with every Bible it brought.
      const ask = disconnect.querySelector('.bi-confirm');
      ask.textContent = 'Remove key and its Bibles?';
      ask.hidden = false;
      setTimeout(() => { delete disconnect.dataset.sure; ask.hidden = true; }, 5000);
      return;
    }
    delete disconnect.dataset.sure;
    disconnect.querySelector('.bi-confirm').hidden = true;
    const r = await post('/api/api-bible/remove', { all: true }).catch(() => ({}));
    if (r.ok) { bibles = null; chosen = null; replacing = false; say('Key removed, with the Bibles it brought.'); await refresh(); }
  });

  // The Bible picker: type to find, grouped the way a church looks for a Bible.
  const search = $('api-bible-search');
  const options = $('api-bible-options');
  const POPULAR = ['NIV', 'NLT', 'MSG', 'ESV', 'NKJV', 'KJV', 'NASB', 'AMP', 'CSB', 'BSB', 'NRSV', 'CEV', 'GNT', 'NLTUK'];
  const ORDER = ['Popular', 'More Bibles', 'Catholic and Orthodox', 'Old Testament only'];
  const groupOf = (b) => {
    if (b.coverage === 'ot') return 'Old Testament only';
    if (/catholic|orthodox/i.test(b.edition || '') || /CE$/.test(b.abbreviation || '')) return 'Catholic and Orthodox';
    if (POPULAR.includes(b.abbreviation)) return 'Popular';
    return 'More Bibles';
  };
  const rank = (b) => POPULAR.indexOf(b.abbreviation) + 1 || 99;
  let active = -1;
  let shown = [];
  function render() {
    const q = search.value.trim().toLowerCase();
    // One entry per Bible: the same Bible listed for several traditions (KJV "Ecumenical" and
    // "Protestant") is one choice; only Catholic and Orthodox editions, whose books differ, stay apart.
    const pref = (e) => ({ protestant: 0, '': 1, ecumenical: 2 }[String(e || '').toLowerCase()] ?? 3);
    const one = new Map();
    for (const b of bibles || []) {
      const g = groupOf(b);
      const key = g === 'Catholic and Orthodox' ? `${b.abbreviation}|${b.edition}` : `${g}|${b.abbreviation}`;
      const have = one.get(key);
      if (!have || pref(b.edition) < pref(have.edition)) one.set(key, b);
    }
    shown = [...one.values()]
      .filter((b) => !q || `${b.abbreviation} ${b.name} ${b.edition || ''}`.toLowerCase().includes(q))
      .sort((a, b) => ORDER.indexOf(groupOf(a)) - ORDER.indexOf(groupOf(b)) || rank(a) - rank(b) || String(a.abbreviation).localeCompare(b.abbreviation));
    const items = [];
    let last = '';
    shown.forEach((b, i) => {
      const g = groupOf(b);
      if (g !== last) {
        const h = document.createElement('li');
        h.className = 'bi-group';
        h.setAttribute('role', 'presentation');
        h.textContent = g;
        items.push(h);
        last = g;
      }
      const li = document.createElement('li');
      li.className = 'bi-opt';
      li.setAttribute('role', 'option');
      li.dataset.i = i;
      li.innerHTML = '<b></b><span></span><em></em>';
      li.children[0].textContent = b.abbreviation || '';
      li.children[1].textContent = String(b.name || '').replace(/^English:\s*/i, '');
      li.children[2].textContent = g === 'Catholic and Orthodox' ? b.edition || '' : '';
      li.addEventListener('mousedown', (e) => { e.preventDefault(); choose(i); });
      items.push(li);
    });
    if (!items.length) {
      const none = document.createElement('li');
      none.className = 'bi-none';
      none.textContent = bibles ? 'No Bible in your account matches.' : 'Loading your Bibles…';
      items.push(none);
    }
    options.replaceChildren(...items);
    active = shown.length ? 0 : -1;
    mark();
  }
  function mark() {
    options.querySelectorAll('.bi-opt').forEach((o) => o.classList.toggle('is-active', Number(o.dataset.i) === active));
    options.querySelector('.is-active')?.scrollIntoView({ block: 'nearest' });
  }
  function open(on) {
    options.hidden = !on;
    search.setAttribute('aria-expanded', String(on));
    if (on) render();
  }
  function choose(i) {
    const b = shown[i];
    if (!b) return;
    chosen = b;
    search.value = `${b.abbreviation} · ${String(b.name || '').replace(/^English:\s*/i, '')}${groupOf(b) === 'Catholic and Orthodox' && b.edition ? ` (${b.edition})` : ''}`;
    open(false);
  }
  search.addEventListener('focus', () => { search.select(); open(true); });
  search.addEventListener('input', () => { chosen = null; open(true); });
  search.addEventListener('blur', () => setTimeout(() => open(false), 120));
  search.addEventListener('keydown', (e) => {
    if (options.hidden && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) return open(true);
    if (e.key === 'ArrowDown') { active = Math.min(shown.length - 1, active + 1); mark(); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { active = Math.max(0, active - 1); mark(); e.preventDefault(); }
    else if (e.key === 'Enter' && !options.hidden) { choose(active); e.preventDefault(); }
    else if (e.key === 'Escape') open(false);
  });

  $('api-bible-add').addEventListener('click', async () => {
    if (!chosen) return say('Choose a Bible from the list first.', true);
    const r = await post('/api/api-bible/add', { id: chosen.id, abbreviation: chosen.abbreviation, name: `${chosen.abbreviation} ${String(chosen.name || '').replace(/^English:\s*/i, '')}`, coverage: chosen.coverage }).catch(() => ({ error: 'Kairo could not reach its engine.' }));
    if (!r.ok) return say(r.error || '', true);
    say(`${r.code} added. It is downloading so it works without internet.`);
    chosen = null;
    search.value = '';
    refresh();
  });

  refresh();

  // The tour's last step can take an API.Bible key: connect it when the tour is saved, quietly; the
  // Bibles are chosen afterwards in Settings → Bible.
  $('first-run-save')?.addEventListener('click', async () => {
    const key = $('first-run-api-bible-key')?.value.trim();
    if (!key) return;
    const r = await post('/api/api-bible/connect', { key }).catch(() => ({}));
    if (r.ok) refresh();
    else window.kairoProblem?.(`${r.error || 'API.Bible could not be reached.'} Add the key again in Settings → Bible.`);
  });
})();
