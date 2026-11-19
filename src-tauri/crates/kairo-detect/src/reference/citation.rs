//! What a spoken citation resolves to.

use kairo_bible::{Book, Corpus};

use super::i18n::{psalm_title_verses, Lang};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verses {
    /// A chapter with no verse ("Romans chapter 8").
    Chapter,
    One(u32),
    Span(u32, u32),
    /// Several spans, in the order spoken ("verses 1 to 3 and 7").
    Spans(Vec<(u32, u32)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Citation {
    pub book: Book,
    pub chapter: u32,
    pub verses: Verses,
    /// Other books this may have been: a numbered stem said without its
    /// number ("Timothy 3"), or a one-chapter book given a chapter it can't
    /// have ("third John five", really "first John 5"). The caller chooses by
    /// context or offers them all.
    pub alternatives: Vec<Book>,
    /// A bare number heard between "chapter" and "verse" that may have been
    /// the chapter restated. The words read next decide.
    pub alt_chapter: Option<u32>,
}

impl Citation {
    pub(crate) fn new(book: Book, chapter: u32, verses: Verses) -> Citation {
        Citation { book, chapter, verses, alternatives: Vec::new(), alt_chapter: None }
    }

    /// The verse numbers as the KJV has them, for a citation said in `lang`'s numbering: a French
    /// Psalm's verse 2 is the KJV's verse 1 when its title counts as verse 1 (i18n::psalm_title_verses).
    pub(crate) fn in_kjv_numbering(mut self, lang: Lang) -> Citation {
        // German Bibles number Genesis 32 as the Hebrew does: their 32:2 is the KJV's 32:1 (Jacob at
        // Peniel, "1. Mose 32,26" is the KJV's 32:25; de-malgo-johannes15).
        let genesis_32 = lang == Lang::De && self.book == Book::from_name("Genesis").expect("Genesis") && self.chapter == 32;
        let d = if self.book == Book::from_name("Psalms").expect("Psalms") { psalm_title_verses(lang, self.chapter) } else if genesis_32 { 1 } else { 0 };
        if d == 0 {
            return self;
        }
        let shift = |v: u32| v.saturating_sub(d).max(1);
        self.verses = match self.verses {
            Verses::Chapter => Verses::Chapter,
            Verses::One(v) => Verses::One(shift(v)),
            Verses::Span(a, b) => Verses::Span(shift(a), shift(b)),
            Verses::Spans(s) => Verses::Spans(s.into_iter().map(|(a, b)| (shift(a), shift(b))).collect()),
        };
        self
    }
}

/// The Bible's shape — how many chapters each book has, how many verses each
/// chapter has — read from the corpus, never hand-kept.
pub struct Shape {
    chapters: Vec<u32>,
    verses: Vec<Vec<u32>>,
    max_verse: u32,
}

impl Shape {
    pub fn new(corpus: &Corpus) -> Shape {
        let mut chapters = Vec::with_capacity(Book::COUNT);
        let mut verses = Vec::with_capacity(Book::COUNT);
        for b in Book::all() {
            let n = corpus.chapter_count(b) as u32;
            chapters.push(n);
            verses.push((1..=n).map(|c| corpus.verse_count(b, c as u16).unwrap_or(0) as u32).collect::<Vec<_>>());
        }
        let max_verse = verses.iter().flatten().copied().max().unwrap_or(0);
        Shape { chapters, verses, max_verse }
    }

    pub fn chapters(&self, b: Book) -> u32 {
        self.chapters[b.index()]
    }

    pub fn verses(&self, b: Book, chapter: u32) -> Option<u32> {
        self.verses[b.index()].get((chapter as usize).checked_sub(1)?).copied()
    }

    /// The most verses any chapter has — the sanity bound for a verse number.
    pub fn max_verse(&self) -> u32 {
        self.max_verse
    }
}
