// KAIRO — is the input usable while listening? Fed every level report (about ten a second from
// the app's capture, six from the server's), it says when the input has gone silent or is
// clipping, so the operator sees it on screen instead of finding out from verses that never come.
//
// The church PC's 15 Nov 2026 service is what this is for: a USB adapter whose feed fell from full
// scale to hiss for three minutes (frames kept arriving, so nothing looked disconnected), and an
// input clipping for most of the service.
//   - Silent: the raw input (before the level control, which raises a quiet feed up to +30 dB)
//     stays under SILENT_PEAK for SILENT_MS, or no report arrives at all for that long. An
//     ordinary pause between sentences is shorter; it clears on the first report with sound.
//   - Clipping: at least CLIP_REPORTS clipped reports within CLIP_WINDOW_MS (one shout is not
//     enough); it clears after CLEAR_CLIP_MS with none.
// Silent wins over clipping: a feed that clipped and then died is silent now.
(function (root) {
  'use strict';
  const SILENT_PEAK = 0.003;       // about -50 dBFS: hiss, not a voice
  const SILENT_MS = 8000;
  const CLIP_WINDOW_MS = 5000;
  const CLIP_REPORTS = 5;
  const CLEAR_CLIP_MS = 8000;

  class InputHealth {
    constructor() { this.reset(0); }
    // A new listening session (or a rebuilt capture): nothing is known yet, so nothing is wrong.
    reset(now) {
      this.since = now;
      this.lastSound = now;
      this.lastReport = now;
      this.clips = [];
      this.lastClip = 0;
      this._wasClipping = false;
    }
    // One level report: `peak` is the raw input, 0..1; `clipped` whether it hit full scale.
    note(peak, clipped, now) {
      this.lastReport = now;
      if (peak >= SILENT_PEAK) this.lastSound = now;
      if (clipped) {
        this.clips.push(now);
        this.lastClip = now;
      }
      while (this.clips.length && this.clips[0] < now - CLIP_WINDOW_MS) this.clips.shift();
      return this.state(now);
    }
    // 'silent' | 'clipping' | 'ok', as of `now` (also called on a timer, for reports that stop).
    state(now) {
      if (now - Math.max(this.lastSound, this.since) >= SILENT_MS) return 'silent';
      const clipping = this.clips.length >= CLIP_REPORTS || (this.lastClip > 0 && this._wasClipping && now - this.lastClip < CLEAR_CLIP_MS);
      this._wasClipping = clipping;
      return clipping ? 'clipping' : 'ok';
    }
  }

  // What the operator reads: short, and what to do about it.
  const MESSAGES = {
    silent: 'No sound from the input. Check the mic or cable.',
    clipping: 'Input too loud: it is clipping. Turn it down.',
  };

  const api = { InputHealth, MESSAGES, SILENT_PEAK, SILENT_MS, CLIP_WINDOW_MS, CLIP_REPORTS, CLEAR_CLIP_MS };
  root.KairoInputHealth = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
