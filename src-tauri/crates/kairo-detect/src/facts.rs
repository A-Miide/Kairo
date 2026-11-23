//! Facts about every verse that make live detection go wrong, worked out once when the Bible loads,
//! from every wording Kairo matches (the KJV, the church's own Bible, and the ones matched for
//! everyone). Each fact feeds one rule in the engine, and the `bible-profile` report (kairo-eval)
//! reads the same table, so the analysis and the detector can't drift apart. Look-alike verses are
//! the lexicon's (`Lexicon::lookalikes`), built with the twin table.
//!
//! - `numbers`: number phrases in a verse's words ("two thousand fifty and six" in Ezra 2:14), so a
//!   number read out of the passage isn't taken for "verse six" (engine/ranges.rs).
//! - `opening_inside_before`: the verse's first words also come in the verse before it, in any wording
//!   ("…the LORD spake unto Moses…", or both opening "Blessed shall be"), so its opening alone can't move a passage on before the
//!   verse up is read to its end. Every other verse moves on as its opening is heard.
//! - `book_words`: words of the verse the citation reader takes for a book when a number follows
//!   ("Levites" for Leviticus, "curtains" for Corinthians, "mark", "acts"), so reading them out of a
//!   passage isn't a citation (engine/ranges.rs).
//! - `short`: four words or fewer in every wording: read once it's up.

use std::collections::{HashMap, HashSet};

use kairo_bible::{Book, Corpus, VerseId};

use crate::lexical::Lexicon;
use crate::reference::numbers::consume_number;
use crate::reference::{Active, Parser};

/// A verse this short in every wording is read by the time it's up.
pub const SHORT_VERSE_WORDS: usize = 4;
/// The opening that can be said too early: this many first words.
const OPENING_WORDS: usize = 3;
/// Words kept before a number phrase, to tell it from a verse called ("…fifty and six").
const NUMBER_CONTEXT_WORDS: usize = 2;

#[derive(Default)]
pub struct BibleFacts {
    numbers: HashMap<VerseId, Vec<Vec<String>>>,
    opening_inside_before: HashSet<VerseId>,
    book_words: HashMap<VerseId, Vec<(String, Book)>>,
    short: HashSet<VerseId>,
}

/// A verse's words as the engine compares them: lowercase, letters and digits only.
pub fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()).filter(|w| !w.is_empty()).collect()
}

fn is_number_word(w: &str) -> bool {
    consume_number(&[w], 0, None).is_some()
}

impl BibleFacts {
    pub fn build(corpus: &Corpus, lexicon: &Lexicon, parser: &Parser) -> BibleFacts {
        let mut f = BibleFacts::default();
        // Each distinct word once: is it a number, and the book the citation reader makes of it when
        // a number follows ("levites one" is Leviticus 1).
        let mut kind: HashMap<String, (bool, Option<Book>)> = HashMap::new();
        let mut kind_of = |x: &str| -> (bool, Option<Book>) {
            if let Some(k) = kind.get(x) {
                return *k;
            }
            let number = is_number_word(x);
            let book = (!number && x.len() >= 3).then(|| parser.parse_one(&format!("{x} one"), Active::default()).map(|c| c.book)).flatten();
            kind.insert(x.to_string(), (number, book));
            (number, book)
        };
        let wordings: Vec<_> = std::iter::once(lexicon.kjv()).chain(lexicon.others().iter()).collect();
        let ids: Vec<VerseId> = corpus.ids().collect();
        // Each wording's tokens of the verse before, for the opening check.
        let mut before: Vec<Option<Vec<String>>> = vec![None; wordings.len()];
        for (i, &v) in ids.iter().enumerate() {
            let same_chapter = i > 0 && {
                let (a, b) = (corpus.verse_ref(ids[i - 1]), corpus.verse_ref(v));
                (a.book, a.chapter) == (b.book, b.chapter)
            };
            let mut all_short = true;
            let mut any = false;
            let mut now: Vec<Option<Vec<String>>> = vec![None; wordings.len()];
            for (wi, w) in wordings.iter().enumerate() {
                let Some(text) = w.text(v) else { continue };
                any = true;
                let t = tokens(text);
                if t.len() > SHORT_VERSE_WORDS {
                    all_short = false;
                }
                let kinds: Vec<(bool, Option<Book>)> = t.iter().map(|x| kind_of(x)).collect();
                // Number phrases, each with the words before it: "fifty and six" with "thousand".
                let mut j = 0;
                while j < t.len() {
                    if !kinds[j].0 {
                        j += 1;
                        continue;
                    }
                    let start = j;
                    while j < t.len() && (kinds[j].0 || (t[j] == "and" && kinds.get(j + 1).is_some_and(|k| k.0))) {
                        j += 1;
                    }
                    let phrase = t[start.saturating_sub(NUMBER_CONTEXT_WORDS)..j].to_vec();
                    let list = f.numbers.entry(v).or_default();
                    if !list.contains(&phrase) {
                        list.push(phrase);
                    }
                }
                for (x, (_, book)) in t.iter().zip(&kinds) {
                    if let Some(b) = book {
                        let list = f.book_words.entry(v).or_default();
                        if !list.iter().any(|(w, _)| w == x) {
                            list.push((x.clone(), *b));
                        }
                    }
                }
                // The verse before, same chapter, in any wording, says this one's opening: in its middle,
                // or at its own start ("Blessed shall be…" opens Deuteronomy 28:3 to 28:6). A reader may be
                // in one version while Kairo hears the words of another.
                if same_chapter && t.len() >= OPENING_WORDS && before.iter().flatten().any(|p| p.windows(OPENING_WORDS).any(|x| x == &t[..OPENING_WORDS])) {
                    f.opening_inside_before.insert(v);
                }
                now[wi] = Some(t);
            }
            if all_short && any {
                f.short.insert(v);
            }
            before = now;
        }
        f
    }

    /// Number phrases in the verse's words, each with up to two words before it.
    pub fn numbers(&self, v: VerseId) -> &[Vec<String>] {
        self.numbers.get(&v).map_or(&[], Vec::as_slice)
    }

    /// `said` (newest words last) ends with a number the verse itself says, words before it and all.
    pub fn number_read_in(&self, v: VerseId, said: &[String]) -> bool {
        self.numbers(v).iter().any(|phrase| {
            (2..=phrase.len()).any(|n| said.len() >= n && said[said.len() - n..] == phrase[phrase.len() - n..])
        })
    }

    pub fn opening_inside_before(&self, v: VerseId) -> bool {
        self.opening_inside_before.contains(&v)
    }

    /// Words of the verse the citation reader takes for a book, and the book.
    pub fn book_words(&self, v: VerseId) -> &[(String, Book)] {
        self.book_words.get(&v).map_or(&[], Vec::as_slice)
    }

    pub fn short(&self, v: VerseId) -> bool {
        self.short.contains(&v)
    }

    /// Counts for the report: verses with numbers, with an opening said early, with book-name
    /// words, short ones.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (self.numbers.len(), self.opening_inside_before.len(), self.book_words.len(), self.short.len())
    }
}
