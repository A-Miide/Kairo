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
    last: null,             // one-step undo: { live, preview, verse }
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
  function syncPreviewFrame() {
    post('pvw', { type: 'vd-source', sourceId: D.previewId });
    if (D.staged) post('pvw', { type: 'detection', target: 'viewer', verses: [D.staged.verse], look: D.staged.look || null });
    else post('pvw', { type: 'clear' });
  }

  // ── Staging ──────────────────────────────────────────────────────────────
  const sameVerse = (a, b) => !!a && !!b && (a.reference || a.ref) === (b.reference || b.ref) && (a.text || a.verse) === (b.text || b.verse);
  function stage(verse, look) {
    D.staged = { verse, look: look || null };
    syncPreviewFrame();
    refresh();
  }
  function clearStage() {
    D.staged = null;
    syncPreviewFrame();
    refresh();
  }
  function takeVerse() {
    if (!D.staged) return;
    const { verse, look, slide } = D.staged;
    D.staged = null;
    D.last = { ...(D.last || {}), verse: true };
    syncPreviewFrame();
    refresh();
    // A staged slide goes out through the playlist's own send, a verse through the normal one.
    if (slide && window.KairoService) return window.KairoService.sendSlide(slide.item, slide.index);
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
  function wrapSend() {
    if (D.origSend || typeof sendVerseToServer !== 'function') return;
    D.origSend = sendVerseToServer;
    // eslint-disable-next-line no-global-assign
    sendVerseToServer = function (verse, look = null) {
      if (!D.active || !stageManual()) return D.origSend(verse, look);
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
  function snapshot() { D.last = { live: { ...videoLiveMap() }, preview: D.previewId }; }
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
    // The verse goes back through the app's own "Back to the previous screen".
    if (last.verse) document.getElementById('output-back-btn')?.click();
    refresh();
  }
  // Space: send what is staged (the verse, and the Preview source when it differs).
  function takeStaged() {
    const ps = programState();
    const mode = transitionPref();
    if (D.staged) takeVerse();
    if (ps.on.length && (D.previewId || null) !== (ps.mixed ? undefined : (ps.id || null))) take(mode);
  }
  function clearStaging() {
    D.staged = null;
    D.previewId = null;
    syncPreviewFrame();
    refresh();
  }
  // Preview → Program with `mode`; the old Program source is staged next.
  function take(mode) {
    const ps = programState();
    if (!ps.on.length) return;
    snapshot();
    setTransition(mode);                       // written first: the display reads it when the source changes
    const outgoing = ps.mixed ? D.previewId : ps.id;
    setVideoLiveFor(ps.on.map(o => o.id), D.previewId);
    D.previewId = outgoing;
    syncPreviewFrame();
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
    if (mode) {
      try { localStorage.setItem('kairo-video-transition', JSON.stringify(mode)); } catch {}
      setTimeout(publishTransition, fadeMs() + 400);
    } else publishTransition();
    setVideoLiveFor(ps.on.map(o => o.id), id || null);
    refresh();
  }

  // ── T-bar: fade Preview onto Output by hand ──────────────────────────────
  function writeTbar(sourceId, t) {
    try { localStorage.setItem('kairo-video-tbar', JSON.stringify({ sourceId: sourceId || null, t, n: Date.now() })); } catch {}
  }
  let tbarBusy = false;
  function tbarMove(t) {
    if (!programState().on.length) return;
    tbarBusy = true;
    writeTbar(D.previewId, t);
  }
  // Let go past halfway and the fade completes; short of it, it goes back.
  function tbarRelease(t) {
    if (!tbarBusy) return;
    tbarBusy = false;
    const ps = programState();
    if (t >= 0.5 && ps.on.length) {
      snapshot();
      writeTbar(D.previewId, 1);
      const outgoing = ps.mixed ? D.previewId : ps.id;
      try { localStorage.setItem('kairo-video-transition', JSON.stringify('cut')); } catch {}
      setVideoLiveFor(ps.on.map(o => o.id), D.previewId);
      D.previewId = outgoing;
      setTimeout(publishTransition, 600);
      syncPreviewFrame();
    } else writeTbar(D.previewId, 0);
    if (D.ui.tbar) D.ui.tbar.value = '0';
    refresh();
  }
  function setPreview(id) {
    D.previewId = id || null;
    syncPreviewFrame();
    refresh();
  }

  // ── Layout: three columns over a bottom half, with draggable splits ───────
  const LAYOUT_KEY = 'kairo-vd-layout';
  const layoutDefaults = { c1: 0.14, c2: 0.60, top: 0.5 };
  function readLayout() {
    try {
      const v = JSON.parse(localStorage.getItem(LAYOUT_KEY) || 'null');
      if (v && typeof v === 'object') return { c1: clamp(v.c1, 0.1, 0.3, layoutDefaults.c1), c2: clamp(v.c2, 0.35, 0.75, layoutDefaults.c2), top: clamp(v.top, 0.25, 0.8, layoutDefaults.top) };
    } catch {}
    return { ...layoutDefaults };
  }
  function clamp(n, lo, hi, d) { n = Number(n); return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : d; }
  function saveLayout() { try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(D.layout)); } catch {} }
  function applyLayout() {
    const { c1, c2, top } = D.layout;
    const c3 = Math.max(0.12, 1 - c1 - c2);
    D.top.style.gridTemplateColumns = `minmax(0, ${c1}fr) 8px minmax(0, ${c2}fr) 8px minmax(0, ${c3}fr)`;
    D.top.style.flexGrow = String(top);
    D.bottomWrap.style.flexGrow = String(1 - top);
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
  function fitScreens() {
    const band = D.root;
    if (!band || !D.ui.strip) return;
    const w = band.clientWidth, h = band.clientHeight;
    if (!w || !h) return;
    const barH = D.ui.strip.offsetHeight + 8;
    const head = 28;                    // each screen's label line
    const gap = 8;
    const byWidth = (w - gap) / 2;
    const byHeight = (h - head - barH) * 16 / 9;
    const fw = Math.max(80, Math.floor(Math.min(byWidth, byHeight)));
    band.style.setProperty('--vd-fw', fw + 'px');
  }

  function build() {
    const center = document.querySelector('main.center-stage');
    if (!center) return false;
    D.layout = readLayout();

    // ── Column 1: Inputs ──
    const inputs = vEl('section', 'vd-section vd-section-inputs');
    inputs.setAttribute('aria-label', 'Inputs');
    const ihead = vEl('div', 'vd-section-head');
    ihead.appendChild(vEl('span', 'vd-label', 'Inputs'));
    D.grid = window.KairoVideoGrid.create({
      getSources: () => videoSourceList().filter(s => s.enabled !== false),
      getDevices: () => videoState.devices,
      getState: () => { const ps = D.programState; return { programId: ps.id, mixed: ps.mixed, hasOutputs: ps.on.length > 0, previewId: D.previewId }; },
      onProgram: (id) => sendToProgram(id),
      onPreview: setPreview,
      onPlaceholder: (source, el) => openDevicePicker(source.id, el),
      onAdd: (el) => openAddInputPanel(el),
      mode: settings.videoGridMode === 2 ? 2 : 4,
      onMode: (n) => { settings.videoGridMode = n; saveSettingsPatch({ videoGridMode: n }); },
      layout: 'column',
    });
    ihead.appendChild(D.grid.controls);
    const ifoot = vEl('div', 'vd-section-foot');
    D.ui.noVideo = vBtn('No video', 'Send no video to Output', () => sendToProgram(null), 'vd-mini');
    ifoot.append(
      D.ui.noVideo,
      vBtn('+ Add', 'Add an input without leaving Director mode', (e) => openAddInputPanel(e.currentTarget), 'vd-mini'),
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
    pvwHead.appendChild(D.ui.pvwRef);
    pvw.append(pvwHead, buildFrame('pvw'));

    // The transition bar, centred under the two screens.
    const bar = vEl('div', 'vd-transbar');
    bar.setAttribute('role', 'group');
    bar.setAttribute('aria-label', 'Transition');
    D.ui.takeVerse = vBtn('Take verse', 'Send the staged verse to Output (Space)', takeVerse, 'vd-take vd-take-verse');
    D.ui.clearStage = vBtn('Clear', 'Clear what is staged (Esc)', clearStaging, 'vd-take');
    D.ui.cut = vBtn('Cut', 'Preview to Output at once', () => take('cut'), 'vd-take');
    D.ui.fade = vBtn('Fade', 'Fade Preview onto Output', () => take('fade'), 'vd-take');
    D.ui.black = vBtn('Fade to black', 'Fade Output to no video', () => sendToProgram(null, 'fade'), 'vd-take');
    const dur = vEl('label', 'vd-dur');
    const durIn = vEl('input'); durIn.type = 'number'; durIn.min = '0'; durIn.max = '5000'; durIn.step = '50'; durIn.value = String(fadeMs());
    durIn.setAttribute('aria-label', 'Fade length in milliseconds');
    durIn.title = 'Fade length';
    durIn.addEventListener('change', () => {
      const n = Math.min(5000, Math.max(0, Math.round(Number(durIn.value) || 0)));
      durIn.value = String(n);
      settings.videoFadeMs = n; saveSettingsPatch({ videoFadeMs: n }); publishFade();
    });
    dur.append(durIn, vEl('span', null, 'ms'));
    const tb = vEl('div', 'vd-tbar-wrap');
    D.ui.tbar = vEl('input', 'vd-tbar');
    D.ui.tbar.type = 'range'; D.ui.tbar.min = '0'; D.ui.tbar.max = '1000'; D.ui.tbar.value = '0';
    D.ui.tbar.setAttribute('aria-label', 'T-bar: fade Preview onto Output by hand');
    D.ui.tbar.title = 'T-bar: drag to fade by hand. Let go past halfway to finish.';
    D.ui.tbar.addEventListener('input', () => tbarMove(Number(D.ui.tbar.value) / 1000));
    for (const ev of ['change', 'pointerup', 'keyup']) D.ui.tbar.addEventListener(ev, () => tbarRelease(Number(D.ui.tbar.value) / 1000));
    tb.append(vEl('span', 'vd-tbar-label', 'T-bar'), D.ui.tbar);
    D.ui.undo = vBtn('Undo', 'Undo the last take (Cmd/Ctrl+Z)', undo, 'vd-take');
    const sep = () => vEl('span', 'vd-bar-sep');
    bar.append(D.ui.takeVerse, D.ui.clearStage, sep(), D.ui.cut, D.ui.fade, D.ui.black, dur, tb, sep(), D.ui.undo);
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
    const displays = vEl('div', 'vd-col-displays');
    displays.appendChild(band);

    // ── Column 3: Scripture sends ──
    const sends = vEl('section', 'vd-section vd-section-sends');
    sends.setAttribute('aria-label', 'Scripture sends');
    D.sendsHost = sends;

    // ── Top row with its two vertical splits ──
    const top = vEl('div', 'vd-top');
    const split1 = splitter('v', (x, y, step) => {
      if (step) { D.layout.c1 = clamp(D.layout.c1 + step, 0.1, 0.3, D.layout.c1); return; }
      const r = top.getBoundingClientRect();
      D.layout.c1 = clamp((x - r.left) / r.width, 0.1, 0.3, D.layout.c1);
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
    applyLayout();
    return true;
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
    D.grid.render();
    D.ui.noVideo.classList.toggle('is-on', !ps.mixed && ps.on.length > 0 && !ps.id);
    fitScreens();
    D.ui.pgm.classList.toggle('is-live', isOnAir());
    if (D.frames.pgm) D.frames.pgm.name.textContent = !ps.on.length ? '' : ps.mixed ? 'Mixed' : pgmSource ? pgmSource.name : 'No video';

    // Preview header
    const pvwSource = sources.find(s => s.id === D.previewId);
    const staged = D.staged ? (D.staged.verse.reference || D.staged.verse.ref || 'Verse') : '';
    D.ui.pvwRef.textContent = staged ? `${pvwSource ? pvwSource.name : 'No video'} · ${staged}` : (pvwSource ? pvwSource.name : 'No video');
    D.ui.takeVerse.disabled = !D.staged;
    D.ui.clearStage.disabled = !D.staged && !D.previewId;
    D.ui.undo.disabled = !D.last;
    if (D.frames.pvw) D.frames.pvw.name.textContent = pvwSource ? pvwSource.name : '';
    const canTake = ps.on.length > 0;
    const cur = transitionPref();
    for (const [btn, mode] of [[D.ui.cut, 'cut'], [D.ui.fade, 'fade']]) {
      btn.disabled = !canTake;
      btn.classList.toggle('is-current', mode === cur);
      btn.setAttribute('aria-pressed', String(mode === cur));
      btn.title = canTake ? (mode === 'cut' ? 'Put the Preview video source on Output at once' : 'Fade the Preview video source onto Output') : 'Turn on Video input for an output in Director mode settings';
    }
    D.ui.black.disabled = !canTake;
    D.ui.tbar.disabled = !canTake;
    D.ui.black.title = canTake ? 'Fade Output to no video' : 'Turn on Video input for an output in Director mode settings';
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
    const layers = outputLayerMap();
    videoOutputs().forEach(o => {
      card.appendChild(row(`Video on ${o.name}`, 'Show the live input behind scripture.', !!(layers[o.id] && layers[o.id].video), (on) => setOutputLayer(o.id, 'video', on)));
    });
    card.appendChild(row('Stage sends on Preview', 'Verses you send by hand wait for Take.', stageManual(), (on) => {
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
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'z') {
      if (D.last) { e.preventDefault(); undo(); }
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
    wrapSend();
    publishTransition();
    publishFade();
    await startVideoSources();     // the Sources pane, device list, published state
    window.KairoVideoLayout?.start();
    window.KairoVideoLayout?.fillSends(D.sendsHost);
    window.KairoVideoDock?.mount(D.bottomHost);
    mountSettings();
    // The Program frame follows the verse going live and clearing.
    const queue = document.getElementById('current-display-card');
    if (queue) {
      D.liveObserver = new MutationObserver(() => D.ui.pgm && D.ui.pgm.classList.toggle('is-live', isOnAir()));
      D.liveObserver.observe(queue, { attributes: true, attributeFilter: ['class'], subtree: true, childList: true });
    }
    refresh();
    // A Preview frame needs its page up before it can be fed.
    D.frames.pvw.frame.addEventListener('load', syncPreviewFrame);
  }

  // Mode off: the verse on screen stays; the video goes to nothing with a
  // fade, every capture is released and the screen is the normal one again.
  function stop() {
    if (!D.active) return;
    D.active = false;
    document.removeEventListener('keydown', onKey);
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
    D.fitObs?.disconnect(); D.fitObs = null;
    D.main?.remove();
    D.root = null; D.main = null; D.top = null; D.bottomWrap = null;
    try { localStorage.setItem('kairo-video-live', '{}'); localStorage.removeItem('kairo-video-fade'); localStorage.removeItem('kairo-video-tbar'); } catch {}
  }

  // Is video on air right now? (the toggle asks before taking it off)
  function isLive() { return programState().on.some(o => videoLiveMap()[o.id]); }

  window.KairoVideoDirector = {
    start, stop, refresh, isLive,
    // For the dock's input tiles.
    state: () => ({ previewId: D.previewId, program: D.programState, transition: transitionPref(), active: D.active }),
    setPreview, sendToProgram, mountSettings,
  };
})();
