// KAIRO — Video Sources: cameras, capture cards and named placeholders for
// live video input.
//
// A Settings page: every configured input as a row (name, device, size, rate,
// on/off, reorder, remove) with one add flow below it, and which source is
// live on each output. The same add flow opens as a small panel on the
// Director screen. Pure list logic lives in
// video_sources.js. The display windows open the devices themselves
// (display_video.js); this file only holds a device open to draw previews
// while the Sources pane is showing, so a camera is not kept busy otherwise.
//
// Persistence: settings.videoSources [{id, name, deviceId, label, width,
// height, fps, enabled}] and settings.videoLive {[outputId]: sourceId|null}, saved with
// saveSettingsPatch, and mirrored to localStorage (kairo-video-sources /
// kairo-video-live) for the display windows, as the layer map is.
// Lazy-loaded by video_director_boot.js only while Video director mode is on;
// startVideoSources() builds the pane and stopVideoSources() removes it again.
'use strict';

const VSRC = window.KairoVideoSources;
const videoState = {
  devices: [],          // videoinput MediaDeviceInfo list
  blind: false,         // devices listed but unlabelled: camera access not granted yet
  error: '',            // last permission/detect problem, in operator words
  selectedId: null,
  leases: [],           // hub leases held by the thumbnails now drawn
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
  // The display chosen for the multiview carries no output of its own.
  const mv = isDirectorOn() ? settings.videoMultiview : '';
  return displayOutputs().filter(d => d.id !== mv).map(d => ({ id: d.id, name: d.name, monitor: d.kind === 'monitor' }));
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
  const probe = async () => {
    // Labels (and every real camera beyond one blank entry) stay hidden until camera access
    // has been allowed once.
    videoState.asked = true;
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
      s.getTracks().forEach(t => t.stop());
    } catch (err) {
      videoState.error = VSRC.describeCaptureError(err);
      logVideoProblem('camera-access', err);
    }
  };
  if (askPermission) await probe();
  const list = async () => {
    try {
      const all = await navigator.mediaDevices.enumerateDevices();
      videoState.devices = all.filter(d => d.kind === 'videoinput');
      videoState.blind = videoState.devices.some(d => !d.label);
    } catch (err) {
      videoState.error = 'The device list could not be read.';
      logVideoProblem('device-list', err);
    }
  };
  await list();
  // The first look in a session asks for access by itself: otherwise a built-in camera
  // (FaceTime HD) only shows up after Detect devices, which nothing pointed to.
  if (videoState.blind && !videoState.asked && !askPermission) { await probe(); await list(); }
  // Nameless devices are WebKit's stand-in for cameras it won't name yet, not inputs.
  if (videoState.blind && !videoState.error) videoState.devices = videoState.devices.filter(d => d.label || d.deviceId);
}

// Camera trouble goes to the debug log (databases/debug.log), so it can be looked at later.
function logVideoProblem(what, err) {
  window.kairoLog('video-problem', { what, name: err && err.name, message: String(err && err.message || err).slice(0, 300) });
}

function previewKey(source) {
  return VSRC.resolveDevice(source, videoState.devices);
}

// Thumbnails and the properties preview take their stream from the hub: one
// low-rate capture per device, shared with the director's Preview and Program.
function releaseAllPreviews() {
  videoState.leases.forEach(release => release());
  videoState.leases = [];
}
function attachVideoPreview(el, deviceId, statusEl) {
  const release = window.KairoVideoHub.lease(deviceId, 'thumb', (stream) => {
    if (!el.isConnected) return;
    el.srcObject = stream;
    el.play().catch(() => {});
    if (statusEl) statusEl.textContent = '';
  }, (err) => { if (statusEl) statusEl.textContent = VSRC.describeCaptureError(err); });
  videoState.leases.push(release);
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
  const st = VSRC.stateOf(src, videoState.devices);
  if (st === 'placeholder') return 'No device yet';
  if (st === 'missing') return 'Not connected';
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
  navBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="14" height="12" rx="2"/><path d="M16 10l6-3v10l-6-3z"/></svg><span>Video Sources</span>';
  const anchor = nav.querySelector('[data-pane="director"]') || nav.querySelector('[data-pane="outputs"]');
  anchor ? anchor.after(navBtn) : nav.appendChild(navBtn);

  const pane = vEl('section', 'settings-pane');
  pane.dataset.pane = 'sources';
  videoState.root = vEl('div', 'vsrc-root');
  pane.appendChild(videoState.root);
  panes.appendChild(pane);

  // Previews run only while this pane is on screen.
  videoState.observer = new IntersectionObserver(async (entries) => {
    const on = entries.some(e => e.isIntersecting);
    if (on === videoState.visible) return;
    videoState.visible = on;
    if (on) { await refreshVideoDevices(false); renderVideoSources(); }
    else releaseAllPreviews();
  });
  videoState.observer.observe(videoState.root);
  videoState.onDeviceChange = async () => {
    await refreshVideoDevices(false);
    renderVideoSources();
  };
  navigator.mediaDevices?.addEventListener?.('devicechange', videoState.onDeviceChange);
}

