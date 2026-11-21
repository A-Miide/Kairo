// KAIRO — a video taken off the page lets go of its decoder and frames.
//
// WebKit keeps a removed <video> playing, with its decoder and decoded frames, until garbage
// collection happens to reach it. Flipping through the media bin on 4.7.1 (21 Nov 2026) left one
// such video behind per send, in the preview and in each Monitor tile, until the main window
// passed macOS's memory limit (19.7 GB, mostly 1600x900 frames) and was reloaded.
//
// So every page watches for removed elements: a video (or audio) that is no longer in the page
// once the removal has settled is paused and emptied. One moved elsewhere (removed and put back
// in the same task, as insertBefore does) is connected again by then and is left alone.
(function (root) {
  'use strict';

  // Stops a media element and frees what it holds.
  function release(el) {
    try { el.pause(); } catch {}
    try { if (el.srcObject) el.srcObject = null; } catch {}
    if (el.hasAttribute && el.hasAttribute('src')) {
      el.removeAttribute('src');
      try { el.load(); } catch {}   // drops the decoder and its buffered frames now
    }
  }

  // The media elements in a removed node (itself or below it).
  function mediaIn(node) {
    if (!node || node.nodeType !== 1) return [];
    const tag = node.tagName;
    if (tag === 'VIDEO' || tag === 'AUDIO') return [node];
    if (!node.getElementsByTagName) return [];
    return [...node.getElementsByTagName('video'), ...node.getElementsByTagName('audio')];
  }

  // A MutationObserver callback: releases removed media that stayed out of the page.
  function onMutations(records) {
    for (const r of records) {
      for (const n of r.removedNodes) {
        for (const el of mediaIn(n)) if (!el.isConnected) release(el);
      }
    }
  }

  function watch(doc) {
    if (!doc || typeof MutationObserver !== 'function') return null;
    const target = doc.documentElement || doc;
    const mo = new MutationObserver(onMutations);
    mo.observe(target, { childList: true, subtree: true });
    return mo;
  }

  const api = { release, mediaIn, onMutations, watch };
  root.KairoMediaRelease = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else if (typeof document !== 'undefined') watch(document);
})(typeof globalThis !== 'undefined' ? globalThis : this);
