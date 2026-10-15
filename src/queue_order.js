// KAIRO — Live Queue order and Possible Matches overlap (pure helpers)
//
// Two rules the Live Queue / Possible Matches panels follow, kept here so
// they can be tested in Node without a DOM:
//
// 1. A passage reads in order. A cited range already lands in reading order
//    (handleRangeVerses), but the same passage read verse by verse — or the
//    verse after a finished range — used to stack newest-first, so "2 Cor
//    4:16-18" showed as 18, 17, 16 (owner, 2026-10-14: "sometimes it stays
//    last, other times it auto arranges to the top"). The run of cards at the
//    top of the queue that form one passage stays in reading order: the next
//    verse joins the end of it, and a verse already in it stays put.
//
// 2. A verse already in the Live Queue isn't offered again in Possible
//    Matches for a while — including the same chapter:verse in the book's
//    numbered sibling (2 Corinthians 4:16 live, 1 Corinthians 4:16 offered at
//    93%, same incident). That's the engine guessing the book number, not a
//    second citation.

(function (root) {
  'use strict';

  // How long a queued verse keeps its sibling-book look-alikes out of
  // Possible Matches (the same span a suggestion itself lives for).
  const OVERLAP_WINDOW_MS = 5 * 60 * 1000;

  // "2 Corinthians 4:18" → { book: '2 Corinthians', chapter: 4, verse: 18 }
  function parseRef(reference) {
    const m = /^(.+?)\s+(\d+):(\d+)$/.exec(String(reference || '').trim());
    return m ? { book: m[1], chapter: Number(m[2]), verse: Number(m[3]) } : null;
  }

  // The book with any number in front of it taken off: 1/2/3 John, I/II/III
  // John and "First/Second/Third John" all become "john".
  function bookFamily(book) {
    return String(book || '').trim().toLowerCase()
      .replace(/^(?:[123]|iii|ii|i|first|second|third|1st|2nd|3rd)\s+/, '');
  }

  // Same chapter:verse in the same book or one of its numbered siblings.
  function sameVerseAnyBookNumber(a, b) {
    const x = parseRef(a), y = parseRef(b);
    return !!(x && y && x.chapter === y.chapter && x.verse === y.verse && bookFamily(x.book) === bookFamily(y.book));
  }

  // Does any queued entry ({ ref, at }) cover this reference, exactly (for
  // as long as it's queued) or as a sibling-book look-alike (within the window)?
  function coveredByQueue(reference, queued, now, windowMs = OVERLAP_WINDOW_MS) {
    return (queued || []).some(q => q.ref === reference
      || (now - (q.at || 0) <= windowMs && sameVerseAnyBookNumber(q.ref, reference)));
  }

  // The cards at the top of the queue (refs, top first) that form one passage:
  // same book and chapter, consecutive verses in reading order. Returns how
  // many of the leading refs belong to it (0 when the queue is empty).
  function topPassageLength(refs) {
    if (!refs || !refs.length) return 0;
    const first = parseRef(refs[0]);
    if (!first) return 1;
    let n = 1, prev = first;
    while (n < refs.length) {
      const r = parseRef(refs[n]);
      if (!r || r.book !== first.book || r.chapter !== first.chapter || r.verse !== prev.verse + 1) break;
      prev = r; n++;
    }
    return n;
  }

  // Where a verse being sent goes in the queue (refs, top first):
  //   { action: 'stay' }            — already part of the top passage
  //   { action: 'after', index: i } — the next verse of the top passage: just after refs[i]
  //   { action: 'top' }             — anything else: a new send goes to the top
  function placeInQueue(refs, reference) {
    const n = topPassageLength(refs);
    if (!n) return { action: 'top' };
    const passage = refs.slice(0, n);
    // A one-card "passage" is just the last send: re-sending it is a no-op,
    // and the verse after it reads on below it.
    if (passage.includes(reference)) return { action: 'stay' };
    const last = parseRef(passage[n - 1]), r = parseRef(reference);
    if (last && r && r.book === last.book && r.chapter === last.chapter && r.verse === last.verse + 1) {
      return { action: 'after', index: n - 1 };
    }
    return { action: 'top' };
  }

  const API = { OVERLAP_WINDOW_MS, parseRef, bookFamily, sameVerseAnyBookNumber, coveredByQueue, topPassageLength, placeInQueue };

  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoQueueOrder = API;

})(typeof window !== 'undefined' ? window : null);
