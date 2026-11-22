// KAIRO — is the input clipping while listening? Fed every level report (about ten a second from
// the app's capture, six from the server's), it says when the input is too hot, so the operator
// turns it down instead of finding out from words the engine can't make out.
//
// The church PC's 15 Nov 2026 service clipped for most of the service. Clipping: at least
// CLIP_REPORTS clipped reports within CLIP_WINDOW_MS (one shout is not enough); it clears after
// CLEAR_CLIP_MS with none. Silence is not reported here: the "No audio!" pill in Live Status
// already says it (owner, 22 Nov 2026: a second line for it wasn't needed).
(function (root) {
  'use strict';
  const CLIP_WINDOW_MS = 5000;
  const CLIP_REPORTS = 5;
  const CLEAR_CLIP_MS = 8000;

  class InputHealth {
    constructor() { this.reset(0); }
    // A new listening session (or a rebuilt capture): nothing is known yet, so nothing is wrong.
    reset() {
      this.clips = [];
      this.lastClip = 0;
      this.clipping = false;
    }
    // One level report: `clipped` whether it hit full scale.
    note(peak, clipped, now) {
      if (clipped) {
        this.clips.push(now);
        this.lastClip = now;
      }
      return this.state(now);
    }
    // 'clipping' | 'ok', as of `now` (also called on a timer, for reports that stop).
    state(now) {
      while (this.clips.length && this.clips[0] < now - CLIP_WINDOW_MS) this.clips.shift();
      this.clipping = this.clips.length >= CLIP_REPORTS || (this.clipping && now - this.lastClip < CLEAR_CLIP_MS);
      return this.clipping ? 'clipping' : 'ok';
    }
  }

  // What the operator reads: short, and what to do about it.
  const MESSAGES = {
    clipping: 'Input too loud: it is clipping. Turn it down.',
  };

  const api = { InputHealth, MESSAGES, CLIP_WINDOW_MS, CLIP_REPORTS, CLEAR_CLIP_MS };
  root.KairoInputHealth = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
