//! "Go back to Romans 10": a chapter cited earlier in the service, returned to. The preacher means
//! the verse called there, not wherever the reading of it ended (owner, 2026-11-04): Romans 10:10
//! cited, read on to 10:13, a detour elsewhere, then "let's go back to Romans 10" is 10:10 again; a
//! range called (10:9–13) comes back at its first verse. A chapter not cited before in the service,
//! or a chapter named without a return cue, is handled as any chapter named alone; a verse named or
//! read after the return takes over as usual.

use std::sync::LazyLock;

use kairo_bible::{Book, VerseId};

use super::citations::interim_looks_incomplete;
use super::core::{Candidate, Engine};
use super::types::{Method, Target};
use crate::re::{re, re_kept};

/// What comes just before the book when the preacher returns to it: "go back to", "back in",
/// "let's return to the book of", "come back again to First".
static RETURN_CUE: LazyLock<regex::Regex> = LazyLock::new(|| {
    re(r"(?i)\b(back|return|returning)\s+(again\s+)?(to|into|in|unto)?\s*(the\s+book\s+of\s+)?((first|second|third|1st|2nd|3rd|[123]|one|two|three|i{1,3})\s+)?$")
});

#[derive(Default)]
pub(crate) struct BackTo {
    /// Each chapter's verse last called this service (a range: its first verse).
    called: std::collections::HashMap<(Book, u16), VerseId>,
    /// The return already acted on: its chapter and where in the word buffer it was said, so the
    /// interims and final that repeat it don't send it again.
    done: Option<(Book, u16, u64)>,
}

impl Engine {
    /// A verse was called by reference (or its chapter's verse chosen for a citation).
    pub(crate) fn note_called(&mut self, v: VerseId) {
        let r = self.r(v);
        self.back_to.called.insert((r.book, r.chapter), v);
    }

    /// `book` `chapter` named alone with a return cue before it, and called earlier this service: its
    /// verse goes back up. True when the citation is handled.
    pub(crate) fn back_to_chapter(&mut self, book: Book, chapter: u16, text: &str, is_final: bool, now: u64) -> bool {
        let Some(&v) = self.back_to.called.get(&(book, chapter)) else { return false };
        let Some(before) = text_before_book(text, book) else { return false };
        if !RETURN_CUE.is_match(before) {
            return false;
        }
        // "…back to Romans ten" may still be growing into "ten thirteen": the final or more words decide.
        if !is_final && interim_looks_incomplete(text, self.cfg.lang) {
            return true;
        }
        let from = self.said_at(book, text);
        if self.back_to.done == Some((book, chapter, from)) {
            return true;
        }
        self.back_to.done = Some((book, chapter, from));
        self.note(|e| format!("back to {} {chapter}: {} called there earlier", book.name(), e.label(v)));
        self.chapter_pending = None;
        self.chapter_resolved_for = Some((book, chapter));
        self.screen.last_citation_at = now;
        self.reading.update(book, Some(chapter as u32), now);
        self.set_soft_range(v, now);
        self.decide(Candidate::new(vec![v], Method::Direct, 1.0, Target::Screen), now);
        true
    }
}

/// The text before the last mention of `book`.
fn text_before_book(text: &str, book: Book) -> Option<&str> {
    let last = book.name().to_lowercase().split(' ').next_back().unwrap_or("").to_string();
    let stem = last.strip_suffix('s').unwrap_or(&last);
    re_kept(&format!(r"(?i)\b{}s?\b", regex::escape(stem))).find_iter(text).last().map(|m| &text[..m.start()])
}
