// KAIRO — Sources: cameras and capture cards for live video input.
//
// A Settings pane (add / rename / remove / reorder, live thumbnails, a
// properties view, which source is live on each output) plus a quick switcher
// under the transition bar in the main window. Pure list logic lives in
// video_sources.js. The display windows open the devices themselves
// (display_video.js); this file only holds a device open to draw previews
// while the Sources pane is showing, so a camera is not kept busy otherwise.
//
// Persistence: settings.videoSources [{id, name, deviceId, label, width,
// height, fps}] and settings.videoLive {[outputId]: sourceId|null}, saved with
// saveSettingsPatch, and mirrored to localStorage (kairo-video-sources /
// kairo-video-live) for the display windows, as the layer map is.
// Loaded after app_outputs.js; initVideoSources() is called once settings load.
'use strict';

const VSRC = window.KairoVideoSources;
const videoState = {
  devices: [],          // videoinput MediaDeviceInfo list
  blind: false,         // devices listed but unlabelled: camera access not granted yet
  error: '',            // last permission/detect problem, in operator words
  selectedId: null,
  previews: new Map(),  // deviceId → { promise, stream, refs }
  visible: false,
  root: null,
  built: false,
};

function videoSourceList() {
  const list = VSRC.normalizeSources(settings.videoSources);
  settings.videoSources = list;
  return list;
}
// Outputs that can take video in Phase 1: display windows and Monitors.
function videoOutputs() {
  return displayOutputs().map(d => ({ id: d.id, name: d.name, monitor: d.kind === 'monitor' }));
}
function videoLiveMap() {
  return VSRC.normalizeLive(settings.videoLive, videoSourceList(), videoOutputs().map(o => o.id));
}

// What display windows read: the source list and the live map.
function publishVideoState() {
  try {
    localStorage.setItem('kairo-video-sources', JSON.stringify(videoSourceList()));
    localStorage.setItem('kairo-video-live', JSON.stringify(videoLiveMap()));
  } catch (err) {
    console.warn('[Video] could not publish state:', err.message);
  }
}

function saveVideoSources(list) {
  settings.videoSources = list;
  settings.videoLive = videoLiveMap();   // a removed source drops out of every output
  saveSettingsPatch({ videoSources: settings.videoSources, videoLive: settings.videoLive });
  publishVideoState();
  renderVideoSources();
}

function setVideoLiveFor(outputIds, sourceId) {
  settings.videoLive = VSRC.setLive(videoLiveMap(), outputIds, sourceId);
  saveSettingsPatch({ videoLive: settings.videoLive });
  publishVideoState();
  renderVideoSources();
}

// ── Devices and previews ───────────────────────────────────────────────────
async function refreshVideoDevices(askPermission) {
  videoState.error = '';
  if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) {
    videoState.error = 'This window cannot use cameras.';
    return;
  }
  if (askPermission) {
    // Labels stay empty until the camera has been allowed once.
    try {
      const probe = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
      probe.getTracks().forEach(t => t.stop());
    } catch (err) {
      videoState.error = VSRC.describeCaptureError(err);
    }
  }
  try {
    const all = await navigator.mediaDevices.enumerateDevices();
    videoState.devices = all.filter(d => d.kind === 'videoinput');
    videoState.blind = videoState.devices.some(d => !d.label);
  } catch (err) {
    videoState.error = 'The device list could not be read.';
  }
}

function previewKey(source) {
  return VSRC.resolveDevice(source, videoState.devices);
}

// One shared low-res stream per physical device for every thumbnail and the
// properties preview.
function acquirePreview(deviceId) {
  let entry = videoState.previews.get(deviceId);
  if (!entry) {
    entry = { refs: 0, stream: null, promise: null };
    entry.promise = navigator.mediaDevices.getUserMedia({
      video: { deviceId: { exact: deviceId }, width: { ideal: 1280 }, height: { ideal: 720 } }, audio: false,
    }).then(s => { entry.stream = s; return s; }).catch(err => { videoState.previews.delete(deviceId); throw err; });
    videoState.previews.set(deviceId, entry);
  }
  return entry;
}
function releaseAllPreviews() {
  for (const entry of videoState.previews.values()) {
    if (entry.stream) entry.stream.getTracks().forEach(t => t.stop());
    else entry.promise.then(s => s.getTracks().forEach(t => t.stop())).catch(() => {});
  }
  videoState.previews.clear();
}
// Keep only the devices still wanted; stop the rest.
function pruneVideoPreviews(wanted) {
  for (const [id, entry] of [...videoState.previews]) {
    if (wanted.has(id)) continue;
    if (entry.stream) entry.stream.getTracks().forEach(t => t.stop());
    else entry.promise.then(s => s.getTracks().forEach(t => t.stop())).catch(() => {});
    videoState.previews.delete(id);
  }
}
async function attachVideoPreview(el, deviceId, statusEl) {
  try {
    const entry = acquirePreview(deviceId);
    const stream = await entry.promise;
    if (!el.isConnected) return;
    el.srcObject = stream;
    el.play().catch(() => {});
    if (statusEl) statusEl.textContent = '';
  } catch (err) {
    if (statusEl) statusEl.textContent = VSRC.describeCaptureError(err);
  }
}

