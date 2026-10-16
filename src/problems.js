// KAIRO — one place for things that went wrong (main window).
//
// A failure the operator needs to know about used to go nowhere: toast() is a no-op on
// purpose (no pop-ups), most requests never looked at the server's answer, and nothing
// caught a thrown error. Here:
// - kairoProblem(message): a small strip at the bottom of the window, in the operator's
//   words, that stays until dismissed (or until the same action works again). The same
//   message twice shows once, with a count. Success messages stay silent.
// - every action request (anything but a GET) that the server refuses or never answers
//   is reported by itself, named by what it was doing (Send to screen, Save settings…),
//   so a send that failed can't look like it worked;
// - every uncaught error and rejected promise goes to the debug log (databases/debug.log).
'use strict';
(function () {
  const log = (event, data) => window.kairoLog(event, data);   // client_common.js

  // ── The strip ──
  let strip = null;
  const shown = new Map();   // key → { text, count, at }
  function draw() {
    if (!strip) {
      strip = document.createElement('div');
      strip.className = 'kp-strip';
      strip.setAttribute('role', 'status');
      strip.setAttribute('aria-live', 'polite');
      document.body.appendChild(strip);
    }
    strip.innerHTML = '';
    const items = [...shown.entries()].sort((a, b) => b[1].at - a[1].at).slice(0, 3);
    strip.hidden = !items.length;
    items.forEach(([key, p]) => {
      const row = document.createElement('div');
      row.className = 'kp-row' + (p.actions ? ' kp-calm' : '');
      const t = document.createElement('span');
      t.className = 'kp-text';
      t.textContent = p.count > 1 ? `${p.text} (×${p.count})` : p.text;
      const x = document.createElement('button');
      x.type = 'button'; x.className = 'kp-close'; x.textContent = '✕';
      x.title = 'Dismiss'; x.setAttribute('aria-label', 'Dismiss');
      x.addEventListener('click', () => { shown.delete(key); draw(); });
      if (p.actions) {
        p.actions.forEach((a) => {
          const b = document.createElement('button');
          b.type = 'button'; b.className = 'kp-action'; b.textContent = a.label;
          b.addEventListener('click', () => { shown.delete(key); draw(); try { a.run(); } catch (e) { log('js-error', { msg: String(e && e.message).slice(0, 200), src: 'suggestion' }); } });
          row.appendChild(b);
        });
        row.insertBefore(t, row.firstChild);
        row.appendChild(x);
        strip.appendChild(row);
        return;
      }
      row.append(t, x);
      strip.appendChild(row);
    });
  }
  function kairoProblem(message, key) {
    const text = String(message || '').trim();
    if (!text) return;
    const k = key || text;
    const p = shown.get(k);
    shown.set(k, { text, count: p ? p.count + 1 : 1, at: Date.now() });
    if (document.body) draw(); else document.addEventListener('DOMContentLoaded', draw, { once: true });
    log('problem', { text: text.slice(0, 300), key: String(k).slice(0, 80) });
  }
  // A suggestion, not a problem: a calm row with buttons ({label, run}); never repeated once answered.
  function kairoSuggest(message, actions, key) {
    const text = String(message || '').trim();
    if (!text || !key) return;
    shown.set(key, { text, count: 1, at: Date.now(), actions });
    if (document.body) draw(); else document.addEventListener('DOMContentLoaded', draw, { once: true });
    log('suggestion', { text: text.slice(0, 200), key: String(key).slice(0, 60) });
  }
  window.kairoSuggest = kairoSuggest;
  // The same action worked: its earlier problem is over.
  function kairoProblemCleared(key) { if (shown.delete(key)) draw(); }
  window.kairoProblem = kairoProblem;
  window.kairoProblemCleared = kairoProblemCleared;
  // Callers of toast(msg, 'error') / showToast(msg, 'error') get the strip; anything else
  // (success, info) stays silent as before.
  window.showToast = (msg, type) => { if (type === 'error') kairoProblem(msg); };

  // ── Action requests that failed ──
  // What each request was doing, in the operator's words.
  const ACTIONS = [
    [/\/api\/(service\/send(-slide)?|send)(\?|$)/, 'Send to screen'],
    [/\/api\/service\/send-media/, 'Send media to screen'],
    [/\/api\/service\/send-timer/, 'Start the timer on screen'],
    [/\/api\/service\/(clear-layer|clear)/, 'Clear the screen'],
    [/\/api\/output\/back/, 'Go back on screen'],
    [/\/api\/service\/media-control/, 'Control the video on screen'],
    [/\/api\/settings/, 'Save settings'],
    [/\/api\/look\/apply/, 'Update the screens'],
    [/\/api\/(media\/bin|media\/folders\/[^/]+|audio)\/upload/, 'Upload'],
    [/\/api\/overlays/, 'Change an overlay'],
    [/\/api\/monitor-message/, 'Send the message'],
    [/\/api\/segments/, 'Save the timer'],
    [/\/api\/songs/, 'Save the song'],
    [/\/api\/service\/import/, 'Import'],
    [/\/api\/range\//, 'Move through the verses'],
    [/\/api\/media\/prepare/, 'Prepare the video'],
  ];
  // Requests that report their own failures in their own place, or are allowed to fail.
  const QUIET = /\/api\/(debug-log|start-listening|stream\/check|obs\/test|search|lookup|translate)/;
  function actionOf(url, method) {
    if (!method || method.toUpperCase() === 'GET' || QUIET.test(url)) return null;
    const hit = ACTIONS.find(([re]) => re.test(url));
    return hit ? hit[1] : null;
  }
  async function reason(res) {
    try {
      const b = await res.clone().json();
      return (b && (b.error || b.message)) || '';
    } catch { return ''; }
  }
  // Wraps whatever fetch is in place (app.js adds the auth header around it).
  window.kairoWatchFetch = function (fetchImpl) {
    return async function (input, init = {}) {
      const url = typeof input === 'string' ? input : (input && input.url) || '';
      const what = actionOf(url, (init && init.method) || (input && input.method) || 'GET');
      let res;
      try {
        res = await fetchImpl(input, init);
      } catch (err) {
        // A deliberate cancel (AbortError) is not a failure; a timeout is.
        if (what && !(err && err.name === 'AbortError')) kairoProblem(`${what} didn't work: Kairo's server didn't answer${err && err.name === 'TimeoutError' ? ' in time' : ''}.`, what);
        throw err;
      }
      if (what) {
        if (!res.ok) {
          const why = await reason(res);
          kairoProblem(`${what} didn't work${why ? `: ${why}` : ` (${res.status})`}.`, what);
        } else kairoProblemCleared(what);
      }
      return res;
    };
  };

  // ── Uncaught errors, for the log ──
  window.addEventListener('error', (e) => log('js-error', { msg: String(e.message || '').slice(0, 300), src: String(e.filename || '').split('/').pop(), line: e.lineno, col: e.colno }));
  window.addEventListener('unhandledrejection', (e) => {
    const r = e.reason;
    log('js-rejection', { msg: String((r && (r.message || r)) || '').slice(0, 300), stack: String((r && r.stack) || '').split('\n').slice(0, 3).join(' | ').slice(0, 300) });
  });

  const style = document.createElement('style');
  style.textContent = `
    .kp-strip { position: fixed; left: 50%; bottom: 14px; transform: translateX(-50%); z-index: 3000; display: flex; flex-direction: column; gap: 6px; max-width: min(640px, calc(100vw - 32px)); }
    .kp-strip[hidden] { display: none; }
    .kp-row { display: flex; align-items: center; gap: 10px; padding: 8px 10px 8px 14px; border-radius: 10px; border: 1px solid #8f3d1f; background: #2a1712; color: #ffe9e0; font-size: 12.5px; line-height: 1.35; box-shadow: 0 10px 28px rgba(0,0,0,.4); }
    .kp-row.kp-calm { border-color: var(--border); background: var(--bg-elevated, #1b1b1f); color: var(--text); }
    .kp-action { flex: none; padding: 5px 10px; border-radius: 7px; border: 1px solid var(--border); background: transparent; color: inherit; font-size: 12px; cursor: pointer; }
    .kp-action:hover { background: rgba(255,255,255,.08); }
    .kp-text { flex: 1; min-width: 0; }
    .kp-close { flex: none; width: 24px; height: 24px; border: 0; border-radius: 6px; background: transparent; color: inherit; cursor: pointer; opacity: .7; }
    .kp-close:hover { opacity: 1; background: rgba(255,255,255,.08); }
  `;
  document.head.appendChild(style);
})();
