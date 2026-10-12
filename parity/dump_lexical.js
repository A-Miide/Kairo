// KAIRO — dumps what the Node detection worker's word matchers find in real
// transcript text (verbatim, fingerprint), so the Rust matchers can be
// compared with them text by text. Development sources only
// (docs/RUST_REWRITE_PLAN.md, "Evaluation split").
//
//   node parity/dump_lexical.js > parity/lexical_node.jsonl
'use strict';

const fs = require('fs'), path = require('path');
const { Worker } = require('worker_threads');
const ROOT = path.join(__dirname, '..');

const DEV_FIXTURES = ['enough-is-enough', 'oyedepo-kingdomlaw', 'shiloh2025-impartation'];
const texts = [];
for (const name of DEV_FIXTURES) {
  const fx = JSON.parse(fs.readFileSync(path.join(ROOT, 'server', 'eval', 'fixtures', `${name}.json`), 'utf8'));
  const cues = fx.transcript.map(c => c.text);
  for (let i = 0; i < cues.length; i++) {
    texts.push(cues[i]);
    if (i + 1 < cues.length) texts.push(`${cues[i]} ${cues[i + 1]}`);
    if (i % 3 === 0 && i + 3 < cues.length) texts.push(cues.slice(i, i + 4).join(' ').split(/\s+/).slice(-40).join(' '));
  }
}

const worker = new Worker(path.join(ROOT, 'server', 'detection_worker.js'), {
  workerData: { dataDir: path.join(ROOT, 'databases', 'bibles'), altTranslation: null },
  stdout: true, stderr: true,
});
let id = 0;
const pending = new Map();
worker.on('message', (msg) => {
  if (msg.type === 'ready') return start();
  const p = pending.get(msg.id);
  if (p) { pending.delete(msg.id); p(msg); }
});
const call = (type, payload) => new Promise(resolve => {
  const n = ++id;
  pending.set(n, resolve);
  worker.postMessage({ type, id: n, ...payload });
});

async function start() {
  for (const text of texts) {
    const v = await call('verbatimSearch', { text, minWords: 6, limit: 3 });
    const f = await call('fingerprintSearch', { text, limit: 5 });
    process.stdout.write(JSON.stringify({
      text,
      verbatim: (v.results || []).map(r => [r.reference, +r.similarity.toFixed(4), r.matchedIdf == null ? null : +r.matchedIdf.toFixed(3)]),
      fingerprint: { confidence: f.confidence, hits: (f.results || []).map(r => [r.reference, +r.similarity.toFixed(4)]) },
    }) + '\n');
  }
  process.exit(0);
}
