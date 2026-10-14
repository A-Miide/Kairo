# 02 — Spoken citations (`processForReferences` and its helpers)

Source: `server/server.js` ~5103–5946, `server/reference_parser.js`,
`server/chapter_resolver.js`. Parsing itself is `kairo-detect::reference`
(see the decision log); this is what happens to a parsed citation.

## Before a citation counts

- **Choosing among books** (`resolveAmbiguousRefs`), for a citation whose book
  could be several (unnumbered stem, misheard ordinal):
  1. an ordinal spoken just before the stem in the last ~15 s, across segment
     breaks ("…listed in second" | "Timothy three");
  2. else the book being read;
  3. else the book of the last citation that went up in the past minute;
  4. else unresolved: every alternative is offered in Possible Matches, none
     sent, and nothing is recorded as context.
- **A book named without its number continues the numbered book being read**
  (bare "John 5:19" right after 1 John 4), only if that verse exists there.
  General rule: any book whose name is also a numbered book's stem.
- **Interim citations** (the last citation in a growing interim, while the text
  looks unfinished — ending on a number or "and/to/through/verse/verses"):
  - ending on a number that could still grow (four, six, seven, eight, nine,
    round tens, hundred — their spoken form begins a longer number) → wait;
  - otherwise act only when the identical citation was parsed on an earlier
    interim within 3 s.
  - A citation the interim has already moved past acts immediately.
- **Final segments ending on a dangling word** (and/to/through/verse/verses)
  don't act; the next segment's join completes them.

## Joining segments

- No citation in this final (or this final opens with a number while the
  previous one stopped mid-citation): parse the previous final + this one,
  within 8 s. Only citations the join creates or extends count — one the
  previous segment already held was handled with it.
- The offline engine locks a few words at a time; while speech runs on across
  locks, the previous text carries the last 12 locked words, not one lock.

## A chapter said twice

`altChapter` from the parser ("Luke chapter six four and verse 14"): when both
readings exist, hold for up to 3 s; the words read after the citation's verse
choose (a 2-word margin on words only one verse has, ignoring everyday
preaching words). Undecided at the hold's end → the plain reading goes up and
the other is offered. If the screen has already moved to either, or another
citation came, nothing is sent. Re-hearing the same citation without the extra
number: alone, the second chapter is the one meant.

## When a citation has no verse

- Bare "Book chapter N":
  - the chapter of the verse on screen named again → the preacher is pointing
    at what's up; nothing happens;
  - interim → a strict number callout only ("…Matthew 11, say, come to me.
    28, all you…") — `resolveChapterCallout`;
  - final → the words that follow are matched to the chapter's verses
    (`resolveChapterByKeywords`) and sent as `chapter-keyword`;
  - not resolved → the chapter waits (`chapterOnlyPending`): following
    segments add their words (last 50) and retry, until a verse of that chapter
    reaches the screen by any path.
- Bare book mentions (no chapter) set the reading context's book, keeping its
  chapter — but a different book only replaces an active chapter when it is
  announced ("the book of", "turn to", or followed by a chapter number).

## When the verse doesn't exist

- "Genesis 24:83" (Genesis 24 has 67): search the cited chapter's verses
  against the recent speech (`resolveInvalidVerseByContext`). Not for
  one-chapter books — there the "chapter" was never said, so the citation
  itself is unreliable.
- Still nothing, and a book that sounds like it has that exact verse ("Jonah
  3:16" → John 3:16): offered, never sent. (A chapter the heard book lacks is
  swapped in the parser — that one is certain.)

## Sending

- The context is updated (book, chapter) and the sermon context records the
  citation — never for an unresolved choice among books.
- A range is active and this is its next verse → the range advances.
- Final:
  - several verses → set the range queue (first verse up, the rest queued);
    one verse → clear it;
  - already put up from this utterance's interim → not re-sent;
  - the final re-hears a citation of the same book under another chapter
    than the one already up from this sentence's interim, and the words read
    fit the verse up at least as well → it stays.
  - Sent as `direct`; a single verse then starts the miscitation check.
- Interim: same decision path, plus: the same verse isn't re-entered from the
  same growing interim; a different chapter of a book already put up from this
  sentence waits for the final (re-hearing), unless the sentence still
  contains the first citation (a genuine second citation goes up).

## The bare "verse N" (context) path

See `00-funnel.md`; resolution rules are in `kairo-detect::reference::partial`.
A resolution refreshes the reading context only when it reached the screen, or
that exact verse is already on screen (within 15 s).
