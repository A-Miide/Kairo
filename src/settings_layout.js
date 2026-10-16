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
