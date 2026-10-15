// KAIRO — Video director mode: the Preview / Program band.
//
// Lazy-loaded by video_director_boot.js while the mode is on, and removed
// again when it is turned off. The band sits at the top of the centre column
// and nothing else in the layout moves: the sidebars, Live Queue, Possible
// Matches, the range bar, the Monitor message bar and the listen button are
// the normal ones.
//
//   PROGRAM  what the outputs show now: a live copy of the output's own page
//            (display.html?monitor=1&shared=1), fed by the same socket.
//   PREVIEW  a staging screen: the same page (display.html?preview=1), fed by
//            this window, showing the next source and a staged verse. Both
//            screens draw scripture over video with display.html's own code.
//
// CUT / FADE put the Preview source on Program and swap the old Program
// source onto Preview. A source change never touches the verse on screen.
// Manual sends and Possible Matches are staged on Preview (sendVerseToServer
// is wrapped while the mode is on) and TAKE sends them to Program; verses the
// detection engine sends by itself go live exactly as they do without the
// mode. Slide and timer sends are not staged.
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
    leases: [],
    visibleObserver: null,
    pgmOutput: null,
  };

  const stageManual = () => settings.videoStageManual !== false;
  const transitionPref = () => (settings.videoTransition === 'cut' ? 'cut' : 'fade');

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
    try { localStorage.setItem('kairo-video-transition', JSON.stringify(mode)); } catch {}
  }
  function take(mode) {
    const ps = programState();
    if (!ps.on.length) return;
    setTransition(mode);                       // written first: the display reads it when the source changes
    const outgoing = ps.mixed ? D.previewId : ps.id;
    setVideoLiveFor(ps.on.map(o => o.id), D.previewId);
    D.previewId = outgoing;                    // the old Program source is staged next
    syncPreviewFrame();
    refresh();
  }
  function setPreview(id) {
    D.previewId = id || null;
    syncPreviewFrame();
    refresh();
  }

  // ── Band ─────────────────────────────────────────────────────────────────
  function build() {
    const center = document.querySelector('main.center-stage');
    if (!center) return false;
    const band = vEl('section', 'vd-band');
    band.id = 'vd-band';
    band.setAttribute('aria-label', 'Video director');

    const screens = vEl('div', 'vd-screens');
    // Preview
    const pvw = vEl('div', 'vd-screen');
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
    D.ui.fade = vBtn('Fade', 'Fade the Preview source onto Program', () => take('fade'), 'vd-take vd-take-primary');
    ctl.append(D.ui.cut, D.ui.fade);
    // Program
    const pgm = vEl('div', 'vd-screen');
    const pgmHead = vEl('div', 'vd-screen-head');
    pgmHead.appendChild(vEl('span', 'vd-tag vd-tag-pgm', 'Program'));
    D.ui.pgmName = vEl('span', 'vd-ref');
    pgmHead.appendChild(D.ui.pgmName);
    pgm.append(pgmHead, buildFrame('pgm'));
    screens.append(pvw, ctl, pgm);

    // Sources strip
    D.ui.sources = vEl('div', 'vd-sources');
    D.ui.sources.setAttribute('role', 'listbox');
    D.ui.sources.setAttribute('aria-label', 'Sources for Preview');

    // Outputs + staging
    D.ui.outputs = vEl('div', 'vd-outputs');

    band.append(screens, D.ui.sources, D.ui.outputs);
    center.prepend(band);
    D.root = band;
    return true;
  }

  function tile(id, name, status, thumbSource) {
    const ps = D.programState;
    const t = vEl('div', 'vd-tile'
      + (D.previewId === id ? ' is-pvw' : '')
      + (!ps.mixed && ps.id === id && ps.on.length ? ' is-pgm' : ''));
    t.tabIndex = 0;
    t.setAttribute('role', 'option');
    t.setAttribute('aria-selected', String(D.previewId === id));
    const thumb = vEl('div', 'vd-thumb');
    if (thumbSource) {
      const v = vEl('video'); v.muted = true; v.playsInline = true; v.setAttribute('aria-hidden', 'true');
      thumb.appendChild(v);
      const devId = previewKey(thumbSource);
      if (devId && D.thumbsOn) {
        D.leases.push(window.KairoVideoHub.lease(devId, 'thumb', (stream) => {
          if (!v.isConnected) return;
          v.srcObject = stream; v.play().catch(() => {});
        }, () => { thumb.dataset.msg = 'Unavailable'; }));
      } else thumb.dataset.msg = devId ? '' : 'Not connected';
    } else thumb.classList.add('vd-thumb-none');
    const label = vEl('div', 'vd-tile-name', name);
    t.append(thumb, label);
    if (status) t.appendChild(vEl('span', 'vd-flag ' + (status === 'PGM' ? 'vd-flag-pgm' : 'vd-flag-pvw'), status));
    const choose = () => setPreview(id);
    t.addEventListener('click', choose);
    t.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(); } });
    return t;
  }

  function refresh() {
    if (!D.active || !D.root) return;
    const sources = videoSourceList();
    if (D.previewId && !sources.some(s => s.id === D.previewId)) D.previewId = null;
    const ps = programState();
    D.programState = ps;

    // Program screen follows the first output that carries video, else the primary.
    const pgmOut = (ps.on[0] && ps.on[0].id) || PRIMARY_DISPLAY;
    setFrameSrc('pgm', `/display.html?output=${encodeURIComponent(pgmOut)}&monitor=1&shared=1`);
    setFrameSrc('pvw', `/display.html?output=${encodeURIComponent(PRIMARY_DISPLAY)}&preview=1`);
    const pgmSource = ps.mixed ? null : sources.find(s => s.id === ps.id);
    D.ui.pgmName.textContent = !ps.on.length ? 'Video off on every output' : ps.mixed ? 'Mixed' : pgmSource ? pgmSource.name : 'No video';

    // Preview header
    const pvwSource = sources.find(s => s.id === D.previewId);
    const staged = D.staged ? (D.staged.verse.reference || D.staged.verse.ref || 'Verse') : '';
    D.ui.pvwRef.textContent = staged ? `${pvwSource ? pvwSource.name : 'No video'} · ${staged}` : (pvwSource ? pvwSource.name : 'No video');
    D.ui.takeVerse.hidden = !D.staged;
    D.ui.clearStage.hidden = !D.staged;
    const canTake = ps.on.length > 0;
    D.ui.cut.disabled = !canTake;
    D.ui.fade.disabled = !canTake;
    D.ui.cut.title = canTake ? 'Put the Preview source on Program at once' : 'Turn Video input on for an output first';
    D.ui.fade.title = canTake ? 'Fade the Preview source onto Program' : 'Turn Video input on for an output first';

    // Sources strip: thumbnails take a fresh lease before the old ones go, so
    // the hub keeps each camera open across the redraw.
    const oldLeases = D.leases;
    D.leases = [];
    const strip = D.ui.sources;
    strip.replaceChildren();
    strip.appendChild(tile('', 'No video', (!ps.mixed && !ps.id && ps.on.length) ? 'PGM' : (D.previewId == null ? 'PVW' : ''), null));
    sources.forEach(s => {
      const flag = (!ps.mixed && ps.id === s.id && ps.on.length) ? 'PGM' : (D.previewId === s.id ? 'PVW' : '');
      strip.appendChild(tile(s.id, s.name, flag, s));
    });
    strip.appendChild(vBtn('Manage sources', 'Add, rename or remove sources', () => {
      document.getElementById('settings-modal')?.classList.remove('hidden');
      if (typeof showSettingsPane === 'function') showSettingsPane('sources');
    }, 'vd-mini vd-manage'));
    oldLeases.forEach(release => release());
    if (!sources.length) strip.appendChild(vEl('p', 'vd-empty', 'No sources yet. Choose Manage sources, then Detect devices.'));

    // Outputs row
    const out = D.ui.outputs;
    out.replaceChildren();
    out.appendChild(vEl('span', 'vd-label', 'Video on'));
    const layers = outputLayerMap();
    videoOutputs().forEach(o => {
      const on = !!(layers[o.id] && layers[o.id].video);
      const chip = vBtn((on ? '✓ ' : '') + o.name, `${on ? 'Turn off' : 'Turn on'} video input on ${o.name}`, () => setOutputLayer(o.id, 'video', !on), 'vd-chip' + (on ? ' is-on' : ''));
      chip.setAttribute('aria-pressed', String(on));
      out.appendChild(chip);
    });
    const stageLabel = vEl('label', 'vd-stage-toggle');
    const cb = vEl('input'); cb.type = 'checkbox'; cb.checked = stageManual();
    cb.addEventListener('change', () => {
      settings.videoStageManual = cb.checked;
      saveSettingsPatch({ videoStageManual: cb.checked });
    });
    stageLabel.append(cb, document.createTextNode('Stage manual sends on Preview'));
    stageLabel.title = 'Verses you send by hand wait on Preview until you take them. Detected verses always go straight to Program.';
    out.appendChild(stageLabel);
    out.appendChild(vEl('p', 'vd-note', 'NDI, Syphon and DeckLink outputs do not carry video input yet.'));
  }

  // Thumbnails run only while the band is on screen and the window is visible.
  function watchVisibility() {
    const update = (shown) => {
      const on = shown && !document.hidden;
      if (on === D.thumbsOn) return;
      D.thumbsOn = on;
      refresh();
    };
    D.shown = true;
    D.visibleObserver = new IntersectionObserver((es) => { D.shown = es.some(e => e.isIntersecting); update(D.shown); });
    D.visibleObserver.observe(D.root);
    D.onVis = () => update(D.shown);
    document.addEventListener('visibilitychange', D.onVis);
  }

  async function start() {
    if (D.active) return;
    if (!build()) return;
    D.active = true;
    document.body.classList.add('video-director');
    D.thumbsOn = !document.hidden;
    wrapSend();
    await startVideoSources();     // the Sources pane, device list, published state
    watchVisibility();
    refresh();
    // A Preview frame needs its page up before it can be fed.
    D.frames.pvw.frame.addEventListener('load', syncPreviewFrame);
  }

  // Mode off: the verse on screen stays; the video goes to nothing with a
  // fade, every capture is released and the band comes out of the layout.
  function stop() {
    if (!D.active) return;
    D.active = false;
    unwrapSend();
    D.staged = null;   // a staged verse is dropped, never sent: the screen keeps what it shows
    setTransition('fade');
    setVideoLiveFor(videoOutputs().map(o => o.id), null);
    D.visibleObserver?.disconnect();
    document.removeEventListener('visibilitychange', D.onVis);
    D.leases.forEach(release => release()); D.leases = [];
    Object.values(D.frames).forEach(f => { f.ro.disconnect(); f.frame.src = 'about:blank'; });
    D.frames = {};
    stopVideoSources();
    window.KairoVideoHub?.releaseAll();
    D.root?.remove();
    D.root = null;
    document.body.classList.remove('video-director');
    try { localStorage.setItem('kairo-video-live', '{}'); localStorage.removeItem('kairo-video-transition'); } catch {}
  }

  // Is video on air right now? (the toggle asks before taking it off)
  function isLive() { return programState().on.some(o => videoLiveMap()[o.id]); }

  window.KairoVideoDirector = { start, stop, refresh, isLive };
})();
