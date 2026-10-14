// KAIRO — bundled gradients (Theme Studio's gradient presets, and the
// default Bible theme's canvas).
//
// The colour stops were sampled from gradient packs the owner chose
// (2026-10-01: "use those to be the bundled gradients that come with the
// app") — a vector gradient colour set and a halftone gradient cover
// collection — at their own RGB previews, so they read the way the designer
// rendered them. The third pack, soft mesh gradients, isn't here: no CSS
// gradient reproduces a mesh (an imitation with radial spots read as smudges —
// "I gave you some samples so you could just replicate 1:1"), so its artwork
// ships as bundled backgrounds instead (backgrounds/mesh-*.jpg, see
// scripts/make-backgrounds.js). A preset's optional `glows` are radial spots
// over the linear base (gradientCss, layer_geometry.js), x/y the spot's centre
// and rx/ry its reach, as % of the box. Angles are CSS degrees (0 = to top,
// 90 = to right, 180 = to bottom).
(function () {
  const g = (id, name, angle, stops, glows) => ({
    id, name, angle,
    stops: stops.map(([pos, color]) => ({ pos, color })),
    ...(glows ? { glows: glows.map(([x, y, rx, ry, color]) => ({ x, y, rx, ry, color })) } : {}),
  });

  window.KairoGradients = [
    // Kairo's own red, into its darker shade — the default Bible and timer
    // canvases (2026-10-12: the app's brand colours, deep enough at the far
    // end to keep white verse text readable on a projector).
    g('kairo', 'Kairo Red', 135, [[0, '#F5301A'], [100, '#9E1608']]),
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

    // ── Night set — the Timer's own ──
    // Side by side in the Timer grid, the mesh pictures fought each other
    // (owner, 2026-10-01: "it lacks the visual balance, and it's very
    // stimulating to the eye"). Each of these keeps a mesh's colour (Indigo
    // ← Nebula, Raspberry ← Ember Glow, Teal ← Northern Lights, Ember ←
    // Tropical, Violet ← Twilight, Ocean ← Lagoon) as a deep jewel tone, all
    // running the same way into the same navy, with one soft glow where the
    // light comes from — so the set reads as one family under white digits.
    g('night-indigo', 'Indigo Night', 150, [[0, '#3d217d'], [52, '#1d1759'], [100, '#101741']],
      [[24, 30, 56, 48, '#482cba']]),
    g('night-raspberry', 'Raspberry Night', 150, [[0, '#7b2460'], [52, '#3d1858'], [100, '#101741']],
      [[24, 30, 56, 48, '#b82e65']]),
    g('night-teal', 'Teal Night', 150, [[0, '#1e6680'], [52, '#16345a'], [100, '#101741']],
      [[24, 30, 56, 48, '#30acb5']]),
    g('night-ember', 'Ember Night', 150, [[0, '#7d2143'], [52, '#491759'], [100, '#101741']],
      [[24, 30, 56, 48, '#ba482c']]),
    g('night-violet', 'Violet Night', 150, [[0, '#572678'], [52, '#271958'], [100, '#101741']],
      [[24, 30, 56, 48, '#9f37ae']]),
    g('night-ocean', 'Ocean Night', 150, [[0, '#185a86'], [52, '#142e5d'], [100, '#101741']],
      [[24, 30, 56, 48, '#2796be']]),
  ];
})();
