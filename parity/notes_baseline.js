// KAIRO — sermon notes as the Node module makes them, for the dev sermons
// (ground-truth verses as the verses shown), to compare the Rust notes with.
//   node parity/notes_baseline.js <outdir>
'use strict';
const fs = require('fs'), path = require('path');
const { Worker } = require('worker_threads');
const ROOT = path.join(__dirname, '..');
const notes = require(path.join(ROOT, 'server', 'sermon_notes'));
const out = process.argv[2];
const DEV = ['enough-is-enough', 'oyedepo-kingdomlaw', 'shiloh2025-impartation'];
const map = new Map(JSON.parse(fs.readFileSync(path.join(ROOT, 'databases/bibles/map.json'), 'utf8')).verses.map(v => [v.reference, v.kjv_text]));
const worker = new Worker(path.join(ROOT, 'server', 'detection_worker.js'), { workerData: { dataDir: path.join(ROOT, 'databases', 'bibles'), altTranslation: null }, stdout: true, stderr: true });
let id = 0; const pending = new Map();
worker.on('message', (m) => { if (m.type === 'ready') return start(); const p = pending.get(m.id); if (p) { pending.delete(m.id); p(m); } });
const call = (type, payload) => new Promise(r => { const n = ++id; pending.set(n, r); worker.postMessage({ type, id: n, ...payload }); });
async function start() {
  for (const name of DEV) {
    const fx = JSON.parse(fs.readFileSync(path.join(ROOT, 'server/eval/fixtures', `${name}.json`), 'utf8'));
    const t0 = Date.UTC(2026, 9, 11, 14, 0);
    const transcript = fx.transcript.map(c => ({ text: c.text, at: t0 + c.startMs }));
    const verses = fx.groundTruth.filter(g => g.verse).map(g => { const ref = `${g.book} ${g.chapter}:${g.verse}`; return { ref, text: map.get(ref) || '', at: t0 + g.startMs }; });
    const embed = async (texts, { check } = {}) => {
      const r = await call('embedSentences', { texts, check });
      if (!r || !r.count) return null;
      return { vectors: Array.from({ length: r.count }, (_, i) => r.vectors.subarray(i * r.dims, (i + 1) * r.dims)), quoteShares: r.quoteShares };
    };
    const n = await notes.buildNotesWithMeaning({ name: '', transcript, verses }, embed);
    fs.writeFileSync(path.join(out, `${name}.json`), JSON.stringify(n, null, 1));
    fs.writeFileSync(path.join(out, `${name}.pdf`), await notes.renderNotesPdf(n));
    console.log(name, n.sections.length, 'sections', n.prayerPoints.length, 'prayer points');
  }
  process.exit(0);
}
