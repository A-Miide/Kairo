// KAIRO — parity recorder for the Rust rewrite.
//
// Feeds every sermon fixture (server/eval/fixtures) and live-run transcript
// (server/eval/live_runs) through the current Node pipeline and writes, per
// source, the exact input events and every `detection` broadcast to
// parity/golden/<name>.json. The Rust engine replays the same inputs at the
// same offsets and must reproduce the same decisions (docs/RUST_REWRITE_PLAN.md,
// "Parity").
//
// Each source runs twice. The Node pipeline reads the wall clock, so a few
// decisions can differ between two runs of identical input; those are kept
// under `unstable` and compared loosely, the rest under `stable`.
//
//   KAIRO_EVAL_MODE=1 node parity/record_golden.js [name ...]
'use strict';

const fs = require('fs'), os = require('os'), path = require('path');
if (!process.env.KAIRO_EVAL_MODE) { console.error('Set KAIRO_EVAL_MODE=1'); process.exit(1); }

const appData = path.join(os.tmpdir(), `kairo-parity-${Date.now()}`);
fs.mkdirSync(appData, { recursive: true });
fs.writeFileSync(path.join(appData, 'settings.json'), '{}');
process.env.KAIRO_APP_DATA_DIR = appData;

const ROOT = path.join(__dirname, '..');
const server = require(path.join(ROOT, 'server', 'server'));
const { referenceContext } = require(path.join(ROOT, 'server', 'reference_parser'));

const FIXTURES = path.join(ROOT, 'server', 'eval', 'fixtures');
const LIVE_RUNS = path.join(ROOT, 'server', 'eval', 'live_runs');
const OUT = path.join(__dirname, 'golden');
const RUNS = 2;
// Same pacing as server/eval/run_eval.js: a short real delay between interim
// steps so the pipeline's clock-based windows see time pass.
const STEP_MS = 15;
const SETTLE_MS = 2000;
const SEED = 20261011;

const wait = ms => new Promise(r => setTimeout(r, ms));

// mulberry32 — the interim split must be identical on every run and in the
// Rust replay, so it can't use Math.random like run_eval.js does.
function rng(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Growing word prefixes of 3-8 words, then the whole utterance as the final —
// the interim cadence run_eval.js models for Deepgram.
function chunk(text, rand) {
  const words = text.split(/\s+/).filter(Boolean);
  const out = [];
  for (let i = 0; i < words.length;) {
    i = Math.min(words.length, i + 3 + Math.floor(rand() * 6));
    out.push(words.slice(0, i).join(' '));
  }
  return out;
}

function sources(names) {
  const all = [
    ...fs.readdirSync(FIXTURES).filter(f => f.endsWith('.json')).map(f => {
      const fx = JSON.parse(fs.readFileSync(path.join(FIXTURES, f), 'utf8'));
      return { name: f.replace(/\.json$/, ''), kind: 'fixture', cues: fx.transcript.map(c => ({ text: c.text, audioMs: c.startMs })) };
    }),
    ...(fs.existsSync(LIVE_RUNS) ? fs.readdirSync(LIVE_RUNS).filter(f => f.endsWith('.txt')) : []).map(f => {
      const sentences = fs.readFileSync(path.join(LIVE_RUNS, f), 'utf8').replace(/\s+/g, ' ').match(/[^.?!]+[.?!]+/g) || [];
      return { name: f.replace(/\.txt$/, ''), kind: 'live_run', cues: sentences.map(s => ({ text: s.trim(), audioMs: null })) };
    }),
  ];
  return names.length ? all.filter(s => names.includes(s.name)) : all;
}

let sink = null;
let cueIdx = -1;
let t0 = 0;
server.onBroadcast(m => {
  if (!sink || m.type !== 'detection' || !m.verses?.length) return;
  sink.push({
    t: Math.round(performance.now() - t0),
    cue: cueIdx,
    target: m.target,
    method: m.method,
    refs: m.verses.map(v => v.reference),
    ...(m.corrected ? { correctedFrom: m.correctedFrom ?? null } : {}),
  });
});

async function freshSession() {
  server.resetDetectionSession();
  referenceContext.reset();
  server.clearRangeQueue();
  await server.clearLayer('all');
  await wait(300);
}

async function runOnce(src, events) {
  const outs = [];
  const inputs = [];
  await freshSession();
  sink = outs;
  t0 = performance.now();
  const rand = rng(SEED);
  for (let c = 0; c < src.cues.length; c++) {
    cueIdx = c;
    const steps = chunk(src.cues[c].text, rand);
    for (let i = 0; i < steps.length; i++) {
      const isFinal = i === steps.length - 1;
      inputs.push({ t: Math.round(performance.now() - t0), cue: c, text: steps[i], isFinal });
      await server.handleTranscriptSegment(steps[i], isFinal, 0.9, isFinal);
      await wait(STEP_MS);
    }
  }
  cueIdx = src.cues.length;
  await wait(SETTLE_MS);
  sink = null;
  events.push(inputs);
  return outs;
}

// A decision's identity for comparing runs: where it happened (cue) and what
// it was, not the exact millisecond.
const key = o => `${o.cue}|${o.target}|${o.method}|${o.refs.join('+')}`;

async function main() {
  const names = process.argv.slice(2);
  server.spawnDetectionWorker();
  await server.workerReadyPromise;
  const started = Date.now();
  while (!(server.isSemanticReady() && server.isRerankerReady())) {
    if (Date.now() - started > 120000) throw new Error('semantic/reranker layers never became ready — are the models installed?');
    await wait(250);
  }
  fs.mkdirSync(OUT, { recursive: true });

  for (const src of sources(names)) {
    const runs = [], inputsPerRun = [];
    const began = Date.now();
    for (let r = 0; r < RUNS; r++) runs.push(await runOnce(src, inputsPerRun));
    const counts = runs.map(run => {
      const m = new Map();
      for (const o of run) m.set(key(o), (m.get(key(o)) || 0) + 1);
      return m;
    });
    const stable = [], unstable = [];
    for (const o of runs[0]) {
      const k = key(o);
      if (counts.every(m => (m.get(k) || 0) > 0)) stable.push(o); else unstable.push({ ...o, run: 0 });
    }
    for (let r = 1; r < runs.length; r++) for (const o of runs[r]) if (!counts[0].has(key(o))) unstable.push({ ...o, run: r });

    const golden = {
      source: src.name, kind: src.kind,
      recordedAt: new Date().toISOString(),
      pacing: { stepMs: STEP_MS, settleMs: SETTLE_MS, seed: SEED, interim: '3-8 word growing prefixes' },
      cues: src.cues,
      inputs: inputsPerRun[0],
      stable, unstable,
      runs: runs.map(r => r.length),
    };
    fs.writeFileSync(path.join(OUT, `${src.name}.json`), JSON.stringify(golden, null, 1));
    const screen = stable.filter(o => o.target === 'viewer').length;
    console.log(`${src.name}: ${stable.length} stable (${screen} on screen), ${unstable.length} unstable, ${Math.round((Date.now() - began) / 1000)}s`);
  }
  process.exit(0);
}

main().catch(err => { console.error(err); process.exit(1); });
