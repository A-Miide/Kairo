// KAIRO — what the numbers on a slide are.
//
// A scripture typed or imported as a SLIDE (a ProPresenter deck, say) carries its words in
// separate boxes: the verse, a big "48." that is the verse's number, and the reference
// "JOHN 6:48-58". Joined in drawing order that reads "I am that bread of life. 48. John
// 6:48-58". This tells the three apart:
//   marker     a box that is only a number ("48.", "(49)", "3-4"): the verse number when the slide
//              also names a passage it falls inside, else an index (a list's "2.")
//   reference  a box that is only a citation ("JOHN 6:48-58")
//   body       everything else, in its order
// The marker is tracked either way, so a deck can be followed by it: a spoken "verse 49" belongs
// to the slide marked 49 of the passage on screen, not to every slide of that passage.
'use strict';

(function (root) {
  const NUM = /^[(\[]?\s*(\d{1,3})(?:\s*[-–—]\s*(\d{1,3}))?\s*[.):\]]?$/;
  const CITE = /^((?:[1-3]|i{1,3}|first|second|third)\s*)?([a-z]+(?:\s+(?:of\s+)?[a-z]+){0,2})\.?\s+(\d{1,3})\s*[:.]\s*(\d{1,3})(?:\s*[-–—]\s*(\d{1,3}))?$/i;
  // Words that make a "citation" something else ("Page 3:15").
  // Days and months too: an announcement's "Sunday 9:30" is a time, not a chapter and verse.
  const NOT_BOOK = /^(page|pages|slide|slides|chapter|verse|verses|step|point|at|time|room|from|until|by|meet|meets|start|starts|begin|begins|end|ends|call|doors|service|services|ratio|score|vote|sunday|monday|tuesday|wednesday|thursday|friday|saturday|today|tonight|tomorrow|january|february|march|april|may|june|july|august|september|october|november|december|sun|mon|tue|tues|wed|thu|thur|thurs|fri|sat)$/i;

  function parse(text) {
    const lines = String(text || '').split(/\n+/).map(s => s.trim()).filter(Boolean);
    let marker = null, markerEnd = null, reference = null, citeRange = null;
    const body = [];
    for (const line of lines) {
      const n = marker == null && NUM.exec(line);
      if (n) { marker = Number(n[1]); markerEnd = n[2] ? Number(n[2]) : marker; continue; }
      const c = reference == null && CITE.exec(line);
      if (c && !c[2].split(/\s+/).some(w => NOT_BOOK.test(w))) {
        reference = line;
        citeRange = { chapter: Number(c[3]), verse: Number(c[4]), verseEnd: c[5] ? Number(c[5]) : Number(c[4]) };
        continue;
      }
      body.push(line);
    }
    // A number inside the passage the slide names is a verse number; any other is an index.
    let kind = null;
    if (marker != null) kind = citeRange && marker >= citeRange.verse && markerEnd <= citeRange.verseEnd ? 'verse' : 'index';
    return { marker, markerEnd, kind, reference, body: body.join(' '), citeRange };
  }

  // The words as one line for a screen that shows plain text: "48. I am that bread of life."
  function display(text) {
    const p = parse(text);
    if (p.marker == null && !p.reference) return { text: String(text || ''), reference: '' };
    const num = p.marker == null ? '' : `${p.marker}${p.markerEnd !== p.marker ? '–' + p.markerEnd : ''}. `;
    return { text: (num + p.body).trim(), reference: p.reference || '' };
  }

  // Does this deck slide carry the spoken verse? `slide` is { refs, marker, markerEnd, markerKind }
  // (refs from /api/slides/scripture); a slide marked as one verse of the passage carries only that
  // verse, one without a marker carries the whole passage it names.
  function carries(slide, v) {
    if (!slide || !v) return false;
    const refs = Array.isArray(slide.refs) ? slide.refs : [];
    const inRef = refs.some(r => r.book === v.book && r.chapter === v.chapter && v.verse >= r.verse && v.verse <= r.verseEnd);
    if (!inRef) return false;
    if (slide.marker != null && slide.markerKind === 'verse') return v.verse >= slide.marker && v.verse <= (slide.markerEnd ?? slide.marker);
    return true;
  }

  const API = { parse, display, carries };
  if (typeof module !== 'undefined' && module.exports) module.exports = API;
  if (root) root.KairoSlideMarkers = API;
})(typeof window !== 'undefined' ? window : null);
