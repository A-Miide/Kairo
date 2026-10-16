// How pictures and videos come on screen (the Media Bin's Transition): Fade, Slide, Cut, or
// the same as the words (the Bible/slides transition, the default). Kept in
// settings.mediaTransition and handed to the output windows through shared storage
// ('kairo-media-transition'), as the Director's camera transition is.
'use strict';
(function () {
  const KEY = 'kairo-media-transition';
  const sel = () => document.getElementById('media-bin-transition');
  function publish(v) {
    try { if (v) localStorage.setItem(KEY, JSON.stringify({ animation: v })); else localStorage.removeItem(KEY); } catch {}
  }
  function apply() {
    const v = (typeof settings === 'object' && settings && settings.mediaTransition) || '';
    const s = sel();
    if (s && s.value !== v) s.value = v;
    publish(v);
  }
  document.addEventListener('change', (e) => {
    if (e.target !== sel()) return;
    const v = sel().value;
    settings.mediaTransition = v || null;
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ mediaTransition: v || null });
    publish(v);
  });
  // Settings arrive after this file loads.
  let tries = 0;
  const wait = setInterval(() => { tries++; if ((typeof settingsLoaded !== 'undefined' && settingsLoaded) || tries > 60) { clearInterval(wait); apply(); } }, 500);
})();
