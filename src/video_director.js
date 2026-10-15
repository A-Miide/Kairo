// KAIRO — Video director mode: the stage (Preview / Program) and the mode's
// lifecycle.
//
// Lazy-loaded by video_director_boot.js while the mode is on, and removed
// again when it is turned off. Three more lazy files build the rest of the
// screen: video_director_layout.js (rails, top line, verses rail), and
// video_dock.js (the bottom dock: inputs and media). Nothing in the normal
// layout's own code is edited for this mode.
//
//   PROGRAM  what the outputs show now: a live copy of the output's own page
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
    const ro = new ResizeObserver(() => { frame.style.transform = `scale(${box.clientWidth / 1920})`; });
    ro.observe(box);
    D.frames[kind] = { box, frame, ro };
    return box;
  }
  function setFrameSrc(kind, src) {
    const f = D.frames[kind];
    if (!f || f.src === src) return;
    f.src = src;
    f.frame.title = kind === 'pvw' ? 'Preview' : 'Program';
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
    const { verse, look } = D.staged;
    D.staged = null;
    syncPreviewFrame();
    refresh();
    return D.origSend(verse, look);
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
  // Preview → Program with `mode`; the old Program source is staged next.
  function take(mode) {
    const ps = programState();
    if (!ps.on.length) return;
    setTransition(mode);                       // written first: the display reads it when the source changes
    const outgoing = ps.mixed ? D.previewId : ps.id;
    setVideoLiveFor(ps.on.map(o => o.id), D.previewId);
    D.previewId = outgoing;
    syncPreviewFrame();
    refresh();
  }
  // One click on an input tile: straight to Program with the current transition.
  function sendToProgram(id) {
    let ps = programState();
    if (!ps.on.length) {
      // Nothing is set to carry video yet: the primary display does.
      setOutputLayer(PRIMARY_DISPLAY, 'video', true);
      ps = programState();
    }
    publishTransition();
    setVideoLiveFor(ps.on.map(o => o.id), id || null);
    refresh();
  }
  function setPreview(id) {
    D.previewId = id || null;
    syncPreviewFrame();
    refresh();
  }

  // ── Stage ────────────────────────────────────────────────────────────────
  function build() {
    const center = document.querySelector('main.center-stage');
    if (!center) return false;
    const band = vEl('section', 'vd-band');
    band.id = 'vd-band';
    band.setAttribute('aria-label', 'Preview and Program');

    const screens = vEl('div', 'vd-screens');
    // Preview
    const pvw = vEl('div', 'vd-screen vd-screen-pvw');
    const pvwHead = vEl('div', 'vd-screen-head');
    pvwHead.appendChild(vEl('span', 'vd-tag vd-tag-pvw', 'Preview'));
    D.ui.pvwRef = vEl('span', 'vd-ref');
    pvwHead.appendChild(D.ui.pvwRef);
    D.ui.takeVerse = vBtn('Take verse', 'Send the staged verse to Program', takeVerse, 'vd-mini vd-mini-primary');
    D.ui.clearStage = vBtn('Clear', 'Remove the staged verse from Preview', clearStage, 'vd-mini');
    pvwHead.append(D.ui.takeVerse, D.ui.clearStage);
    pvw.append(pvwHead, buildFrame('pvw'));
    // Controls
    const ctl = vEl('div', 'vd-ctl');
    ctl.setAttribute('role', 'group');
    ctl.setAttribute('aria-label', 'Transition');
    D.ui.cut = vBtn('Cut', 'Put the Preview source on Program at once', () => take('cut'), 'vd-take');
    D.ui.fade = vBtn('Fade', 'Fade the Preview source onto Program', () => take('fade'), 'vd-take');
    ctl.append(D.ui.cut, D.ui.fade);
    // Program
    const pgm = vEl('div', 'vd-screen vd-screen-pgm');
    const pgmHead = vEl('div', 'vd-screen-head');
    pgmHead.appendChild(vEl('span', 'vd-tag vd-tag-pgm', 'Program'));
    D.ui.pgmName = vEl('span', 'vd-ref');
    pgmHead.appendChild(D.ui.pgmName);
    pgm.append(pgmHead, buildFrame('pgm'));
    D.ui.pgm = pgm;
    screens.append(pvw, ctl, pgm);

    band.appendChild(screens);
    center.prepend(band);
    D.root = band;
    return true;
  }

  // Something is on air: a live source, or a verse marked live in the queue.
  function isOnAir() {
    const ps = D.programState;
    if (ps.on.length && (ps.mixed || ps.id)) return true;
    return !!document.querySelector('#current-display-card .is-live');
  }

  function refresh() {
    if (!D.active) return;
    if (window.KairoVideoDock) window.KairoVideoDock.refresh();
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
    D.ui.pgm.classList.toggle('is-live', isOnAir());

    // Preview header
    const pvwSource = sources.find(s => s.id === D.previewId);
    const staged = D.staged ? (D.staged.verse.reference || D.staged.verse.ref || 'Verse') : '';
    D.ui.pvwRef.textContent = staged ? `${pvwSource ? pvwSource.name : 'No video'} · ${staged}` : (pvwSource ? pvwSource.name : 'No video');
    D.ui.takeVerse.hidden = !D.staged;
    D.ui.clearStage.hidden = !D.staged;
    const canTake = ps.on.length > 0;
    const cur = transitionPref();
    for (const [btn, mode] of [[D.ui.cut, 'cut'], [D.ui.fade, 'fade']]) {
      btn.disabled = !canTake;
      btn.classList.toggle('is-current', mode === cur);
      btn.setAttribute('aria-pressed', String(mode === cur));
      btn.title = canTake ? (mode === 'cut' ? 'Put the Preview source on Program at once' : 'Fade the Preview source onto Program') : 'Turn on Video input for an output in Director mode settings';
    }
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

  async function start() {
    if (D.active) return;
    if (!build()) return;
    D.active = true;
    wrapSend();
    publishTransition();
    await startVideoSources();     // the Sources pane, device list, published state
    window.KairoVideoLayout?.start();
    window.KairoVideoDock?.mount();
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
    unwrapSend();
    D.staged = null;   // a staged verse is dropped, never sent: the screen keeps what it shows
    D.liveObserver?.disconnect();
    unmountSettings();
    window.KairoVideoDock?.unmount();
    window.KairoVideoLayout?.stop();
    // Going off air fades, without changing the transition the operator chose.
    try { localStorage.setItem('kairo-video-transition', JSON.stringify('fade')); } catch {}
    setVideoLiveFor(videoOutputs().map(o => o.id), null);
    setTimeout(() => { try { localStorage.removeItem('kairo-video-transition'); } catch {} }, 1500);
    Object.values(D.frames).forEach(f => { f.ro.disconnect(); f.frame.src = 'about:blank'; });
    D.frames = {};
    stopVideoSources();
    window.KairoVideoHub?.releaseAll();
    D.root?.remove();
    D.root = null;
    try { localStorage.setItem('kairo-video-live', '{}'); } catch {}
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
