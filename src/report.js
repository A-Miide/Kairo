// KAIRO — Report a problem (owner, 2026-10-18).
//
// One window: the person says what happened, sees exactly what would go with it, and presses Send.
// Nothing leaves the computer before that press. The words, the version, the kind of computer, the
// speech engine and a short list of recent events (and last time's crash details, when there are
// some) go to the project's own feedback form through the app's server (report.rs); no sermon
// text, audio, verses, names or file paths are ever part of it. "Copy report" is for anyone who
// would rather send it another way. Opened from Help -> Report a Problem… and Settings -> Help.
'use strict';
(function () {
  let back = null;

  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
  const SERVER_URL = () => (typeof SERVER !== 'undefined' ? SERVER : '');

  function close() { if (back) { back.remove(); back = null; } document.removeEventListener('keydown', onKey, true); }
  function onKey(e) { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); } }

  async function open() {
    if (back) return;
    let info = { version: '', computer: '', engine: '', events: '', crash: null, ready: false };
    try { info = await (await fetch(`${SERVER_URL()}/api/report/preview`)).json(); } catch {}

    back = el('div', 'rp-back');
    const card = el('div', 'rp-card');
    card.setAttribute('role', 'dialog'); card.setAttribute('aria-modal', 'true'); card.setAttribute('aria-label', 'Report a problem');
    back.appendChild(card);
    back.addEventListener('mousedown', (e) => { if (e.target === back) close(); });

    const head = el('div', 'rp-head');
    head.append(el('h2', 'rp-title', 'Report a problem'), Object.assign(el('button', 'modal-btn secondary', 'Close'), { type: 'button', onclick: close }));
    card.appendChild(head);
    card.appendChild(el('p', 'rp-lead', 'Tell us what went wrong. Nothing is sent until you press Send, and you can see everything that goes with it below.'));

    const field = (label, hint, rows) => {
      const wrap = el('label', 'rp-field');
      wrap.appendChild(el('span', 'rp-label', label));
      const t = el('textarea', 'setting-input rp-text'); t.rows = rows; t.maxLength = 3000;
      if (hint) t.placeholder = hint;
      wrap.appendChild(t);
      card.appendChild(wrap);
      return t;
    };
    const what = field('What happened?', 'For example: the verse did not show on the screen after I pressed Send.', 4);
    const doing = field('What were you doing? (optional)', 'For example: I had just switched to Director mode.', 3);

    // What goes with it, readable before anything is sent.
    const box = el('div', 'rp-sends');
    box.appendChild(el('div', 'rp-sends-title', 'What is sent with your words'));
    const list = el('ul', 'rp-list');
    [['Kairo version', info.version], ['Computer', info.computer], ['Speech engine', info.engine]].forEach(([k, v]) => {
      const li = el('li'); li.append(el('strong', null, `${k}: `), document.createTextNode(v || 'unknown')); list.appendChild(li);
    });
    box.appendChild(list);
    const opt = (label, detail, on) => {
      const row = el('label', 'rp-opt');
      const cb = el('input'); cb.type = 'checkbox'; cb.checked = on;
      row.append(cb, el('span', null, label));
      box.appendChild(row);
      const d = el('details', 'rp-detail'); d.appendChild(el('summary', null, 'Show what this includes'));
      d.appendChild(el('pre', 'rp-pre', detail || '(nothing yet)'));
      box.appendChild(d);
      return cb;
    };
    const events = opt('Recent events from Kairo (no sermon text, audio, verses, names or file paths)', info.events, true);
    const crash = info.crash ? opt('Kairo hit an internal error last time: add its details', info.crash, true) : null;
    card.appendChild(box);

    const status = el('p', 'rp-status'); status.setAttribute('role', 'status');
    const bar = el('div', 'rp-bar');
    const send = Object.assign(el('button', 'modal-btn primary', 'Send'), { type: 'button' });
    const copy = Object.assign(el('button', 'modal-btn secondary', 'Copy report'), { type: 'button' });
    bar.append(send, copy);
    card.append(status, bar);

    const reportText = () => [
      'Kairo problem report', `Version: ${info.version}`, `Computer: ${info.computer}`, `Speech engine: ${info.engine}`, '',
      'What happened:', what.value.trim(), '', 'What I was doing:', doing.value.trim(),
      ...(events.checked && info.events ? ['', 'Recent events:', info.events] : []),
      ...(crash && crash.checked ? ['', 'Crash details:', info.crash] : []),
    ].join('\n');

    if (!info.ready) {
      send.disabled = true;
      send.title = 'Sending is not set up in this copy of Kairo. Use Copy report.';
      status.textContent = 'Sending is not set up in this copy of Kairo. Use Copy report instead.';
    }
    send.addEventListener('click', async () => {
      if (!what.value.trim()) { status.textContent = 'Please say what happened first.'; what.focus(); return; }
      send.disabled = true; status.textContent = 'Sending…';
      try {
        const r = await (await fetch(`${SERVER_URL()}/api/report/send`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ what: what.value, doing: doing.value, includeEvents: events.checked, includeCrash: !!(crash && crash.checked) }),
        })).json();
        if (r.ok) { status.textContent = 'Thank you. Your report has been sent.'; setTimeout(close, 2500); return; }
        status.textContent = r.error || 'The report could not be sent. You can copy it instead.';
      } catch { status.textContent = 'No connection, so the report was not sent. You can copy it instead.'; }
      send.disabled = false;
    });
    copy.addEventListener('click', async () => {
      try { await navigator.clipboard.writeText(reportText()); status.textContent = 'Copied. You can paste it into a message or an email.'; }
      catch { status.textContent = 'Could not copy. Select the text above and copy it yourself.'; }
    });

    document.body.appendChild(back);
    document.addEventListener('keydown', onKey, true);
    what.focus();
  }

  window.KairoReport = { open, close };
  document.addEventListener('click', (e) => { if (e.target.closest && e.target.closest('#report-problem-btn')) open(); });
})();
