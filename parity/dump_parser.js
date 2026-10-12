// KAIRO — dumps what the Node reference parser reads from real transcript
// text, so the Rust parser can be compared against it line by line.
// Development sources only (docs/RUST_REWRITE_PLAN.md, "Evaluation split").
//
//   node parity/dump_parser.js > parity/parser_node.jsonl
'use strict';

const fs = require('fs'), path = require('path');
const ROOT = path.join(__dirname, '..');
const { parseAllSpokenReferences, referenceContext } = require(path.join(ROOT, 'server', 'reference_parser'));

const DEV_FIXTURES = ['enough-is-enough', 'oyedepo-kingdomlaw', 'shiloh2025-impartation'];
const texts = [];
for (const name of DEV_FIXTURES) {
  const fx = JSON.parse(fs.readFileSync(path.join(ROOT, 'server', 'eval', 'fixtures', `${name}.json`), 'utf8'));
  for (const c of fx.transcript) texts.push({ src: name, text: c.text });
}
const runs = path.join(ROOT, 'server', 'eval', 'live_runs');
for (const f of fs.readdirSync(runs).filter(n => n.endsWith('.txt'))) {
  const sentences = fs.readFileSync(path.join(runs, f), 'utf8').replace(/\s+/g, ' ').match(/[^.?!]+[.?!]+/g) || [];
  for (const s of sentences) texts.push({ src: f, text: s.trim() });
}

const fmt = (r) => {
  const v = r.ranges ? r.ranges.map(x => x.verseStart === x.verseEnd ? `${x.verseStart}` : `${x.verseStart}-${x.verseEnd}`).join('+')
    : r.verseStart != null ? `${r.verseStart}-${r.verseEnd}` : r.verse != null ? `${r.verse}` : '';
  return `${r.book} ${r.chapter}:${v}${r.altChapter ? ` or ${r.altChapter}` : ''}`;
};

for (const { src, text } of texts) {
  referenceContext.reset();
  const refs = parseAllSpokenReferences(text);
  // Alternatives of one ambiguous mention are one citation with alternatives.
  const seen = new Set(), out = [];
  for (const r of refs) {
    if (r.ambiguousGroup) { if (seen.has(r.ambiguousGroup)) continue; seen.add(r.ambiguousGroup); }
    out.push(fmt(r));
  }
  process.stdout.write(JSON.stringify({ src, text, refs: out }) + '\n');
}
