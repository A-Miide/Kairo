// KAIRO — Video director mode: the screen around the stage.
//
// Lazy-loaded with the mode and fully undone when it ends. Nothing in the
// normal layout's code changes: this adds a body class (the stylesheet does the
// rest), thin rails the sidebars collapse to, a top line (the live transcript
// on one line, beside the Listening button), and it moves the Listening button
// and the Media Bin out and back. Possible Matches is hidden by the stylesheet
// while the mode is on; detection keeps running underneath.
'use strict';
(function () {
  const L = { on: false, rails: [], ticker: null, topline: null, tickObs: null, tickTimer: null, listenHome: null, tabClick: null, prevView: null };

  // ── Rails: the sidebars as thin strips that open over the stage on click ──
  function mountRail(aside, side, label) {
    aside.classList.add('vd-rail', 'vd-rail-' + side);
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'vd-rail-btn';
    btn.setAttribute('aria-expanded', 'false');
    btn.title = `Open ${label}`;
    btn.setAttribute('aria-label', `Open ${label}`);
    btn.innerHTML = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="${side === 'left' ? '9 6 15 12 9 18' : '15 6 9 12 15 18'}"/></svg><span class="vd-rail-label">${label}</span>`;
    const set = (open) => {
      aside.classList.toggle('vd-rail-open', open);
      btn.setAttribute('aria-expanded', String(open));
      btn.title = `${open ? 'Close' : 'Open'} ${label}`;
      btn.setAttribute('aria-label', btn.title);
      btn.querySelector('svg').style.transform = open ? 'scaleX(-1)' : '';
    };
    btn.addEventListener('click', (e) => { e.stopPropagation(); set(!aside.classList.contains('vd-rail-open')); });
    const outside = (e) => { if (aside.classList.contains('vd-rail-open') && !aside.contains(e.target)) set(false); };
    document.addEventListener('mousedown', outside, true);
    aside.prepend(btn);
    L.rails.push({ aside, btn, outside });
  }
  function unmountRails() {
    for (const { aside, btn, outside } of L.rails) {
      document.removeEventListener('mousedown', outside, true);
      btn.remove();
      aside.classList.remove('vd-rail', 'vd-rail-left', 'vd-rail-right', 'vd-rail-open');
    }
    L.rails = [];
  }

  // ── Top line: Listening button + the transcript's last words ─────────────
  function mountTopline() {
    const center = document.querySelector('main.center-stage');
    const listen = document.getElementById('listen-btn');
    if (!center || !listen) return;
    const line = document.createElement('div');
    line.className = 'vd-topline';
    L.listenHome = document.createComment('listen-btn');
    listen.before(L.listenHome);
    line.appendChild(listen);        // the same button, so its state keeps updating
    const ticker = document.createElement('div');
    ticker.className = 'vd-ticker';
    ticker.setAttribute('role', 'status');
    ticker.setAttribute('aria-label', 'Live transcript');
    ticker.setAttribute('aria-live', 'off');
    const text = document.createElement('span');
    text.className = 'vd-ticker-text';
    ticker.appendChild(text);
    line.appendChild(ticker);
    center.prepend(line);
    L.topline = line; L.ticker = text;

    const src = document.getElementById('transcript-content');
    const update = () => {
      L.tickTimer = null;
      const t = src && src.querySelector('.transcript-text');
      const words = t ? t.textContent.replace(/\s+/g, ' ').trim() : '';
      text.textContent = words.slice(-240);
      text.classList.toggle('is-empty', !words);
      if (!words) text.textContent = 'Transcript appears here when listening';
    };
    if (src) {
      // Interim words arrive several times a second: at most one redraw per 150 ms.
      L.tickObs = new MutationObserver(() => { if (!L.tickTimer) L.tickTimer = setTimeout(update, 150); });
      L.tickObs.observe(src, { childList: true, subtree: true, characterData: true });
    }
    update();
  }
  function unmountTopline() {
    L.tickObs?.disconnect(); L.tickObs = null;
    clearTimeout(L.tickTimer); L.tickTimer = null;
    const listen = document.getElementById('listen-btn');
    if (listen && L.listenHome && L.listenHome.parentNode) { L.listenHome.after(listen); L.listenHome.remove(); }
    L.listenHome = null;
    L.topline?.remove(); L.topline = null; L.ticker = null;
  }

  // ── Views: the stage, verses and dock replace the Slides, Songs, Media and
  // Timer screens, so the Bible view is the one shown; those tabs reach the
  // dock instead (or do nothing, for Songs and Timer).
  function pinBibleView() {
    L.prevView = ['slides-btn', 'timer-btn', 'songs-btn', 'media-btn', 'bible-btn'].find(id => document.getElementById(id)?.classList.contains('active')) || 'bible-btn';
    document.getElementById('bible-btn')?.click();
    const bar = document.querySelector('.top-bar-center');
    L.tabClick = (e) => {
      const b = e.target.closest('button');
      if (!b || !bar.contains(b)) return;
      const map = { 'slides-btn': 'slides', 'media-btn': 'images' };
      if (b.id in map) { e.stopPropagation(); e.preventDefault(); window.KairoVideoDock?.show(map[b.id]); }
      else if (['timer-btn', 'songs-btn'].includes(b.id)) { e.stopPropagation(); e.preventDefault(); }
    };
    bar?.addEventListener('click', L.tabClick, true);
    ['timer-btn', 'songs-btn'].forEach(id => {
      const b = document.getElementById(id);
      if (b) { b.dataset.vdTitle = b.title || ''; b.title = 'Not in director mode. Turn it off to use this.'; b.classList.add('vd-tab-off'); }
    });
  }
  function unpinView() {
    document.querySelector('.top-bar-center')?.removeEventListener('click', L.tabClick, true);
    L.tabClick = null;
    ['timer-btn', 'songs-btn'].forEach(id => {
      const b = document.getElementById(id);
      if (b) { b.title = b.dataset.vdTitle || ''; delete b.dataset.vdTitle; b.classList.remove('vd-tab-off'); }
    });
    if (L.prevView && L.prevView !== 'bible-btn') document.getElementById(L.prevView)?.click();
    L.prevView = null;
  }

  // ── Scripture sends: the search bar and the Live Queue, moved into the
  // section (and put back exactly where they were when the mode ends). ──────
  function fillSends(host) {
    if (!host || L.sends) return;
    const parts = [document.querySelector('.cs-lookup'), document.getElementById('live-queue-section')].filter(Boolean);
    L.sends = { host, parts: parts.map(node => { const home = document.createComment('vd-home'); node.before(home); return { node, home }; }) };
    const label = document.createElement('div');
    label.className = 'vd-label';
    label.textContent = 'Scripture sends';
    host.appendChild(label);
    L.sends.label = label;
    parts.forEach(n => host.appendChild(n));
    // The queue's own heading says what it is in this mode.
    const q = document.querySelector('#live-queue-section .cs-section-label');
    if (q) { L.sends.queueLabel = q.textContent; q.textContent = 'Verses'; L.sends.queueEl = q; }
  }
  function emptySends() {
    if (!L.sends) return;
    const { parts, queueEl, queueLabel, label } = L.sends;
    if (queueEl) queueEl.textContent = queueLabel;
    parts.forEach(({ node, home }) => { if (home.parentNode) { home.after(node); home.remove(); } });
    label.remove();
    L.sends = null;
  }

  function start() {
    if (L.on) return;
    L.on = true;
    document.body.classList.add('video-director');
    const left = document.querySelector('aside.left-sidebar');
    const right = document.querySelector('aside.right-sidebar');
    if (left) mountRail(left, 'left', 'playlist and transcript');
    if (right) mountRail(right, 'right', 'clear, message and transition');
    mountTopline();
    pinBibleView();
  }
  function stop() {
    if (!L.on) return;
    L.on = false;
    unpinView();
    unmountTopline();
    unmountRails();
    document.body.classList.remove('video-director');
  }

  window.KairoVideoLayout = { start, stop, fillSends, emptySends };
})();
