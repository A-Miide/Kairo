// KAIRO — how a change comes on screen, published from one place.
//
// Three transitions exist, each chosen somewhere different, and they reached the output
// windows by three hand-rolled routes (a settings POST, and two sets of localStorage writes
// scattered across video_director.js and media_transition.js). They all publish here now.
// Where each lands is unchanged, so display.html reads exactly what it did:
//   words    Bible verses and slides: the output's Transition (Fade / Slide / Cut and its
//            length). Display-level, so it is saved as settings and the server broadcasts it
//            to every output as 'output-transition'.
//   media    pictures and videos from the Media Bin: its own transition, or the words'.
//            localStorage 'kairo-media-transition' = {animation}.
//   video    the Director's camera cut or fade: localStorage 'kairo-video-transition'
//            = 'cut' | 'fade', its length in 'kairo-video-fade' (ms).
// Passing null takes a local one away, so the output falls back to the words' transition.
'use strict';
const KairoTransitions = (() => {
  const KEYS = { media: 'kairo-media-transition', video: 'kairo-video-transition', videoFade: 'kairo-video-fade' };
  const write = (key, value) => {
    try { if (value == null) localStorage.removeItem(key); else localStorage.setItem(key, JSON.stringify(value)); } catch {}
  };
  return {
    KEYS,
    // { animation?: 'fade' | 'slide-up' | 'cut', speed?: number }
    words({ animation, speed } = {}) {
      const patch = {};
      if (animation) patch.outputAnimation = animation;
      if (typeof speed === 'number') patch.outputAnimationSpeed = speed;
      return saveSettingsPatch(patch);
    },
    media(animation) { write(KEYS.media, animation ? { animation } : null); },
    video(mode) { write(KEYS.video, mode || null); },
    videoFade(ms) { write(KEYS.videoFade, ms == null ? null : ms); },
  };
})();
