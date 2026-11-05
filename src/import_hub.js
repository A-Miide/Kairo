// Import hub (#import-options-modal): lists every format Kairo reads and
// routes a picked or dropped file by its extension. Content goes through
// KairoService.quickImportFile (playlist or Song Library), themes through
// Theme Studio's own importer (#import-look-file).
(function () {
  const modal = document.getElementById('import-options-modal');
  if (!modal) return;

  const KINDS = {
    propresenter: ['pro', 'pro6', 'pro7', 'probundle', 'proplaylist'],
    // A schedule on its own, or a song library's two files picked together (easyworship.rs).
    easyworship: ['ewsx', 'ews', 'db', 'mb'],
    pptx: ['pptx'],
    docx: ['docx'],
    text: ['txt', 'md', 'rtf'],
    theme: ['protheme', 'kairotheme', 'json'],
  };
  const CONTENT_EXTS = [...KINDS.propresenter, ...KINDS.easyworship, ...KINDS.pptx, ...KINDS.docx, ...KINDS.text];
  const THEME_EXTS = ['protheme', 'kairotheme'];
  const extOf = (name) => (name.toLowerCase().match(/\.([a-z0-9]+)$/) || [])[1] || '';

  let destination = 'playlist';
  const status = document.getElementById('imp-status');
  const setStatus = (t) => { if (status) status.textContent = t || ''; };

  const picker = document.createElement('input');
  picker.type = 'file';
  picker.style.display = 'none';
  document.body.appendChild(picker);
  let pickerKind = null;

  function open(opts = {}) {
    if (opts.destination) setDestination(opts.destination);
    setStatus('');
    modal.classList.remove('hidden');
  }
  function close() { modal.classList.add('hidden'); }

  function setDestination(d) {
    destination = d === 'library' ? 'library' : 'playlist';
    modal.querySelectorAll('.imp-dest-btn').forEach(b => b.classList.toggle('active', b.dataset.dest === destination));
  }

  function importTheme(file) {
    if (typeof looksModal !== 'undefined' && looksModal?.classList.contains('hidden') && typeof openThemeStudio === 'function') openThemeStudio();
    const input = document.getElementById('import-look-file');
    if (!input) return;
    const dt = new DataTransfer();
    dt.items.add(file);
    input.files = dt.files;
    input.dispatchEvent(new Event('change'));
  }

  // Several files picked or dropped at once: an EasyWorship library's two go in together
  // (Songs.db / Songs.DB first, the other as its companion); anything else, the first.
  function routeAll(files, kind) {
    const list = [...(files || [])];
    if (!list.length) return;
    const ew = list.filter(f => KINDS.easyworship.includes(extOf(f.name)));
    if (ew.length > 1 && ew.length === list.length) {
      const main = ew.find(f => /^songs\.db$/i.test(f.name)) || ew[0];
      close();
      window.KairoService?.quickImportFile?.(main, destination, ew.filter(f => f !== main));
      return;
    }
    route(list[0], kind);
  }

  // `kind` is the row the operator picked; a .json is only a theme when it
  // came from the Theme row (plain-text .json content imports as slides).
  function route(file, kind) {
    const ext = extOf(file.name);
    const isTheme = THEME_EXTS.includes(ext) || (ext === 'json' && kind === 'theme');
    if (isTheme) { close(); importTheme(file); return; }
    if (CONTENT_EXTS.includes(ext) || ext === 'json') {
      close();
      window.KairoService?.quickImportFile?.(file, destination);
      return;
    }
    setStatus(ext === 'pdf'
      ? 'PDF isn’t supported yet. Export it as PowerPoint or text first.'
      : `Kairo can’t import .${ext || '?'} files. Pick one of the formats listed above.`);
  }

  function choose(kind) {
    pickerKind = kind;
    const exts = kind ? KINDS[kind] : [...CONTENT_EXTS, ...THEME_EXTS];
    picker.accept = exts.map(e => '.' + e).join(',');
    picker.multiple = kind === 'easyworship' || !kind;
    picker.value = '';
    picker.click();
  }
  picker.addEventListener('change', () => {
    const files = [...(picker.files || [])];
    picker.value = '';
    routeAll(files, pickerKind);
  });

  modal.querySelectorAll('.imp-row').forEach(row => row.addEventListener('click', () => {
    const kind = row.dataset.kind;
    if (kind === 'clipboard') { close(); window.KairoService?.quickImportClipboard?.(destination); return; }
    choose(kind);
  }));
  modal.querySelectorAll('.imp-dest-btn').forEach(b => b.addEventListener('click', () => setDestination(b.dataset.dest)));

  const drop = document.getElementById('imp-drop');
  drop?.addEventListener('click', (e) => { e.preventDefault(); choose(null); });
  drop?.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(null); } });
  const content = modal.querySelector('.modal-content');
  content?.addEventListener('dragover', (e) => {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    drop?.classList.add('over');
  });
  content?.addEventListener('dragleave', (e) => { if (!content.contains(e.relatedTarget)) drop?.classList.remove('over'); });
  content?.addEventListener('drop', (e) => {
    e.preventDefault();
    drop?.classList.remove('over');
    routeAll(e.dataTransfer?.files, null);
  });

  document.getElementById('close-import-options')?.addEventListener('click', close);
  modal.querySelector('.modal-overlay')?.addEventListener('click', close);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !modal.classList.contains('hidden')) close(); });

  window.KairoImport = { open, close };
})();
