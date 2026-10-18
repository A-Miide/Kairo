// Settings → Streaming: where Kairo's output goes on the internet. A list of
// destinations (YouTube, Facebook, Twitch or any RTMP/RTMPS address), each with
// its own switch, and the video and audio the stream is made with.
//
// Everything is saved in settings.stream:
//   { destinations: [{ id, name, preset, url, key, enabled }],
//     video: { resolution: '1080p', fps: 30, bitrate: 6000 },
//     audio: { bitrate: 160, music: true, input: true },
//     mix: [sound mixer channels, sound_mixer.js],
//     record: { output, bitrate: 12000, folder: null (Movies/Kairo), button: false } }
// Stream keys stay on this computer (they are saved in Kairo's own settings file and are
// never put in a log, an export or a link).
(function () {
  const PRESETS = [
    { id: 'youtube', label: 'YouTube Live', url: 'rtmp://a.rtmp.youtube.com/live2', keyHint: 'Stream key from YouTube Studio → Go live' },
    { id: 'facebook', label: 'Facebook Live', url: 'rtmps://live-api-s.facebook.com:443/rtmp/', keyHint: 'Stream key from the Facebook Live producer' },
    { id: 'twitch', label: 'Twitch', url: 'rtmp://live.twitch.tv/app', keyHint: 'Primary stream key from your Twitch dashboard' },
    { id: 'custom', label: 'Other RTMP or RTMPS', url: '', keyHint: 'The stream key your service gave you (may be blank)' },
  ];
  const RESOLUTIONS = [['720p', '1280 × 720'], ['1080p', '1920 × 1080']];
  const FPS = [30, 60];
  const VIDEO_BITRATES = [[2500, '2.5 Mbps · weak internet'], [4500, '4.5 Mbps'], [6000, '6 Mbps · recommended 1080p'], [9000, '9 Mbps · 1080p60']];
  const AUDIO_BITRATES = [128, 160, 192];
  const RECORD_BITRATES = [[8000, '8 Mbps · smaller files'], [12000, '12 Mbps · recommended'], [20000, '20 Mbps · best']];
  const DEFAULTS = {
    destinations: [],
    video: { resolution: '1080p', fps: 30, bitrate: 6000 },
    audio: { bitrate: 160, music: true, input: true },
  };

  let editingId = null;      // the destination whose form is open ('new' while adding)
  let checking = {};          // id → 'checking' | { ok, message }
  let revealed = new Set();   // ids whose key is shown
  let mixerEl = null;         // the sound mixer, kept across redraws

  const host = () => document.getElementById('streaming-panel');
  const { el } = window.KairoUI;
  const uid = () => 'dest-' + Math.random().toString(36).slice(2, 8);

  // settings.stream is shared: the sound mixer (mix) and Recording (record) live in it too, so
  // every read keeps all of it and every save merges into it. (This used to rebuild it from
  // destinations/video/audio only, so changing a destination wiped the mix and recording.)
  function cfg() {
    const s = (typeof settings === 'object' && settings && settings.stream) || {};
    return {
      ...s,
      destinations: Array.isArray(s.destinations) ? s.destinations.filter(d => d && typeof d === 'object') : [],
      video: { ...DEFAULTS.video, ...(s.video || {}) },
      audio: { ...DEFAULTS.audio, ...(s.audio || {}) },
    };
  }
  function save(next) {
    next = { ...((typeof settings === 'object' && settings && settings.stream) || {}), ...next };
    settings.stream = next;
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ stream: next });
  }

  // rtmp:// or rtmps:// with a host; returns '' when fine, else what is wrong.
  function urlProblem(url) {
    const u = String(url || '').trim();
    if (!u) return 'Add the server address.';
    if (!/^rtmps?:\/\//i.test(u)) return 'The address must start with rtmp:// or rtmps://';
    if (!/^rtmps?:\/\/[^/\s:]+(:\d+)?(\/|$)/i.test(u)) return 'That address has no server name.';
    return '';
  }
  const hostOf = (url) => (String(url).match(/^rtmps?:\/\/([^/\s:]+)/i) || [])[1] || '';
  const mask = (key) => (key ? '•'.repeat(Math.min(12, Math.max(6, key.length))) : 'No key');
  function presetOf(d) { return PRESETS.find(p => p.id === d.preset) || PRESETS[PRESETS.length - 1]; }

  const toggle = (checked, title, onChange) => window.KairoUI.toggle(checked, title, onChange);
  function select(options, value, onChange) {
    const s = el('select', 'setting-input');
    options.forEach(([v, label]) => { const o = document.createElement('option'); o.value = String(v); o.textContent = label; if (String(v) === String(value)) o.selected = true; s.appendChild(o); });
    s.addEventListener('change', () => onChange(s.value));
    return s;
  }
  // A settings row: the name on the left, the control on the right with its short hint
  // right under it (the same as every other settings page).
  function row(name, hint, control) {
    const r = el('div', 'st-row');
    const t = el('div', 'st-text');
    t.appendChild(el('div', 'st-name', name));
    const c = el('div', 'st-control');
    c.appendChild(control);
    // A switch sits on the right in line with its name; what it does is said under the name.
    if (control.classList && control.classList.contains('output-toggle')) {
      r.classList.add('st-row-toggle');
      if (hint) t.appendChild(el('p', 'setting-hint st-hint', hint));
    } else if (hint) c.appendChild(el('p', 'setting-hint st-hint', hint));
    r.append(t, c);
    return r;
  }

  async function checkDestination(d) {
    const bad = urlProblem(d.url);
    if (bad) { checking[d.id] = { ok: false, message: bad }; render(); return; }
    checking[d.id] = 'checking'; render();
    try {
      const r = await fetch(`${SERVER}/api/stream/check`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ url: d.url }),
      });
      const j = await r.json();
      checking[d.id] = j.ok ? { ok: true, message: `Reached ${j.host}` } : { ok: false, message: j.error || 'Could not reach that server.' };
    } catch {
      checking[d.id] = { ok: false, message: 'Could not run the check.' };
    }
    render();
  }

  function destinationCard(d) {
    const c = cfg();
    const card = el('div', 'st-dest' + (d.enabled === false ? ' off' : ''));
    const p = presetOf(d);
    const head = el('div', 'st-dest-head');
    const title = el('div', 'st-dest-title');
    title.appendChild(el('span', 'st-dest-name', d.name || p.label));
    if ((d.name || p.label) !== p.label) title.appendChild(el('span', 'st-dest-kind', p.label));
    head.appendChild(title);
    const actions = el('div', 'st-dest-actions');
    const test = el('button', 'modal-btn secondary', checking[d.id] === 'checking' ? 'Checking…' : 'Check');
    test.type = 'button'; test.disabled = checking[d.id] === 'checking';
    test.title = 'Checks that this server can be reached from this computer. It does not start a stream.';
    test.addEventListener('click', () => checkDestination(d));
    const edit = el('button', 'modal-btn secondary', 'Edit'); edit.type = 'button';
    edit.addEventListener('click', () => { editingId = d.id; render(); });
    actions.append(test, edit, toggle(d.enabled !== false, 'Send the stream here', (on) => {
      save({ ...c, destinations: c.destinations.map(x => x.id === d.id ? { ...x, enabled: on } : x) }); render();
    }));
    head.appendChild(actions);
    card.appendChild(head);

    const meta = el('div', 'st-dest-meta');
    meta.appendChild(el('span', '', d.url || 'No address'));
    const keyEl = el('span', 'st-key', revealed.has(d.id) ? (d.key || 'No key') : mask(d.key));
    const reveal = el('button', 'st-link', revealed.has(d.id) ? 'Hide key' : 'Show key'); reveal.type = 'button';
    reveal.addEventListener('click', () => { revealed.has(d.id) ? revealed.delete(d.id) : revealed.add(d.id); render(); });
    meta.append(keyEl, reveal);
    card.appendChild(meta);

    const live = window.KairoStreaming && window.KairoStreaming.destStatus(d.id);
    if (live) {
      const label = { live: 'Live', connecting: 'Connecting…', failed: 'Not reaching it' }[live.state] || live.state;
      const line = el('p', 'st-result st-live ' + (live.state === 'live' ? 'ok' : live.state === 'failed' ? 'bad' : ''), `● ${label}${live.error ? ' · ' + live.error : ''}`);
      card.appendChild(line);
    }
    const res = checking[d.id];
    if (res && res !== 'checking') card.appendChild(el('p', 'st-result ' + (res.ok ? 'ok' : 'bad'), res.message));
    return card;
  }

  function destinationForm(d, isNew) {
    const c = cfg();
    const draft = { ...d };
    const form = el('div', 'st-form');
    const p0 = () => presetOf(draft);

    const presetSel = select(PRESETS.map(p => [p.id, p.label]), draft.preset || 'custom', (v) => {
      const was = p0();
      draft.preset = v;
      const next = presetOf(draft);
      if (!draft.url || draft.url === was.url) draft.url = next.url;
      if (!draft.name || draft.name === was.label) draft.name = next.label;
      fill();
    });
    const nameIn = el('input', 'setting-input'); nameIn.maxLength = 40; nameIn.placeholder = 'Name';
    const urlIn = el('input', 'setting-input'); urlIn.placeholder = 'rtmp://…'; urlIn.spellcheck = false; urlIn.autocapitalize = 'off';
    const keyIn = el('input', 'setting-input'); keyIn.type = 'password'; keyIn.placeholder = 'Stream key'; keyIn.autocomplete = 'off'; keyIn.spellcheck = false;
    const keyHint = el('p', 'setting-hint');
    const problem = el('p', 'st-result bad'); problem.hidden = true;
    function fill() {
      presetSel.value = draft.preset || 'custom';
      nameIn.value = draft.name || ''; urlIn.value = draft.url || ''; keyIn.value = draft.key || '';
      keyHint.textContent = p0().keyHint;
    }
    nameIn.addEventListener('input', () => { draft.name = nameIn.value; });
    urlIn.addEventListener('input', () => { draft.url = urlIn.value.trim(); });
    keyIn.addEventListener('input', () => { draft.key = keyIn.value.trim(); });
    fill();

    const outputs = typeof displayOutputs === 'function' ? displayOutputs() : [];
    const outSel = outputs.length > 1 ? select(outputs.map(o => [o.id, o.name]), draft.output || outputs[0].id, (v) => { draft.output = v; }) : null;
    form.append(
      row('Service', 'Fills in the address for you', presetSel),
      ...(outSel ? [row('Streams', 'Which output this destination gets', outSel)] : []),
      row('Name', 'How you will recognise it here', nameIn),
      row('Server address', 'Starts with rtmp:// or rtmps://', urlIn),
      row('Stream key', null, keyIn),
      keyHint, problem,
    );
    const buttons = el('div', 'st-form-buttons');
    const cancel = el('button', 'modal-btn secondary', 'Cancel'); cancel.type = 'button';
    cancel.addEventListener('click', () => { editingId = null; render(); });
    const done = el('button', 'modal-btn primary', isNew ? 'Add destination' : 'Save'); done.type = 'button';
    done.addEventListener('click', () => {
      const bad = urlProblem(draft.url);
      if (bad) { problem.textContent = bad; problem.hidden = false; return; }
      const finalD = { ...draft, name: (draft.name || p0().label).trim().slice(0, 40), preset: draft.preset || 'custom', url: draft.url.trim(), key: (draft.key || '').trim() };
      const list = isNew ? [...c.destinations, finalD] : c.destinations.map(x => x.id === d.id ? finalD : x);
      save({ ...c, destinations: list });
      delete checking[d.id];
      editingId = null; render();
    });
    buttons.append(cancel, done);
    if (!isNew) {
      const del = el('button', 'modal-btn secondary st-delete', 'Delete'); del.type = 'button';
      del.addEventListener('click', () => { save({ ...c, destinations: c.destinations.filter(x => x.id !== d.id) }); editingId = null; render(); });
      buttons.prepend(del);
    }
    form.appendChild(buttons);
    return form;
  }

  function render() {
    const h = host();
    if (!h) return;
    const c = cfg();
    h.innerHTML = '';

    // Destinations
    const dests = el('div', 'st-card');
    const dHead = el('div', 'st-card-head');
    dHead.appendChild(el('div', 'st-card-title', 'Destinations'));
    const S = window.KairoStreaming;
    if (S && c.destinations.some(d => d.enabled !== false && d.url)) {
      const isLive = S.isLive();
      const go = el('button', 'modal-btn ' + (isLive ? 'secondary st-stop' : 'secondary st-go'), isLive ? 'Stop stream' : 'Go live');
      go.type = 'button';
      go.disabled = !S.available;
      if (!S.available) go.title = 'Streaming needs the Kairo app';
      go.addEventListener('click', () => (isLive ? S.stop() : S.start()));
      dHead.appendChild(go);
    }
    const add = el('button', 'modal-btn primary', '+ Add destination'); add.type = 'button';
    add.disabled = editingId === 'new';
    add.addEventListener('click', () => { editingId = 'new'; render(); });
    dHead.appendChild(add);
    dests.appendChild(dHead);
    const body = el('div', 'st-card-body');
    if (!c.destinations.length && editingId !== 'new') {
      body.appendChild(el('p', 'setting-hint', 'Nothing to stream to yet. Add YouTube, Facebook, Twitch or any RTMP address. You can add several and send to all of them at once.'));
    }
    c.destinations.forEach(d => body.appendChild(editingId === d.id ? destinationForm(d, false) : destinationCard(d)));
    if (editingId === 'new') body.appendChild(destinationForm({ id: uid(), preset: 'youtube', name: 'YouTube Live', url: PRESETS[0].url, key: '', enabled: true }, true));
    dests.appendChild(body);
    h.appendChild(dests);

    // Video
    const video = el('div', 'st-card');
    video.appendChild(el('div', 'st-card-head')).appendChild(el('div', 'st-card-title', 'Video'));
    const vb = el('div', 'st-card-body');
    vb.append(
      row('Size', 'What the viewers get', select(RESOLUTIONS.map(([v, l]) => [v, `${v} · ${l}`]), c.video.resolution, (v) => save({ ...cfg(), video: { ...cfg().video, resolution: v } }))),
      row('Frame rate', '30 suits most services', select(FPS.map(f => [f, `${f} fps`]), c.video.fps, (v) => save({ ...cfg(), video: { ...cfg().video, fps: Number(v) } }))),
      row('Quality', 'Pick one your internet upload can carry with room to spare', select(VIDEO_BITRATES, c.video.bitrate, (v) => save({ ...cfg(), video: { ...cfg().video, bitrate: Number(v) } }))),
    );
    video.appendChild(vb);
    h.appendChild(video);

    // Audio
    const audio = el('div', 'st-card');
    audio.appendChild(el('div', 'st-card-head')).appendChild(el('div', 'st-card-title', 'Sound'));
    const ab = el('div', 'st-card-body');
    // The same mixer each redraw (the page redraws with the live status): a fader being
    // dragged is never replaced under the pointer.
    if (!mixerEl) mixerEl = el('div');
    if (window.KairoMixer && !window.KairoMixer.isMounted(mixerEl)) window.KairoMixer.mount(mixerEl);
    const mixer = mixerEl;
    ab.append(
      mixer,
      row('Quality', null, select(AUDIO_BITRATES.map(b => [b, `${b} kbps`]), c.audio.bitrate, (v) => save({ ...cfg(), audio: { ...cfg().audio, bitrate: Number(v) } }))),
    );
    audio.appendChild(ab);
    h.appendChild(audio);

    // Recording
    const S3 = window.KairoStreaming;
    const r = c.record || {};
    const setRec = (patch) => save({ ...cfg(), record: { ...(cfg().record || {}), ...patch } });
    const rec = el('div', 'st-card');
    const rHead = el('div', 'st-card-head');
    rHead.appendChild(el('div', 'st-card-title', 'Recording'));
    if (S3) {
      const on = S3.isRecording();
      const rb = el('button', 'modal-btn secondary', on ? 'Stop recording' : 'Record'); rb.type = 'button';
      rb.disabled = !S3.available;
      if (!S3.available) rb.title = 'Recording needs the Kairo app';
      rb.addEventListener('click', () => (on ? S3.stopRecording() : S3.startRecording()));
      rHead.appendChild(rb);
    }
    rec.appendChild(rHead);
    const rbBody = el('div', 'st-card-body');
    const outs = typeof displayOutputs === 'function' ? displayOutputs() : [];
    const folderCtl = el('div', 'st-folder');
    const folderName = el('span', 'st-folder-name', r.folder || 'Movies → Kairo');
    folderName.title = r.folder || '';
    const pick = el('button', 'modal-btn secondary', 'Change…'); pick.type = 'button';
    const dialog = window.__TAURI__ && window.__TAURI__.dialog;
    pick.disabled = !(dialog && dialog.open);
    pick.addEventListener('click', async () => {
      try { const f = await dialog.open({ directory: true, multiple: false, title: 'Where recordings go' }); if (typeof f === 'string' && f) setRec({ folder: f }); } catch {}
    });
    folderCtl.append(folderName, pick);
    if (r.folder) { const reset = el('button', 'modal-btn secondary', 'Use Movies'); reset.type = 'button'; reset.addEventListener('click', () => setRec({ folder: null })); folderCtl.appendChild(reset); }
    rbBody.append(
      ...(outs.length > 1 ? [row('Records', 'Which output is recorded', select(outs.map(o => [o.id, o.name]), r.output || outs[0].id, (v) => setRec({ output: v })))] : []),
      row('Quality', 'Same size and frame rate as the stream', select(RECORD_BITRATES, r.bitrate || 12000, (v) => setRec({ bitrate: Number(v) }))),
      row('Folder', 'Each recording is an MP4 named by its date and time', folderCtl),
      row('Top-bar button', 'Shows Record in the top bar; always there in Director mode', toggle(r.button === true, '', (on) => setRec({ button: on }))),
    );
    const ro = S3 ? S3.recOverall() : { state: 'off', files: [] };
    if (ro.files && ro.files.length) {
      const last = el('div', 'st-folder');
      const file = ro.files[ro.files.length - 1];
      last.append(el('span', 'st-folder-name', file.split(/[\\/]/).pop()));
      const show = el('button', 'modal-btn secondary', 'Show'); show.type = 'button';
      show.addEventListener('click', () => S3.reveal(file));
      last.appendChild(show);
      rbBody.appendChild(row(ro.state === 'off' ? 'Last recording' : 'Recording to', ro.files.length > 1 ? `In ${ro.files.length} parts: it restarted after trouble${ro.why ? ` (${ro.why})` : ''}` : null, last));
    }
    if (ro.state !== 'off') (ro.warnings || []).concat(ro.errors || []).forEach(t => rbBody.appendChild(el('p', 'st-result bad', t)));
    rec.appendChild(rbBody);
    h.appendChild(rec);

    const S2 = window.KairoStreaming;
    const o = S2 ? S2.overall() : { state: 'off' };
    if (o.state !== 'off') {
      (o.warnings || []).concat(o.errors || []).forEach(t => h.appendChild(el('p', 'st-result bad st-foot', t)));
    }
    if (S2 && S2.isLive()) h.insertBefore(el('p', 'st-result st-foot', 'Changes here take effect the next time you go live.'), h.firstChild);
    const note = el('p', 'setting-hint st-foot', 'The stream and recordings are what the output shows: scripture, slides, the layout, overlays and the camera on air in Director mode, with the sound of the audio input chosen under Audio (music played inside Kairo is not in them). Your stream keys stay on this computer.');
    h.appendChild(note);
  }

  const style = document.createElement('style');
  style.textContent = `
    #streaming-panel { display: flex; flex-direction: column; gap: 14px; }
    .st-card { border: 1px solid var(--border); border-radius: var(--radius-lg); background: rgba(255,255,255,0.015); }
    .st-card-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
    .st-card-head .modal-btn { white-space: nowrap; flex: none; }
    .st-card-head .st-go { margin-left: auto; }
    .st-card-head .st-stop { margin-left: auto; border-color: #b5532c; color: #ffd9cc; }
    .st-live { font-weight: 600; }
    .st-card-title { font-size: 12px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; color: var(--text-dim, var(--text)); }
    .st-card-body { padding: 4px 16px 12px; display: flex; flex-direction: column; }
    .st-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 10px 0; border-bottom: 1px solid var(--border); }
    .st-row:last-child { border-bottom: 0; }
    .st-name { font-size: 13px; font-weight: 600; color: var(--text); }
    .st-text .setting-hint { margin: 2px 0 0; }
    .st-row { align-items: flex-start; }
    .st-name { padding-top: 8px; }
    .st-control { flex: none; width: 260px; display: flex; flex-direction: column; gap: 4px; align-items: stretch; }
    .st-control .setting-input { width: 100%; }
    .st-control .output-toggle { align-self: flex-end; }
    .st-row-toggle { align-items: center; }
    .st-row-toggle .st-name { padding-top: 0; }
    .st-row-toggle .st-text { flex: 1 1 auto; min-width: 0; }
    .st-row-toggle .st-control { width: auto; }
    .st-row-toggle .st-control .output-toggle { align-self: center; }
    .st-row-toggle .st-hint { margin-top: 2px; }
    .st-hint { margin: 0; font-size: 12px; color: var(--text-3, #8b8b96); }
    .st-dest { border: 1px solid var(--border); border-radius: var(--radius-md, 10px); padding: 12px 14px; margin: 10px 0 0; }
    .st-dest.off { opacity: 0.6; }
    .st-dest-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
    .st-dest-title { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
    .st-dest-name { font-size: 14px; font-weight: 700; color: var(--text); }
    .st-dest-kind { font-size: 11.5px; color: var(--text-muted, #8b8b96); }
    .st-dest-actions { display: flex; align-items: center; gap: 8px; }
    .st-dest-meta { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-top: 8px; font-size: 12px; color: var(--text-muted, #8b8b96); font-family: ui-monospace, Menlo, monospace; }
    .st-link { background: none; border: 0; padding: 0; color: var(--accent, #ff7a30); cursor: pointer; font: inherit; font-family: inherit; font-size: 12px; }
    .st-result { margin: 8px 0 0; font-size: 12px; }
    .st-result.ok { color: #4cc38a; }
    .st-result.bad { color: var(--orange, #ff9130); }
    .st-form { border: 1px solid var(--border); border-radius: var(--radius-md, 10px); padding: 4px 14px 12px; margin: 10px 0 0; background: rgba(255,255,255,0.02); }
    .st-form-buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 10px; }
    .st-delete { margin-right: auto; }
    .st-foot { margin: 2px 2px 0; }
    .st-folder { display: flex; align-items: center; gap: 8px; min-width: 0; flex-wrap: wrap; }
    .st-folder-name { font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; max-width: 100%; }
  `;
  document.head.appendChild(style);

  // Live status: redraw while the pane is open (not while a destination form is being typed in).
  const hook = () => { if (window.KairoStreaming) window.KairoStreaming.onChange(() => { if (!editingId && host() && host().offsetParent) render(); }); else setTimeout(hook, 500); };
  hook();
  // Draw when the pane is opened (settings are loaded by then), and once now.
  document.addEventListener('click', (e) => {
    const b = e.target.closest && e.target.closest('[data-pane="streaming"]');
    if (b) setTimeout(render, 0);
  });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', () => setTimeout(render, 0));
  else setTimeout(render, 0);
})();
