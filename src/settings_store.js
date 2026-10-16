// KAIRO — the one way the operator window saves and reads settings.
//
// Before this, settings were saved four ways: saveSettingsPatch (app_outputs.js), a full
// `{...settings, x}` POST in app.js (three places), bare fetches in service.js, and each with
// its own error handling. They all go through saveSettingsPatch now.
//
// Guarantees:
//  - Order. Saves leave one after another, so two quick changes to the same setting reach the
//    server in the order they were made (parallel requests could land the older one last).
//  - Failure is reported, never thrown: the promise resolves true when the server took the
//    save, false when it refused or could not be reached. opts.onError(err) is told why (an
//    Error, or { status } for a refusal). A request that gets no answer in 15 s is dropped so
//    the saves queued behind it are not held up for good.
//  - Rollback. A caller that changed `settings` before saving can pass opts.rollback, the
//    values to put back: they are restored when the save fails.
//  - Everyone hears of it: 'kairo:settings' fires on window right away (detail.reason is
//    'saved', 'loaded' or 'rollback'; detail.keys the settings concerned), so a button that
//    depends on a setting repaints from the event instead of a timer.
//
// Loaded after client_common.js, before everything else. SERVER and `settings` are app.js's;
// they are only used when a call is made, after app.js has run.
'use strict';

let _settingsChain = Promise.resolve();
const SETTINGS_SAVE_TIMEOUT_MS = 15000;

function notifySettings(reason, keys) {
  try { window.dispatchEvent(new CustomEvent('kairo:settings', { detail: { reason, keys: keys || [] } })); } catch {}
}

// fn(detail) on every settings change; returns the function that stops listening.
function onSettingsChange(fn) {
  const h = (e) => { try { fn(e.detail); } catch (err) { console.warn('[Settings] listener:', err); } };
  window.addEventListener('kairo:settings', h);
  return () => window.removeEventListener('kairo:settings', h);
}

// Persist a partial settings change without clobbering unrelated fields (the server merges
// it key by key). Resolves true when saved.
function saveSettingsPatch(patch, opts) {
  const o = opts || {};
  notifySettings('saved', Object.keys(patch || {}));
  const send = async () => {
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), SETTINGS_SAVE_TIMEOUT_MS);
    let failure = null;
    try {
      const r = await fetch(`${SERVER}/api/settings`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(patch),
        signal: ctl.signal,
      });
      if (r.ok) return true;
      failure = { status: r.status };
    } catch (err) {
      console.warn('[Settings] save failed:', err.message);
      failure = err;
    } finally { clearTimeout(timer); }
    if (o.rollback && typeof settings === 'object' && settings) {
      Object.assign(settings, o.rollback);
      notifySettings('rollback', Object.keys(o.rollback));
    }
    if (typeof o.onError === 'function') { try { o.onError(failure); } catch {} }
    return false;
  };
  const done = _settingsChain.then(send);
  _settingsChain = done.catch(() => {});
  return done;
}

// The server's saved settings (secrets masked). Calls made while one is on its way share it.
// Each caller gets its own copy to keep or change.
let _settingsRead = null;
function fetchServerSettings() {
  if (!_settingsRead) {
    _settingsRead = fetch(`${SERVER}/api/settings`)
      .then((r) => { if (!r.ok) throw new Error(`settings ${r.status}`); return r.json(); });
    const clear = () => { _settingsRead = null; };
    _settingsRead.then(clear, clear);
  }
  return _settingsRead.then((s) => structuredClone(s));
}

// ── Director mode ──────────────────────────────────────────────────────────
// The one question "is Director mode on?" is isDirectorOn(): the saved setting, which is the
// operator's choice (and what the outputs, the Outputs pane and the buttons follow). It was
// asked five ways (settings.videoDirector in a dozen files, KairoVideoDirector.state().active,
// ...). The running session's own flag stays inside video_director.js.
//
// Anything that follows the mode listens with onDirectorChange(fn) instead of a timer. fn gets
// { on, reason }, where reason is
//   'flag'    the choice was changed (video_director_boot.js),
//   'started' the Director screen finished starting,
//   'stopped' it was taken down,
//   'program' the camera on air (or the Preview) changed while the mode runs.
function isDirectorOn() {
  return !!(typeof settings === 'object' && settings && settings.videoDirector);
}
function emitDirectorChange(reason) {
  try { window.dispatchEvent(new CustomEvent('kairo:director', { detail: { on: isDirectorOn(), reason } })); } catch {}
}
function onDirectorChange(fn) {
  const h = (e) => { try { fn(e.detail); } catch (err) { console.warn('[Director] listener:', err); } };
  window.addEventListener('kairo:director', h);
  return () => window.removeEventListener('kairo:director', h);
}

// ── Cameras (video sources) ────────────────────────────────────────────────
// ONE list: settings.videoSources, saved to the server like every setting. Display windows are
// other webviews that cannot read `settings`, so the same list is mirrored to localStorage
// ('kairo-video-sources', with the live map in 'kairo-video-live'); the mirror is written only
// here, and read only by display windows. Everything in this window asks for the list with
// KairoCameras.list() / find(id) rather than reading either copy.
const KairoCameras = (() => {
  const SOURCES_KEY = 'kairo-video-sources', LIVE_KEY = 'kairo-video-live';
  const set = (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch (err) { console.warn('[Video] could not publish state:', err.message); } };
  return {
    list() {
      const l = typeof settings === 'object' && settings ? settings.videoSources : null;
      return Array.isArray(l) ? l.filter(x => x && typeof x === 'object') : [];
    },
    find(id) { return this.list().find(s => s.id === id) || null; },
    // Write what the display windows read. `live` is the normalized {outputId: sourceId|null}.
    mirror(sources, live) {
      set(SOURCES_KEY, sources);
      if (live !== undefined) set(LIVE_KEY, live);
    },
    // A new list (and optionally the live map that goes with it): kept, saved, mirrored.
    save(sources, live) {
      settings.videoSources = sources;
      const patch = { videoSources: sources };
      if (live !== undefined) { settings.videoLive = live; patch.videoLive = live; }
      saveSettingsPatch(patch);
      this.mirror(sources, live);
    },
    // Which source is live on which output.
    saveLive(live) {
      settings.videoLive = live;
      saveSettingsPatch({ videoLive: live });
      set(LIVE_KEY, live);
    },
  };
})();