// ── DOM helpers ────────────────────────────────────────────────────────────
function vEl(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
function vBtn(label, title, onClick, cls = 'modal-btn') {
  const b = vEl('button', cls, label);
  b.type = 'button';
  if (title) b.title = title;
  b.addEventListener('click', onClick);
  return b;
}
function vSelect(options, value, onChange) {
  const sel = vEl('select', 'setting-input');
  options.forEach(([v, l]) => sel.add(new Option(l, v, false, v === value)));
  sel.value = value;
  sel.addEventListener('change', () => onChange(sel.value));
  return sel;
}
function sourceStatusText(src) {
  const id = previewKey(src);
  if (!videoState.devices.length && !videoState.blind) return 'Looking for devices';
  if (!id) return 'Not connected';
  const res = src.width && src.height ? `${src.width} × ${src.height}` : 'Automatic size';
  return src.fps ? `${res} · ${src.fps} fps` : res;
}

// ── The pane ───────────────────────────────────────────────────────────────
function buildVideoPane() {
  const panes = document.getElementById('settings-panes');
  const nav = document.getElementById('settings-nav');
  if (!panes || !nav || videoState.built) return;
  videoState.built = true;

  const navBtn = vEl('button', 'settings-nav-item');
  navBtn.dataset.pane = 'sources';
  navBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg><span>Sources</span>';
  const outputsBtn = nav.querySelector('[data-pane="outputs"]');
  outputsBtn ? outputsBtn.after(navBtn) : nav.appendChild(navBtn);

  const pane = vEl('section', 'settings-pane');
  pane.dataset.pane = 'sources';
  videoState.root = vEl('div', 'vsrc-root');
  pane.appendChild(videoState.root);
  panes.appendChild(pane);

  // Previews run only while this pane is on screen.
  new IntersectionObserver(async (entries) => {
    const on = entries.some(e => e.isIntersecting);
    if (on === videoState.visible) return;
    videoState.visible = on;
    if (on) { await refreshVideoDevices(false); renderVideoSources(); }
    else releaseAllPreviews();
  }).observe(videoState.root);
  navigator.mediaDevices?.addEventListener?.('devicechange', async () => {
    await refreshVideoDevices(false);
    renderVideoSources();
  });

  // The quick switcher under the transition bar.
  const bar = document.getElementById('rs-transition-bar');
  if (bar) {
    const sw = vEl('div', 'rs-video-bar hidden');
    sw.id = 'rs-video-bar';
    bar.after(sw);
  }
}

function renderVideoSources() {
  renderVideoSwitcher();
  const root = videoState.root;
  if (!root) return;
  const sources = videoSourceList();
  if (!sources.some(s => s.id === videoState.selectedId)) videoState.selectedId = sources[0]?.id || null;
  const selected = sources.find(s => s.id === videoState.selectedId) || null;
  root.innerHTML = '';

  // Sources
  const sec = vEl('div', 'settings-section');
  sec.appendChild(vEl('div', 'settings-section-label', 'Sources'));
  sec.appendChild(vEl('p', 'setting-hint', 'Cameras and capture cards. Turn on Video input for an output in Outputs, then choose what is live on it below. Video input goes to display windows and Monitors. NDI, Syphon and DeckLink outputs do not carry it yet.'));

  const addRow = vEl('div', 'vsrc-add-row');
  const free = videoState.devices.filter(d => !sources.some(s => VSRC.resolveDevice(s, [d]) === d.deviceId));
  const placeholder = [['', free.length ? 'Choose a device' : 'No new devices']];
  const sel = vSelect(placeholder.concat(free.map((d, i) => [d.deviceId, d.label || `Camera ${i + 1}`])), '', () => {});
  sel.disabled = !free.length;
  sel.setAttribute('aria-label', 'Device to add');
  addRow.appendChild(sel);
  addRow.appendChild(vBtn('Add source', 'Add the chosen device as a source', () => {
    const d = free.find(x => x.deviceId === sel.value);
    if (!d) return;
    const r = VSRC.addSource(sources, d);
    videoState.selectedId = r.added.id;
    saveVideoSources(r.list);
  }, 'modal-btn primary'));
  addRow.appendChild(vBtn('Detect devices', 'Look for cameras again and ask for camera access if needed', async () => {
    await refreshVideoDevices(true);
    renderVideoSources();
  }));
  sec.appendChild(addRow);
  if (videoState.error) sec.appendChild(vEl('p', 'setting-hint vsrc-error', videoState.error));
  else if (videoState.blind) sec.appendChild(vEl('p', 'setting-hint', 'Choose Detect devices and allow camera access to see device names.'));
  else if (!videoState.devices.length) sec.appendChild(vEl('p', 'setting-hint', 'No cameras found. Connect a camera or capture card, then choose Detect devices.'));

  const wanted = new Set();
  const split = vEl('div', 'vsrc-split');
  const listCol = vEl('div', 'vsrc-list');
  listCol.setAttribute('role', 'listbox');
  listCol.setAttribute('aria-label', 'Sources');
  const attachments = [];
  sources.forEach((src, i) => {
    const row = vEl('div', 'vsrc-item' + (src.id === videoState.selectedId ? ' active' : ''));
    row.tabIndex = 0;
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', String(src.id === videoState.selectedId));
    const thumb = vEl('div', 'vsrc-thumb');
    const vid = vEl('video'); vid.muted = true; vid.playsInline = true; vid.setAttribute('aria-hidden', 'true');
    thumb.appendChild(vid);
    const msg = vEl('span', 'vsrc-thumb-msg');
    thumb.appendChild(msg);
    const info = vEl('div', 'vsrc-item-info');
    info.appendChild(vEl('div', 'vsrc-item-name', src.name));
    info.appendChild(vEl('div', 'vsrc-item-sub', sourceStatusText(src)));
    const tools = vEl('div', 'vsrc-item-tools');
    const up = vBtn('↑', 'Move up', (e) => { e.stopPropagation(); saveVideoSources(VSRC.moveSource(sources, src.id, -1)); }, 'ls-icon-btn');
    const down = vBtn('↓', 'Move down', (e) => { e.stopPropagation(); saveVideoSources(VSRC.moveSource(sources, src.id, 1)); }, 'ls-icon-btn');
    up.disabled = i === 0; down.disabled = i === sources.length - 1;
    up.setAttribute('aria-label', `Move ${src.name} up`); down.setAttribute('aria-label', `Move ${src.name} down`);
    tools.append(up, down);
    row.append(thumb, info, tools);
    const choose = () => { videoState.selectedId = src.id; renderVideoSources(); };
    row.addEventListener('click', choose);
    row.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(); } });
    listCol.appendChild(row);
    const id = previewKey(src);
    if (id && videoState.visible) { wanted.add(id); attachments.push([vid, id, msg]); }
    else msg.textContent = id ? '' : 'Not connected';
  });
  if (!sources.length) listCol.appendChild(vEl('p', 'setting-hint vsrc-empty', 'No sources yet. Choose a device above and add it.'));
  split.appendChild(listCol);

  // Properties
  const props = vEl('div', 'vsrc-props');
  if (selected) {
    const big = vEl('div', 'vsrc-preview');
    const bv = vEl('video'); bv.muted = true; bv.playsInline = true; bv.setAttribute('aria-label', `Preview of ${selected.name}`);
    const bmsg = vEl('span', 'vsrc-thumb-msg');
    big.append(bv, bmsg);
    props.appendChild(big);
    const sid = previewKey(selected);
    if (sid && videoState.visible) { wanted.add(sid); attachments.push([bv, sid, bmsg]); }
    else bmsg.textContent = sid ? '' : 'Not connected';

    const field = (label, control) => {
      const w = vEl('label', 'vsrc-field');
      w.appendChild(vEl('span', 'vsrc-field-label', label));
      w.appendChild(control);
      return w;
    };
    const name = vEl('input', 'setting-input');
    name.type = 'text'; name.value = selected.name; name.maxLength = 60;
    name.addEventListener('change', () => saveVideoSources(VSRC.renameSource(sources, selected.id, name.value)));
    props.appendChild(field('Name', name));

    const devOpts = videoState.devices.map((d, i) => [d.deviceId, d.label || `Camera ${i + 1}`]);
    const current = previewKey(selected) || '';
    if (!current) devOpts.unshift(['', `${selected.label || 'Saved device'} (not connected)`]);
    props.appendChild(field('Device', vSelect(devOpts, current, (v) => {
      const d = videoState.devices.find(x => x.deviceId === v);
      if (d) saveVideoSources(VSRC.updateSource(sources, selected.id, { deviceId: d.deviceId, label: d.label }));
    })));
    const resValue = `${selected.width}x${selected.height}`;
    const resOpts = VSRC.RESOLUTIONS.map(r => [r.value, r.label]);
    props.appendChild(field('Resolution', vSelect(resOpts, resOpts.some(o => o[0] === resValue) ? resValue : '0x0', (v) => {
      const r = VSRC.RESOLUTIONS.find(x => x.value === v);
      saveVideoSources(VSRC.updateSource(sources, selected.id, { width: r.width, height: r.height }));
    })));
    props.appendChild(field('Frame rate', vSelect(VSRC.FPS_OPTIONS.map(f => [String(f), f ? `${f} fps` : 'Automatic']), String(selected.fps), (v) => {
      saveVideoSources(VSRC.updateSource(sources, selected.id, { fps: Number(v) }));
    })));
    props.appendChild(vEl('p', 'setting-hint', 'The camera uses the closest size and rate it supports.'));
    props.appendChild(vBtn('Remove source', 'Remove this source from Kairo', () => saveVideoSources(VSRC.removeSource(sources, selected.id)), 'modal-btn vsrc-remove'));
  } else {
    props.appendChild(vEl('p', 'setting-hint', 'Select a source to see its preview and properties.'));
  }
  split.appendChild(props);
  sec.appendChild(split);
  root.appendChild(sec);

  // Live on each output
  const live = videoLiveMap();
  const layers = outputLayerMap();
  const out = vEl('div', 'settings-section');
  out.appendChild(vEl('div', 'settings-section-label', 'Live on each output'));
  out.appendChild(vEl('p', 'setting-hint', 'One source is live per output. Changes use the output transition. The Video input layer must be on for the output, in Outputs.'));
  const table = vEl('div', 'vsrc-live');
  videoOutputs().forEach(o => {
    const row = vEl('div', 'vsrc-live-row');
    row.appendChild(vEl('span', 'vsrc-live-name', o.monitor ? `${o.name} (Monitor)` : o.name));
    const opts = [['', 'No video']].concat(sources.map(s => [s.id, s.name]));
    const select = vSelect(opts, live[o.id] || '', (v) => setVideoLiveFor([o.id], v || null));
    select.setAttribute('aria-label', `Live source on ${o.name}`);
    row.appendChild(select);
    const layerOn = layers[o.id] && layers[o.id].video;
    const toggle = vEl('label', 'vsrc-layer-toggle');
    const cb = vEl('input'); cb.type = 'checkbox'; cb.checked = !!layerOn;
    cb.addEventListener('change', () => { setOutputLayer(o.id, 'video', cb.checked); renderVideoSources(); });
    toggle.append(cb, document.createTextNode('Video input on'));
    row.appendChild(toggle);
    table.appendChild(row);
  });
  out.appendChild(table);
  root.appendChild(out);

  pruneVideoPreviews(wanted);
  attachments.forEach(([el, id, msg]) => attachVideoPreview(el, id, msg));
}

