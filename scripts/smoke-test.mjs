#!/usr/bin/env node
// Kairo release smoke test: run the packaged app the way a church will, before tagging.
//
//   node scripts/smoke-test.mjs --app "/Applications/Kairo.app/Contents/MacOS/Kairo"
//   node scripts/smoke-test.mjs --app "C:\Program Files\Kairo\Kairo.exe"
//   node scripts/smoke-test.mjs --url http://127.0.0.1:7792            (a running dev server)
//
// It launches the app with a port and a one-time token of its own (KAIRO_SERVER_PORT,
// KAIRO_SERVER_TOKEN: the token is 43 random characters and still guards every request, so
// nothing is opened up), then checks, against what the windows report they show
// (/api/state-reports):
//   1. the app starts: the server answers, the speech engine is ready, the main window reports;
//   2. each display output that has a screen opens a window on that screen;
//   3. Output Control: with Scripture switched off for the test output, a verse sent by speech
//      does not show there; switched back on, it does;
//   4. a replayed transcript (scripts/smoke-test.json) auto-sends each verse, and each lands in
//      the Live Queue and on the test output.
// It runs on the computer's own settings, so its screens show the test verses: run it before a
// service, not during one. Output Control is put back as it was. Exit code 0 = every check passed.
// See README.md, "Releasing".

import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = Object.fromEntries(process.argv.slice(2).reduce((acc, a, i, all) => {
  if (a.startsWith('--')) acc.push([a.slice(2), all[i + 1] && !all[i + 1].startsWith('--') ? all[i + 1] : true]);
  return acc;
}, []));
if (!args.app && !args.url) {
  console.error('Usage: node scripts/smoke-test.mjs --app <path to the Kairo executable> | --url <running server> [--token <token>] [--output display-1]');
  process.exit(2);
}
const fixture = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'smoke-test.json'), 'utf8'));
const OUTPUT = args.output || 'display-1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── Results ─────────────────────────────────────────────────────────────────
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
  return ok;
}

// ── The app ─────────────────────────────────────────────────────────────────
function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.unref();
    s.on('error', reject);
    s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); });
  });
}

let child = null;
let base = args.url ? String(args.url).replace(/\/$/, '') : null;
let token = typeof args.token === 'string' ? args.token : '';
if (args.app) {
  const port = await freePort();
  token = randomBytes(32).toString('base64url');
  base = `http://127.0.0.1:${port}`;
  child = spawn(args.app, [], { env: { ...process.env, KAIRO_SERVER_PORT: String(port), KAIRO_SERVER_TOKEN: token }, stdio: 'ignore' });
  child.on('exit', (code) => { if (code !== null && !finished) console.error(`The app quit (code ${code}).`); });
}
let finished = false;
const auth = token ? { Authorization: `Bearer ${token}` } : {};
async function api(path, body) {
  const res = await fetch(base + path, body === undefined ? { headers: auth } : { method: 'POST', headers: { ...auth, 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`);
  return res.json();
}
async function waitFor(what, fn, ms) {
  const until = Date.now() + ms;
  let last;
  while (Date.now() < until) {
    try { last = await fn(); if (last) return last; } catch {}
    await sleep(500);
  }
  return null;
}
const reports = () => api('/api/state-reports');
const queueHas = (r, ref) => (r.main?.liveQueue || []).includes(ref);
const outputShows = (r, ref) => (r[`output:${OUTPUT}`]?.reference || '').includes(ref);

// What is on screen and the switches the server keeps, as a window that connects now gets them.
function catchUp() {
  return new Promise((resolve) => {
    const out = { layers: null };
    const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/${token ? `?token=${encodeURIComponent(token)}` : ''}`);
    const done = () => { try { ws.close(); } catch {} resolve(out); };
    ws.onmessage = (e) => { try { const m = JSON.parse(e.data); if (m.type === 'look-update' && m.layers) out.layers = m.layers; } catch {} };
    ws.onerror = done;
    setTimeout(done, 1500);
  });
}

async function speak(text) {
  const r = await api('/api/test-transcript', { text, isFinal: true });
  if (r.error) throw new Error(`test-transcript: ${r.error}`);
}

