// KAIRO — what the window is holding, written to the debug log.
//
// The window's web process runs to a few hundred MB, most of it graphics memory (decoded images,
// canvases, video frames) that no JavaScript counter shows. Every two minutes this adds up what
// the page has on the screen's behalf: pictures and their pixels, canvases, video elements, layers
// that blur what is behind them. Read it with `grep client:window-memory databases/debug.log`
// next to `footprint <web content pid>`. It sends nothing anywhere and shows nothing.
'use strict';

(function () {
  const MB = n => Math.round(n / 1048576 * 10) / 10;

  function probe() {
    const all = document.getElementsByTagName('*');
    let imgs = 0, imgPx = 0, bg = 0, blur = 0, hidden = 0;
    const big = [];
    for (const el of all) {
      if (el.tagName === 'IMG' && el.complete && el.naturalWidth) {
        imgs++;
        const px = el.naturalWidth * el.naturalHeight;
        imgPx += px;
        if (px > 1e6) big.push(`${(el.currentSrc || el.src || '').split('/').slice(-2).join('/').slice(0, 40)} ${el.naturalWidth}x${el.naturalHeight}`);
      }
      const cs = el.tagName === 'SCRIPT' || el.tagName === 'STYLE' ? null : getComputedStyle(el);
      if (!cs) continue;
      if (cs.backgroundImage && cs.backgroundImage.includes('url(')) bg++;
      if ((cs.backdropFilter && cs.backdropFilter !== 'none') || (cs.webkitBackdropFilter && cs.webkitBackdropFilter !== 'none')) blur++;
      if (cs.display === 'none') hidden++;
    }
    let canvases = 0, canvasPx = 0;
    for (const c of document.getElementsByTagName('canvas')) { canvases++; canvasPx += c.width * c.height; }
    let videos = 0, playing = 0, videoPx = 0;
    for (const v of document.getElementsByTagName('video')) {
      videos++;
      if (!v.paused && !v.ended) playing++;
      videoPx += (v.videoWidth || 0) * (v.videoHeight || 0);
    }
    const heap = performance.memory && performance.memory.usedJSHeapSize;
    return {
      elements: all.length, hidden,
      images: imgs, imageMB: MB(imgPx * 4), bigImages: big.slice(0, 5),
      backgroundImages: bg, blurLayers: blur,
      canvases, canvasMB: MB(canvasPx * 4),
      videos, playing, videoFrameMB: MB(videoPx * 1.5),
      jsHeapMB: heap == null ? null : MB(heap),
      mode: document.body.className.split(/\s+/).filter(c => /director|mode/i.test(c)).join(' ') || null,
    };
  }

  function log() {
    try { if (window.kairoLog) window.kairoLog('window-memory', probe()); } catch {}
  }

  // What the director's Preview and Output frames hold, for finding why a picture isn't showing:
  // each frame's address, whether its page has a video playing, and the director's own idea of the
  // Preview and Output sources. Written every 15 s for the first ten minutes, then every two.
  function frames() {
    const out = [];
    for (const f of document.querySelectorAll('iframe')) {
      try {
        const d = f.contentDocument;
        out.push({
          src: (f.getAttribute('src') || '').replace(/^.*display\.html/, ''),
          drawn: Math.round(f.getBoundingClientRect().width), laidOut: f.offsetWidth,
          hasVideo: d.body.classList.contains('has-video'),
          videos: [...d.querySelectorAll('video')].map(v => ({ ready: v.readyState, w: v.videoWidth, paused: v.paused, stream: !!v.srcObject })),
          zone: !!d.querySelector('#video-layer[data-zone]'),
          slide: (() => { const sl = d.getElementById('slide-layer'); if (!sl) return null; const r = sl.getBoundingClientRect(); return { text: (sl.innerText || '').replace(/\s+/g, ' ').slice(0, 40), w: Math.round(r.width), h: Math.round(r.height), opacity: getComputedStyle(sl).opacity }; })(),
          clock: (d.getElementById('clock-badge') || {}).className === 'hidden' ? null : ((d.getElementById('clock-badge') || {}).textContent || '').slice(0, 20),
        });
      } catch { out.push({ src: (f.getAttribute('src') || '').slice(0, 60), unreadable: true }); }
    }
    let director = null;
    try { director = window.KairoVideoDirector && window.KairoVideoDirector.state && window.KairoVideoDirector.state(); } catch {}
    return { frames: out, director };
  }
  function logFrames() { try { if (window.kairoLog && document.querySelector('iframe')) window.kairoLog('director-frames', frames()); } catch {} }

  window.KairoMemoryProbe = { probe, frames };
  setTimeout(log, 20000);
  setInterval(log, 120000);
  let early = 0;
  const earlyTimer = setInterval(() => { logFrames(); if (++early >= 40) { clearInterval(earlyTimer); setInterval(logFrames, 120000); } }, 15000);
})();
