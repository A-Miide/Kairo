// Puts each setting's name and hint together on the left and its control on
// the right (see "Settings, row layout" in styles.css). Run once at start and
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
      Array.from(g.children).forEach((c) => {
        if (c === label || (c.matches('.setting-hint') && !c.hidden && !c.classList.contains('input-missing'))) copy.appendChild(c);
        else control.appendChild(c);
      });
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
  window.KairoSettingsLayout = { arrange: () => arrange(document) };
  const start = () => {
    arrange(document);
    const panes = document.querySelector('.settings-panes') || document.body;
    new MutationObserver(soon).observe(panes, { childList: true, subtree: true });
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else start();
})();
