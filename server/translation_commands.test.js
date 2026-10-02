// KAIRO — spoken commands that change the Bible translation (translation_commands.js).
// What a preacher says about translations all through a sermon must not change
// one; what an operator is asked to do must.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { detectTranslationCommand } = require('./translation_commands');

const code = (t) => detectTranslationCommand(t)?.code ?? null;

test('commands to the operator change the translation', () => {
  const cases = [
    ['Switch to NIV.', 'NIV'],
    ['switch to the NIV', 'NIV'],
    ["Let's switch to the NIV.", 'NIV'],
    ['Change it to the New Living Translation.', 'NLT'],
    ['Can we switch to the ESV please?', 'ESV'],
    ['Could you put it in the New King James?', 'NKJV'],
    ["Let's read this in the King James.", 'KJV'],
    ['Could you please use the NASB?', 'NASB'],
    ["Okay, switch the translation to NKJV.", 'NKJV'],
    ["Alright, let's go to the English Standard Version.", 'ESV'],
    ['I want us to read from the NIV today.', 'NIV'],
    ['Switch back to the King James.', 'KJV'],
    ['Switch it back to the King James Version.', 'KJV'],
    ['Change the Bible to the NLT.', 'NLT'],
    ["We'll use the New American Standard Bible for this one.", 'NASB'],
    ['Put it in the NIV.', 'NIV'],
    ['Give us the ESV.', 'ESV'],
    ['Set the translation to NLT.', 'NLT'],
    ['Romans chapter eight, switch to the ESV.', 'ESV'],
    ['turn to John three and let us use the New International Version', 'NIV'],
  ];
  for (const [text, want] of cases) assert.equal(code(text), want, text);
});

test('the letters, spoken or written one by one, count', () => {
  assert.equal(code("Let's use N I V."), 'NIV');
  assert.equal(code('Switch to N.L.T.'), 'NLT');
  assert.equal(code('switch to e s v'), 'ESV');
  assert.equal(code('Change to N K J V'), 'NKJV');
  assert.equal(code('Switch to N-A-S-B'), 'NASB');
  assert.equal(code('Switch to K J V'), 'KJV');
});

test('"New King James" is not the King James', () => {
  assert.equal(code('Switch to the New King James Version.'), 'NKJV');
  assert.equal(code('Switch to the King James Version.'), 'KJV');
});

test('mentioning a translation is not a command', () => {
  const talk = [
    'In the NIV it says that the Lord is my shepherd.',
    'The King James Version was translated in 1611.',
    'If you go to the NIV, verse 16 reads differently.',
    'I love the New Living Translation.',
    'Some people use the ESV and some prefer the NASB.',
    'You can switch to the NLT if you want to follow along.',
    'When you switch to the NIV the wording changes.',
    'Let me read it from my NKJV.',
    'He changed to the NIV last year.',
    'I switched to the ESV years ago.',
    "Don't switch to the NIV just because it sounds easier.",
    'The NIV, the ESV and the NLT all say it a little differently.',
    'In the New Living Translation this is verse eleven.',
    'What does the King James say about this?',
    'Read the NIV and you will see it.',
    'Use the NIV if you like.',
    'Please read the NIV with me.',
    'The ESV is a good translation.',
    'My Bible is the NKJV.',
  ];
  for (const text of talk) assert.equal(code(text), null, text);
});

test('only a translation Kairo has is a command', () => {
  assert.equal(code('Switch to the Message.'), null);
  assert.equal(code('Switch to the Amplified.'), null);
  assert.equal(code("Let's use the NRSV."), null);
});

test('the last command in a stretch of speech wins', () => {
  assert.equal(code('Switch to the NIV. No, switch to the ESV.'), 'ESV');
});

test('empty and non-text input is not a command', () => {
  assert.equal(detectTranslationCommand(''), null);
  assert.equal(detectTranslationCommand(null), null);
  assert.equal(detectTranslationCommand(undefined), null);
});
