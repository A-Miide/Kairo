// KAIRO — spoken commands that change the Bible translation.
//
// "Switch to the NIV", "let's read this in the New Living Translation", "can we
// put it in the King James" — said to the operator, with the words of the
// translation's name or its letters. The preacher mentions translations all the
// time without meaning a change ("in the NIV it says…", "if you go to the ESV…",
// "the King James was translated in 1611"), and a change is visible to the whole
// congregation, so only a COMMAND counts: a verb phrase that starts its clause
// ("switch to", "let's use", "can we put it in") and then the translation. A bare
// mention never does. A miss costs the operator one click; a false switch changes
// every verse after it.
//
// Pure — no state, no I/O. server.js decides what to do with a match.
'use strict';

// Code → the ways it is said, longest first within each code (the alternation
// below is sorted by length so "new king james" is tried before "king james").
const ALIASES = {
  NKJV: ['new king james version', 'new king james bible', 'new king james', 'nkjv'],
  KJV:  ['king james version', 'king james bible', 'king james', 'authorized version', 'kjv'],
  NLT:  ['new living translation', 'new living', 'nlt'],
  NIV:  ['new international version', 'new international', 'niv'],
  ESV:  ['english standard version', 'english standard', 'esv'],
  NASB: ['new american standard bible', 'new american standard version', 'new american standard', 'nasb'],
};

const BY_ALIAS = new Map();
for (const [code, list] of Object.entries(ALIASES)) for (const a of list) BY_ALIAS.set(a, code);
const ALIAS_RE = [...BY_ALIAS.keys()].sort((a, b) => b.length - a.length).map(a => a.replace(/ /g, '\\s+')).join('|');

// Letters said or written one by one: "N I V", "N.I.V.", "e-s-v". Only the codes.
const SPELLED = Object.keys(ALIASES)
  .sort((a, b) => b.length - a.length)
  .map(code => ({ code, re: new RegExp(`\\b${code.toLowerCase().split('').join('[.\\s-]*')}\\b`, 'g') }));

function normalize(text) {
  let t = String(text || '').toLowerCase()
    .replace(/[’‘`]/g, "'")
    .replace(/[—–]/g, ', ');
  for (const { code, re } of SPELLED) t = t.replace(re, code.toLowerCase());
  // Contractions become two words ("let's" → "let s", "we'll" → "we ll") so the
  // patterns below don't confuse them with other words ("we'll" is not "well").
  return t.replace(/'/g, ' ').replace(/[ \t]+/g, ' ').trim();
}

// ── The grammar ───────────────────────────────────────────────────────────────
const T = `(?:the\\s+)?(${ALIAS_RE})(?![a-z])`;   // the translation, captured
const OBJ = '(?:it|this|that|us|everything|over|back|the\\s+(?:bible|scripture|scriptures|translation|version|text|screen|display)|our\\s+(?:bible|translation|version))';
const OBJ_V = '(?:it|this|that|us|the\\s+(?:verse|scripture|passage|text|bible))';

// What a clause may open with before the verb.
const FILLER = '(?:(?:please|kindly|okay|ok|now|alright|all\\s+right|so|and|then|just|well|right|yes)\\s+)*';
const MODAL = '(?:' + [
  '(?:can|could|would|will)\\s+(?:we|you|somebody|someone|the\\s+(?:tech|media|sound)\\s+(?:team|guys|crew)|tech|media)\\s+(?:please\\s+)?',
  'let(?:\\s+s|s|\\s+us)\\s+',
  'i\\s+(?:want|need|would\\s+like|d\\s+like)\\s+(?:us\\s+)?to\\s+',
  'we(?:\\s+will|\\s+ll|\\s+are\\s+going\\s+to|\\s+re\\s+going\\s+to|\\s+are\\s+gonna|\\s+re\\s+gonna)\\s+',
  'i(?:\\s+am|\\s+m|\\s+will|\\s+ll)\\s+(?:going\\s+to\\s+|gonna\\s+)?',
].join('|') + ')';

// A verb phrase that works on its own (an instruction to the operator)…
const BARE = '(?:' + [
  `(?:switch|change|swap|flip|set)\\s+(?:${OBJ}\\s+){0,3}(?:to|over\\s+to|into|back\\s+to)\\s+${T}`,
  `(?:put|pull\\s+up|bring\\s+up|display)\\s+(?:${OBJ_V}\\s+){0,2}(?:in|on|with|using)\\s+${T}`,
  `give\\s+us\\s+${T}`,
].join('|') + ')';
// …and ones that are only a command when someone asks or proposes ("let's use").
// Said bare — "use the NIV", "read it in the ESV" — they are as often an
// instruction to the congregation as to the operator.
const WITH_MODAL = '(?:' + [
  BARE,
  `(?:go|move|jump|head)\\s+(?:over\\s+|back\\s+)?(?:to|into)\\s+${T}`,
  `(?:use|read|show|get|display|put|have)\\s+(?:${OBJ_V}\\s+){0,2}(?:in|from|on|with|using)?\\s*${T}`,
  `(?:pull\\s+up|bring\\s+up)\\s+(?:${OBJ_V}\\s+){0,2}(?:in\\s+)?${T}`,
].join('|') + ')';

// Opens a clause: the start of the text, after punctuation, or after a joining word.
const CLAUSE = '(?:^|[.!?;:,]\\s+|\\b(?:and|so|okay|ok|now|alright|then|please)\\s+)';
const COMMAND_RE = new RegExp(`${CLAUSE}${FILLER}(?:${MODAL}${WITH_MODAL}|${BARE})`, 'g');

// The last translation command in `text`: { code, heard } — or null.
function detectTranslationCommand(text) {
  const t = normalize(text);
  if (!t) return null;
  let found = null, m;
  COMMAND_RE.lastIndex = 0;
  while ((m = COMMAND_RE.exec(t))) {
    // The captured translation is the last capture group that matched.
    const said = m.slice(1).filter(Boolean).pop();
    const code = BY_ALIAS.get(String(said).replace(/\s+/g, ' '));
    if (code) found = { code, heard: m[0].replace(/^[.!?;:,\s]+/, '').trim() };
    if (m.index === COMMAND_RE.lastIndex) COMMAND_RE.lastIndex++;
  }
  return found;
}

module.exports = { detectTranslationCommand, TRANSLATIONS: Object.keys(ALIASES) };
