// KAIRO — an interpreter in the room (main window).
//
// The server tells when the interpreter is speaking (server: interpretation.rs). Here:
// - the transcript shows "(interpretation)" once for each turn, never the garbled words;
// - after two turns taken (the preacher, the interpreter, the preacher again — not tongues, not
//   applause), Kairo offers the Multi-Language theme, with the interpreter's language when it
//   is one it can translate to (French, Spanish, Portuguese). One offer per listening session,
//   and never when Multi-Language is already on the screen.
'use strict';
(function () {
  const LANGUAGES = [{ code: 'es', name: 'Spanish' }, { code: 'fr', name: 'French' }, { code: 'pt', name: 'Portuguese' }];
  let marked = false, turns = 0, offered = false, active = false;

  // Called with every transcript message: 0 = the preacher's (carry on), 1 = the first words of
  // an interpreter's turn (the transcript marks it), 2 = more of the same turn (nothing to show).
  function transcript(msg) {
    if (!msg || !msg.interpretation) { marked = false; return 0; }
    if (marked) return 2;
    marked = true;
    return 1;
  }

  function primaryTheme() {
    try { return typeof outputThemeMap === 'function' ? outputThemeMap()[typeof PRIMARY_DISPLAY !== 'undefined' ? PRIMARY_DISPLAY : 'display-1'] : null; } catch { return null; }
  }

  function useMultiLanguage(code) {
    if (typeof settings !== 'object' || !settings) return;
    settings.bibleTranslateTo = code;
    settings.interpreterLanguage = code;
    const key = typeof PRIMARY_DISPLAY !== 'undefined' ? PRIMARY_DISPLAY : 'display-1';
    settings.outputThemes = { ...(typeof outputThemeMap === 'function' ? outputThemeMap() : {}), [key]: 'multi-language' };
    if (typeof saveSettingsPatch === 'function') saveSettingsPatch({ bibleTranslateTo: code, interpreterLanguage: code, outputThemes: settings.outputThemes });
    if (typeof applyOutputThemes === 'function') applyOutputThemes();
  }

  function offer() {
    if (offered || typeof window.kairoSuggest !== 'function') return;
    if (primaryTheme() === 'multi-language') return;
    offered = true;
    const known = typeof settings === 'object' && settings && LANGUAGES.find((l) => l.code === settings.interpreterLanguage);
    const actions = known
      ? [{ label: `Use Multi-Language (${known.name})`, run: () => useMultiLanguage(known.code) }]
      : LANGUAGES.map((l) => ({ label: l.name, run: () => useMultiLanguage(l.code) }));
    window.kairoSuggest(
      known ? 'An interpreter is speaking. Show the verses in both languages?' : 'An interpreter is speaking. Show the verses in both languages — which language?',
      actions, 'interpreter');
  }

  function onMessage(msg) {
    if (!msg) return;
    if (msg.active) { active = true; return; }
    if (active) { active = false; turns += 1; if (turns >= 2) offer(); }
  }

  // A new listening session starts the count again.
  function reset() { marked = false; turns = 0; offered = false; active = false; }

  const style = document.createElement('style');
  style.textContent = '.transcript-interp { color: var(--text-2, var(--text)); opacity: .6; font-style: italic; }';
  document.head.appendChild(style);

  window.KairoInterpreter = { transcript, onMessage, reset };
})();
