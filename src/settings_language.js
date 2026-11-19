// Settings → Language, Audio's offline models and Settings → Downloads: one set of rows for every
// download Kairo has, so the three places always agree (owner, 2026-11-05).
//   Language: the preacher speaks → Kairo hears it with → Bible version → Slide translation.
//   Audio: both offline models, the one the service language needs marked.
//   Downloads: everything Kairo can download, per language.
// Status comes from the server (/api/offline/status, /api/translate-model/status); downloads use the
// same install routes as before (offline: /api/offline/install, translators: streamMtInstall).
(function () {
  const $ = (id) => document.getElementById(id);
  const MODELS = {
    en: { name: 'English only', sub: 'English services.' },
    multi: { name: 'Multilingual', sub: 'English, Spanish, Portuguese, French and German, and services that switch between them.' },
  };
  const VERSION_NAMES = { KJV: 'King James Version', BSB: 'Berean Standard Bible' };

  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };
  const ok = (text) => el('span', 'set-ok', `✓ ${text}`);
  const button = (label, primary, onClick) => {
    const b = el('button', `modal-btn ${primary ? 'primary' : 'secondary'} set-btn`, label);
    b.type = 'button';
    b.addEventListener('click', onClick);
    return b;
  };
  const link = (label, onClick) => {
    const b = el('button', 'lang-link', label);
    b.type = 'button';
    b.addEventListener('click', onClick);
    return b;
  };
  const progress = (pct) => {
    const wrap = el('span', 'set-progress');
    wrap.append(el('span', null, pct == null ? 'Downloading…' : `Downloading ${pct}%`));
    const bar = el('span', 'set-bar');
    const fill = el('i');
    fill.style.width = `${pct || 0}%`;
    bar.append(fill);
    wrap.append(bar);
    return wrap;
  };
  function row({ title, sub, right = [] }) {
    const r = el('div', 'set-row');
    const text = el('div', 'set-row-text');
    text.append(el('span', 'set-row-title', title));
    if (sub) text.append(el('span', 'set-row-sub', sub));
    const side = el('div', 'set-row-side');
    side.append(...right.filter(Boolean));
    r.append(text, side);
    return r;
  }

  const get = async (url) => { try { return await (await fetch(`${SERVER}${url}`)).json(); } catch (_) { return null; } };
  const offlineStatus = (m) => get(`/api/offline/status?model=${m}`);
  const mtStatus = (code) => get(`/api/translate-model/status?lang=${code}`);

  const spokenBase = () => String($('stt-language')?.value || settings.sttLanguage || 'en-US').toLowerCase().split('-')[0];
  const serviceName = () => $('stt-language')?.selectedOptions?.[0]?.textContent || 'English';
  const neededModel = () => (spokenBase() === 'en' ? 'en' : 'multi');
  const engine = () => ($('speech-engine-toggle')?.querySelector('.toggle-btn.active')?.dataset.engine || settings.speechEngine || 'deepgram');
  const offlineChosen = () => engine() === 'browser' || engine() === 'offline';
  const hasKey = () => !!(($('deepgram-key')?.value || '').trim() || settings.deepgramApiKey);
  const openPane = (pane) => { if (typeof showSettingsPane === 'function') showSettingsPane(pane); };

  // ── Downloads that are running, started here or anywhere else ──
  const running = new Set();
  async function installOffline(model) {
    running.add(model);
    refresh();
    try {
      const res = await fetch(`${SERVER}/api/offline/install?model=${model}`, { method: 'POST' });
      if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = '';
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop();
        for (const line of lines) {
          let evt;
          try { evt = JSON.parse(line); } catch { continue; }
          if (evt.phase === 'complete' && !evt.ok) toast(`The download stopped: ${evt.error || 'unknown error'}`, 'error');
        }
      }
    } catch (err) {
      toast(`The download stopped: ${err.message}`, 'error');
    }
    running.delete(model);
    refresh();
    if (typeof renderOfflineSummary === 'function') renderOfflineSummary();
  }
  function installTranslator(code) {
    running.add(`mt-${code}`);
    refresh();
    streamMtInstall(code, {
      onComplete: (success, error) => {
        running.delete(`mt-${code}`);
        if (!success) toast(`The download stopped: ${error || 'unknown error'}`, 'error');
        refresh();
      },
      onError: (msg) => { running.delete(`mt-${code}`); toast(`The download stopped: ${msg}`, 'error'); refresh(); },
    });
  }

  // What a model's row shows on its right: installed, downloading, or the button to get it.
  function modelSide(model, st, primary) {
    if (st?.installing || running.has(model)) return [progress(typeof st?.progress === 'number' ? st.progress : null)];
    if (st?.needsUpdate) return [button('Update', primary, () => installOffline(model))];
    if (st?.installed) return [ok('Installed')];
    return [button('Download', primary, () => installOffline(model))];
  }
  function translatorSide(code, st, primary, removable) {
    if (st?.installing || running.has(`mt-${code}`)) return [progress(null)];
    if (st?.installed) return [ok('Installed'), removable ? button('Remove', false, () => removeMtModel(code)) : null];
    return [button('Download', primary, () => installTranslator(code))];
  }
  const size = (st) => (st?.approxMB ? `${st.approxMB} MB` : '');

  // ── Language, step 2: how Kairo hears it (the Audio choice, shown here) ──
  async function renderLangEngine(en, multi) {
    const host = $('lang-engine');
    if (!host) return;
    const toAudio = link('Change in Audio ›', () => openPane('audio'));
    const rows = [];
    if (!offlineChosen()) {
      rows.push(row({
        title: 'Deepgram',
        sub: `Hears ${serviceName()} with nothing to download.`,
        right: [hasKey() ? ok('Key added') : el('span', 'set-row-sub', 'No key yet'), toAudio],
      }));
    } else {
      const need = neededModel();
      const st = need === 'en' ? en : multi;
      const other = need === 'en' ? multi : en;
      const sub = st?.installed ? MODELS[need].sub
        : other?.installed ? `${MODELS[need === 'en' ? 'multi' : 'en'].name} is installed, but ${serviceName()} needs ${MODELS[need].name}. ${size(st)}`
        : `Needed for ${serviceName()}. ${size(st)}`;
      rows.push(row({ title: `Offline · ${MODELS[need].name}`, sub, right: [...modelSide(need, st, true), toAudio] }));
    }
    host.replaceChildren(...rows);
  }

  // ── Tour, Listening step: the one model the language picked on the first step needs ──
  function renderTourModel(en, multi) {
    const host = $('first-run-model');
    if (!host) return;
    const need = neededModel();
    const st = need === 'en' ? en : multi;
    host.replaceChildren(row({ title: MODELS[need].name, sub: `For ${serviceName()}. ${st?.installed ? '' : size(st)}`, right: modelSide(need, st, true) }));
    if (st?.installed) window.checkOnboardingSpeechSetup?.();
  }

  // ── Audio: both offline models ──
  function renderAudioModels(en, multi) {
    const host = $('offline-models');
    if (host) {
      const need = neededModel();
      host.replaceChildren(...['en', 'multi'].map((m) => {
        const st = m === 'en' ? en : multi;
        const sub = (m === need ? `Needed for ${serviceName()}. ` : '') + MODELS[m].sub;
        return row({ title: MODELS[m].name, sub, right: [st?.installed ? null : el('span', 'set-row-sub', size(st)), ...modelSide(m, st, m === need)] });
      }));
    }
    const name = $('au-service-lang-name');
    if (name) name.textContent = serviceName();
    const free = $('deepgram-free');
    if (free) free.hidden = hasKey();
  }

  // ── Downloads ──
  async function renderDownloads(en, multi) {
    const listening = $('dl-listening');
    if (!listening) return;
    const mt = await Promise.all(MT_LANGUAGES.map((l) => mtStatus(l.code)));
    const need = neededModel();
    listening.replaceChildren(...['en', 'multi'].map((m) => {
      const st = m === 'en' ? en : multi;
      const inUse = offlineChosen() && m === need && st?.installed;
      return row({ title: MODELS[m].name + (inUse ? ' · in use' : ''), sub: MODELS[m].sub, right: [el('span', 'set-row-sub', size(st)), ...modelSide(m, st, false)] });
    }));
    $('dl-translation')?.replaceChildren(...MT_LANGUAGES.map((l, i) => row({
      title: l.name,
      sub: l.code === 'pt' ? 'Larger model. Its licence allows non-commercial use only.' : '',
      right: [el('span', 'set-row-sub', size(mt[i])), ...translatorSide(l.code, mt[i], false, true)],
    })));
    const api = await get('/api/api-bible/mine');
    const saved = (api?.bibles || []).filter((b) => b.total && b.chapters >= b.total).length;
    const packs = LANG_PACKS.map((p) => `${p.name} (${p.translations.replace(/\s*\(.*\)$/, '').replace(' · ', ', ')})`).join(', ');
    $('dl-bibles')?.replaceChildren(
      row({ title: 'Built in', sub: packs, right: [ok('Included')] }),
      row({
        title: 'Saved from API.Bible',
        sub: 'Offline copies of your API.Bible versions.',
        right: [el('span', 'set-row-sub', saved ? `${saved} saved` : 'None saved'), button(saved ? 'Manage' : 'Connect', false, () => openPane('bible'))],
      }),
    );
    const mb = [en, multi, ...mt].filter((s) => s?.installed).reduce((n, s) => n + (s.approxMB || 0), 0);
    const space = $('dl-space');
    if (space) space.textContent = mb ? `${mb >= 1000 ? `${(mb / 1000).toFixed(2)} GB` : `${mb} MB`} used on this computer` : 'Nothing downloaded yet';
  }

  // ── Language, step 3: one picker for the version, grouped by language ──
  function versionName(code) {
    return VERSION_NAMES[code] ? `${VERSION_NAMES[code]} (${code})` : code;
  }
  function renderBibleVersions() {
    const sel = $('lang-bible-version');
    const langSel = $('bible-language');
    const tr = $('translation-select-settings');
    if (!sel || !langSel || !tr || typeof LANG_PACKS === 'undefined') return;
    const base = spokenBase();
    const order = [...LANG_PACKS].sort((a, b) => (b.code === base) - (a.code === base) || (b.code === 'en') - (a.code === 'en'));
    sel.replaceChildren(...order.map((p) => {
      const g = el('optgroup');
      g.label = p.name;
      if (p.code === 'en') {
        [...tr.options].forEach((o) => g.append(new Option(`English \u00b7 ${versionName(o.value)}`, `en:${o.value}`)));
      } else {
        const label = p.translations.replace(/\s*\(.*\)$/, '');
        // The app's dropdowns list options without their group, so each says its language.
        g.append(new Option(`${p.name} \u00b7 ${/New Testament/.test(label) ? 'New Testament' : label}`, p.code));
      }
      return g;
    }));
    sel.value = (langSel.value || 'en') === 'en' ? `en:${tr.value || 'KJV'}` : langSel.value;
    // A Bible in another language than the preacher's: allowed (an English service shown in
    // Yoruba), and said plainly, with the way back.
    const note = $('lang-bible-mismatch');
    if (note) {
      const bibleLang = LANG_PACKS.find((p) => p.code === (langSel.value || 'en'));
      const servicePack = LANG_PACKS.find((p) => p.code === base);
      const differs = servicePack && bibleLang && bibleLang.code !== base;
      note.hidden = !differs;
      if (differs) {
        note.replaceChildren(`The preacher speaks ${servicePack.name}, so verses read aloud in ${servicePack.name} may not be recognised in a ${bibleLang.name} Bible. `,
          link(`Use ${servicePack.name}`, () => { langSel.value = base; langSel.dispatchEvent(new Event('change')); }));
      }
    }
  }
  $('lang-bible-version')?.addEventListener('change', (e) => {
    const v = e.target.value;
    const langSel = $('bible-language');
    const tr = $('translation-select-settings');
    if (v.startsWith('en:')) {
      const code = v.slice(3);
      tr.value = code;
      settings.translation = code;
      const toolbar = $('translation-select');
      if (toolbar) toolbar.value = code;
      saveSettingsPatch({ translation: code });
      tr.dispatchEvent(new Event('change'));
    }
    const lang = v.startsWith('en:') ? 'en' : v;
    if (langSel.value !== lang) { langSel.value = lang; langSel.dispatchEvent(new Event('change')); }
  });
  $('lang-more-versions')?.addEventListener('click', () => openPane('bible'));
  $('au-service-lang-change')?.addEventListener('click', () => openPane('language'));
  // Versions added or removed (bible_import.js) show up in the picker.
  const tr = $('translation-select-settings');
  if (tr) new MutationObserver(renderBibleVersions).observe(tr, { childList: true, subtree: true });

  // ── Everything at once, and again while something is downloading ──
  let timer = null;
  async function refresh() {
    clearTimeout(timer);
    const [en, multi] = await Promise.all([offlineStatus('en'), offlineStatus('multi')]);
    renderLangEngine(en, multi);
    renderAudioModels(en, multi);
    renderTourModel(en, multi);
    await renderDownloads(en, multi);
    renderBibleVersions();
    if (typeof renderSecondLangModel === 'function') renderSecondLangModel();
    window.renderLangCapability?.();
    if (en?.installing || multi?.installing || running.size) timer = setTimeout(refresh, 1500);
  }

  $('speech-engine-toggle')?.addEventListener('click', () => setTimeout(refresh, 0));
  $('speech-engine-toggle')?.addEventListener('toggle-change', refresh);
  $('first-run-engine-toggle')?.addEventListener('click', () => setTimeout(refresh, 0));
  $('stt-language')?.addEventListener('change', () => setTimeout(refresh, 0));
  $('bible-language')?.addEventListener('change', renderBibleVersions);
  $('deepgram-key')?.addEventListener('input', refresh);
  $('settings-nav')?.addEventListener('click', () => setTimeout(refresh, 0));

  window.renderBibleVersions = renderBibleVersions;
  window.refreshDownloads = refresh;
  refresh();
})();
