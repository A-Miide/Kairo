// KAIRO — an interpreter in the room (main window).
//
// The server tells when the interpreter is speaking (server: interpretation.rs). Here the
// transcript shows "(interpretation)" once for each turn, never the garbled words.
//
// It used to offer the Multi-Language theme after two turns; the owner turned that off (22 Nov
// 2026): it guessed an interpreter in an English-only service and the prompt sat over the media
// bin during the service. Multi-Language is chosen in Settings when a church wants it.
'use strict';
(function () {
  let marked = false;

  // Called with every transcript message: 0 = the preacher's (carry on), 1 = the first words of
  // an interpreter's turn (the transcript marks it), 2 = more of the same turn (nothing to show).
  function transcript(msg) {
    if (!msg || !msg.interpretation) { marked = false; return 0; }
    if (marked) return 2;
    marked = true;
    return 1;
  }

  // A new listening session starts again.
  function reset() { marked = false; }

  const style = document.createElement('style');
  style.textContent = '.transcript-interp { color: var(--text-2, var(--text)); opacity: .6; font-style: italic; }';
  document.head.appendChild(style);

  window.KairoInterpreter = { transcript, reset };
})();
