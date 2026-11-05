//! What this church's own services taught Kairo (owner, 2026-10-31): kept by the app between
//! services, on this computer only, and empty until the operator has corrected something. It
//! never decides alone and never holds back a verse the preacher cites by reference.
//!
//! - Words the recogniser makes of this preacher's book names ("San one ten, one to three" for
//!   Psalm 110:1-3, live 2026-10-29), learned when the operator puts up the verse they meant:
//!   put back as the book before a citation is read.
//! - Verses the church has had up time and again: offered as Possible Matches as readily as a
//!   passage in play (meaning search).
//! - Verses the operator took back down after they went up on wording alone: such a verse is
//!   offered, not sent, when it comes up on wording again.

use std::collections::{HashMap, HashSet};

use kairo_bible::{Book, VerseId};
use regex::Regex;

#[derive(Clone, Debug, Default)]
pub struct Learned {
    book_words: Option<(Regex, HashMap<String, Book>)>,
    pub familiar: HashSet<VerseId>,
    pub taken_down: HashSet<VerseId>,
}

/// A number as it's spoken or written, what a learned word must be followed by to be a book.
const NUMBER: &str = r"(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|chapter)";

impl Learned {
    pub fn new(book_words: HashMap<String, Book>, familiar: HashSet<VerseId>, taken_down: HashSet<VerseId>) -> Learned {
        let words: Vec<String> = book_words.keys().map(|w| regex::escape(w)).collect();
        let book_words = (!words.is_empty()).then(|| {
            let re = Regex::new(&format!(r"(?i)\b({})\b(\s+{NUMBER}\b)", words.join("|"))).expect("learned book words");
            (re, book_words.into_iter().map(|(w, b)| (w.to_lowercase(), b)).collect())
        });
        Learned { book_words, familiar, taken_down }
    }

    /// `text` with this preacher's learned words for books put back as the books, where a
    /// number follows ("san one ten" → "Psalms one ten").
    pub fn books_put_back(&self, text: &str) -> String {
        let Some((re, books)) = &self.book_words else { return text.to_string() };
        re.replace_all(text, |c: &regex::Captures| match books.get(&c[1].to_lowercase()) {
            Some(b) => format!("{}{}", b.name(), &c[2]),
            None => c[0].to_string(),
        })
        .into_owned()
    }
}

/// A word in speech just before the operator put up `cited`, that stood for its book: followed
/// by the verse's chapter, and not a word Kairo already knows (`known`). "San one one zero one to
/// three" then Psalms 110:1 → "san".
pub fn book_word(text: &str, chapter: u32, known: impl Fn(&str) -> bool) -> Option<String> {
    let words: Vec<String> = text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).collect();
    for i in (0..words.len().saturating_sub(1)).rev() {
        let w = &words[i];
        if w.len() < 2 || w.chars().any(|c| !c.is_ascii_alphabetic()) || known(w) {
            continue;
        }
        if chapter_follows(&words[i + 1..], chapter) {
            return Some(w.clone());
        }
    }
    None
}

/// The words begin with `chapter`, said as a number or digit by digit ("one ten", "one one zero").
fn chapter_follows(words: &[String], chapter: u32) -> bool {
    use crate::reference::numbers::spoken_to_number;
    let target = chapter.to_string();
    let mut digits = String::new();
    let mut whole: i64 = 0;
    for w in words.iter().take(4) {
        let Some(n) = spoken_to_number(w) else { break };
        if (0..=9).contains(&n) {
            digits.push(char::from(b'0' + n as u8));
        }
        // "one ten" (110) and "one hundred ten" said in pieces
        whole = if n == 100 { whole.max(1) * 100 } else if whole > 0 && whole < 10 { whole * 100 + n } else { whole + n };
        if digits == target || whole.to_string() == target {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preachers_word_for_a_book_is_learned_from_the_verse_put_up() {
        let known = |w: &str| ["one", "to", "three", "zero", "and"].contains(&w);
        assert_eq!(book_word("San one one zero one to three", 110, known).as_deref(), Some("san"));
        assert_eq!(book_word("San one ten, one to three", 110, known).as_deref(), Some("san"));
        assert_eq!(book_word("San one hundred ten", 110, |w| known(w) || w == "hundred").as_deref(), Some("san"));
        assert_eq!(book_word("and the Lord said one to three", 110, |w| known(w) || w == "lord" || w == "said"), None);
        assert_eq!(book_word("San twelve", 110, known), None, "the chapter must follow");
    }

    #[test]
    fn a_learned_word_is_put_back_only_before_a_number() {
        let l = Learned::new(HashMap::from([("san".to_string(), Book::from_name("Psalms").unwrap())]), HashSet::new(), HashSet::new());
        assert_eq!(l.books_put_back("San one ten one to three"), "Psalms one ten one to three");
        assert_eq!(l.books_put_back("san Francisco is far"), "san Francisco is far");
    }
}
