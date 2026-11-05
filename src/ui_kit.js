// KAIRO — the small UI pieces every panel builds with, in one place (main window).
//
// el(tag, cls, text)          an element; text 0 or '' shows as given (null/undefined: none)
// toggle(checked, title, onChange, { stopClicks })
//                             the pill switch (same look as the output switches)
// popover(anchor, content, { cls, label, role, width, onClose })
//                             a panel under a top-bar button: closes on a click outside or
//                             Escape (focus goes back to the button), stays on screen at the
//                             right edge, placed after it is drawn. Returns { el, close, isOpen }.
//
// These were copied into each panel (macros, sound mixer, replay, streaming, overlays) and had
// drifted: one popover had no Escape, one showed nothing for 0, one went off screen.
'use strict';
(function () {
  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function toggle(checked, title, onChange, opts = {}) {
    const wrap = el('label', 'output-toggle');
    wrap.title = title || '';
    const input = document.createElement('input');
    input.type = 'checkbox';
    input.checked = !!checked;
    input.addEventListener('change', () => onChange(input.checked));
    wrap.append(input, el('span', 'output-toggle-track'));
    // Inside a clickable row the switch is its own control, not a click on the row.
    if (opts.stopClicks) wrap.addEventListener('click', (e) => e.stopPropagation());
    return wrap;
  }

  // One popover open at a time, like a menu.
  let open = null;
  function popover(anchor, content, opts = {}) {
    if (open) open.close();
    const pop = el('div', 'kui-pop' + (opts.cls ? ' ' + opts.cls : ''));
    pop.setAttribute('role', opts.role || 'dialog');
    if (opts.label) pop.setAttribute('aria-label', opts.label);
    if (opts.width) pop.style.width = typeof opts.width === 'number' ? `${opts.width}px` : opts.width;
    if (content) pop.appendChild(content);
    document.body.appendChild(pop);
    const place = () => {
      const r = anchor.getBoundingClientRect();
      const w = pop.offsetWidth;
      pop.style.top = `${Math.round(r.bottom + 6)}px`;
      pop.style.left = `${Math.round(Math.max(8, Math.min(window.innerWidth - w - 8, r.right - w)))}px`;
    };
    place();
    const outside = (e) => { if (!pop.contains(e.target) && !anchor.contains(e.target)) close(); };
    const esc = (e) => { if (e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); close(); try { anchor.focus({ preventScroll: true }); } catch {} } };
    const onResize = () => place();
    document.addEventListener('mousedown', outside, true);
    document.addEventListener('keydown', esc, true);
    window.addEventListener('resize', onResize);
    let closed = false;
    function close() {
      if (closed) return;
      closed = true;
      pop.remove();
      document.removeEventListener('mousedown', outside, true);
      document.removeEventListener('keydown', esc, true);
      window.removeEventListener('resize', onResize);
      if (open && open.el === pop) open = null;
      if (typeof opts.onClose === 'function') opts.onClose();
    }
    open = { el: pop, close, isOpen: () => !closed, place };
    return open;
  }

  const style = document.createElement('style');
  style.textContent = `
    .kui-pop { position: fixed; z-index: 2000; max-width: calc(100vw - 16px); max-height: 70vh; overflow: auto; border-radius: 12px; border: 1px solid var(--border); background: var(--bg-elevated, #1b1b1f); box-shadow: 0 16px 40px rgba(0,0,0,.45); }
  `;
  document.head.appendChild(style);

  window.KairoUI = { el, toggle, popover };
})();