let restoreLayers = null;
try {
  // 1. Starts
  const up = await waitFor('server', () => api('/health'), 120000);
  if (!check('App starts and its server answers', !!up)) throw new Error('stop');
  check('Speech engine ready', !!(await waitFor('engine', async () => (await api('/health')).workerBasicReady, 120000)));
  check('Main window reports (UI loaded, token accepted)', !!(await waitFor('main', async () => (await reports()).main, 60000)));
  const settings = await api('/api/settings');
  if (settings.autoSend === false) console.log('NOTE  Auto-send is off in Settings > Bible: the auto-send checks will fail until it is on.');

  // 2. Outputs on their screens
  await sleep(4000);   // outputs reopen a moment after the main window
  const screens = settings.outputScreens && typeof settings.outputScreens === 'object' ? settings.outputScreens : {};
  const r0 = await reports();
  const displays = Object.entries(screens).filter(([, s]) => s && typeof s === 'object');
  if (!displays.length) console.log('NOTE  No display output has a screen in Settings > Outputs: screen placement not checked.');
  for (const [id, s] of displays) {
    const w = r0[`output:${id}`]?.window;
    const cx = w ? w.x + w.w / 2 : NaN, cy = w ? w.y + w.h / 2 : NaN;
    const inside = w && cx >= s.left && cx < s.left + s.width && cy >= s.top && cy < s.top + s.height;
    check(`Output ${id} open on its screen${s.name ? ` (${s.name})` : ''}`, !!inside,
      w ? `window at ${w.x},${w.y} ${w.w}x${w.h}; screen at ${s.left},${s.top} ${s.width}x${s.height}` : 'no window reported');
  }
  if (!r0[`output:${OUTPUT}`]) {
    check(`Output ${OUTPUT} is open (the switch and send checks use it)`, false, 'give it a screen in Settings > Outputs, or pass --output <id>');
    throw new Error('stop');
  }

  // 3. Output Control: Scripture off hides a verse, back on shows it
  const kept = await catchUp();
  const original = kept.layers || {};
  restoreLayers = kept.layers ? original : { [OUTPUT]: { ...(settings.outputLayers?.[OUTPUT] || {}), bible: settings.outputLayers?.[OUTPUT]?.bible !== false } };
  const off = { ...original, [OUTPUT]: { ...(original[OUTPUT] || {}), bible: false } };
  await api('/api/look/apply', { layers: off });
  await speak(fixture.switchOff.text);
  const ref = fixture.switchOff.expect;
  const sent = await waitFor('switch-off send', async () => queueHas(await reports(), ref), 15000);
  check(`Scripture off: "${ref}" auto-sent to the Live Queue`, !!sent);
  await sleep(1500);
  check(`Scripture off: ${OUTPUT} does not show it`, !outputShows(await reports(), ref), (await reports())[`output:${OUTPUT}`]?.reference || 'nothing shown');
  await api('/api/look/apply', { layers: { ...off, [OUTPUT]: { ...off[OUTPUT], bible: true } } });
  check(`Scripture back on: ${OUTPUT} shows it`, !!(await waitFor('switch-on', async () => outputShows(await reports(), ref), 8000)));

  // 4. Replayed transcript: each verse auto-sent, in the Live Queue and on screen
  for (const step of fixture.transcript) {
    await speak(step.text);
    const r = await waitFor(step.expect, async () => { const x = await reports(); return queueHas(x, step.expect) && outputShows(x, step.expect) && x; }, 15000);
    const x = r || await reports();
    check(`"${step.expect}" auto-sent: in the Live Queue and on ${OUTPUT}`, !!r,
      r ? '' : `queue top: ${(x.main?.liveQueue || []).slice(0, 3).join(', ') || 'empty'}; ${OUTPUT} shows: ${x[`output:${OUTPUT}`]?.reference || 'nothing'}`);
    await sleep(1500);
  }
} catch (e) {
  if (e.message !== 'stop') check('Smoke test ran to the end', false, e.message);
} finally {
  if (restoreLayers) { try { await api('/api/look/apply', { layers: restoreLayers }); } catch {} }
  finished = true;
  if (child) child.kill();
}

const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `\n${failed} of ${results.length} checks failed.` : `\nAll ${results.length} checks passed.`);
process.exit(failed ? 1 : 0);
