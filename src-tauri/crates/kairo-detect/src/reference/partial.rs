//! "verse 17", "verses 3 through 5", "chapter 4 verse 2", "the last verse",
//! "…fifteen." — resolved against the book and chapter being read.
//!
//! In order of specificity:
//! 1. "chapter N verse M" with no book: the book being read, chapter N.
//! 2. "the first/last verse" (not "…of <something>", which names its own
//!    passage): that verse of the chapter being read.
//! 3. "verse N" (with ranges and "N and N+1" lists): the chapter being read —
//!    unless another passage is being named just before it (another book, a
//!    word that sounds like a book, or another chapter number), or "verse N
//!    of …" names one that isn't "this/that/the same chapter".
//! 4. A number said on its own (only filler before it, nothing after): a
//!    verse call. Off for interim transcripts — a fleeting partial transcript
//!    is often just a number before the sentence arrives. Never a number word
//!    that is mostly an ordinary word ("for", "first"), never "one" inside a
//!    longer phrase ("until one repents"), never right after a book name
//!    (that makes it a chapter).
//!
//! Spoken clock times ("4:10") are removed first: nobody says a colon in a
//! citation, so in a transcript it's the engine formatting a time.

use std::sync::LazyLock;

use super::books::{levenshtein, Books};
use super::citation::{Citation, Shape, Verses};
use super::i18n::Lang;
use super::numbers::{consume_number, is_ambiguous_number_word};
use super::parse::Active;
use crate::re::re;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partial {
    pub citation: Citation,
    /// The chapter was spoken ("chapter 4 verse 2"), not taken from context.
    pub chapter_given: bool,
}

/// Words that can come before a bare verse number and still leave it a
/// verse call.
pub(crate) const BARE_NUMBER_FILLER: &[&str] =
    &["and", "so", "now", "okay", "ok", "um", "uh", "then", "also", "yes", "yeah", "right", "alright", "verse", "verses", "next"];

/// Words skipped when looking back from "verse N" for another passage. "from" is one letter from
/// "rom" (Romans): "go from verse twenty one" was read as another book and the verse dropped (live,
/// 2026-10-27).
const LOOKBACK_SKIP: &[&str] = &["and", "i", "mean", "sorry", "uh", "um", "so", "now", "the", "in", "from", "at", "start", "starting", "read", "reading", "go", "let", "lets", "us"];

/// Everyday words one letter away from a book name, which often come right
/// before "verse" ("like verse 3 says").
const NOT_BOOK_NAMES: &[&str] = &[
    "like", "lake", "luck", "lure", "duke", "truth", "join", "jobs", "facts", "arts", "park", "dark", "mask", "marks",
    "games", "palm", "june", "ester",
];

pub struct PartialResolver<'a> {
    pub books: &'a Books,
    pub shape: &'a Shape,
    pub lang: Lang,
}

fn get<'a>(w: &[&'a str], i: usize) -> &'a str {
    w.get(i).copied().unwrap_or("")
}

fn is_verse_word(w: &str) -> bool {
    matches!(w, "verse" | "verses" | "vers")
}

fn n32(v: i64) -> u32 {
    v.clamp(0, u32::MAX as i64) as u32
}

