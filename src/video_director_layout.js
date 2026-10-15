// KAIRO — Video director mode: the screen around the stage.
//
// Lazy-loaded with the mode and fully undone when it ends. Nothing in the
// normal layout's code changes: this adds a body class (the stylesheet does the
// rest), hides both sidebars, builds a top line (the listening state, the live
// transcript on one line and the Clear buttons), and moves the search bar, the
// Live Queue, the range bar, the Monitor message and the verse transition into
// the Scripture sends group and back. Possible Matches is hidden by the stylesheet
// while the mode is on; detection keeps running underneath.
'use strict';
(function () {
  const L = { on: false, moved: [], ticker: null, topline: null, tickObs: null, tickTimer: null, tabClick: null, prevView: null };

  // Pieces of the sidebars that this mode keeps, moved into the director
  // groups and put back exactly where they were when the mode ends. The
  // sidebars themselves are hidden (the stylesheet), so nothing stands beside
  // the screen.
  function moveTo(node, dest, before) {
    if (!node || !dest) return;
    const home = document.createComment('vd-home');
    node.before(home);
    before ? dest.insertBefore(node, before) : dest.appendChild(node);
    L.moved.push({ node, home });
  }
  function restoreMoved() {
    for (const { node, home } of L.moved.reverse()) { if (home.parentNode) { home.after(node); home.remove(); } }
    L.moved = [];
  }

  // ── Top line: the listening state and the transcript's last words. The
  // Start Listening button stays in the top bar where it always is. ────────
  function mountTopline() {
    const center = document.querySelector('main.center-stage');
    const listen = document.getElementById('listen-btn');
    if (!center) return;
    const line = document.createElement('div');
    line.className = 'vd-topline';
    const state = document.createElement('span');
    state.className = 'vd-listen-state';
    state.setAttribute('role', 'status');
    line.appendChild(state);
    const ticker = document.createElement('div');
    ticker.className = 'vd-ticker';
    ticker.setAttribute('aria-label', 'Live transcript');
    const text = document.createElement('span');
    text.className = 'vd-ticker-text';
    ticker.appendChild(text);
    line.appendChild(ticker);
    center.prepend(line);
    L.topline = line; L.ticker = text; L.state = state;
    // The per-layer Clear buttons live at the right end of this line.
    moveTo(document.querySelector('.rs-layer-clear-bar'), line);

    const paintState = () => {
      const on = !!(listen && listen.classList.contains('active'));
      state.textContent = on ? 'Listening' : 'Not listening';
      state.classList.toggle('is-on', on);
    };
    if (listen) { L.listenObs = new MutationObserver(paintState); L.listenObs.observe(listen, { attributes: true, attributeFilter: ['class'] }); }
    paintState();

    const src = document.getElementById('transcript-content');
    const update = () => {
      L.tickTimer = null;
      const t = src && src.querySelector('.transcript-text');
      const words = t ? t.textContent.replace(/\s+/g, ' ').trim() : '';
      text.textContent = words ? words.slice(-240) : 'Transcript appears here when listening';
      text.classList.toggle('is-empty', !words);
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
    L.listenObs?.disconnect(); L.listenObs = null;
    clearTimeout(L.tickTimer); L.tickTimer = null;
    L.topline?.remove(); L.topline = null; L.ticker = null; L.state = null;
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
    // Header row: the Bible select and an icon-only Theme button, then the search bar.
    const theme = document.getElementById('bible-theme-btn');
    if (theme) { L.sends.themeTitle = theme.title; theme.title = 'Theme'; theme.setAttribute('aria-label', 'Theme'); }
    // The rest of what the right sidebar held, under the queue.
    const foot = document.createElement('div');
    foot.className = 'vd-sends-foot';
    host.appendChild(foot);
    L.sends.foot = foot;
    ['range-nav-bar', 'media-transport', 'monitor-message-bar', 'rs-transition-bar'].forEach(id => moveTo(document.getElementById(id), foot));
    const q = document.querySelector('#live-queue-section .cs-section-label');
    if (q) { L.sends.queueLabel = q.textContent; q.textContent = 'Verses'; L.sends.queueEl = q; }
  }
  function emptySends() {
    if (!L.sends) return;
    const { parts, queueEl, queueLabel, label, foot, themeTitle } = L.sends;
    if (queueEl) queueEl.textContent = queueLabel;
    const theme = document.getElementById('bible-theme-btn');
    if (theme) { theme.title = themeTitle || ''; theme.removeAttribute('aria-label'); }
    restoreMovedInto(foot);
    foot.remove();
    parts.forEach(({ node, home }) => { if (home.parentNode) { home.after(node); home.remove(); } });
    label.remove();
    L.sends = null;
  }

  // Everything moved into `foot` goes back (the Clear bar, moved earlier, stays until stop).
  function restoreMovedInto(foot) {
    const keep = [];
    for (const m of L.moved) {
      if (foot.contains(m.node)) { if (m.home.parentNode) { m.home.after(m.node); m.home.remove(); } }
      else keep.push(m);
    }
    L.moved = keep;
  }

  function start() {
    if (L.on) return;
    L.on = true;
    document.body.classList.add('video-director');
    mountTopline();
    pinBibleView();
  }
  function stop() {
    if (!L.on) return;
    L.on = false;
    unpinView();
    restoreMoved();
    unmountTopline();
    document.body.classList.remove('video-director');
  }

  window.KairoVideoLayout = { start, stop, fillSends, emptySends };
})();
