//! Books named without a chapter ("…over in Leviticus…", "turn to 1 John"),
//! which hold the book in play until the chapter and verse are said.
//!
//! - Numbered books ("1 John", "first Corinthians") always count.
//! - A book introduced as a book ("the book of", "turn to", "read from", …)
//!   counts, even one whose name is also an everyday word or a person's name.
//! - Otherwise only names that aren't everyday words or people's names count.
//! - A numbered book's stem alone ("Corinthians") never does: nothing says
//!   which one; the citation that follows decides.

use kairo_bible::Book;

use super::books::{numbered_prefix, Books};
use super::i18n::{localize, Lang};
use crate::re::words;

/// Phrases that introduce a book as a book.
const INTRODUCTIONS: &[&str] = &[
    "book of", "turn to", "turn with me to", "open to", "open your bible to", "read from", "read in", "read out of",
    "scripture in", "found in", "writings of", "gospel of", "gospel according to", "epistle of", "letter of", "letter to",
    "prophet",
];

pub fn book_mentions(text: &str, books: &Books, lang: Lang) -> Vec<Book> {
    let localized;
    let text = if lang == Lang::En {
        text
    } else {
        localized = localize(text, lang);
        localized.as_str()
    };
    let mut found: Vec<Book> = Vec::new();
    let mut add = |b: Book| {
        if !found.contains(&b) {
            found.push(b);
        }
    };

    // Introductions are read on the raw text: normalizing would turn
    // "the book of Exodus" into "the exodus" and lose the introduction.
    let raw: String = text
        .to_lowercase()
        .replace(['\'', '’'], "'")
        .split(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':') || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.strip_suffix("'s").or_else(|| s.strip_suffix('\'')).unwrap_or(s))
        .collect::<Vec<_>>()
        .join(" ");
    for intro in INTRODUCTIONS {
        let mut from = 0;
        while let Some(at) = raw[from..].find(intro) {
            let start = from + at + intro.len();
            let after: Vec<&str> = raw[start..].split_whitespace().take(2).collect();
            let first = after.first().copied().unwrap_or("");
            if let Some(n) = numbered_prefix(first) {
                if let Some(b) = after.get(1).and_then(|stem| books.numbered(n, stem)) {
                    add(b);
                }
            } else if books.is_single_word(first) && books.numbered_variants(first).is_none() {
                if let Some(b) = books.alias(first) {
                    add(b);
                }
            }
            from = start;
        }
    }

    let prepared = books.apply_titles(&super::normalize::normalize(text, books, Lang::En));
    let w = words(&prepared);
    let mut i = 0;
    while i < w.len() {
        if let Some(n) = numbered_prefix(w[i]) {
            if let Some(b) = w.get(i + 1).and_then(|stem| books.numbered(n, stem)) {
                add(b);
                i += 2;
                continue;
            }
        }
        if books.is_single_word(w[i]) && !books.is_ambiguous(w[i]) && books.numbered_variants(w[i]).is_none() {
            if let Some(b) = books.alias(w[i]) {
                add(b);
            }
        }
        i += 1;
    }
    found
}
