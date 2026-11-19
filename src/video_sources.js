// KAIRO — live video input: the pure logic behind the Sources list. No DOM and
// no media APIs here, so it runs in the main window, in display windows and in
// node tests alike. The panel (sources_panel.js) and the output runtime
// (display_video.js) are the only callers.
//
// A source is an input the operator added: a camera or capture card, or a
// placeholder with only a name and no device yet:
//   { id, name, deviceId, label, width, height, fps, enabled }
// `deviceId` is what getUserMedia wants, but webviews can rotate it (cleared
// site data, a different USB port, an OS update); `label` is the fallback used
// to find the same device again. Sources persist in settings.videoSources.
// Which source is live on which output persists in settings.videoLive:
//   { [outputId]: sourceId | null }   (null / missing = no video)
'use strict';
(function () {
  const MAX_NAME = 60;
  const RESOLUTIONS = [
    { value: '0x0', label: 'Automatic', width: 0, height: 0 },
    { value: '1280x720', label: '1280 × 720', width: 1280, height: 720 },
    { value: '1920x1080', label: '1920 × 1080', width: 1920, height: 1080 },
    { value: '3840x2160', label: '3840 × 2160', width: 3840, height: 2160 },
  ];
  const FPS_OPTIONS = [0, 24, 25, 30, 50, 60];

  const str = (v) => (typeof v === 'string' ? v : '');
  const int = (v, lo, hi) => {
    const n = Math.round(Number(v));
    return Number.isFinite(n) && n >= lo && n <= hi ? n : 0;
  };

  function cleanName(name) {
    return str(name).replace(/\s+/g, ' ').trim().slice(0, MAX_NAME);
  }

  // A name no other source uses: "Camera", then "Camera 2", "Camera 3".
  function uniqueName(list, base, exceptId) {
    const taken = new Set(list.filter(s => s.id !== exceptId).map(s => s.name.toLowerCase()));
    const root = cleanName(base) || 'Camera';
    if (!taken.has(root.toLowerCase())) return root;
    for (let n = 2; n < 1000; n++) {
      const candidate = `${root} ${n}`.slice(0, MAX_NAME + 4);
      if (!taken.has(candidate.toLowerCase())) return candidate;
    }
    return root;
  }

  function newSourceId(list) {
    const used = new Set(list.map(s => s.id));
    for (;;) {
      const id = 'vid-' + Math.random().toString(36).slice(2, 8);
      if (!used.has(id)) return id;
    }
  }

  // Whatever settings.json holds → a clean list. Drops entries with no id or
  // device, and de-duplicates ids so a hand-edited file can't break the UI.
  // A key this input is put on air with ("q", "F2", "7"): one printable key or a function key, not
  // Space, Enter, Escape, Tab or the arrows (those have jobs), not a modifier. '' = none.
  const RESERVED_KEYS = new Set([' ', 'space', 'enter', 'escape', 'esc', 'tab', 'backspace', 'delete', 'shift', 'control', 'alt', 'meta', 'capslock', 'arrowup', 'arrowdown', 'arrowleft', 'arrowright']);
  function keyName(k) {
    const t = str(k).trim();
    if (!t) return '';
    const l = t.toLowerCase();
    if (RESERVED_KEYS.has(l)) return '';
    if (/^f([1-9]|1[0-2])$/.test(l)) return l.toUpperCase();
    return t.length === 1 ? l : '';
  }
  function normalizeSources(raw) {
    if (!Array.isArray(raw)) return [];
    const out = [];
    const seen = new Set();
    for (const r of raw) {
      if (!r || typeof r !== 'object') continue;
      const id = str(r.id).trim();
      if (!id || seen.has(id)) continue;
      seen.add(id);
      const label = str(r.label).trim();
      const src = {
        id,
        name: '',
        deviceId: str(r.deviceId),
        label,
        width: int(r.width, 160, 7680),
        height: int(r.height, 120, 4320),
        fps: int(r.fps, 1, 240),
        enabled: r.enabled !== false,
        key: keyName(r.key),
      };
      src.name = uniqueName(out, cleanName(r.name) || label || 'Input', id);
      out.push(src);
    }
    return out;
  }

  // `device` is a MediaDeviceInfo-like { deviceId, label }, or null for a
  // placeholder (a name and nothing else: it opens no device).
  function addSource(list, device, opts = {}) {
    const d = device || {};
    const added = {
      id: newSourceId(list),
      name: uniqueName(list, opts.name || d.label || 'Input'),
      deviceId: str(d.deviceId),
      label: str(d.label).trim(),
      width: int(opts.width, 160, 7680),
      height: int(opts.height, 120, 4320),
      fps: int(opts.fps, 1, 240),
      enabled: true,
    };
    return { list: [...list, added], added };
  }

  // Give an input a device (or take it away with null) and keep everything else.
  function assignDevice(list, id, device) {
    const d = device || {};
    return list.map(s => (s.id === id ? { ...s, deviceId: str(d.deviceId), label: str(d.label).trim() } : s));
  }
  function setEnabled(list, id, on) {
    return list.map(s => (s.id === id ? { ...s, enabled: !!on } : s));
  }

  // 'placeholder' (no device chosen), 'missing' (chosen, not plugged in now) or 'ready'.
  function stateOf(source, devices, sources) {
    if (!source || (!source.deviceId && !source.label)) return 'placeholder';
    return resolveDevice(source, devices, sources) ? 'ready' : 'missing';
  }
  // Devices no input uses yet.
  function freeDevices(list, devices) {
    const inputs = (devices || []).filter(d => d && d.kind === 'videoinput');
    const used = new Set();
    for (const s of list) { const id = resolveDevice(s, inputs, list); if (id) used.add(id); }
    return inputs.filter(d => !used.has(d.deviceId));
  }
  // One new input for every device not used yet.
  function addAll(list, devices) {
    let next = list;
    for (const d of freeDevices(list, devices)) next = addSource(next, d).list;
    return next;
  }

  function renameSource(list, id, name) {
    const clean = cleanName(name);
    if (!clean) return list;
    return list.map(s => (s.id === id ? { ...s, name: uniqueName(list, clean, id) } : s));
  }

  function updateSource(list, id, patch) {
    return list.map(s => {
      if (s.id !== id) return s;
      const next = { ...s };
      if ('enabled' in patch) next.enabled = !!patch.enabled;
      if ('deviceId' in patch) next.deviceId = str(patch.deviceId);
      if ('label' in patch) next.label = str(patch.label).trim();
      if ('width' in patch) next.width = int(patch.width, 160, 7680);
      if ('height' in patch) next.height = int(patch.height, 120, 4320);
      if ('fps' in patch) next.fps = int(patch.fps, 1, 240);
      if ('key' in patch) next.key = keyName(patch.key);
      return next;
    });
  }

  function removeSource(list, id) {
    return list.filter(s => s.id !== id);
  }

  // delta -1 = up, +1 = down; clamps at the ends.
  function moveSource(list, id, delta) {
    const from = list.findIndex(s => s.id === id);
    if (from < 0) return list;
    const to = Math.max(0, Math.min(list.length - 1, from + delta));
    if (to === from) return list;
    const next = list.slice();
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    return next;
  }

  // The deviceId to open for `source`, given the current enumerateDevices()
  // videoinput list: the saved id if it is still there, else the same label,
  // else null (unplugged — the output shows no video and the panel says so).
  //
  // The name fallback never takes a device another input holds (two cameras of one model: unplug
  // one and its input used to quietly show the other). Inputs that share a name and have all lost
  // their saved id take the free devices of that name in list order. `sources` is the list to
  // compare with; by default the app's own (setSourceList), none in tests.
  let listProvider = null;
  function setSourceList(fn) { listProvider = typeof fn === 'function' ? fn : null; }
  function currentList() { try { return listProvider ? listProvider() : null; } catch { return null; } }
  function resolveDevice(source, devices, sources) {
    const inputs = (devices || []).filter(d => d && d.kind === 'videoinput');
    if (!inputs.length || !source) return null;
    const byId = source.deviceId && inputs.find(d => d.deviceId === source.deviceId);
    if (byId) return byId.deviceId;
    const want = str(source.label).toLowerCase();
    if (!want) return null;
    let cands = inputs.filter(d => str(d.label).toLowerCase() === want);
    const list = Array.isArray(sources) ? sources : currentList();
    if (!list || !list.length) return cands.length ? cands[0].deviceId : null;
    const holds = (o) => !!(o && o.deviceId && inputs.some(d => d.deviceId === o.deviceId));
    const claimed = new Set(list.filter(o => o.id !== source.id && holds(o)).map(o => o.deviceId));
    cands = cands.filter(d => !claimed.has(d.deviceId));
    // Earlier inputs of this name with no live id take the first free devices.
    const at = list.findIndex(o => o.id === source.id);
    let before = 0;
    for (let i = 0; i < at; i++) {
      const o = list[i];
      if (!holds(o) && str(o.label).toLowerCase() === want) before++;
    }
    return cands[before] ? cands[before].deviceId : null;
  }

  // getUserMedia constraints. Size and rate are ideals, never exact: a camera
  // that can't do 4K should still give a picture rather than fail.
  // `cap` ({ width, height }): the most the picture is ever shown at (the output's screen).
  // A 4K camera on a 1080p screen is asked for 1080p: a quarter of the pixels to decode and
  // draw, the same picture on screen. The shape of the source is kept.
  function buildConstraints(source, deviceId, cap) {
    const video = {};
    if (deviceId) video.deviceId = { exact: deviceId };
    let w = source && source.width, h = source && source.height;
    if (w && h && cap && cap.width && cap.height && (w > cap.width || h > cap.height)) {
      const k = Math.min(cap.width / w, cap.height / h);
      w = Math.round(w * k); h = Math.round(h * k);
    }
    if (w) video.width = { ideal: w };
    if (h) video.height = { ideal: h };
    if (source && source.fps) video.frameRate = { ideal: source.fps };
    return { video: Object.keys(video).length ? video : true, audio: false };
  }

  // A sentence for a getUserMedia failure (DOMException name → operator words).
  function describeCaptureError(err) {
    const name = err && err.name;
    if (name === 'NotAllowedError' || name === 'SecurityError') return 'Camera access is blocked. Allow Kairo to use the camera in system settings.';
    if (name === 'NotFoundError' || name === 'OverconstrainedError') return 'This device is not connected.';
    if (name === 'NotReadableError' || name === 'AbortError') return 'Another app is using this device.';
    return 'The device could not be opened.';
  }

  // settings.videoLive, cleaned against the sources and outputs that exist:
  // an entry for a removed source or removed output is dropped.
  function normalizeLive(raw, sources, outputIds) {
    const src = raw && typeof raw === 'object' ? raw : {};
    const ids = new Set((sources || []).map(s => s.id));
    const out = {};
    for (const o of outputIds || []) out[o] = ids.has(src[o]) ? src[o] : null;
    return out;
  }

  // Make `sourceId` (or null for no video) live on each of `outputIds`.
  function setLive(live, outputIds, sourceId) {
    const next = { ...(live || {}) };
    for (const o of outputIds) next[o] = sourceId || null;
    return next;
  }

  // The source an output should be playing right now, or null: its Video layer
  // must be on AND a source chosen AND that source must still exist.
  function liveSourceFor(outputId, layers, live, sources) {
    if (!layers || layers.video !== true) return null;
    const id = live && live[outputId];
    // A placeholder or an input switched off is never taken to air.
    return (sources || []).find(s => s.id === id && s.enabled !== false && (s.deviceId || s.label)) || null;
  }

  // How a switch should be done: 'cut' when the output's transition is Cut (or
  // nothing is on screen yet), otherwise a fade of the same length slides get.
  function switchPlan(animation, speed, hasCurrent) {
    const factor = typeof speed === 'number' && speed > 0 ? speed : 1;
    if (animation === 'cut' || !hasCurrent) return { mode: 'cut', ms: 0 };
    return { mode: 'fade', ms: Math.round(300 * factor) };
  }

  const API = {
    RESOLUTIONS, FPS_OPTIONS, normalizeSources, addSource, assignDevice, setEnabled, stateOf, freeDevices, addAll, renameSource, updateSource,
    removeSource, moveSource, keyName, resolveDevice, setSourceList, buildConstraints, describeCaptureError,
    normalizeLive, setLive, liveSourceFor, switchPlan, uniqueName,
  };
  if (typeof window !== 'undefined') window.KairoVideoSources = API;
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
})();