// ── Quick switcher (main window, under the transition bar) ────────────────
function renderVideoSwitcher() {
  const host = document.getElementById('rs-video-bar');
  if (!host) return;
  const sources = videoSourceList();
  host.innerHTML = '';
  host.classList.toggle('hidden', !sources.length);
  if (!sources.length) return;
  const outs = videoOutputs();
  const layers = outputLayerMap();
  const live = videoLiveMap();
  const on = outs.filter(o => layers[o.id] && layers[o.id].video);
  // What the outputs with Video input on agree on; mixed shows no active chip.
  const basis = on.length ? on : outs;
  const ids = new Set(basis.map(o => live[o.id] || ''));
  const activeId = ids.size === 1 ? [...ids][0] : null;

  host.appendChild(vEl('span', 'rs-video-label', 'Video'));
  const group = vEl('div', 'rs-video-chips');
  group.setAttribute('role', 'group');
  group.setAttribute('aria-label', 'Live video source');
  const chip = (id, label) => {
    const b = vBtn(label, `Make ${label.toLowerCase() === 'no video' ? 'no video' : label} live`, () => setVideoLiveFor(outs.map(o => o.id), id || null), 'rs-video-chip' + (activeId === id ? ' active' : ''));
    b.setAttribute('aria-pressed', String(activeId === id));
    return b;
  };
  group.appendChild(chip('', 'No video'));
  sources.forEach(s => group.appendChild(chip(s.id, s.name)));
  host.appendChild(group);
  if (!on.length) {
    const hint = vEl('p', 'rs-video-hint', 'Video input is off on every output. Turn it on in Settings, Outputs.');
    host.appendChild(hint);
  }
}

// Called by setOutputLayer (app_outputs.js) when a Video input box changes.
function onVideoLayerChanged() { renderVideoSwitcher(); if (videoState.visible) renderVideoSources(); }

async function initVideoSources() {
  buildVideoPane();
  videoSourceList();
  settings.videoLive = videoLiveMap();
  publishVideoState();
  await refreshVideoDevices(false);
  renderVideoSources();
}