// ── Add flow: one input at a time, as a placeholder or from a detected device ──
// Used by the Video Sources page and by the small panel the Director screen
// opens, so inputs can be added one after another without leaving either.
videoState.addFlows = new Set();
videoState.dismissed = new Set();

function buildAddFlow() {
  const wrap = vEl('div', 'vsrc-add');
  const flow = { el: wrap, render };

  function render() {
    wrap.replaceChildren();
    const list = videoSourceList();
    const free = VSRC.freeDevices(list, videoState.devices);

    // Offer: a device appeared that a placeholder was waiting for.
    const waiting = list.filter(x => VSRC.stateOf(x, videoState.devices) === 'placeholder' && x.enabled !== false);
    outer: for (const src of waiting) for (const d of free) {
      if (videoState.dismissed.has(src.id + '|' + d.deviceId)) continue;
      const offer = vEl('div', 'vsrc-offer');
      offer.append(vEl('span', null, `Use ${d.label || 'this device'} for ${src.name}?`),
        vBtn('Use', 'Use this device for this input', () => saveVideoSources(VSRC.assignDevice(list, src.id, d)), 'vd-mini vd-mini-primary'),
        vBtn('Not now', 'Ask again later', () => { videoState.dismissed.add(src.id + '|' + d.deviceId); render(); }, 'vd-mini'));
      wrap.appendChild(offer);
      break outer;
    }

    // + Add input: a name now, a device any time later.
    const row = vEl('div', 'vsrc-add-row');
    const name = vEl('input', 'setting-input'); name.type = 'text'; name.maxLength = 60;
    name.placeholder = 'Name, for example Pulpit camera';
    name.setAttribute('aria-label', 'Name for the new input');
    const add = () => {
      const r = VSRC.addSource(videoSourceList(), null, { name: name.value });
      videoState.selectedId = r.added.id;
      name.value = '';
      saveVideoSources(r.list);
    };
    name.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); add(); } });
    row.append(name, vBtn('+ Add input', 'Add an input with this name. Choose its device now or later.', add, 'modal-btn primary'));
    wrap.appendChild(row);

    // Detected devices, each with its own Add.
    const head = vEl('div', 'vsrc-det-head');
    head.appendChild(vEl('span', 'vd-label', 'Detected devices'));
    head.appendChild(vBtn('Detect devices', 'Look for cameras and capture cards, and ask for camera access if needed', async () => {
      await refreshVideoDevices(true);
      renderVideoSources();
    }, 'vd-mini'));
    const addAllBtn = vBtn('Add all', 'Add an input for every detected device not used yet', () => saveVideoSources(VSRC.addAll(videoSourceList(), videoState.devices)), 'vd-mini');
    addAllBtn.disabled = !free.length;
    head.appendChild(addAllBtn);
    wrap.appendChild(head);
    if (videoState.error) wrap.appendChild(vEl('p', 'setting-hint vsrc-error', videoState.error));
    else if (videoState.blind) wrap.appendChild(vEl('p', 'setting-hint', 'Choose Detect devices and allow camera access to see device names.'));
    const det = vEl('div', 'vsrc-det');
    if (!videoState.devices.length) det.appendChild(vEl('p', 'setting-hint', 'No cameras found. Connect one and choose Detect devices.'));
    videoState.devices.forEach((d, i) => {
      const used = list.find(x => VSRC.resolveDevice(x, [d]) === d.deviceId);
      const r = vEl('div', 'vsrc-det-row' + (used ? ' is-added' : ''));
      r.appendChild(vEl('span', 'vsrc-det-name', d.label || `Camera ${i + 1}`));
      if (used) r.appendChild(vEl('span', 'vsrc-det-added', `Added as ${used.name}`));
      else r.appendChild(vBtn('Add', 'Add this device as an input', () => {
        const res = VSRC.addSource(videoSourceList(), d);
        videoState.selectedId = res.added.id;
        saveVideoSources(res.list);
      }, 'vd-mini'));
      det.appendChild(r);
    });
    wrap.appendChild(det);
  }
  videoState.addFlows.add(flow);
  render();
  return flow;
}

