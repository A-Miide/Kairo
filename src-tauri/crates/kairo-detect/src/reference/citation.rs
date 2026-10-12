//! What a spoken citation resolves to.

use kairo_bible::{Book, Corpus};

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
