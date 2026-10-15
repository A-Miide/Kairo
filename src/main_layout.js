// The main screen's two filters: the playlist panel (All, Slides, Songs,
// Scripture) and the media bin (All, Video, Images, Music, Overlays). Music and
// Overlays come from the audio lane and the overlay library, so the bin holds
// every kind of thing the operator puts on the screens.
(function () {
  const PLAYLIST_KEY = 'kairo-playlist-kind';
  const BIN_KEY = 'kairo-bin-kind';
  const read = (k, d) => { try { return localStorage.getItem(k) || d; } catch { return d; } };
  const write = (k, v) => { try { localStorage.setItem(k, v); } catch {} };

  const layout = {
    binKind: read(BIN_KEY, 'all'),
    paintBinExtra,
  };
  window.KairoMainLayout = layout;

  function chips(rootId, current, onPick) {
    const root = document.getElementById(rootId);
    if (!root) return;
    const mark = (kind) => root.querySelectorAll('.lang-pill').forEach(b => b.classList.toggle('active', b.dataset.kind === kind));
    mark(current);
    root.addEventListener('click', (e) => {
      const b = e.target.closest('.lang-pill');
      if (!b) return;
      mark(b.dataset.kind);
      onPick(b.dataset.kind);
    });
  }

  const list = () => document.getElementById('svc-items-list');
  function applyPlaylist(kind) { const l = list(); if (l) l.dataset.filter = kind; }
  const playlistKind = read(PLAYLIST_KEY, 'all');
  applyPlaylist(playlistKind);
  chips('ls-playlist-chips', playlistKind, (kind) => { write(PLAYLIST_KEY, kind); applyPlaylist(kind); });

  chips('media-bin-chips', layout.binKind, (kind) => {
    layout.binKind = kind;
    write(BIN_KEY, kind);
    // The bin repaints itself when its folder menu changes.
    document.getElementById('media-bin-folder')?.dispatchEvent(new Event('change'));
  });

  function tile(label, sub, thumb, active, onClick) {
    const t = document.createElement('button');
    t.type = 'button';
    t.className = 'mb-extra' + (active ? ' on' : '');
    const pic = document.createElement('div');
    pic.className = 'mb-extra-pic';
    if (thumb) pic.appendChild(thumb); else pic.textContent = sub;
    const name = document.createElement('div');
    name.className = 'mb-extra-name';
    name.textContent = label;
    t.append(pic, name);
    t.addEventListener('click', onClick);
    return t;
  }

  async function paintBinExtra(strip, kind) {
    strip.innerHTML = '';
    if (kind === 'music') {
      const lane = window.KairoAudioLane;
      if (!lane) return;
      const draw = () => {
        if (!strip.isConnected || layout.binKind !== 'music') return;
        strip.innerHTML = '';
        const st = lane.state();
        if (!lane.tracks.length) { strip.innerHTML = '<div class="svc-empty">No music yet. Add some under Settings, Audio.</div>'; return; }
        lane.tracks.forEach((t, i) => strip.appendChild(tile(t.label, st.index === i && st.playing ? 'Playing' : 'Music', null, st.index === i && st.playing, () => (i === st.index ? lane.toggle() : lane.play(i)))));
      };
      await lane.refresh();
      draw();
      return;
    }
    let items = [];
    try { items = await window.KairoOverlays.load(); } catch {}
    if (layout.binKind !== 'overlays') return;
    if (!items.length) { strip.innerHTML = '<div class="svc-empty">No overlays yet. Add some under Settings, Overlays.</div>'; return; }
    items.forEach((o) => {
      const img = document.createElement(o.kind === 'video' ? 'video' : 'img');
      img.src = typeof authedMediaUrl === 'function' ? authedMediaUrl(o.url) : o.url;
      if (o.kind === 'video') { img.muted = true; img.preload = 'metadata'; }
      strip.appendChild(tile(o.name, o.live ? 'On' : 'Off', img, o.live, async () => {
        await fetch(`${SERVER}/api/overlays/${o.id}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ live: !o.live }) });
        paintBinExtra(strip, 'overlays');
      }));
    });
  }
})();