// Every open floating panel, so leaving Director mode can close them all.
const videoPanelClosers = new Set();
window.KairoVideoPanels = { closeAll() { [...videoPanelClosers].forEach(c => { try { c(); } catch {} }); } };

// A small floating panel for the Director screen (and the device picker).
function openVideoPanel(anchor, title, build) {
  document.querySelectorAll('.vd-add-panel').forEach(p => p.remove());
  const panel = vEl('div', 'vd-add-panel');
  panel.setAttribute('role', 'dialog');
  panel.setAttribute('aria-label', title);
  const head = vEl('div', 'vd-add-head');
  head.appendChild(vEl('span', 'vd-add-title', title));
  const flows = [];
  const close = () => {
    videoPanelClosers.delete(close);
    panel.remove();
    flows.forEach(f => videoState.addFlows.delete(f));
    document.removeEventListener('mousedown', outside, true);
    document.removeEventListener('keydown', esc, true);
    try { if (anchor && anchor.isConnected) anchor.focus(); } catch {}   // focus goes back to what opened it
  };
  const outside = (e) => { if (!panel.contains(e.target) && !(anchor && anchor.contains(e.target))) close(); };
  const esc = (e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } };   // the Director's own Esc (clear staging) must not also fire
  head.appendChild(vBtn('Close', 'Close', close, 'vd-mini'));
  panel.appendChild(head);
  const body = build(close, (f) => flows.push(f));
  panel.appendChild(body);
  document.body.appendChild(panel);
  const r = anchor ? anchor.getBoundingClientRect() : { left: 40, top: 80, bottom: 80 };
  const w = 340;
  panel.style.left = Math.max(8, Math.min(window.innerWidth - w - 8, r.left)) + 'px';
  const h = panel.offsetHeight;
  panel.style.top = Math.max(8, Math.min(window.innerHeight - h - 8, r.top - h - 6 > 8 ? r.top - h - 6 : r.bottom + 6)) + 'px';
  document.addEventListener('mousedown', outside, true);
  document.addEventListener('keydown', esc, true);
  videoPanelClosers.add(close);
  // Keyboard users land inside the panel: the first field, else the first button after Close.
  const first = panel.querySelector('input, select') || panel.querySelectorAll('button')[1] || panel.querySelector('button');
  if (first) try { first.focus({ preventScroll: true }); } catch {}
  return close;
}

// The Director screen's "+": the same add flow in a small panel.
function openAddInputPanel(anchor) {
  openVideoPanel(anchor, 'Add input', (close, track) => { const f = buildAddFlow(); track(f); return f.el; });
}

// Click on an input with no device: choose one for it.
function openDevicePicker(sourceId, anchor) {
  const src = videoSourceList().find(x => x.id === sourceId);
  if (!src) return;
  openVideoPanel(anchor, `Choose a device for ${src.name}`, (close) => {
    const box = vEl('div', 'vsrc-det');
    const draw = () => {
      box.replaceChildren();
      const list = videoSourceList();
      const free = VSRC.freeDevices(list, videoState.devices);
      if (!free.length) {
        box.appendChild(vEl('p', 'setting-hint', 'No free devices found. Connect one, then detect again.'));
        box.appendChild(vBtn('Detect devices', 'Look for cameras and capture cards', async () => { await refreshVideoDevices(true); draw(); }, 'vd-mini'));
      }
      free.forEach((d, i) => {
        const r = vEl('div', 'vsrc-det-row');
        r.appendChild(vEl('span', 'vsrc-det-name', d.label || `Camera ${i + 1}`));
        r.appendChild(vBtn('Use', `Use this device for ${src.name}`, () => { saveVideoSources(VSRC.assignDevice(videoSourceList(), sourceId, d)); close(); }, 'vd-mini vd-mini-primary'));
        box.appendChild(r);
      });
    };
    draw();
    return box;
  });
}

