// KAIRO — Video director mode: the screen around the stage.
//
// Lazy-loaded with the mode and fully undone when it ends. Nothing in the
// normal layout's code changes: this adds a body class (the stylesheet does the
// rest), hides both sidebars, builds a top line (the live transcript on one line,
// with no height until words arrive), and moves the search bar, the Live Queue,
// the Clear buttons, the range bar, the Monitor message and the verse transition into
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

  // ── Top line: the live transcript's last words, on one line. It has no
  // height at all until words arrive, so the screen below it moves up. ──────
  function mountTopline() {
    const center = document.querySelector('main.center-stage');
    if (!center) return;
    const line = document.createElement('div');
    line.className = 'vd-topline';
    const ticker = document.createElement('div');
    ticker.className = 'vd-ticker';
    ticker.setAttribute('aria-label', 'Live transcript');
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
      line.classList.toggle('has-text', !!words);
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

  // ── Scripture sends: one group for everything about verses. Header row (title,
  // Bible select, Theme icon, Monitor message, a Clear menu and Back), the search
  // bar, then the Live Queue filling the rest. The controls moved in here are
  // the app's own elements, put back when the mode ends. ───────────────────
  const ICONS = {
    monitor: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>',
    back: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14L4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/></svg>',
    eraser: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 20H9l-5-5a2 2 0 0 1 0-2.8l9.2-9.2a2 2 0 0 1 2.8 0l5 5a2 2 0 0 1 0 2.8L13 20"/><path d="M7 11l6 6"/></svg>',
    caret: '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>',
  };
  function hb(cls, title, html, onClick) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = cls; b.title = title; b.setAttribute('aria-label', title);
    b.innerHTML = html;
    b.addEventListener('click', onClick);
    return b;
  }
  // Press one of the app's own buttons (they hold the real logic).
  const press = (id) => document.getElementById(id)?.click();

  function closePop() {
    const S = L.sends;
    if (!S) return;
    if (S.pop) {
      const bar = document.getElementById('monitor-message-bar');
      if (bar && S.popHome && S.popHome.parentNode) { S.popHome.after(bar); S.popHome.remove(); }
      S.popHome = null;
      S.pop.remove(); S.pop = null;
    }
    S.menu && S.menu.remove(); S.menu = null;
    if (S.off) { document.removeEventListener('mousedown', S.off, true); S.off = null; }
  }
  function placeUnder(el, anchor, right) {
    const r = anchor.getBoundingClientRect();
    el.style.top = (r.bottom + 4) + 'px';
    const w = el.offsetWidth;
    el.style.left = Math.max(8, Math.min(window.innerWidth - w - 8, right ? r.right - w : r.left)) + 'px';
  }
  function openOverlay(anchor, el) {
    const S = L.sends;
    closePop();
    document.body.appendChild(el);
    placeUnder(el, anchor, true);
    S.off = (e) => { if (!el.contains(e.target) && !anchor.contains(e.target)) closePop(); };
    document.addEventListener('mousedown', S.off, true);
  }

  function fillSends(host) {
    if (!host || L.sends) return;
    const parts = [document.querySelector('.cs-lookup'), document.getElementById('live-queue-section')].filter(Boolean);
    L.sends = { host, parts: parts.map(node => { const home = document.createComment('vd-home'); node.before(home); return { node, home }; }) };
    const S = L.sends;

    // Header row: one line, every control the same height.
    const head = document.createElement('div');
    head.className = 'vd-sends-head';
    const title = document.createElement('span');
    title.className = 'vd-label vd-sends-title';
    title.textContent = 'Scripture sends';
    head.appendChild(title);
    moveTo(document.getElementById('translation-select'), head);
    const theme = document.getElementById('bible-theme-btn');
    if (theme) { S.themeTitle = theme.title; theme.title = 'Theme'; theme.setAttribute('aria-label', 'Theme'); moveTo(theme, head); }
    const spacer = document.createElement('span'); spacer.className = 'vd-sends-spacer'; head.appendChild(spacer);

    // Monitor message: an icon that opens the app's own message bar in a small popover.
    const bar = document.getElementById('monitor-message-bar');
    S.monBtn = hb('vd-hbtn', 'Monitor message', ICONS.monitor, (e) => {
      if (S.pop) { closePop(); return; }
      const b = document.getElementById('monitor-message-bar');
      if (!b) return;
      const pop = document.createElement('div');
      pop.className = 'vd-pop';
      S.popHome = document.createComment('vd-home');
      b.before(S.popHome);
      b.classList.remove('hidden');
      pop.appendChild(b);
      openOverlay(e.currentTarget, pop);
      S.pop = pop;
      b.querySelector('input')?.focus();
    });
    if (bar) {
      const sync = () => { S.monBtn.hidden = bar.classList.contains('hidden') && !S.pop; };
      S.monObs = new MutationObserver(sync);
      S.monObs.observe(bar, { attributes: true, attributeFilter: ['class'] });
      sync();
    }
    head.appendChild(S.monBtn);

    host.appendChild(head);
    S.head = head;
    parts.forEach(n => host.appendChild(n));

    // A slim footer, only visible while a verse range or a playing video needs its controls.
    const foot = document.createElement('div');
    foot.className = 'vd-sends-foot';
    host.appendChild(foot);
    S.foot = foot;
    ['range-nav-bar', 'media-transport'].forEach(id => moveTo(document.getElementById(id), foot));
    // The queue's own heading says what it is in this mode.
    const q = document.querySelector('#live-queue-section .cs-section-label');
    if (q) { S.queueLabel = q.textContent; q.textContent = 'Verses'; S.queueEl = q; }
  }
  function emptySends() {
    if (!L.sends) return;
    closePop();
    const { parts, queueEl, queueLabel, head, foot, themeTitle, monObs } = L.sends;
    monObs && monObs.disconnect();
    if (queueEl) queueEl.textContent = queueLabel;
    const theme = document.getElementById('bible-theme-btn');
    if (theme) { theme.title = themeTitle || ''; theme.removeAttribute('aria-label'); }
    restoreMovedInto(head);
    restoreMovedInto(foot);
    head.remove(); foot.remove();
    parts.forEach(({ node, home }) => { if (home.parentNode) { home.after(node); home.remove(); } });
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

  window.KairoVideoLayout = { start, stop, fillSends, emptySends, moveTo };
})();
