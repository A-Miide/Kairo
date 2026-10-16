// Instant replay (Director mode): while switched on, the camera on air is kept recording in
// the background, so Replay can put the last few seconds in Preview — a video like any
// other, sent to the screens with Send. Nothing goes out by itself.
//
// How: two recorders on the camera, each restarting every SEG seconds and half a period
// apart, so one of them always holds between REPLAY and SEG seconds of the latest picture
// as a complete file. Replay finishes that one, saves it to the Media Bin ("Replay …") and
// stages it. Off by default: it keeps the camera open and encoding while on.
(function () {
  const REPLAY = 8, SEG = 16;
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
  const D = () => window.KairoVideoDirector;
  const enabled = () => !!(typeof settings === 'object' && settings && settings.videoDirector && settings.videoReplay);

  let stream = null, sourceId = null, slots = [], timers = [], busy = false, opening = false;
  const mime = () => {
    const MR = window.MediaRecorder;
    if (!MR) return null;
    return ['video/mp4;codecs=avc1', 'video/mp4', 'video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm'].find(t => MR.isTypeSupported && MR.isTypeSupported(t)) || '';
  };

  function programSourceId() {
    try { const st = D() && D().state(); const p = st && st.active && st.program; return p && !p.mixed && p.id ? p.id : null; } catch { return null; }
  }
  function sourceById(id) {
    try { return (JSON.parse(localStorage.getItem('kairo-video-sources') || '[]') || []).find(s => s && s.id === id) || null; } catch { return null; }
  }

  function stopAll() {
    timers.forEach(clearTimeout); timers = [];
    slots.forEach(s => { s.discard = true; clearTimeout(s.timer); try { if (s.mr && s.mr.state !== 'inactive') s.mr.stop(); } catch {} });
    slots = [];
    if (stream) { stream.getTracks().forEach(t => t.stop()); stream = null; }
    sourceId = null;
    render();
  }

  // One recorder: records until it rolls over (thrown away) or Replay takes it.
  function begin(slot) {
    if (!stream) return;
    const type = mime();
    let mr;
    try { mr = new MediaRecorder(stream, { ...(type ? { mimeType: type } : {}), videoBitsPerSecond: 6_000_000 }); } catch { return; }
    slot.mr = mr; slot.chunks = []; slot.started = Date.now(); slot.discard = false; slot.take = null;
    mr.ondataavailable = (e) => { if (e.data && e.data.size) slot.chunks.push(e.data); };
    mr.onstop = () => {
      const take = slot.take, chunks = slot.chunks, discard = slot.discard;
      if (take && chunks.length) take(new Blob(chunks, { type: mr.mimeType || type || 'video/webm' }));
      else if (take) take(null);
      if (!discard && stream) begin(slot);
    };
    mr.start(1000);
    clearTimeout(slot.timer);
    slot.timer = setTimeout(() => { if (slot.mr === mr && mr.state !== 'inactive') mr.stop(); }, SEG * 1000);
  }

  async function open(id) {
    if (opening) return;
    opening = true;
    try {
      const src = sourceById(id);
      const V = window.KairoVideoSources;
      const devices = await navigator.mediaDevices.enumerateDevices();
      const deviceId = src && V && V.resolveDevice ? V.resolveDevice(src, devices) : null;
      if (!deviceId) return;
      const s = await navigator.mediaDevices.getUserMedia({ audio: false, video: { deviceId: { exact: deviceId }, width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30, max: 30 } } });
      if (!enabled() || programSourceId() !== id) { s.getTracks().forEach(t => t.stop()); return; }
      stopAll();
      stream = s; sourceId = id;
      slots = [{}, {}];
      begin(slots[0]);
      timers.push(setTimeout(() => begin(slots[1]), (SEG / 2) * 1000));
    } catch (e) {
      console.warn('[Replay] camera:', e && e.message);
    } finally { opening = false; render(); }
  }

  // Follows the camera on air; let go of it when replay is off or nothing is on air.
  function follow() {
    const want = enabled() ? programSourceId() : null;
    if (!want) { if (stream) stopAll(); else render(); return; }
    if (want !== sourceId) open(want);
    else render();
  }

  const ready = () => slots.some(s => s.mr && s.mr.state === 'recording' && Date.now() - s.started >= REPLAY * 1000);
  async function replay() {
    if (busy || !stream) return false;
    const now = Date.now();
    const candidates = slots.filter(s => s.mr && s.mr.state === 'recording');
    // The one holding the most (up to SEG seconds), as long as it has at least REPLAY.
    const slot = candidates.sort((a, b) => a.started - b.started).find(s => now - s.started >= REPLAY * 1000) || candidates[0];
    if (!slot) return false;
    busy = true; render();
    try {
      const blob = await new Promise((res) => { slot.take = res; slot.mr.stop(); });
      if (!blob || !blob.size) throw new Error('nothing was recorded');
      const ext = /mp4/.test(blob.type) ? 'mp4' : 'webm';
      const d = new Date(), p = (n) => String(n).padStart(2, '0');
      const filename = `Replay ${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}.${ext}`;
      const dataBase64 = await new Promise((res, rej) => { const r = new FileReader(); r.onload = () => res(String(r.result).split(',')[1] || ''); r.onerror = rej; r.readAsDataURL(blob); });
      const up = await fetch(`${SERVER}/api/media/bin/upload`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ filename, dataBase64 }) }).then(r => r.json());
      if (!up || !up.ok) throw new Error((up && up.error) || 'it could not be saved');
      const bin = await fetch(`${SERVER}/api/media/bin`).then(r => r.json()).catch(() => ({}));
      const item = (bin.items || []).find(i => i.name === up.name) || { name: up.name, kind: 'video', url: `/api/media/bin/file/${encodeURIComponent(up.name)}` };
      // In Director mode a media item goes to Preview; Send puts it on the screens.
      await window.KairoService?.sendMedia?.(item, 'cover');
      if (typeof showToast === 'function') showToast('Replay is in Preview. Send puts it on the screens.', 'success');
      return true;
    } catch (e) {
      if (typeof showToast === 'function') showToast(`Replay failed: ${e && e.message ? e.message : e}`, 'error');
      return false;
    } finally { busy = false; render(); }
  }

  function setEnabled(on) {
    settings.videoReplay = !!on;
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ videoReplay: !!on });
    follow();
  }

  // ── The top bar: Replay (Director mode only) with its switch ──
  let btn = null, pop = null;
  function closePop() { if (pop) { pop.remove(); pop = null; document.removeEventListener('mousedown', outside, true); document.removeEventListener('keydown', esc, true); } }
  const outside = (e) => { if (pop && !pop.contains(e.target) && e.target !== btn) closePop(); };
  const esc = (e) => { if (e.key === 'Escape') { e.stopPropagation(); closePop(); } };
  function openPop() {
    closePop();
    pop = el('div', 'rp-pop');
    pop.setAttribute('role', 'dialog');
    pop.setAttribute('aria-label', 'Instant replay');
    const row = el('label', 'rp-row');
    const cb = el('input'); cb.type = 'checkbox'; cb.checked = enabled();
    cb.addEventListener('change', () => { setEnabled(cb.checked); openPop(); });
    row.append(cb, el('span', null, 'Keep the camera on air recording for replays'));
    pop.appendChild(row);
    pop.appendChild(el('p', 'rp-hint', enabled()
      ? (stream ? (ready() ? `Replay puts about the last ${REPLAY}–${SEG} seconds in Preview.` : 'Getting the first seconds…') : 'Waiting for a camera on air.')
      : 'Off: the camera isn’t recorded. On, it uses a little more of the computer while Director mode runs.'));
    const go = el('button', 'modal-btn primary', busy ? 'Saving…' : 'Replay to Preview');
    go.type = 'button';
    go.disabled = busy || !stream || !ready();
    go.addEventListener('click', () => { closePop(); replay(); });
    pop.appendChild(go);
    const r = btn.getBoundingClientRect();
    pop.style.top = `${r.bottom + 6}px`; pop.style.left = `${Math.max(8, Math.min(window.innerWidth - 300, r.right - 290))}px`;
    document.body.appendChild(pop);
    document.addEventListener('mousedown', outside, true);
    document.addEventListener('keydown', esc, true);
  }
  function render() {
    const listen = document.getElementById('listen-btn');
    if (!listen) return;
    if (!btn) {
      btn = el('button', 'tb-stream-btn tb-replay-btn', 'Replay');
      btn.type = 'button';
      btn.title = 'Instant replay of the camera on air';
      btn.setAttribute('aria-haspopup', 'dialog');
      btn.addEventListener('click', () => (pop ? closePop() : openPop()));
      listen.parentNode.insertBefore(btn, document.getElementById('record-btn') || document.getElementById('stream-btn') || listen);
    }
    btn.hidden = !(typeof settings === 'object' && settings && settings.videoDirector);
    btn.classList.toggle('is-armed', !!stream && ready());
    btn.textContent = busy ? 'Replay…' : 'Replay';
    if (btn.hidden) closePop();
  }

  const style = document.createElement('style');
  style.textContent = `
    .tb-replay-btn[hidden] { display: none; }
    .tb-replay-btn.is-armed { border-color: #6f9a5b; }
    .rp-pop { position: fixed; z-index: 2000; width: 290px; padding: 12px 14px; border-radius: 12px; border: 1px solid var(--border); background: var(--bg-elevated, #1b1b1f); box-shadow: 0 16px 40px rgba(0,0,0,.45); display: flex; flex-direction: column; gap: 10px; }
    .rp-row { display: flex; gap: 8px; align-items: flex-start; font-size: 13px; color: var(--text); cursor: pointer; }
    .rp-hint { margin: 0; font-size: 12px; color: var(--text-2, var(--text)); line-height: 1.4; }
  `;
  document.head.appendChild(style);

  window.KairoReplay = { replay, setEnabled, isReady: () => !!stream && ready() };
  setInterval(follow, 1000);
})();