// ── The Video Sources page ────────────────────────────────────────────────
function renderVideoSources() {
  if (window.KairoVideoDirector) window.KairoVideoDirector.refresh();
  videoState.addFlows.forEach(f => f.render());
  const root = videoState.root;
  if (!root) return;
  const sources = videoSourceList();
  if (!sources.some(s => s.id === videoState.selectedId)) videoState.selectedId = null;
  root.replaceChildren();
  // Re-rendering takes new leases before the old ones go, so the hub keeps the open camera across the redraw.
  const old = videoState.leases;
  videoState.leases = [];

  const sec = vEl('div', 'settings-section');
  sec.appendChild(vEl('div', 'settings-section-label', 'Video Sources'));
  sec.appendChild(vEl('p', 'setting-hint', 'Add every camera and capture card you use, or a named placeholder for one that is not connected yet.'));

  // Every configured input, one row each.
  const listEl = vEl('div', 'vsrc-inputs');
  listEl.setAttribute('role', 'list');
  sources.forEach((src, i) => {
    const st = VSRC.stateOf(src, videoState.devices);
    const row = vEl('div', 'vsrc-input' + (st !== 'ready' ? ' is-empty' : '') + (src.enabled === false ? ' is-off' : '') + (src.id === videoState.selectedId ? ' is-sel' : ''));
    row.setAttribute('role', 'listitem');
    const top = vEl('div', 'vsrc-input-top');
    const name = vEl('input', 'setting-input vsrc-name'); name.type = 'text'; name.value = src.name; name.maxLength = 60;
    name.setAttribute('aria-label', 'Input name');
    name.addEventListener('change', () => saveVideoSources(VSRC.renameSource(videoSourceList(), src.id, name.value)));
    const sw = vEl('label', 'output-toggle');
    sw.title = src.enabled === false ? 'Switched off: not shown in Director mode' : 'On: shown in Director mode';
    const cb = vEl('input'); cb.type = 'checkbox'; cb.checked = src.enabled !== false; cb.setAttribute('aria-label', `${src.name} on or off`);
    cb.addEventListener('change', () => {
      saveVideoSources(VSRC.setEnabled(videoSourceList(), src.id, cb.checked));
      // An input switched off while it is on air goes to no video.
      if (!cb.checked) { const live = videoLiveMap(); const ids = Object.keys(live).filter(k => live[k] === src.id); if (ids.length) setVideoLiveFor(ids, null); }
    });
    sw.append(cb, vEl('span', 'output-toggle-track'));
    const up = vBtn('↑', 'Move up', () => saveVideoSources(VSRC.moveSource(videoSourceList(), src.id, -1)), 'ls-icon-btn');
    const down = vBtn('↓', 'Move down', () => saveVideoSources(VSRC.moveSource(videoSourceList(), src.id, 1)), 'ls-icon-btn');
    up.disabled = i === 0; down.disabled = i === sources.length - 1;
    up.setAttribute('aria-label', `Move ${src.name} up`); down.setAttribute('aria-label', `Move ${src.name} down`);
    const rm = vBtn('Remove', `Remove ${src.name}`, () => saveVideoSources(VSRC.removeSource(videoSourceList(), src.id)), 'vd-mini vsrc-remove');
    top.append(name, sw, up, down, rm);

    const sub = vEl('div', 'vsrc-input-sub');
    // Device: every detected one, or none.
    const devOpts = [['', 'No device yet']].concat(videoState.devices.map((d, k) => [d.deviceId, d.label || `Camera ${k + 1}`]));
    const cur = VSRC.resolveDevice(src, videoState.devices) || '';
    if (st === 'missing') devOpts.push(['__gone', `${src.label || 'Saved device'} (not connected)`]);
    const dev = vSelect(devOpts, st === 'missing' ? '__gone' : cur, (v) => {
      if (v === '__gone') return;
      const d = videoState.devices.find(x => x.deviceId === v);
      saveVideoSources(VSRC.assignDevice(videoSourceList(), src.id, d || null));
    });
    dev.setAttribute('aria-label', `Device for ${src.name}`);
    const resValue = `${src.width}x${src.height}`;
    const resOpts = VSRC.RESOLUTIONS.map(r => [r.value, r.label]);
    const res = vSelect(resOpts, resOpts.some(o => o[0] === resValue) ? resValue : '0x0', (v) => {
      const r = VSRC.RESOLUTIONS.find(x => x.value === v);
      saveVideoSources(VSRC.updateSource(videoSourceList(), src.id, { width: r.width, height: r.height }));
    });
    res.setAttribute('aria-label', `Resolution for ${src.name}`);
    const fps = vSelect(VSRC.FPS_OPTIONS.map(f => [String(f), f ? `${f} fps` : 'Automatic rate']), String(src.fps), (v) => {
      saveVideoSources(VSRC.updateSource(videoSourceList(), src.id, { fps: Number(v) }));
    });
    fps.setAttribute('aria-label', `Frame rate for ${src.name}`);
    sub.append(dev, res, fps);

    const status = vEl('div', 'vsrc-input-status', sourceStatusText(src));
    row.append(top, sub, status);
    // The selected input shows a live look (one camera at a time).
    row.addEventListener('click', (e) => {
      if (e.target.closest('input,select,button,label')) return;
      videoState.selectedId = videoState.selectedId === src.id ? null : src.id;
      renderVideoSources();
    });
    if (src.id === videoState.selectedId) {
      const box = vEl('div', 'vsrc-preview');
      const v = vEl('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-label', `Preview of ${src.name}`);
      const msg = vEl('span', 'vsrc-thumb-msg');
      box.append(v, msg);
      row.appendChild(box);
      const id = VSRC.resolveDevice(src, videoState.devices);
      if (id && videoState.visible && src.enabled !== false) attachVideoPreview(v, id, msg);
      else msg.textContent = st === 'placeholder' ? 'No device yet' : st === 'missing' ? 'Not connected' : '';
    }
    listEl.appendChild(row);
  });
  if (!sources.length) listEl.appendChild(vEl('p', 'setting-hint vsrc-empty', 'No inputs yet. Add one below.'));
  sec.appendChild(listEl);

  // + Add input, and the detected devices
  sec.appendChild(vEl('div', 'vsrc-add-title', 'Add another input'));
  if (videoState.pageFlow) videoState.addFlows.delete(videoState.pageFlow);   // the form of the last redraw is gone
  const flow = buildAddFlow();
  videoState.pageFlow = flow;
  sec.appendChild(flow.el);
  root.appendChild(sec);

  // Live on each output
  const live = videoLiveMap();
  const layers = outputLayerMap();
  const out = vEl('div', 'settings-section');
  out.appendChild(vEl('div', 'settings-section-label', 'Live on each output'));
  out.appendChild(vEl('p', 'setting-hint', 'One video source is live per output. Video input must be on for the output, in Director mode settings.'));
  const table = vEl('div', 'vsrc-live');
  videoOutputs().forEach(o => {
    const row = vEl('div', 'vsrc-live-row');
    row.appendChild(vEl('span', 'vsrc-live-name', o.monitor ? `${o.name} (Monitor)` : o.name));
    const usable = sources.filter(x => x.enabled !== false && (x.deviceId || x.label));
    const opts = [['', 'No video']].concat(usable.map(s => [s.id, s.name]));
    const select = vSelect(opts, live[o.id] || '', (v) => setVideoLiveFor([o.id], v || null));
    select.setAttribute('aria-label', `Live video source on ${o.name}`);
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

  old.forEach(release => release());
}

// Called by setOutputLayer (app_outputs.js) when a Video input box changes.
function onVideoLayerChanged() { if (window.KairoVideoDirector) window.KairoVideoDirector.refresh(); if (videoState.visible) renderVideoSources(); }

async function startVideoSources() {
  buildVideoPane();
  videoSourceList();
  settings.videoLive = videoLiveMap();
  publishVideoState();
  await refreshVideoDevices(false);
  renderVideoSources();
}

// Mode off: take the pane and every capture away again.
function stopVideoSources() {
  videoState.observer?.disconnect();
  navigator.mediaDevices?.removeEventListener?.('devicechange', videoState.onDeviceChange);
  releaseAllPreviews();
  videoState.root?.closest('.settings-pane')?.remove();
  document.querySelector('#settings-nav .settings-nav-item[data-pane="sources"]')?.remove();
  if (document.querySelector('.settings-pane.active') == null && typeof showFirstSettingsPane === 'function') showFirstSettingsPane();
  videoState.root = null;
  videoState.built = false;
  videoState.visible = false;
  videoState.addFlows.clear();
  videoState.pageFlow = null;
}
