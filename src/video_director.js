// KAIRO — Video director mode: the stage (Preview / Program) and the mode's
// lifecycle.
//
// Lazy-loaded by video_director_boot.js while the mode is on, and removed
// again when it is turned off. The screen has a top half and a bottom half with
// a draggable split between them (remembered). The top half is three columns
// with draggable splits: Inputs (video_input_grid.js), the two displays with a
// narrow transition strip between them (Preview | strip | Output), and one
// Scripture sends group (the search bar, Bible and Theme controls and the Live
// Queue, moved in by video_director_layout.js). The bottom half holds the
// playlist group and the media bin (video_dock.js). Nothing in the normal
// layout's own code is edited for this mode.
//
//   OUTPUT   what the outputs show now: a live copy of the output's own page
//            (display.html?monitor=1&shared=1), fed by the same socket. Its
//            frame goes red while something is live.
//   PREVIEW  a staging screen: the same page (display.html?preview=1), fed by
//            this window, showing the next source and a staged verse. Both
//            screens draw scripture over video with display.html's own code.
//
// CUT / FADE put the Preview source on Program, swap the old Program source
// onto Preview, and become the transition a click on an input tile uses (Cut
// to begin with). A source change never touches the verse on screen. Manual
// sends from search and the Bible tab are staged on Preview
// (sendVerseToServer is wrapped while the mode is on) and TAKE sends them to
// Program; verses the detection engine sends by itself go live exactly as they
// do without the mode. Slide, media and overlay sends are not staged.
'use strict';
(function () {
  const D = {
    active: false,
    root: null,
    previewId: null,        // source staged on Preview; null = no video
    staged: null,           // { verse, look } waiting on Preview
    origSend: null,
    frames: {},             // 'pvw' | 'pgm' → { box, frame, ro }
    ui: {},
    liveObserver: null,
    last: null,             // one-step undo: { live, preview, layers, verse }
    pendingLook: null, previewLookId: null, liveLookId: null,   // Looks (video_looks.js)
    programState: { on: [], id: null, mixed: false },
  };

  const stageManual = () => settings.videoStageManual !== false;
  const transitionPref = () => (settings.videoTransition === 'fade' ? 'fade' : 'cut');

  // ── Outputs ──────────────────────────────────────────────────────────────
  function outputsOn() {
    const layers = outputLayerMap();
    return videoOutputs().filter(o => layers[o.id] && layers[o.id].video);
  }
  // The source every output with video on is showing, if they agree.
  function programState() {
    const on = outputsOn();
    const live = videoLiveMap();
    if (!on.length) return { on, id: null, mixed: false };
    const ids = new Set(on.map(o => live[o.id] || ''));
    return ids.size === 1 ? { on, id: [...ids][0] || null, mixed: false } : { on, id: null, mixed: true };
  }

  // ── Frames: display.html pages scaled into a 16:9 box ───────────────────
  function buildFrame(kind) {
    const box = vEl('div', 'vd-frame');
    const frame = document.createElement('iframe');
    frame.setAttribute('tabindex', '-1');
    frame.setAttribute('aria-hidden', 'true');
    frame.style.width = '1920px';
    frame.style.height = '1080px';
    box.appendChild(frame);
    if (kind === 'pvw') frame.addEventListener('load', () => syncPreviewFrame());   // a (re)loaded Preview picks its state back up
    const name = vEl('span', 'vd-frame-name');
    box.appendChild(name);
    if (kind === 'pgm') box.appendChild(vEl('span', 'vd-live-badge', 'Live'));
    const ro = new ResizeObserver(() => { frame.style.transform = `scale(${box.clientWidth / 1920})`; });
    ro.observe(box);
    D.frames[kind] = { box, frame, name, ro };
    // Drop an input on either screen to send it there; drop a slide on Preview to stage it.
    box.addEventListener('dragover', (e) => {
      const t = e.dataTransfer && e.dataTransfer.types ? [...e.dataTransfer.types] : [];
      if (t.includes('application/x-kairo-input') || (kind === 'pvw' && t.includes('application/x-kairo-slide'))) { e.preventDefault(); box.classList.add('is-drop'); }
    });
    box.addEventListener('dragleave', () => box.classList.remove('is-drop'));
    box.addEventListener('drop', (e) => {
      box.classList.remove('is-drop');
      const dt = e.dataTransfer; if (!dt) return;
      const inp = dt.getData('application/x-kairo-input');
      const sl = dt.getData('application/x-kairo-slide');
      if (inp !== '' && dt.types.includes('application/x-kairo-input')) {
        e.preventDefault();
        if (kind === 'pgm') sendToProgram(inp || null); else setPreview(inp || null);
      } else if (sl && kind === 'pvw') {
        e.preventDefault();
        try { const v = JSON.parse(sl); stageSlide(v.itemId, v.index); } catch {}
      }
    });
    return box;
  }
  function setFrameSrc(kind, src) {
    const f = D.frames[kind];
    if (!f || f.src === src) return;
    f.src = src;
    f.frame.title = kind === 'pvw' ? 'Preview' : 'Output';
    f.frame.src = src;
  }
  function post(kind, msg) {
    const f = D.frames[kind];
    try { f && f.frame.contentWindow && f.frame.contentWindow.postMessage({ kairoPreview: true, msg }, location.origin); } catch {}
  }
  // Preview is a persistent staging screen: it holds the layout (frame, layers), the staged content and the
  // staged input until something else is chosen, and it keeps showing them after a send. With nothing
  // staged it shows what the Output shows (the live layout frame, slide or verse and the live input).
  const liveFrameTheme = () => themeById(frameMap()[PRIMARY_DISPLAY]);
  const hasZone = (t) => !!(t && (t.layers || []).some(l => l && l.type === 'videozone'));
  function pvwFrameTheme() { return D.pendingLook ? (D.pendingLook.themeId ? themeById(D.pendingLook.themeId) : null) : liveFrameTheme(); }
  function syncPreviewFrame() {
    const ps = programState();
    const src = D.previewId != null ? D.previewId : (ps.mixed ? null : (ps.id || null));
    post('pvw', { type: 'vd-source', sourceId: src });
    post('pvw', { type: 'vd-layers', layers: D.pendingLook ? D.pendingLook.layers : null });
    post('pvw', { type: 'vd-frame', look: pvwFrameTheme() });
    if (D.staged && D.staged.media) {
      const m = D.staged.media;
      post('pvw', { type: 'clear' });
      post('pvw', { type: 'media', target: 'viewer', src: m.src, kind: m.kind, fit: m.fit });
    } else if (D.staged) {
      post('pvw', { type: 'clear-layer', layer: 'media' });
      post('pvw', { type: 'detection', target: 'viewer', verses: [D.staged.verse], look: D.staged.look || null, next: D.staged.next || undefined });
    } else {
      // Nothing staged: the content follows the Output.
      if (D.liveMsg) post('pvw', { type: 'detection', target: 'viewer', verses: D.liveMsg.verses, look: D.liveMsg.look || null, next: D.liveMsg.next });
      else post('pvw', { type: 'clear' });
      if (D.liveMedia) post('pvw', { type: 'media', target: 'viewer', src: D.liveMedia.src, kind: D.liveMedia.kind, fit: D.liveMedia.fit });
      else post('pvw', { type: 'clear-layer', layer: 'media' });
    }
  }
  // What the Output is showing, from the server's broadcasts (app.js hands them over).
  function onServerMessage(msg) {
    if (!D.active || !msg) return;
    let changed = false;
    if (msg.type === 'detection' && msg.target === 'viewer' && msg.verses && msg.verses.length && !msg.retranslated) { D.liveMsg = msg; changed = true; }
    else if (msg.type === 'media' && msg.target === 'viewer') { D.liveMedia = msg; changed = true; }
    else if (msg.type === 'clear-layer') {
      if (msg.layer === 'slide' || msg.layer === 'all') { D.liveMsg = null; changed = true; }
      if (msg.layer === 'media' || msg.layer === 'all') { D.liveMedia = null; changed = true; }
    }
    if (changed && !D.staged) syncPreviewFrame();
  }

  // ── Staging ──────────────────────────────────────────────────────────────
  const sameVerse = (a, b) => !!a && !!b && (a.reference || a.ref) === (b.reference || b.ref) && (a.text || a.verse) === (b.text || b.verse);
  function stage(verse, look) {
    D.staged = { verse, look: look || null };
    syncPreviewFrame();
    refresh();
  }
  function clearStage() {
    D.staged = null; D.stageKey = null;
    syncPreviewFrame();
    refresh();
  }
  function takeVerse() {
    if (!D.staged) return;
    const { verse, look, slide, media } = D.staged;
    D.staged.sent = true;   // Preview keeps showing it: it now matches the Output
    D.last = { ...(D.last || {}), verse: true };
    refresh();
    // A staged slide goes out through the playlist's own send, a verse through the normal one.
    // A staged slide or media item goes out through the app's own send, as an explicit one (not staged again).
    if (slide && window.KairoService) return window.KairoService.sendSlide(slide.item, slide.index, { direct: true });
    if (media && window.KairoService) return window.KairoService.sendMedia(media.item, media.fit, true);
    return D.origSend(verse, look);
  }
  // Drag a slide onto Preview to stage it.
  function stageSlide(itemId, index) {
    const svc = window.KairoService;
    const item = svc && svc.service && svc.service.items.find(i => i.id === itemId);
    const sl = item && svc.slidesFor(item)[index];
    if (!sl) return;
    D.staged = { verse: { text: sl.text || '', reference: sl.reference || sl.label || item.title || '', image: sl.image || undefined }, look: null, slide: { item, index } };
    syncPreviewFrame();
    refresh();
  }
  // ── Slides and media: a click stages them in Preview, Send puts them on the Output. ──
  // A second click on the same thing within 500 ms (a double-click) sends it straight to the Output.
  const DOUBLE_MS = 500;
  function stageContent(key, build, make) {
    if (!D.active) return false;
    const now = Date.now();
    if (D.stageKey === key && now - D.stageAt < DOUBLE_MS) {
      // Quick cut: the real send goes straight out; Preview holds the same item (it now matches the Output).
      D.last = { ...(D.last || {}), verse: true };   // Undo can take the slide back
      if (D.staged && D.staged.key === key) { D.staged.sent = true; refresh(); }
      else D.sentKey = key;        // the staged copy is still being made: it is marked sent when it lands
      D.stageAt = 0;               // the next click starts staging again
      return false;
    }
    D.stageKey = key; D.stageAt = now;
    Promise.resolve(build()).then((b) => {
      if (!b || D.stageKey !== key) return;
      D.staged = make(b);
      D.staged.key = key;
      if (D.sentKey === key) { D.staged.sent = true; D.sentKey = null; }
      syncPreviewFrame(); refresh();
    }).catch(() => {});
    return true;
  }
  // Stage something and mark it as already sent (a direct send): Preview then holds it.
  function stageSent(key, build, make) {
    D.sentKey = key;
    D.last = { ...(D.last || {}), verse: true };
    stageContent(key, build, make);
  }
  // The real dblclick event, as a fallback where the second-click timing above did not catch it, and for
  // playlist rows (a double-click sends their first slide). Director mode wins over the rename dblclick.
  function onDblClick(e) {
    if (!D.active) return;
    const svc = window.KairoService;
    if (!svc || !svc.service) return;
    const t = e.target && e.target.closest ? e.target : null;
    if (!t) return;
    const slideEl = t.closest('.svc-slide');
    const mediaEl = t.closest('.media-card');
    const rowEl = !slideEl && !mediaEl ? t.closest('.svc-item') : null;
    const itemById = (id) => svc.service.items.find(i => i.id === id);
    if (slideEl) {
      const item = itemById(slideEl.dataset.vdItem || (slideEl.closest('[data-id]') || {}).dataset?.id);
      const index = Number(slideEl.dataset.index);
      if (!item || !Number.isFinite(index)) return;
      const key = `slide:${item.id}:${index}`;
      if (D.staged && D.staged.key === key) { if (!D.staged.sent) takeVerse(); return; }
      if (D.sentKey === key) return;
      stageSent(key, () => svc.slideSendBody ? svc.slideSendBody(item, index) : null, (b) => ({ verse: b.verse, look: b.look || null, next: b.next || null, slide: { item, index } }));
      svc.sendSlide(item, index, { direct: true });
    } else if (mediaEl) {
      const url = mediaEl.dataset.url;
      const key = `media:${url}`;
      if (D.staged && D.staged.key === key) { if (!D.staged.sent) takeVerse(); }
    } else if (rowEl) {
      const item = itemById(rowEl.dataset.id);
      if (!item || !svc.slidesFor(item).length) return;
      e.stopImmediatePropagation(); e.preventDefault();   // not the rename
      const key = `slide:${item.id}:0`;
      stageSent(key, () => svc.slideSendBody ? svc.slideSendBody(item, 0) : null, (b) => ({ verse: b.verse, look: b.look || null, next: b.next || null, slide: { item, index: 0 } }));
      svc.sendSlide(item, 0, { direct: true });
    }
  }
  function stageSlideSend(item, index, build) {
    return stageContent(`slide:${item.id}:${index}`, build, (b) => ({ verse: b.verse, look: b.look || null, next: b.next || null, slide: { item, index } }));
  }
  function stageMediaSend(item, fit, build) {
    return stageContent(`media:${item.url}`, build, (b) => ({ verse: { reference: item.name || 'Media' }, media: { item, fit, src: b.src, kind: b.kind || item.kind } }));
  }
  // The slide staged now, as "itemId:index" (what the next Next/Previous steps from).
  const stagedSlideKey = () => (D.staged && D.staged.slide ? `${D.staged.slide.item.id}:${D.staged.slide.index}` : null);
  // Orange outline on the tile of whatever is staged.
  function markStaged() {
    document.querySelectorAll('.is-staged').forEach(n => n.classList.remove('is-staged'));
    // The dock's slide tiles show which slide is live.
    const live = window.KairoService && window.KairoService.liveSlideKey && window.KairoService.liveSlideKey();
    document.querySelectorAll('.vd-dock .svc-slide[data-vd-item], [data-vd-item].svc-slide').forEach(n => n.classList.toggle('live', !!live && live === `${n.dataset.vdItem}:${n.dataset.index}`));
    if (!D.staged) return;
    let sel = null;
    if (D.staged.slide) sel = `.svc-card[data-id="${CSS.escape(String(D.staged.slide.item.id))}"] .svc-slide[data-index="${D.staged.slide.index}"], .svc-slide[data-vd-item="${CSS.escape(String(D.staged.slide.item.id))}"][data-index="${D.staged.slide.index}"]`;
    else if (D.staged.media) sel = `.media-card[data-url="${CSS.escape(D.staged.media.item.url)}"]`;
    if (sel) document.querySelectorAll(sel).forEach(n => n.classList.add('is-staged'));
  }

  function wrapSend() {
    if (D.origSend || typeof sendVerseToServer !== 'function') return;
    D.origSend = sendVerseToServer;
    // eslint-disable-next-line no-global-assign
    sendVerseToServer = function (verse, look = null, opts = null) {
      if (!D.active || !stageManual()) return D.origSend(verse, look);
      // A double-click (direct) goes straight out, and Preview holds the same verse.
      if (opts && opts.direct) {
        if (!(D.staged && sameVerse(D.staged.verse, verse))) { D.staged = { verse, look: look || null }; D.stageKey = null; }
        D.staged.sent = true; D.last = { ...(D.last || {}), verse: true };
        syncPreviewFrame(); refresh();
        return D.origSend(verse, look);
      }
      // Sending the verse that is already staged takes it live.
      if (D.staged && sameVerse(D.staged.verse, verse)) return takeVerse();
      stage(verse, look);
      return Promise.resolve();
    };
  }
  function unwrapSend() {
    if (!D.origSend) return;
    // eslint-disable-next-line no-global-assign
    sendVerseToServer = D.origSend;
    D.origSend = null;
  }

  // ── Video: cut and fade ──────────────────────────────────────────────────
  function setTransition(mode) {
    settings.videoTransition = mode;
    saveSettingsPatch({ videoTransition: mode });
    publishTransition();
  }
  function publishTransition() {
    try { localStorage.setItem('kairo-video-transition', JSON.stringify(transitionPref())); } catch {}
  }
  const fadeMs = () => { const n = Number(settings.videoFadeMs); return Number.isFinite(n) && n >= 0 && n <= 5000 ? Math.round(n) : 300; };
  function publishFade() {
    try { localStorage.setItem('kairo-video-fade', JSON.stringify(fadeMs())); } catch {}
  }
  // The state a one-step Undo goes back to.
  function snapshot() {
    const ly = outputLayerMap();
    const layers = {};
    for (const o of videoOutputs()) layers[o.id] = { ...(ly[o.id] || {}) };
    D.last = { live: { ...videoLiveMap() }, preview: D.previewId, layers, screen: screenSnapshot() };
  }
  // ── Screen themes: a Layout's screen theme is a FRAME drawn around the output (settings.outputFrames). ──
  // The normal theme (Bible theme, slide themes) stays assigned and untouched; the slide or verse keeps
  // its own branding inside the frame's content zone. Leaving Director mode clears the frames.
  const themeTargets = () => videoOutputs().filter(o => !o.monitor).map(o => o.id);
  const themeList = () => (typeof looks !== 'undefined' && Array.isArray(looks) ? looks : []);
  const themeById = (id) => themeList().find(t => t.id === id) || null;
  const screenThemes = () => themeList().filter(t => t.groupId === 'grp-screen');
  const frameMap = () => (settings.outputFrames && typeof settings.outputFrames === 'object' ? settings.outputFrames : {});
  function screenSnapshot() {
    const m = frameMap();
    const frames = {};
    for (const id of themeTargets()) frames[id] = m[id] || null;
    return { frames };
  }
  function writeFrames(patch) {
    const next = { ...frameMap() };
    for (const [id, v] of Object.entries(patch)) { if (v) next[id] = v; else delete next[id]; }
    settings.outputFrames = next;
    saveSettingsPatch({ outputFrames: next });
    if (typeof applyOutputFrames === 'function') applyOutputFrames();
  }
  // themeId null clears the frame.
  function applyScreenTheme(themeId) {
    const id = themeId && themeById(themeId) ? themeId : null;
    const patch = {};
    for (const o of themeTargets()) patch[o] = id;
    writeFrames(patch);
  }
  // Undo: the frames go back as they were.
  function restoreScreen(snap) { if (snap && snap.frames) writeFrames(snap.frames); }
  // An earlier build swapped the output's own theme; put the saved normal themes back once.
  function migrateScreenOrig() {
    const saved = settings.directorScreenOrig;
    if (!saved || typeof saved !== 'object' || !Object.keys(saved).length) return;
    const patch = {};
    for (const [id, t] of Object.entries(saved)) if (t) patch[id] = t;
    if (Object.keys(patch).length && typeof outputThemeMap === 'function') {
      settings.outputThemes = { ...outputThemeMap(), ...patch };
      saveSettingsPatch({ outputThemes: settings.outputThemes });
      if (typeof applyOutputThemes === 'function') applyOutputThemes();
    }
    settings.directorScreenOrig = {};
    saveSettingsPatch({ directorScreenOrig: {} });
  }
  function undo() {
    const last = D.last;
    if (!last) return;
    D.last = null;
    if (last.live) {
      const byValue = new Map();
      for (const [out, id] of Object.entries(last.live)) { const k = id || ''; byValue.set(k, [...(byValue.get(k) || []), out]); }
      for (const [id, outs] of byValue) setVideoLiveFor(outs, id || null);
      D.previewId = last.preview || null;
      syncPreviewFrame();
    }
    if (last.screen) restoreScreen(last.screen);
    // Layers a Look changed go back as they were.
    if (last.layers) {
      const now = outputLayerMap();
      for (const [id, ly] of Object.entries(last.layers)) for (const k of Object.keys(ly)) if (now[id] && now[id][k] !== ly[k]) setOutputLayer(id, k, ly[k]);
    }
    D.liveLookId = null;
    // The verse goes back through the app's own "Back to the previous screen".
    if (last.verse) document.getElementById('output-back-btn')?.click();
    refresh();
  }
  // Space: send what is staged (the verse, and the Preview source when it differs).
  function takeStaged() {
    // A brief progress fill on the button for as long as the change takes.
    if (D.ui.take) {
      const ms = transitionPref() === 'fade' ? Math.max(200, fadeMs()) : 300;
      D.ui.take.style.setProperty('--vd-send-ms', ms + 'ms');
      D.ui.take.classList.remove('is-sending'); void D.ui.take.offsetWidth; D.ui.take.classList.add('is-sending');
      clearTimeout(D.sendTimer); D.sendTimer = setTimeout(() => D.ui.take && D.ui.take.classList.remove('is-sending'), ms + 60);
    }
    const mode = transitionPref();
    const hadVerse = !!(D.staged && !D.staged.sent);
    const hadLook = D.pendingLook && !D.lookSent ? D.pendingLook : null;
    if (hadLook) { snapshot(); applyLook(hadLook); }
    const ps = programState();
    if (hadVerse) takeVerse();
    if (ps.on.length && D.previewId != null && D.previewId !== (ps.mixed ? undefined : (ps.id || null))) take(mode);
    if (hadVerse) D.last = { ...(D.last || {}), verse: true };   // Undo takes the verse back too
    if (hadLook) { D.last = { ...(D.last || {}), layers: D.lookSnap && D.lookSnap.layers, screen: D.lookSnap && D.lookSnap.screen }; D.liveLookId = hadLook.id; D.lookSnap = null; }
    syncPreviewFrame();
    refresh();
  }

  // ── Looks: what Preview shows, until it is taken ─────────────────────────
  const firstReady = () => (videoSourceList().filter(x => x.enabled !== false && VSRC.stateOf(x, videoState.devices) === 'ready')[0] || {}).id || null;
  const lookVideo = (look) => (look.videoId === '@first' ? firstReady() : look.videoId);
  function loadLookToPreview(look) {
    D.pendingLook = look;
    D.lookSent = false;
    if (settings.directorPreviewLook !== look.id) { settings.directorPreviewLook = look.id; saveSettingsPatch({ directorPreviewLook: look.id }); }
    D.previewLookId = look.id;
    D.previewId = lookVideo(look) || null;
    syncPreviewFrame();
    post('pvw', { type: 'vd-layers', layers: look.layers });
    post('pvw', { type: 'vd-frame', look: look.themeId ? themeById(look.themeId) : null });
    refresh();
  }
  // What the output shows now, as a Look's recipe.
  async function captureLook() {
    const ps = programState();
    const ly = outputLayerMap()[PRIMARY_DISPLAY] || {};
    const layers = {};
    for (const k of ['bible', 'slide', 'media', 'timer']) layers[k] = ly[k] !== false;
    layers.video = ly.video === true;
    let overlays = null;
    if (window.KairoOverlays && window.KairoOverlays.load) { try { overlays = (await window.KairoOverlays.load()).filter(o => o.live).map(o => o.id); } catch {} }
    // The screen theme framing the primary output, only when it is a screen theme.
    const cur = themeById(frameMap()[PRIMARY_DISPLAY]);
    return { videoId: ps.mixed ? null : ps.id, layers, overlays, themeId: cur && cur.groupId === 'grp-screen' ? cur.id : null };
  }
  // Take: the Look's layers and overlays go onto the real outputs. Its video goes with the ordinary take.
  function applyLook(look) {
    D.lookSnap = D.last;
    const targets = videoOutputs();
    for (const o of targets) {
      for (const k of ['bible', 'slide', 'media', 'timer']) { if (!o.monitor) setOutputLayerIf(o.id, k, look.layers[k]); }
      setOutputLayerIf(o.id, 'video', look.layers.video);
    }
    if (Array.isArray(look.overlays) && window.KairoOverlays && window.KairoOverlays.load) {
      window.KairoOverlays.load().then(items => items.forEach(it => {
        const want = look.overlays.includes(it.id);
        if (!!it.live !== want) fetch(`${SERVER}/api/overlays/${it.id}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ live: want }) }).catch(() => {});
      })).catch(() => {});
    }
    applyScreenTheme(look.themeId || null);
    D.lookSent = true;   // Preview keeps holding the layout: it now matches the Output
  }
  function setOutputLayerIf(id, key, val) {
    const cur = outputLayerMap()[id];
    if (cur && cur[key] !== !!val) setOutputLayer(id, key, !!val);
  }
  function clearStaging() {
    D.staged = null; D.stageKey = null;
    D.previewId = null;
    syncPreviewFrame();
    refresh();
  }
  // Preview → Program with `mode`; Preview keeps showing the same input.
  function take(mode) {
    const ps = programState();
    if (!ps.on.length) return;
    snapshot();
    D.liveLookId = null;
    setTransition(mode);                       // written first: the display reads it when the source changes
    setVideoLiveFor(ps.on.map(o => o.id), D.previewId);   // Preview keeps the same input: it now matches the Output
    syncPreviewFrame();
    refresh();
  }
  // Take everything down: the video fades to none and the verse, slide, timer and media layers clear.
  // This is what the Clear buttons did; Undo (the app's Back) brings the previous screen back.
  function fadeToBlack() {
    if (programState().on.length) sendToProgram(null, 'fade'); else snapshot();
    D.last = { ...(D.last || {}), verse: true };
    document.getElementById('clear-all-layers-btn')?.click();
    refresh();
  }
  // One click on an input tile: straight to Program with the current transition,
  // or with `mode` for this one send (Fade to black).
  function sendToProgram(id, mode) {
    let ps = programState();
    if (!ps.on.length) {
      // Nothing is set to carry video yet: the primary display does.
      setOutputLayer(PRIMARY_DISPLAY, 'video', true);
      ps = programState();
    }
    snapshot();
    D.liveLookId = null;
    if (mode) {
      try { localStorage.setItem('kairo-video-transition', JSON.stringify(mode)); } catch {}
      setTimeout(publishTransition, fadeMs() + 400);
    } else publishTransition();
    setVideoLiveFor(ps.on.map(o => o.id), id || null);
    syncPreviewFrame();   // with nothing staged, Preview follows the Output's input
    refresh();
  }

  function setPreview(id) {
    D.previewId = id || null;
    syncPreviewFrame();
    refresh();
  }

  // ── Layout: three columns over a bottom half, with draggable splits ───────
  // Only widths the operator set by dragging are kept; the rest follow these defaults.
  const LAYOUT_KEY = 'kairo-vd-layout2';
  const layoutDefaults = { c1: 0.22, c2: 0.56, bl: 0.21 };
  // The top half's share of the height depends on the window: short windows give it more.
  const sizeClass = () => (window.innerHeight < 700 ? 's' : window.innerHeight < 1000 ? 'm' : 'l');
  const topDefaults = { s: 0.64, m: 0.45, l: 0.44 };
  function readLayout() {
    let v = null;
    try { v = JSON.parse(localStorage.getItem(LAYOUT_KEY) || 'null'); } catch {}
    v = v && typeof v === 'object' ? v : {};
    const cls = sizeClass();
    return {
      c1: clamp(v.c1, 0.14, 0.40, layoutDefaults.c1),
      c2: clamp(v.c2, 0.35, 0.75, layoutDefaults.c2),
      bl: clamp(v.bl, 0.14, 0.5, layoutDefaults.bl),
      top: clamp(v['top_' + cls], 0.25, 0.8, topDefaults[cls]),
      cls,
    };
  }
  function clamp(n, lo, hi, d) { n = Number(n); return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : d; }
  // Saves only what was dragged: what differs from the defaults (the top share per window size class).
  function saveLayout() {
    let v = {};
    try { v = JSON.parse(localStorage.getItem(LAYOUT_KEY) || 'null') || {}; } catch {}
    for (const k of Object.keys(layoutDefaults)) { if (Math.abs(D.layout[k] - layoutDefaults[k]) > 0.002) v[k] = D.layout[k]; else delete v[k]; }
    const cls = D.layout.cls;
    if (Math.abs(D.layout.top - topDefaults[cls]) > 0.002) v['top_' + cls] = D.layout.top; else delete v['top_' + cls];
    try { Object.keys(v).length ? localStorage.setItem(LAYOUT_KEY, JSON.stringify(v)) : localStorage.removeItem(LAYOUT_KEY); } catch {}
  }
  function applyLayout() {
    const { top } = D.layout;
    // Columns as clamps: Inputs no narrower than 170px, Scripture sends no narrower than 300px,
    // and the displays take everything else.
    const tw = D.top.clientWidth || 1200;
    // Untouched, the Inputs column is its minimum in a narrow window so the displays keep the room.
    const dragged = Math.abs(D.layout.c1 - layoutDefaults.c1) > 0.002;
    const c1px = Math.max(200, tw < 1100 && !dragged ? 0 : Math.round(D.layout.c1 * tw));
    const c3px = Math.max(300, Math.round(Math.max(0.12, 1 - D.layout.c1 - D.layout.c2) * tw));
    D.top.style.gridTemplateColumns = `${c1px}px 8px minmax(0, 1fr) 8px ${c3px}px`;
    D.top.style.flexGrow = String(top);
    D.bottomWrap.style.flexGrow = String(1 - top);
    // The playlist group is about the width of the normal screen's sidebar; the media bin takes the rest.
    D.bottomHost.style.gridTemplateColumns = `minmax(0, ${D.layout.bl}fr) 8px minmax(0, ${1 - D.layout.bl}fr)`;
    fitScreens();
  }

  // A thin separator that trades space when dragged (or moved with the arrow keys).
  function splitter(kind, onMove, label) {
    const el = vEl('div', 'vd-split vd-split-' + kind);
    el.setAttribute('role', 'separator');
    el.setAttribute('aria-orientation', kind === 'h' ? 'horizontal' : 'vertical');
    el.setAttribute('aria-label', label);
    el.tabIndex = 0;
    el.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      el.setPointerCapture(e.pointerId);
      el.classList.add('is-drag');
      const move = (ev) => { onMove(ev.clientX, ev.clientY); applyLayout(); };
      const up = () => { el.classList.remove('is-drag'); el.removeEventListener('pointermove', move); el.removeEventListener('pointerup', up); saveLayout(); };
      el.addEventListener('pointermove', move);
      el.addEventListener('pointerup', up);
    });
    el.addEventListener('keydown', (e) => {
      const step = e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -0.01 : e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 0.01 : 0;
      if (!step) return;
      e.preventDefault();
      onMove(null, null, step);
      applyLayout(); saveLayout();
    });
    return el;
  }

  // Preview and Output as large as their area allows, always 16:9: half the
  // width, or what the height leaves above the transition bar.
  // ── The "?" popover: the Director keys. ──
  const KEY_HELP = [
    ['1 to 9', 'Send input to the Output'],
    ['Shift + 1 to 9', 'Stage input on Preview'],
    ['Alt + 1 to 9', 'Load Layout into Preview'],
    ['Space', 'Send Preview to the Output'],
    ['Double-click', 'Send a slide, verse or media straight out'],
    ['Esc', 'Clear what is staged'],
    ['Cmd/Ctrl + Z', 'Undo the last send'],
  ];
  function closeKeyHelp() {
    const h = D.keyHelp;
    if (!h) return;
    h.pop.remove();
    document.removeEventListener('mousedown', h.off, true); document.removeEventListener('keydown', h.esc, true);
    h.anchor.setAttribute('aria-expanded', 'false');
    D.keyHelp = null;
    try { h.anchor.focus(); } catch {}
  }
  function toggleKeyHelp(anchor) {
    if (D.keyHelp) { closeKeyHelp(); return; }
    const pop = vEl('div', 'vd-pop vd-pop-keys');
    pop.setAttribute('role', 'dialog'); pop.setAttribute('aria-label', 'Keyboard shortcuts');
    pop.appendChild(vEl('div', 'vd-keys-title', 'Keyboard shortcuts'));
    const dl = vEl('dl', 'vd-keys');
    KEY_HELP.forEach(([k, d]) => { dl.append(vEl('dt', null, k), vEl('dd', null, d)); });
    pop.appendChild(dl);
    document.body.appendChild(pop);
    const r = anchor.getBoundingClientRect();
    pop.style.top = (r.bottom + 4) + 'px';
    pop.style.left = Math.max(8, Math.min(window.innerWidth - pop.offsetWidth - 8, r.right - pop.offsetWidth)) + 'px';
    anchor.setAttribute('aria-expanded', 'true');
    const off = (e) => { if (!pop.contains(e.target) && !anchor.contains(e.target)) closeKeyHelp(); };
    const esc = (e) => { if (e.key === 'Escape') { e.stopPropagation(); closeKeyHelp(); } };
    document.addEventListener('mousedown', off, true); document.addEventListener('keydown', esc, true);
    D.keyHelp = { pop, anchor, off, esc };
  }

  function fitScreens() {
    const band = D.root;
    if (!band || !D.ui.strip) return;
    const w = band.clientWidth, h = band.clientHeight;
    if (!w || !h) return;
    // The group picks its own arrangement from the room it has: the two panes in a row or
    // stacked, and the transition cluster under them or in a column beside them, whichever
    // gives the biggest 16:9 panes. (It answers to the group's size, not the window's.)
    const head = 28, gap = 8, colW = 132;
    const wide = w >= 520;
    // The cluster is one row (left 170 + Send + right 165 + gaps) or, when that does not
    // fit, Send on a row of its own with the rest centred under it.
    const SIDES = 360, ROW = 46, WRAP = 88;
    // Send is as wide as the gap between the displays' centres, but gives way (down to 170px)
    // so the whole cluster stays on one row for as long as it can.
    const MINSEND = 170;
    const sendFor = (stack, fw) => Math.max(MINSEND, Math.min(stack ? Math.min(Math.max(fw, 200), 300) : fw + gap, w - SIDES - 16));
    const mkUnder = (stack) => {
      const fwOf = (u) => (stack ? Math.min(w, ((h - u - head * 2 - gap) / 2) * 16 / 9) : Math.min((w - gap) / 2, (h - head - u) * 16 / 9));
      let fw = fwOf(ROW), wrap = false;
      if (w < SIDES + 16 + MINSEND) { fw = fwOf(WRAP); wrap = true; }
      return { stack, beside: false, ok: wide, fw, wrap };
    };
    const options = [
      mkUnder(false),
      { stack: false, beside: true, ok: true, fw: Math.min((w - colW - gap * 2) / 2, (h - head) * 16 / 9) },
      mkUnder(true),
      { stack: true, beside: true, ok: true, fw: Math.min(w - colW - gap, ((h - head * 2 - gap) / 2) * 16 / 9) },
    ].filter(o => o.ok);
    let best = options[0];
    for (const o of options) if (o.fw > best.fw * 1.04) best = o;   // a clear gain, else keep the simpler one
    band.classList.toggle('is-wrap', !!best.wrap);
    band.style.setProperty('--vd-sendw', Math.round(sendFor(best.stack, best.fw)) + 'px');
    band.classList.toggle('is-stack', best.stack);
    band.classList.toggle('is-beside', best.beside);
    band.style.setProperty('--vd-fw', Math.max(80, Math.floor(best.fw)) + 'px');
    // Beside, the cluster column takes whatever width the panes leave (within reason).
    const panesW = best.stack ? best.fw : best.fw * 2 + gap;
    band.style.setProperty('--vd-colw', Math.round(Math.min(240, Math.max(colW, w - panesW - gap))) + 'px');
  }

  function build() {
    const center = document.querySelector('main.center-stage');
    if (!center) return false;
    D.layout = readLayout();

    // ── Column 1: Inputs ──
    const inputs = vEl('section', 'vd-section vd-section-inputs');
    inputs.setAttribute('aria-label', 'Inputs');
    const ihead = vEl('div', 'vd-gh');
    ihead.appendChild(vEl('span', 'vd-label vd-gh-title', 'Inputs'));
    D.grid = window.KairoVideoGrid.create({
      getSources: () => videoSourceList().filter(s => s.enabled !== false),
      getDevices: () => videoState.devices,
      getState: () => { const ps = D.programState; return { programId: ps.id, mixed: ps.mixed, hasOutputs: ps.on.length > 0, previewId: D.previewId }; },
      onProgram: (id) => sendToProgram(id),
      onPreview: setPreview,
      onPlaceholder: (source, el) => openDevicePicker(source.id, el),
      onAdd: (el) => openAddInputPanel(el),
      grid: settings.videoGridOn !== false,
      onGrid: (on) => { settings.videoGridOn = on; saveSettingsPatch({ videoGridOn: on }); },
      layout: 'column',
    });
    const ictl = vEl('div', 'vd-gh-ctl');
    ictl.append(vBtn('+ Add', 'Add an input without leaving Director mode', (e) => openAddInputPanel(e.currentTarget), 'vd-mini vd-add'));
    ihead.append(D.grid.controls, ictl);   // Grid | List beside the title, + Add at the right end
    const ifoot = vEl('div', 'vd-section-foot');
    D.ui.noVideo = vBtn('No video', 'Send no video to Output', () => sendToProgram(null), 'vd-mini');
    D.ui.ready = vBtn('Ready check', 'List anything not ready before going live', (e) => openReadyCheck(e.currentTarget), 'vd-mini vd-ready');
    ifoot.append(
      D.ui.ready,
      D.ui.noVideo,
      vBtn('Manage', 'Rename, reorder, switch off or remove inputs', () => openSources(), 'vd-mini'),
    );
    inputs.append(ihead, D.grid.el, ifoot);

    // ── Column 2: Preview | strip | Output ──
    const band = vEl('section', 'vd-band');
    band.id = 'vd-band';
    band.setAttribute('aria-label', 'Preview and Output');
    const screens = vEl('div', 'vd-screens');
    const pvw = vEl('div', 'vd-screen vd-screen-pvw');
    const pvwHead = vEl('div', 'vd-screen-head');
    pvwHead.appendChild(vEl('span', 'vd-tag vd-tag-pvw', 'Preview'));
    D.ui.pvwRef = vEl('span', 'vd-ref');
    pvwHead.append(D.ui.pvwRef);
    pvw.append(pvwHead, buildFrame('pvw'));
    if (window.KairoVideoLooks) {
      D.looks = window.KairoVideoLooks.create({
        loadToPreview: loadLookToPreview,
        takeNow: takeStaged,
        capture: captureLook,
        onToggle: (open) => { if (D.ui.looksBtn) { D.ui.looksBtn.classList.toggle('is-open', open); D.ui.looksBtn.setAttribute('aria-expanded', String(open)); } },
        previewLookId: () => D.previewLookId,
        liveLookId: () => D.liveLookId,
        videoName: (look) => { const id = lookVideo(look); const x = videoSourceList().find(v => v.id === id); return x ? x.name : 'No video'; },
        onChange: () => refresh(),
        screenThemes,
        renderThumb: (el, theme) => window.KairoService && window.KairoService.renderLookThumbnail(el, theme),
        // Customize a Look's screen theme in Theme Studio. A built-in theme (or none yet) is copied first,
        // so the original stays as it is; the copy is a new theme in the Screens group.
        customizeTheme: (look) => {
          if (typeof openThemeStudio !== 'function' || typeof saveLooks !== 'function') return null;
          const cur = look.themeId ? themeById(look.themeId) : null;
          let target = cur;
          const builtIn = (t) => typeof DEFAULT_LOOKS !== 'undefined' && DEFAULT_LOOKS.some(d => d.id === t.id);
          if (!cur || builtIn(cur)) {
            const src = cur || themeById('screen-split-left') || (typeof DEFAULT_LOOKS !== 'undefined' && DEFAULT_LOOKS.find(d => d.id === 'screen-split-left'));
            if (!src) return null;
            target = JSON.parse(JSON.stringify(src));
            target.id = 'look-' + Date.now();
            target.name = cur ? `${cur.name} copy` : `${look.name} screen`;
            target.groupId = 'grp-screen'; target.groupName = 'Screens (Director mode)';
            looks.push(target);
            saveLooks();
          }
          openThemeStudio();
          if (typeof selectLook === 'function') selectLook(target);
          return target.id;
        },
        editTheme: (id) => { const t = themeById(id); if (!t || typeof openThemeStudio !== 'function') return; openThemeStudio(); if (typeof selectLook === 'function') selectLook(t); },
      });
    }

    // The transition bar, centred under the two screens.
    const bar = vEl('div', 'vd-transbar');
    bar.setAttribute('role', 'group');
    bar.setAttribute('aria-label', 'Transition');
    // One centred row under the displays: how it goes (Cut | Fade, with the fade length attached to
    // Fade) on the left, "Send" in the middle, and the quiet Fade to black and Undo on the
    // right. No hover tooltips here: they would cover the neighbouring buttons.
    const bare = (b, label) => { b.removeAttribute('title'); b.setAttribute('aria-label', label); return b; };
    D.ui.take = bare(vBtn('', '', takeStaged, 'vd-take vd-send'), 'Send: put Preview on the Output (Space)');
    D.ui.take.append(vEl('span', 'vd-send-text', 'Send'), vEl('span', 'vd-send-key', 'Space'));
    D.ui.cut = bare(vBtn('Cut', '', () => { setTransition('cut'); refresh(); }, 'vd-take'), 'Send with a cut');
    D.ui.fade = bare(vBtn('Fade', '', () => { setTransition('fade'); refresh(); }, 'vd-take'), 'Send with a fade');
    const seg2 = vEl('div', 'vd-seg2');
    seg2.setAttribute('role', 'group'); seg2.setAttribute('aria-label', 'Transition');
    D.ui.black = bare(vBtn('Fade to black', '', fadeToBlack, 'vd-take vd-quiet'), 'Fade to black: no video, and the verse, slide and timer come down');
    D.ui.blackText = D.ui.black.firstChild;
    const dur = vEl('label', 'vd-dur');
    const durIn = vEl('input'); durIn.type = 'number'; durIn.min = '0'; durIn.max = '5000'; durIn.step = '50'; durIn.value = String(fadeMs());
    durIn.setAttribute('aria-label', 'Fade length in milliseconds');
    durIn.addEventListener('change', () => {
      const n = Math.min(5000, Math.max(0, Math.round(Number(durIn.value) || 0)));
      durIn.value = String(n);
      settings.videoFadeMs = n; saveSettingsPatch({ videoFadeMs: n }); publishFade();
    });
    dur.append(durIn, vEl('span', 'vd-dur-unit', 'ms'));
    D.ui.dur = dur;
    seg2.append(D.ui.cut, D.ui.fade, dur);
    D.ui.undo = bare(vBtn('Undo', '', undo, 'vd-take vd-quiet'), 'Undo the last send (Cmd/Ctrl+Z)');
    const cluster = vEl('div', 'vd-cluster');
    const left = vEl('div', 'vd-cl-left'); left.append(seg2);
    const right = vEl('div', 'vd-cl-right'); right.append(D.ui.black, D.ui.undo);
    cluster.append(left, D.ui.take, right);
    bar.append(cluster);
    D.ui.strip = bar;

    const pgm = vEl('div', 'vd-screen vd-screen-pgm');
    const pgmHead = vEl('div', 'vd-screen-head');
    pgmHead.appendChild(vEl('span', 'vd-tag vd-tag-pgm', 'Output'));
    D.ui.pgmName = vEl('span', 'vd-ref');
    pgmHead.appendChild(D.ui.pgmName);
    pgm.append(pgmHead, buildFrame('pgm'));
    D.ui.pgm = pgm;
    screens.append(pvw, pgm);
    band.append(screens, bar);
    // One group card: a single title, Preview and Output, and every transition beneath them.
    const displays = vEl('section', 'vd-section vd-section-displays');
    displays.setAttribute('aria-label', 'Displays');
    const dhead = vEl('div', 'vd-gh');
    dhead.appendChild(vEl('span', 'vd-label vd-gh-title', 'Displays'));
    // Looks (a dialog) sits beside the title.
    const dctl = vEl('div', 'vd-gh-ctl');
    const LK = (window.KairoVideoLooks && window.KairoVideoLooks.LABEL) || { many: 'Layouts', one: 'Layout' };
    const looksBtn = vBtn('', `Saved Preview setups`, () => D.looks && D.looks.open(), 'vd-mini vd-looks-btn');
    looksBtn.innerHTML = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="8" height="8" rx="1.5"/><rect x="13" y="3" width="8" height="8" rx="1.5"/><rect x="3" y="13" width="8" height="8" rx="1.5"/><rect x="13" y="13" width="8" height="8" rx="1.5"/></svg>';
    looksBtn.append(vEl('span', null, LK.many));
    looksBtn.setAttribute('aria-haspopup', 'dialog'); looksBtn.setAttribute('aria-expanded', 'false');
    D.ui.looksBtn = looksBtn;
    D.ui.looksChip = vEl('span', 'vd-looks-chip'); D.ui.looksChip.hidden = true;
    const helpBtn = vBtn('?', 'Keyboard shortcuts', (e) => toggleKeyHelp(e.currentTarget), 'vd-mini vd-help-btn');
    helpBtn.setAttribute('aria-label', 'Keyboard shortcuts'); helpBtn.setAttribute('aria-haspopup', 'dialog');
    dctl.append(D.ui.looksChip, looksBtn, helpBtn);
    dhead.appendChild(dctl);
    displays.append(dhead, band);

    // ── Column 3: Scripture sends ──
    const sends = vEl('section', 'vd-section vd-section-sends');
    sends.setAttribute('aria-label', 'Scripture sends');
    D.sendsHost = sends;

    // ── Top row with its two vertical splits ──
    const top = vEl('div', 'vd-top');
    const split1 = splitter('v', (x, y, step) => {
      if (step) { D.layout.c1 = clamp(D.layout.c1 + step, 0.14, 0.40, D.layout.c1); return; }
      const r = top.getBoundingClientRect();
      D.layout.c1 = clamp((x - r.left) / r.width, 0.14, 0.40, D.layout.c1);
    }, 'Resize Inputs');
    const split2 = splitter('v', (x, y, step) => {
      if (step) { D.layout.c2 = clamp(D.layout.c2 + step, 0.35, 0.75, D.layout.c2); return; }
      const r = top.getBoundingClientRect();
      D.layout.c2 = clamp((x - r.left) / r.width - D.layout.c1, 0.35, 0.75, D.layout.c2);
    }, 'Resize the displays');
    top.append(inputs, split1, displays, split2, sends);

    // ── Bottom half, and the horizontal split ──
    const bottomWrap = vEl('div', 'vd-bottom-wrap');
    D.bottomHost = vEl('div', 'vd-bottom');
    bottomWrap.appendChild(D.bottomHost);
    const hsplit = splitter('h', (x, y, step) => {
      const c = center.getBoundingClientRect();
      if (step) { D.layout.top = clamp(D.layout.top + step, 0.25, 0.8, D.layout.top); return; }
      const t = top.getBoundingClientRect();
      D.layout.top = clamp((y - t.top) / Math.max(1, c.bottom - t.top - 8), 0.25, 0.8, D.layout.top);
    }, 'Resize the top and bottom halves');

    const wrap = vEl('div', 'vd-main');
    wrap.append(top, hsplit, bottomWrap);
    center.appendChild(wrap);
    D.main = wrap; D.top = top; D.bottomWrap = bottomWrap;
    D.root = band;
    D.fitObs = new ResizeObserver(() => fitScreens());
    D.fitObs.observe(band);
    D.topObs = new ResizeObserver(() => applyLayout());
    D.topObs.observe(top);
    D.onResize = () => { if (sizeClass() !== D.layout.cls) { D.layout = readLayout(); } applyLayout(); };
    window.addEventListener('resize', D.onResize);
    applyLayout();
    return true;
  }

  // ── Ready check: what is not ready before going live. It only informs. ──
  function readyIssues() {
    const issues = [];
    const sources = videoSourceList().filter(x => x.enabled !== false);
    for (const x of sources) {
      const st = VSRC.stateOf(x, videoState.devices);
      if (st === 'placeholder') issues.push({ text: `${x.name}: no device`, fix: 'Choose device', run: (btn) => openDevicePicker(x.id, btn) });
      else if (st === 'missing') issues.push({ text: `${x.name}: device not connected`, fix: 'Choose device', run: (btn) => openDevicePicker(x.id, btn) });
    }
    const ps = programState();
    if (!ps.on.length) issues.push({ text: 'Video input is off on every output', fix: 'Turn on', run: () => setOutputLayer(PRIMARY_DISPLAY, 'video', true) });
    else if (!ps.mixed && !ps.id) {
      const first = sources.find(x => VSRC.stateOf(x, videoState.devices) === 'ready');
      issues.push(first ? { text: 'Output has no video source', fix: `Send ${first.name}`, run: () => sendToProgram(first.id) } : { text: 'Output has no video source', fix: 'Add input', run: (btn) => openAddInputPanel(btn) });
    }
    if (typeof allConfiguredOutputs === 'function' && typeof isOutputEnabled === 'function') {
      for (const o of allConfiguredOutputs()) {
        if (o.type === 'display' && !isOutputEnabled(o)) issues.push({ text: `${o.name} is off`, fix: 'Open Outputs', run: () => { document.getElementById('settings-modal')?.classList.remove('hidden'); if (typeof showSettingsPane === 'function') showSettingsPane('outputs'); } });
      }
    }
    return issues;
  }
  function openReadyCheck(anchor) {
    openVideoPanel(anchor, 'Ready check', (close) => {
      const box = vEl('div', 'vsrc-det');
      const issues = readyIssues();
      if (!issues.length) box.appendChild(vEl('p', 'setting-hint', 'Everything is ready.'));
      issues.forEach(it => {
        const r = vEl('div', 'vsrc-det-row');
        r.appendChild(vEl('span', 'vsrc-det-name', it.text));
        r.appendChild(vBtn(it.fix, it.fix, (e) => { const b = e.currentTarget; close(); it.run(anchor || b); }, 'vd-mini'));
        box.appendChild(r);
      });
      return box;
    });
  }
  function paintReady() {
    if (!D.ui.ready) return;
    const n = readyIssues().length;
    D.ui.ready.textContent = n ? `Ready check · ${n}` : 'Ready check';
    D.ui.ready.classList.toggle('has-issues', n > 0);
  }

  function openSources() {
    document.getElementById('settings-modal')?.classList.remove('hidden');
    if (typeof showSettingsPane === 'function') showSettingsPane('sources');
  }

  // Something is on air: a live source, or a verse marked live in the queue.
  function isOnAir() {
    const ps = D.programState;
    if (ps.on.length && (ps.mixed || ps.id)) return true;
    return !!document.querySelector('#current-display-card .is-live');
  }

  function refresh() {
    if (!D.active) return;
    if (!D.root) return;
    const sources = videoSourceList();
    if (D.previewId && !sources.some(s => s.id === D.previewId)) D.previewId = null;
    const ps = programState();
    D.programState = ps;

    // Program screen follows the first output that carries video, else the primary.
    const pgmOut = (ps.on[0] && ps.on[0].id) || PRIMARY_DISPLAY;
    setFrameSrc('pgm', `/display.html?output=${encodeURIComponent(pgmOut)}&monitor=1&shared=1`);
    setFrameSrc('pvw', `/display.html?output=${encodeURIComponent(PRIMARY_DISPLAY)}&preview=1`);
    const pgmSource = ps.mixed ? null : sources.find(s => s.id === ps.id);
    D.ui.pgmName.textContent = !ps.on.length ? 'Video off' : ps.mixed ? 'Mixed' : pgmSource ? pgmSource.name : 'No video';
    // What the multiview display mirrors.
    try {
      const pub = JSON.stringify({ programId: ps.id, mixed: ps.mixed, hasOutputs: ps.on.length > 0, previewId: D.previewId });
      if (D.lastPub !== pub) { D.lastPub = pub; localStorage.setItem('kairo-video-director', pub); }
    } catch {}
    D.grid.render();
    D.ui.noVideo.classList.toggle('is-on', !ps.mixed && ps.on.length > 0 && !ps.id);
    fitScreens();
    paintReady();
    D.ui.pgm.classList.toggle('is-live', isOnAir());
    if (D.frames.pgm) D.frames.pgm.name.textContent = !ps.on.length ? '' : ps.mixed ? 'Mixed' : pgmSource ? pgmSource.name : 'No video';

    // Preview header
    const pvwSource = sources.find(s => s.id === D.previewId);
    const staged = D.staged ? (D.staged.verse.reference || D.staged.verse.ref || D.staged.verse.label || 'Staged') : '';
    markStaged();
    const bits = [];
    if (pvwSource) bits.push(pvwSource.name);
    if (staged) bits.push(staged);
    if (D.pendingLook) bits.push(`Layout: ${D.pendingLook.name}`);
    D.ui.pvwRef.textContent = bits.length ? bits.join(' · ') : 'Same as output';
    // The empty-picture label sits inside the video zone when the layout has one, not over the frame.
    if (D.frames.pvw) D.frames.pvw.box.classList.toggle('has-zone', hasZone(pvwFrameTheme()));
    if (D.frames.pgm) D.frames.pgm.box.classList.toggle('has-zone', hasZone(liveFrameTheme()));
    D.ui.undo.disabled = !D.last;
    if (D.frames.pvw) D.frames.pvw.name.textContent = pvwSource ? pvwSource.name : '';
    const canTake = ps.on.length > 0;
    const cur = transitionPref();
    if (D.ui.dur) D.ui.dur.hidden = transitionPref() !== 'fade';   // the length belongs to Fade
    for (const [btn, mode] of [[D.ui.cut, 'cut'], [D.ui.fade, 'fade']]) {
      btn.classList.toggle('is-current', mode === cur);
      btn.setAttribute('aria-pressed', String(mode === cur));
    }
    // Send is live when there is a verse or slide staged, or a Preview source that differs from Output.
    const videoDiff = canTake && D.previewId != null && D.previewId !== (ps.mixed ? undefined : (ps.id || null));
    const ready = !!((D.staged && !D.staged.sent) || videoDiff || (D.pendingLook && !D.lookSent));
    D.ui.take.disabled = !ready;
    D.ui.take.classList.toggle('is-ready', ready);
    if (ready) D.ui.take.removeAttribute('title'); else D.ui.take.title = 'Nothing staged';
    D.looks && D.looks.render();
    if (D.ui.looksChip) { D.ui.looksChip.textContent = D.pendingLook ? D.pendingLook.name : ''; D.ui.looksChip.title = D.pendingLook ? `Staged in Preview: ${D.pendingLook.name}` : ''; D.ui.looksChip.hidden = !D.pendingLook; }
  }

  // ── Settings → Director mode: rows for what the screen no longer carries ─
  function mountSettings() {
    const section = document.querySelector('.settings-pane[data-pane="director"] .settings-section');
    if (!section || section.querySelector('[data-vd-rows]')) return;
    const card = vEl('div', 'dm-card');
    card.dataset.vdRows = '1';
    card.style.marginTop = '12px';
    const row = (name, hint, checked, onChange) => {
      const r = vEl('div', 'dm-row');
      const text = vEl('div', 'dm-text');
      text.append(vEl('div', 'dm-name', name), vEl('p', 'setting-hint', hint));
      const label = vEl('label', 'output-toggle');
      const cb = vEl('input'); cb.type = 'checkbox'; cb.checked = checked;
      cb.setAttribute('aria-label', name);
      cb.addEventListener('change', () => onChange(cb.checked));
      label.append(cb, vEl('span', 'output-toggle-track'));
      r.append(text, label);
      return r;
    };
    // Multiview display: the input grid full screen on one chosen display (Off by default).
    const mvRow = vEl('div', 'dm-row');
    const mvText = vEl('div', 'dm-text');
    mvText.append(vEl('div', 'dm-name', 'Multiview display'), vEl('p', 'setting-hint', 'Show the inputs full screen on one display.'));
    const mvSel = vEl('select', 'setting-input');
    const mvOpts = [['', 'Off']].concat(displayOutputs().map(d => [d.id, d.name + (d.kind === 'monitor' ? ' (Monitor)' : '')]));
    mvOpts.forEach(([v, l]) => mvSel.add(new Option(l, v, false, v === (settings.videoMultiview || ''))));
    mvSel.setAttribute('aria-label', 'Multiview display');
    const mvConfirm = vEl('div', 'dm-confirm');
    mvConfirm.hidden = true;
    const applyMultiview = (id) => {
      settings.videoMultiview = id || '';
      saveSettingsPatch({ videoMultiview: settings.videoMultiview });
      if (typeof applyOutputModes === 'function') applyOutputModes();
      unmountSettings(); mountSettings();
      refresh();
      if (typeof onVideoLayerChanged === 'function') onVideoLayerChanged();
    };
    mvSel.addEventListener('change', () => {
      const id = mvSel.value;
      // The audience screen asks once, by name, before it stops showing the audience.
      if (id && id === PRIMARY_DISPLAY) {
        const name = (displayOutputs().find(d => d.id === id) || {}).name || 'This display';
        mvConfirm.replaceChildren(
          vEl('span', null, `${name} is your audience screen. Show the multiview there instead?`),
          vBtn('Show multiview', 'Replace the audience screen with the multiview', () => applyMultiview(id), 'vd-mini vd-mini-primary'),
          vBtn('Keep as is', 'Leave the audience screen alone', () => { mvSel.value = settings.videoMultiview || ''; mvConfirm.hidden = true; }, 'vd-mini'));
        mvConfirm.hidden = false;
        return;
      }
      mvConfirm.hidden = true;
      applyMultiview(id);
    });
    mvRow.append(mvText, mvSel);
    card.append(mvRow, mvConfirm);
    const layers = outputLayerMap();
    videoOutputs().forEach(o => {
      card.appendChild(row(`Video on ${o.name}`, 'Show the live input behind scripture.', !!(layers[o.id] && layers[o.id].video), (on) => setOutputLayer(o.id, 'video', on)));
    });
    card.appendChild(row('Stage sends on Preview', 'Verses you send by hand wait until you send them to the screen.', stageManual(), (on) => {
      settings.videoStageManual = on;
      saveSettingsPatch({ videoStageManual: on });
    }));
    section.appendChild(card);
  }
  function unmountSettings() {
    document.querySelector('.settings-pane[data-pane="director"] [data-vd-rows]')?.remove();
  }

  // ── Keyboard: 1-9 send an input, Shift+1-9 stage it, Space takes what is
  // staged, Esc clears staging, Cmd/Ctrl+Z undoes the last take. They do not
  // fire while typing in a field or while a dialog is open. ────────────────
  function onKey(e) {
    if (!D.active) return;
    const t = e.target;
    if (t && (t.isContentEditable || (t.matches && t.matches('input,textarea,select')))) return;
    if (document.querySelector('.modal:not(.hidden), .vd-add-panel')) return;
    if (document.body.classList.contains('vd-sheet')) return;   // a full screen is open over the Director: its keys are not ours
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'z') {
      if (D.last) { e.preventDefault(); undo(); }
      return;
    }
    // Alt/Option+1-9 load a Look into Preview (1-9 alone send inputs).
    if (e.altKey && !e.metaKey && !e.ctrlKey) {
      const a = /^(?:Digit|Numpad)([1-9])$/.exec(e.code || '');
      if (a && D.looks && D.looks.loadIndex(Number(a[1]) - 1)) e.preventDefault();
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const m = /^(?:Digit|Numpad)([1-9])$/.exec(e.code || '');
    if (m) {
      const src = videoSourceList().filter(x => x.enabled !== false)[Number(m[1]) - 1];
      if (!src || VSRC.stateOf(src, videoState.devices) !== 'ready') return;
      e.preventDefault();
      if (e.shiftKey) setPreview(src.id); else sendToProgram(src.id);
      return;
    }
    if (e.code === 'Space') {
      if (t && t.closest && t.closest('button')) return;       // a focused button keeps its own Space
      e.preventDefault();
      takeStaged();
      return;
    }
    if (e.key === 'Escape') clearStaging();
  }

  async function start() {
    if (D.active) return;
    if (!build()) return;
    D.active = true;
    document.addEventListener('keydown', onKey);
    document.addEventListener('dblclick', onDblClick, true);
    wrapSend();
    try { migrateScreenOrig(); } catch {}
    D.restoreLook = settings.directorPreviewLook || null;
    publishTransition();
    publishFade();
    if (typeof applyOutputModes === 'function') applyOutputModes();   // a chosen multiview display switches over
    await startVideoSources();     // the Sources pane, device list, published state
    window.KairoVideoLayout?.start();
    window.KairoVideoLayout?.fillSends(D.sendsHost);
    window.KairoVideoDock?.mount(D.bottomHost);
    if (D.bottomHost.children.length === 2) {
      const bsplit = splitter('v', (x, y, step) => {
        if (step) { D.layout.bl = clamp(D.layout.bl + step, 0.14, 0.5, D.layout.bl); return; }
        const r = D.bottomHost.getBoundingClientRect();
        D.layout.bl = clamp((x - r.left) / r.width, 0.14, 0.5, D.layout.bl);
      }, 'Resize the playlist and the media bin');
      D.bottomHost.insertBefore(bsplit, D.bottomHost.children[1]);
    }
    applyLayout();
    mountSettings();
    // The Program frame follows the verse going live and clearing.
    const queue = document.getElementById('current-display-card');
    if (queue) {
      D.liveObserver = new MutationObserver(() => D.ui.pgm && D.ui.pgm.classList.toggle('is-live', isOnAir()));
      D.liveObserver.observe(queue, { attributes: true, attributeFilter: ['class'], subtree: true, childList: true });
    }
    refresh();
    // The Layout that was on Preview when the app last closed comes back (staged, not sent).
    if (D.restoreLook && D.looks) {
      const l = D.looks.byId(D.restoreLook);
      if (l) loadLookToPreview(l);
      else { settings.directorPreviewLook = null; saveSettingsPatch({ directorPreviewLook: null }); }
    }
    // A Preview frame needs its page up before it can be fed.
    D.frames.pvw.frame.addEventListener('load', syncPreviewFrame);
  }

  // Mode off: the verse on screen stays; the video goes to nothing with a
  // fade, every capture is released and the screen is the normal one again.
  function stop() {
    if (!D.active) return;
    D.active = false;
    document.removeEventListener('keydown', onKey);
    document.removeEventListener('dblclick', onDblClick, true);
    unwrapSend();
    D.staged = null;   // a staged verse is dropped, never sent: the screen keeps what it shows
    D.liveObserver?.disconnect();
    unmountSettings();
    window.KairoVideoDock?.unmount();
    window.KairoVideoLayout?.emptySends();
    D.grid?.destroy(); D.grid = null;
    window.KairoVideoLayout?.stop();
    // Going off air fades, without changing the transition the operator chose.
    try { localStorage.setItem('kairo-video-transition', JSON.stringify('fade')); } catch {}
    setVideoLiveFor(videoOutputs().map(o => o.id), null);
    setTimeout(() => { try { localStorage.removeItem('kairo-video-transition'); } catch {} }, 1500);
    Object.values(D.frames).forEach(f => { f.ro.disconnect(); f.frame.src = 'about:blank'; });
    D.frames = {};
    stopVideoSources();
    window.KairoVideoHub?.releaseAll();
    try { applyScreenTheme(null); } catch {}
    closeKeyHelp();
    D.looks && D.looks.destroy(); D.looks = null; D.pendingLook = null; D.previewLookId = null; D.liveLookId = null;
    D.fitObs?.disconnect(); D.fitObs = null;
    D.topObs?.disconnect(); D.topObs = null;
    if (D.onResize) { window.removeEventListener('resize', D.onResize); D.onResize = null; }
    D.main?.remove();
    D.root = null; D.main = null; D.top = null; D.bottomWrap = null;
    try { localStorage.setItem('kairo-video-live', '{}'); localStorage.removeItem('kairo-video-fade'); localStorage.removeItem('kairo-video-tbar'); localStorage.removeItem('kairo-video-director'); } catch {}
    D.lastPub = null;
    // The display chosen for the multiview goes back to its normal mode (the Monitor gets its confidence view back).
    if (typeof applyOutputModes === 'function') applyOutputModes();
  }

  // Is video on air right now? (the toggle asks before taking it off)
  function isLive() { return programState().on.some(o => videoLiveMap()[o.id]); }

  window.KairoVideoDirector = {
    start, stop, refresh, isLive,
    // For the dock's input tiles.
    state: () => ({ previewId: D.previewId, program: D.programState, transition: transitionPref(), active: D.active }),
    setPreview, sendToProgram, mountSettings,
    stageSlide: stageSlideSend, stageMedia: stageMediaSend, stagedSlideKey, onServerMessage,
    // Remote and OSC actions (app_startup.js). Each is a no-op when Director mode is off, and never throws.
    send() { try { if (!D.active) return false; if (D.ui.take && D.ui.take.disabled) return false; takeStaged(); return true; } catch { return false; } },
    fadeToBlack() { try { if (!D.active) return false; fadeToBlack(); return true; } catch { return false; } },
    undo() { try { if (!D.active || !D.last) return false; undo(); return true; } catch { return false; } },
    loadLayout(n) { try { return !!(D.active && D.looks && D.looks.loadIndex(Number(n) - 1)); } catch { return false; } },
    sendInput(n) {
      try {
        if (!D.active) return false;
        const src = videoSourceList().filter(x => x.enabled !== false)[Number(n) - 1];
        if (!src || VSRC.stateOf(src, videoState.devices) !== 'ready') return false;
        sendToProgram(src.id);
        return true;
      } catch { return false; }
    },
    // A detected verse held back by Auto-send off: staged in Preview, never sent.
    stageDetected: (verse, look) => { if (D.active && verse) stage(verse, look); },
  };
})();
