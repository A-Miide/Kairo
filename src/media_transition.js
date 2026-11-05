// How pictures and videos come on screen (the Media Bin's Transition): Fade (the default),
// Slide or Cut. ("Same as the words" was removed, owner 2026-10-18; a saved one reads as Fade.) Kept in
// settings.mediaTransition and handed to the output windows through KairoTransitions
// (output_transitions.js), as the Director's camera transition is.
'use strict';
(function () {
  const sel = () => document.getElementById('media-bin-transition');
  const publish = (v) => KairoTransitions.media(v);
  function apply() {
    const v = (typeof settings === 'object' && settings && settings.mediaTransition) || 'fade';
    const s = sel();
    if (s && s.value !== v) s.value = v;
    publish(v);
  }
  document.addEventListener('change', (e) => {
    if (e.target !== sel()) return;
    const v = sel().value;
    settings.mediaTransition = v;
    saveSettingsPatch({ mediaTransition: v });
    publish(v);
  });
  // Settings arrive after this file loads.
  onSettingsChange((d) => { if (d.reason === 'loaded') apply(); });
  if (typeof settingsLoaded !== 'undefined' && settingsLoaded) apply();
})();
