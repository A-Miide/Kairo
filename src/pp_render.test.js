// Tests for src/pp_render.js: ProPresenter's Title Case.
'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');

globalThis.window = globalThis.window || {};
require('./pp_render.js');
const { titleCase } = window.KairoPP;
const tc = (t) => titleCase(t, { start: true });

test('text typed in capitals reads like text typed in lower case', () => {
  assert.equal(tc('WHAT A FELLOWSHIP,WHAT A JOY DIVINE.'), 'What a Fellowship,What a Joy Divine.');
  assert.equal(tc('WHAT a Fellowship, What a Joy Divine,'), 'What a Fellowship, What a Joy Divine,');
});

test('small words stay small, except at the start or after a sentence ends', () => {
  assert.equal(tc('leaning on the everlasting arms'), 'Leaning on the Everlasting Arms');
  assert.equal(tc('safe and secure. a peace divine'), 'Safe and Secure. A Peace Divine');
});
