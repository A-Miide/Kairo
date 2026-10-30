// Puts each setting's name on the left and its control on the right, with the
// setting's hint directly under the control it explains (see "Settings, row layout" in styles.css). Run once at start and
// again whenever a settings pane draws new groups. Help and Outputs keep their
// own layouts.
(function () {
  const SKIP = '.setting-checks, .osc-port-row';
  function arrange(root) {
    root.querySelectorAll('.settings-pane:not([data-pane="help"]):not([data-pane="outputs"]) .setting-group').forEach((g) => {
      if (g.dataset.arranged || g.matches(SKIP)) return;
      const label = Array.from(g.children).find(c => c.matches('label.setting-label'));
      if (!label) return;
      const copy = document.createElement('div');
      copy.className = 'setting-copy';
      const control = document.createElement('div');
      control.className = 'setting-control';
      // Hints go after the control, wherever they were written.
      const kids = Array.from(g.children);
      kids.forEach((c) => { if (c === label) copy.appendChild(c); else if (!c.matches('.setting-hint')) control.appendChild(c); });
      kids.forEach((c) => { if (c !== label && c.matches('.setting-hint')) control.appendChild(c); });
      g.append(copy, control);
      g.dataset.arranged = '1';
    });
  }
  let queued = false;
  function soon() {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; arrange(document); });
  }
  const start = () => {
    arrange(document);
    const panes = document.querySelector('.settings-panes') || document.body;
    new MutationObserver(soon).observe(panes, { childList: true, subtree: true });
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else start();
})();

// Numbers each Settings page's sections 1, 2, 3 in the order they show, on any page with more than
// one; a hidden section isn't counted. Done here rather than with CSS counters, which WebKit
// (the app's view) restarted in every section, so every number read 1 (2026-10-29).
(function () {
  function number() {
    document.querySelectorAll('.settings-pane').forEach((pane) => {
      const shown = [...pane.querySelectorAll('.settings-section-label')].filter((l) => {
        const sec = l.closest('.settings-section') || l;
        return sec.style.display !== 'none' && !sec.hidden;
      });
      shown.forEach((l, i) => {
        const n = shown.length > 1 ? String(i + 1) : '';
        if ((l.dataset.n || '') !== n) { if (n) l.dataset.n = n; else delete l.dataset.n; }
      });
    });
  }
  number();
  document.addEventListener('click', (e) => { if (e.target.closest('.settings-nav-item, #settings-btn')) setTimeout(number, 0); });
  // Sections that appear later (Director mode, Video Sources) or are shown and hidden.
  let t = null;
  new MutationObserver((list) => {
    if (!list.some((m) => m.type === 'childList' || m.target.classList?.contains('settings-section'))) return;
    clearTimeout(t);
    t = setTimeout(number, 50);
  }).observe(document.getElementById('settings-modal') || document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['style', 'hidden'] });
})();
