// KAIRO — the offline speech-model installer's bookkeeping: two models (English
// and every other language), each in its own folder, the other-languages one
// verified by checksum before it is extracted. The download itself needs the
// network and ~475 MB, so it is not run here.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs'), os = require('os'), path = require('path');
const crypto = require('crypto');
const inst = require('./sherpa_installer');

test('English and the other languages are separate models in separate folders', () => {
  const base = path.join(os.tmpdir(), 'kairo-inst-x');
  assert.equal(path.basename(inst.modelDir(base)), 'sherpa-streaming-en');
  assert.equal(path.basename(inst.modelDir(base, 'multi')), 'sherpa-streaming-multi');
  assert.equal(inst.modelPath(base, 'multi'), inst.modelDir(base, 'multi'));
  assert.equal(inst.approxMB(), 465);
  assert.equal(inst.approxMB('multi'), 475);
  // An unknown model name is the English one, never a crash.
  assert.equal(inst.modelDir(base, 'nonsense'), inst.modelDir(base, 'en'));
});

test('a model is present only when its own files are', () => {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'kairo-inst-'));
  try {
    assert.equal(inst.isModelPresent(base, 'multi'), false);
    const dir = inst.modelDir(base, 'multi');
    fs.mkdirSync(dir, { recursive: true });
    for (const f of ['decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt']) fs.writeFileSync(path.join(dir, f), 'x');
    fs.writeFileSync(path.join(dir, 'encoder.int8.onnx'), Buffer.alloc(6_000_000));
    assert.equal(inst.isModelPresent(base, 'multi'), true);
    assert.equal(inst.isModelPresent(base, 'en'), false, 'the English model is not the other-languages one');
    assert.equal(inst.needsUpdate(base, 'multi'), true, 'no version marker yet');
    fs.writeFileSync(path.join(dir, '.kairo-model-version'), inst.MODELS.multi.version);
    assert.equal(inst.needsUpdate(base, 'multi'), false);
  } finally { fs.rmSync(base, { recursive: true, force: true }); }
});

test('the other-languages download has a mirror, an upstream fallback and a checksum', () => {
  const m = inst.MODELS.multi;
  assert.match(m.url, /Kairo-live\/Kairo\/releases\/download\/offline-model-multi-v1\//);
  assert.match(m.fallbackUrl, /k2-fsa\/sherpa-onnx\/releases\/download\/asr-models\//);
  assert.equal(path.basename(m.url), path.basename(m.fallbackUrl), 'the same file under both names');
  assert.match(m.sha256, /^[0-9a-f]{64}$/);
  assert.ok(path.basename(m.url).startsWith(m.topDir));
});

test('sha256File matches the real digest', async () => {
  const f = path.join(os.tmpdir(), `kairo-sha-${Date.now()}.bin`);
  const data = crypto.randomBytes(300_000);
  fs.writeFileSync(f, data);
  try {
    assert.equal(await inst.sha256File(f), crypto.createHash('sha256').update(data).digest('hex'));
  } finally { fs.rmSync(f, { force: true }); }
});
