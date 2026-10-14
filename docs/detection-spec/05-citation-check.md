# 05 — Checking a citation against what's read next

Source: `server/server.js` ~7113–7369 (`startCitationCheck`,
`checkCitationSiblings`, `switchCitationToSibling`, `twinInPlay`,
`streamAnchorsNear`).

A citation goes up as heard (it's usually right and seconds matter); the words
around it then decide whether a sibling was meant.

## Siblings of a cited verse (at most 4)

- Books that sound like the cited book (computed in Rust) and same-name books
  with another number ("John" ↔ "1/2/3 John"), at the same chapter and verse.
- Verse numbers that sound like the cited one: tens off by one (53 ↔ 63),
  teen ↔ -ty (13 ↔ 30), a dropped or extra "1" (8 ↔ 18).
- Only siblings that exist.

For a verse put up by the streaming matcher, the siblings are the other verses
the stream anchored on in the last 10 s — most specific phrase first, then
most identifying, then most recent — excluding verses on screen in the last
minute and the verse's twins (twins have their own resolver).

## Deciding (for up to 35 words / 25 s after the citation)

The text judged: the words just before the citation (up to 30, from the
segment and the previous 8 s — preachers often paraphrase first, then cite)
plus everything said since.

1. **Rare words** (IDF ≥ 3.5) only one of the two verses has, heard: the cited
   verse leads by 2 → settled, check ends. A sibling leads by 2 → candidate.
2. **Meaning** (6+ words judged; on every final, and every 8 words of speech
   in progress; one check at a time): cross-encoder and cosine over at most
   three windows (the last 40 words, the first two paraphrase windows).
   - Switch to a sibling when it reads as said (rr ≥ 0.5), the cited verse
     reads as not said (rr ≤ 0.1), and the sibling is closer in meaning by
     ≥ 0.1.
   - Both read as said (a phrase they share) → the rare words decide (citation
     siblings only, not a reading match's looser set).
   - No cross-encoder → the rare words decide.
3. A sibling that is only another verse NUMBER of the same chapter must also
   be the chapter's best match for what was said; a clear chapter winner that
   isn't it vetoes.
4. The check ends when something else reaches the screen.

Switching: the cited verse is remembered as corrected away; the sibling goes up
as a citation marked corrected; sermon and reading contexts move to it.

## Twin in play

A verse is "in play" if it was on screen in the last minute, is in the active
range, or was cited in the last 5 minutes. A text-matched candidate is
explained by the passage in play when one of those is its twin — or, in Rust,
in a chapter confusable with it (derived from twins; Node used a registry).
