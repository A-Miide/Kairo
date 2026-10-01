// KAIRO — bundled gradients (Theme Studio's gradient presets, and the
// default Bible theme's canvas).
//
// The colour stops were sampled from three gradient packs the owner chose
// (2026-10-01: "use those to be the bundled gradients that come with the
// app") — a vector gradient colour set, a set of soft mesh gradients and a
// halftone gradient cover collection — at their own RGB previews, so they
// read the way the designer rendered them. Only the stops are bundled, not the
// artwork. The mesh ones keep their soft spots of colour as `glows`: radial
// spots over the linear base (gradientCss, layer_geometry.js), x/y the spot's
// centre and rx/ry its reach, as % of the box. Angles are CSS degrees
// (0 = to top, 90 = to right, 180 = to bottom).
(function () {
  const g = (id, name, angle, stops, glows) => ({
    id, name, angle,
    stops: stops.map(([pos, color]) => ({ pos, color })),
    ...(glows ? { glows: glows.map(([x, y, rx, ry, color]) => ({ x, y, rx, ry, color })) } : {}),
  });

  window.KairoGradients = [
    // ── Vector gradient colour set ──
    g('sunrise', 'Sunrise', 180, [[0, '#fec900'], [45, '#ff8800'], [100, '#ca3e00']]),
    g('orchid-night', 'Orchid Night', 180, [[0, '#f941bb'], [100, '#1f0e55']]),
    g('coral-dusk', 'Coral Dusk', 180, [[0, '#eb485d'], [35, '#e9475e'], [100, '#582898']]),
    g('deep-sea', 'Deep Sea', 180, [[0, '#0697ce'], [60, '#12498c'], [100, '#192671']]),
    g('indigo', 'Indigo', 180, [[0, '#1142b9'], [60, '#401d9f'], [100, '#491699']]),
    g('clear-sky', 'Clear Sky', 180, [[0, '#00a3fe'], [100, '#003ea1']]),
    g('crimson', 'Crimson', 180, [[0, '#f12d2e'], [40, '#d00808'], [100, '#7b0304']]),

    // ── Halftone gradient covers (their colour sweeps, without the dots) ──
    g('neon-prism', 'Neon Prism', 135, [[0, '#059ee1'], [50, '#ec0d64'], [100, '#5838e8']]),
    g('velvet', 'Velvet', 150, [[0, '#951aef'], [45, '#6a18e0'], [100, '#0010db']],
      [[100, 0, 60, 42, '#e8278a']]),
    g('blue-hour', 'Blue Hour', 60, [[0, '#0b97df'], [55, '#3c58e8'], [100, '#5a34ec']],
      [[0, 0, 38, 30, '#7d00d8']]),
    g('flamingo', 'Flamingo', 90, [[0, '#ef2879'], [50, '#ca23b1'], [100, '#3a17e2']]),
    g('electric', 'Electric', 100, [[0, '#5a2be3'], [50, '#3a62e6'], [100, '#0d93e1']],
      [[100, 0, 60, 20, '#ed0d64']]),
    g('candy', 'Candy', 90, [[0, '#7a1bf0'], [65, '#b821ca'], [100, '#f0256f']]),

    // ── Soft mesh gradients ──
    g('ember-glow', 'Ember Glow', 170, [[0, '#ae006f'], [45, '#7b0075'], [70, '#4a0086'], [100, '#3800b1']],
      [[88, 12, 34, 24, '#e29800'], [0, 98, 28, 15, '#d04d7e']]),
    g('nebula', 'Nebula', 160, [[0, '#a71669'], [40, '#4a27d2'], [70, '#2a3cf8'], [100, '#0b5ef8']],
      [[30, 52, 30, 18, '#7a62ff'], [92, 94, 42, 24, '#6ae0ff']]),
    g('northern-lights', 'Northern Lights', 150, [[0, '#2fb0ba'], [45, '#2415fa'], [70, '#2400d8'], [100, '#280094']],
      [[50, 0, 42, 18, '#c9b218'], [0, 0, 26, 16, '#4fd98a'], [2, 98, 28, 14, '#8e2cfc'], [98, 38, 20, 14, '#6a40ff']]),
    g('lagoon', 'Lagoon', 180, [[0, '#01ffcb'], [25, '#00d4ec'], [55, '#0095fe'], [100, '#009cff']],
      [[22, 55, 26, 16, '#0035f0'], [95, 20, 30, 18, '#6ae3fe'], [0, 95, 26, 14, '#70c8ff']]),
    g('tropical', 'Tropical', 160, [[0, '#a600cf'], [45, '#e3168f'], [75, '#ff2157'], [100, '#ff6100']],
      [[50, 36, 22, 12, '#f45f86'], [70, 75, 26, 16, '#f9ae01']]),
    g('twilight', 'Twilight', 180, [[0, '#5c00df'], [40, '#6a10c8'], [65, '#8a2c8f'], [85, '#c55a3a'], [100, '#ec9a00']],
      [[0, 22, 22, 14, '#bc3ccf'], [98, 40, 22, 14, '#5185ff'], [20, 100, 32, 14, '#f0b500']]),
  ];
})();
