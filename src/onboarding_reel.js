// "Welcome" in every language Kairo carries a Bible for (the nine of Settings → Bible language, app_outputs.js
// LANG_PACKS), one word at a time and quickly: at the top of the welcome step and in the "Every language in the
// room" tile. Any element marked data-welcome-cycle is filled and kept in step; with reduced motion the first
// word (English) stays put.
(() => {
  const WELCOME = [
    ['en', 'Welcome'], ['es', 'Bienvenido'], ['pt', 'Bem-vindo'], ['fr', 'Bienvenue'], ['de', 'Willkommen'],
    ['yo', 'Ẹ kú àbọ̀'], ['ig', 'Nnọ̀ọ'], ['ha', 'Barka da zuwa'], ['sw', 'Karibu'],
  ];
  const STEP_MS = 1000;
  const hosts = [...document.querySelectorAll('[data-welcome-cycle]')];
  if (!hosts.length) return;
  const rows = hosts.map((host) => WELCOME.map(([lang, word]) => {
    const w = document.createElement('span');
    w.className = 'obw';
    w.lang = lang;
    w.setAttribute('aria-hidden', 'true');
    w.textContent = word;
    host.appendChild(w);
    return w;
  }));
  let i = 0;
  const show = () => rows.forEach((row) => row.forEach((w, k) => w.classList.toggle('is-on', k === i)));
  show();
  const still = window.matchMedia('(prefers-reduced-motion: reduce)');
  setInterval(() => {
    if (still.matches || !hosts.some((h) => h.offsetParent !== null)) return; // nobody is looking
    i = (i + 1) % WELCOME.length;
    show();
  }, STEP_MS);
})();

// The welcome step's feature reel: plays its chapters in turn (each about
// 4.5 s, or its own data-hold), with a rail of segments that fill as they play and jump when
// clicked. Pauses while the welcome step is out of view and starts again
// from the first chapter when it comes back. With reduced motion nothing
// advances on its own; the rail still moves between chapters.
(() => {
  const reel = document.querySelector('.ob-reel');
  if (!reel) return;
  const scenes = [...reel.querySelectorAll('.obr-scene')];
  const rail = reel.querySelector('.obr-rail');
  const caption = reel.querySelector('.obr-caption');
  const title = reel.querySelector('.obr-title');
  const sub = reel.querySelector('.obr-sub');
  const HOLD_MS = 4500;
  const holdOf = (i) => Number(scenes[i].dataset.hold) || HOLD_MS;
  const still = window.matchMedia('(prefers-reduced-motion: reduce)');

  const bars = scenes.map((scene, i) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'obr-seg';
    b.setAttribute('aria-label', scene.dataset.title || `Chapter ${i + 1}`);
    b.style.setProperty('--obr-hold', `${holdOf(i)}ms`);
    b.appendChild(document.createElement('i'));
    b.addEventListener('click', () => show(i));
    rail.appendChild(b);
    return b;
  });

  let current = -1;
  let timer = null;
  let wasVisible = false;

  function show(n) {
    current = (n + scenes.length) % scenes.length;
    scenes.forEach((s, i) => s.classList.toggle('is-on', i === current));
    bars.forEach((b, i) => {
      b.classList.toggle('is-done', i < current);
      b.classList.remove('is-on');
    });
    void rail.offsetWidth; // restart the playing segment's fill
    bars[current].classList.add('is-on');
    title.textContent = scenes[current].dataset.title || '';
    sub.textContent = scenes[current].dataset.sub || '';
    caption.classList.remove('is-in');
    void caption.offsetWidth;
    caption.classList.add('is-in');
    schedule();
  }

  function schedule() {
    clearTimeout(timer);
    reel.classList.toggle('is-still', still.matches);
    if (still.matches) return;
    timer = setTimeout(tick, holdOf(current));
  }

  function tick() {
    if (reel.offsetParent === null) { wasVisible = false; timer = setTimeout(watch, 600); return; }
    show(current + 1);
  }

  // While the welcome step is hidden, look in now and then; begin again
  // from the top when it is shown.
  function watch() {
    if (reel.offsetParent === null) { timer = setTimeout(watch, 600); return; }
    if (!wasVisible) { wasVisible = true; show(0); }
  }

  show(0);
  wasVisible = reel.offsetParent !== null;
  if (!wasVisible) { clearTimeout(timer); timer = setTimeout(watch, 600); }
})();
