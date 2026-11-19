// Writes the pre-service announcement scenes (src/announcement_pack.js) as
// data for the Rust server, which seeds new installs' pre-service segments
// with them. Re-run after changing the pack:
//   node scripts/export-announcement-pack.js
'use strict';
const fs = require('fs'), path = require('path');
const pack = require('../src/announcement_pack.js');
const out = path.join(__dirname, '..', 'src-tauri', 'crates', 'kairo-server', 'data', 'announcement_pack.json');
fs.writeFileSync(out, JSON.stringify({ preservice: pack.preserviceScenes({ ready: true }), preservice2: pack.preservice2Scenes(), pace: pack.PACE }, null, 1) + '\n');
console.log(`wrote ${path.relative(process.cwd(), out)}`);