impl PartialResolver<'_> {
    pub fn resolve(&self, text: &str, active: Active, allow_bare_number: bool) -> Option<Partial> {
        static CLOCK: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)[0-9]{1,2}:[0-9]{2}(?-u:\b)"));
        let book = active.book?;
        let text = CLOCK.replace_all(text, " ");
        let prepared = super::normalize::normalize(&text, self.books, self.lang);
        let w: Vec<&str> = prepared.split(' ').filter(|s| !s.is_empty()).collect();
        let found = |chapter: u32, verses: Verses, chapter_given: bool| Some(Partial { citation: Citation::new(book, chapter, verses), chapter_given });

        for i in 0..w.len() {
            if w[i] != "chapter" {
                continue;
            }
            let Some(ch) = consume_number(&w, i + 1, None) else { continue };
            let j = i + 1 + ch.consumed;
            if !is_verse_word(get(&w, j)) {
                continue;
            }
            let Some(v) = consume_number(&w, j + 1, None) else { continue };
            if n32(ch.value) > self.shape.chapters(book) {
                continue;
            }
            return found(n32(ch.value), Verses::One(n32(v.value)), true);
        }

        if let Some(chapter) = active.chapter {
            for i in 0..w.len().saturating_sub(1) {
                if w[i + 1] != "verse" || get(&w, i + 2) == "of" {
                    continue;
                }
                let verse = match w[i] {
                    "first" | "opening" => Some(1),
                    "last" | "final" | "closing" => self.shape.verses(book, chapter),
                    _ => None,
                };
                if let Some(v) = verse {
                    return found(chapter, Verses::One(v), false);
                }
            }
        }

        // The last verse called counts: "go to verse twenty three … I believe from verse twenty three
        // or twenty one … let's go from verse twenty" is verse 20 (live 2026-10-28); "verse twenty four,
        // no, verse twenty two" is 22.
        let mut last = None;
        let mut i = 0;
        while i < w.len() {
            if !is_verse_word(w[i]) {
                i += 1;
                continue;
            }
            let Some(v) = consume_number(&w, i + 1, None) else {
                i += 1;
                continue;
            };
            let start = n32(v.value);
            if start < 1 || start > self.shape.max_verse() {
                i += 1;
                continue;
            }
            let mut chapter = active.chapter?;
            // "the next chapter verse one": the chapter after the one in play, said outright.
            let mut moved = false;
            if i >= 2 && w[i - 1] == "chapter" {
                match w[i - 2] {
                    "next" | "following" if self.shape.chapters(book) > chapter => (chapter, moved) = (chapter + 1, true),
                    "previous" | "last" if chapter > 1 => (chapter, moved) = (chapter - 1, true),
                    _ => {}
                }
            }
            if self.names_other_passage_before(&w, i, active) {
                return None;
            }
            let mut j = i + 1 + v.consumed;
            if get(&w, j) == "of" {
                let k = j + 1 + usize::from(matches!(get(&w, j + 1), "this" | "that" | "the" | "same")) + usize::from(get(&w, j + 2) == "same");
                if !(k > j + 1 && matches!(get(&w, k), "chapter" | "passage")) {
                    return None;
                }
            }
            if matches!(get(&w, j), "to" | "through") {
                let mut a = j + 1;
                if matches!(get(&w, a), "verse" | "verses") {
                    a += 1;
                }
                let end = if get(&w, a) == "end" { self.shape.verses(book, chapter) } else { consume_number(&w, a, None).map(|e| n32(e.value)) };
                if let Some(end) = end.filter(|e| *e >= start) {
                    let used = if get(&w, a) == "end" { a + 1 } else { a + consume_number(&w, a, None).map_or(1, |e| e.consumed) };
                    last = Some((chapter, Verses::Span(start, end), moved));
                    i = used;
                    continue;
                }
            }
            let mut end = start;
            while get(&w, j) == "and" {
                let mut a = j + 1;
                if matches!(get(&w, a), "verse" | "verses") {
                    a += 1;
                }
                let Some(n) = consume_number(&w, a, None) else { break };
                if n32(n.value) != end + 1 || (n.consumed == 1 && is_ambiguous_number_word(get(&w, a))) {
                    break;
                }
                end = n32(n.value);
                j = a + n.consumed;
            }
            last = Some((chapter, if end > start { Verses::Span(start, end) } else { Verses::One(start) }, moved));
            i = j.max(i + 1);
        }
        if let Some((chapter, verses, moved)) = last {
            return found(chapter, verses, moved);
        }

        if allow_bare_number {
            let chapter = active.chapter?;
            for i in 0..w.len().min(3) {
                if i > 0 && !BARE_NUMBER_FILLER.contains(&w[i - 1]) {
                    break;
                }
                let Some(n) = consume_number(&w, i, None) else { continue };
                if i + n.consumed != w.len() {
                    continue;
                }
                if i > 0 && (self.books.alias(w[i - 1]).is_some() || self.books.numbered_variants(w[i - 1]).is_some()) {
                    continue;
                }
                if n.consumed == 1 && (is_ambiguous_number_word(w[i]) || (w[i] == "one" && w.len() > 1)) {
                    continue;
                }
                let v = n32(n.value);
                if v < 1 || v > self.shape.max_verse() {
                    continue;
                }
                return found(chapter, Verses::One(v), false);
            }
        }
        None
    }

    /// Is another passage named just before "verse" at `i`?
    fn names_other_passage_before(&self, w: &[&str], i: usize, active: Active) -> bool {
        let mut looked = 0;
        let mut k = i;
        while k > 0 && looked < 4 {
            k -= 1;
            looked += 1;
            let word = w[k];
            if LOOKBACK_SKIP.contains(&word) {
                continue;
            }
            if is_verse_word(word) {
                return false;
            }
            let alias = if k > 0 { self.books.alias(&format!("{} {}", w[k - 1], word)) } else { None }.or_else(|| self.books.alias(word));
            if let Some(b) = alias {
                return Some(b) != active.book;
            }
            if self.sounds_like_a_book(word) {
                return true;
            }
            for st in k.saturating_sub(2)..=k {
                let Some(n) = consume_number(w, st, None) else { continue };
                if st + n.consumed != k + 1 {
                    continue;
                }
                if n.consumed == 1 && (is_ambiguous_number_word(word) || word == "one" || word == "oh") {
                    return false;
                }
                if st > 0 && is_verse_word(w[st - 1]) {
                    return false;
                }
                return Some(n32(n.value)) != active.chapter;
            }
            return false;
        }
        false
    }

    fn sounds_like_a_book(&self, w: &str) -> bool {
        let len = w.chars().count();
        if len < 4 || NOT_BOOK_NAMES.contains(&w) || self.books.is_single_word(w) {
            return false;
        }
        self.books.single_words().any(|a| a.chars().count().abs_diff(len) <= 1 && levenshtein(w, a) == 1)
    }
}
