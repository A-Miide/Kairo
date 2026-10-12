//! Every verse of the Bible in KJV versification, and translations aligned to
//! it. A `VerseId` is a verse's position in canonical order and is the one
//! identifier every other crate uses (vector index rows, lexical indexes,
//! detection evidence).

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::book::Book;
use crate::text::clean;
use crate::BibleError;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct VerseRef {
    pub book: Book,
    pub chapter: u16,
    pub verse: u16,
}

impl VerseRef {
    pub fn new(book: Book, chapter: u16, verse: u16) -> Self {
        VerseRef { book, chapter, verse }
    }
}

impl fmt::Display for VerseRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}:{}", self.book, self.chapter, self.verse)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct VerseId(pub u32);

impl VerseId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

pub struct Corpus {
    refs: Vec<VerseRef>,
    ids: HashMap<VerseRef, VerseId>,
    /// verse count of every chapter, per book
    chapters: Vec<Vec<u16>>,
    kjv: Translation,
}

/// One translation's cleaned text, one slot per corpus verse. `None` where the
/// translation has no text for that verse (versification differences, gaps).
#[derive(Clone)]
pub struct Translation {
    pub code: String,
    texts: Vec<Option<Box<str>>>,
}

impl Translation {
    pub fn text(&self, id: VerseId) -> Option<&str> {
        self.texts.get(id.index()).and_then(|t| t.as_deref())
    }

    pub fn len(&self) -> usize {
        self.texts.iter().filter(|t| t.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Verses this translation has no text for, in canonical order.
    pub fn missing(&self) -> impl Iterator<Item = VerseId> + '_ {
        self.texts.iter().enumerate().filter(|(_, t)| t.is_none()).map(|(i, _)| VerseId(i as u32))
    }
}

/// `kjv.json`: `[{ abbrev, chapters: [[verse text, ...], ...] }, ...]` in canonical order.
#[derive(Deserialize)]
struct KjvBook {
    chapters: Vec<Vec<String>>,
}

/// `translations/<CODE>.json`, `packs/<lang>.json`, and `map.json`.
#[derive(Deserialize)]
struct FlatFile {
    verses: Vec<FlatVerse>,
}

#[derive(Deserialize)]
struct FlatVerse {
    book: String,
    chapter: u16,
    verse: u16,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    nlt_text: Option<String>,
}

impl Corpus {
    /// Loads the complete KJV from `<dir>/kjv.json`.
    pub fn load(dir: &Path) -> Result<Corpus, BibleError> {
        let path = dir.join("kjv.json");
        let books: Vec<KjvBook> = read_json(&path)?;
        if books.len() != Book::COUNT {
            return Err(BibleError::Format { path, detail: format!("{} books, expected {}", books.len(), Book::COUNT) });
        }
        let mut refs = Vec::with_capacity(31_200);
        let mut texts = Vec::with_capacity(31_200);
        let mut chapters = Vec::with_capacity(Book::COUNT);
        for (b, book) in Book::all().zip(&books) {
            chapters.push(book.chapters.iter().map(|c| c.len() as u16).collect());
            for (c, chapter) in book.chapters.iter().enumerate() {
                for (v, raw) in chapter.iter().enumerate() {
                    refs.push(VerseRef::new(b, c as u16 + 1, v as u16 + 1));
                    texts.push(Some(clean(raw).into_boxed_str()));
                }
            }
        }
        let ids = refs.iter().enumerate().map(|(i, r)| (*r, VerseId(i as u32))).collect();
        Ok(Corpus { refs, ids, chapters, kjv: Translation { code: "KJV".into(), texts } })
    }

    pub fn len(&self) -> usize {
        self.refs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = VerseId> {
        (0..self.refs.len() as u32).map(VerseId)
    }

    pub fn id(&self, r: VerseRef) -> Option<VerseId> {
        self.ids.get(&r).copied()
    }

    pub fn verse_ref(&self, id: VerseId) -> VerseRef {
        self.refs[id.index()]
    }

    pub fn kjv(&self) -> &Translation {
        &self.kjv
    }

    pub fn chapter_count(&self, book: Book) -> u16 {
        self.chapters[book.index()].len() as u16
    }

    pub fn verse_count(&self, book: Book, chapter: u16) -> Option<u16> {
        self.chapters[book.index()].get(chapter.checked_sub(1)? as usize).copied()
    }

    pub fn exists(&self, r: VerseRef) -> bool {
        self.ids.contains_key(&r)
    }

    /// A translation file in the flat `{ verses: [{book, chapter, verse, text}] }`
    /// shape, aligned to this corpus. Verses outside KJV versification are
    /// dropped; KJV verses the file lacks are `None`.
    pub fn load_flat(&self, code: &str, path: &Path) -> Result<Translation, BibleError> {
        let file: FlatFile = read_json(path)?;
        Ok(self.align(code, file.verses.into_iter().filter_map(|v| v.text.map(|t| (v.book, v.chapter, v.verse, t)))))
    }

    /// The NLT carried on `map.json`'s `nlt_text` field.
    pub fn load_map_nlt(&self, path: &Path) -> Result<Translation, BibleError> {
        let file: FlatFile = read_json(path)?;
        Ok(self.align("NLT", file.verses.into_iter().filter_map(|v| {
            v.nlt_text.filter(|t| !t.trim().is_empty()).map(|t| (v.book, v.chapter, v.verse, t))
        })))
    }

    fn align(&self, code: &str, rows: impl Iterator<Item = (String, u16, u16, String)>) -> Translation {
        let mut texts: Vec<Option<Box<str>>> = vec![None; self.len()];
        for (book, chapter, verse, text) in rows {
            let Some(b) = Book::from_name(&book) else { continue };
            if let Some(id) = self.id(VerseRef::new(b, chapter, verse)) {
                let t = clean(&text);
                if !t.is_empty() {
                    texts[id.index()] = Some(t.into_boxed_str());
                }
            }
        }
        Translation { code: code.to_string(), texts }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, BibleError> {
    let bytes = std::fs::read(path).map_err(|source| BibleError::Io { path: path.to_path_buf(), source })?;
    // kjv.json starts with a UTF-8 byte-order mark.
    let json = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    serde_json::from_slice(json).map_err(|e| BibleError::Format { path: path.to_path_buf(), detail: e.to_string() })
}
