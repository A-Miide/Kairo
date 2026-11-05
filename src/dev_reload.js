// KAIRO — live reload for the dev build (inert everywhere else).
//
// `npm run dev` loads the app from http://localhost:7788; the installed app loads from 127.0.0.1. Only on
// "localhost" does this run: every two seconds it asks for the size of each script and stylesheet the page
// loaded (a HEAD request, nothing downloaded) and, when one changed on disk, reloads the window. So a change to
// the app's JS, CSS or HTML shows up on its own, with no dev-build restart (Rust changes still need one).
'use strict';
(function () {
  if (location.hostname !== 'localhost') return;
  const urls = () => [...document.querySelectorAll('script[src], link[rel="stylesheet"]')]
    .map(el => el.src || el.href).filter(u => u && u.startsWith(location.origin));
  const sizes = new Map();
  let busy = false;
  async function poll() {
    if (busy) return;
    busy = true;
    try {
      let changed = false;
      for (const u of urls()) {
        let n = null;
        try { const r = await fetch(u, { method: 'HEAD', cache: 'no-store' }); n = r.headers.get('content-length') || r.headers.get('etag') || r.headers.get('last-modified'); } catch {}
        if (n == null) continue;
        if (sizes.has(u) && sizes.get(u) !== n) changed = true;
        sizes.set(u, n);
      }
      if (changed) { try { window.kairoLog && window.kairoLog('dev-reload', {}); } catch {} setTimeout(() => location.reload(), 400); }
    } finally { busy = false; }
  }
  setTimeout(() => { poll(); setInterval(poll, 2000); }, 6000);
})();
