# 07 — Ranges, spoken triggers and the sermon's context

Source: `server/server.js` ~1288–2368.

## Ranges

A cited passage of several verses: the first goes up, the rest queue. Every
advance goes through one guarded request (no double advance from concurrent
triggers), at most one per 1.2 s.

**What can move a range:** only speech heard since it last moved. The words
of the open segment already said when it moved are spent; earlier finals are
gone (one stretch of speech moves the range once — otherwise a sentence's
final, arriving after its interim already advanced, advanced it again). The
offline engine's lock carry-over applies here too. At most 160 words kept.

Triggers, each independent:
1. **The current verse's ending** — of its last three meaningful words, at
   least 2 (or all if fewer) heard within the last 8 meaningful words, each
   heard as many times as the verse says it (a verse that repeats a word in
   its ending isn't finished by its first occurrence). The reading considered
   is at most the verse's length + 15 words.
2. **The next verse's opening** — its first four meaningful words, minus words
   the current verse also has: one identifying word (IDF ≥ 3.5; ≥ 4 letters
   before IDF is known) heard, or else two of the remaining words. Not when the
   segment names another book.
3. **"Next verse" / "previous verse"** (see below).
4. **A bare number** equal to the next verse.
5. **A spoken reference** to exactly the next verse.
6. **Any method's detection** of exactly the next verse (see `06`).

`jumpRangeTo`: a verse inside the active range repositions it (operator Send,
"verse N", verse-end jump); outside, the range is cleared only if the verse
actually reached the screen.

## Spoken triggers

- **"next verse" / "following verse" / "verse after that"** in the last 8
  words (localized): advance the range; or, with no range, the verse after the
  one on screen (if it went up in the last 3 minutes). **"previous verse" /
  "verse before" / "preceding verse"**: the verse before. 4 s cooldown. Sent as
  `continuation`.
- **Bare-number continuation**: a segment whose last clause is only a number
  (filler aside — "…so do you, 51"), equal to the verse on screen + 1; not a
  homophone word; outside a range only within 15 s of the last verse going up.
  2 s cooldown.
- **Verse-end number jump**: within 8 s of the last three meaningful words of
  the verse on screen being heard, a segment that is only a number (≤ 4 words
  with filler) jumps to that verse of the chapter. 2 s cooldown.
- **Spoken translation change** ("switch to the NIV") — finals, English, a
  clause starting with the command (see `translation_commands.js`); joined
  with the previous final's last 6 words when the pause fell inside it. The
  verse and range on screen are shown again in it; matching follows at a quiet
  moment (no speech for 4 s, no citation for 6 s).

## Reading mode

Verbatim hits ≥ 0.92: 3 or more within a 30 s window turns reading mode on
(for that verse's chapter); fewer turns it off. Feeds passage affinity.

## Topic library

Final words of 5+ letters are counted; every 60 s, words heard twice or more
with IDF ≥ 3 rank by frequency × IDF (top 20) and rebuild the topic library
(80 verses); counts then halve (≈ 2 min memory). Needs ≥ 3 candidates and ≥ 2
topic words.

## Sermon context (recent citations)

The last 8 passages in play within 5 minutes, each marked cited (named) or
read (a strong text match: verbatim ≥ 0.92, fingerprint ≥ 0.87). A repeat
refreshes its entry; a reading of a named verse leaves the citation as it was;
over the cap, read entries go first. Used by the nudge, ranking, affinity,
twin resolution and ambiguity resolution.

## Pace

Words per minute over the last ~90 s of finals: 1 at ≥ 120 wpm, up to 2 at
half that. Stretches the reading context's expiry.
