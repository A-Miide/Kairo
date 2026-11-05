//! The book and chapter being read, so "verse 17" can be resolved.
//!
//! It lasts long enough to survive a normal expository pause between verse
//! calls (`EXPIRE_MS`), stretched up to double for a slow preacher or one
//! pausing for an interpreter. Every successful resolution against it renews
//! it — a chain of bare "verse N" calls keeps the passage alive. Naming a new
//! book without a chapter clears the old chapter, so a later "verse 13" can't
//! land in the previous book's chapter.

use kairo_bible::Book;

use super::parse::Active;

pub const EXPIRE_MS: u64 = 45_000;

#[derive(Clone, Debug, Default)]
pub struct ReadingContext {
    book: Option<Book>,
    chapter: Option<u32>,
    updated_at: u64,
    pace: f64,
}

impl ReadingContext {
    pub fn new() -> Self {
        ReadingContext { pace: 1.0, ..Default::default() }
    }

    /// 1 at a normal speaking rate, up to 2 for a slow one.
    pub fn set_pace(&mut self, factor: f64) {
        self.pace = if factor.is_finite() { factor.clamp(1.0, 2.0) } else { 1.0 };
    }

    pub fn expire_ms(&self) -> u64 {
        (EXPIRE_MS as f64 * self.pace.max(1.0)) as u64
    }

    pub fn update(&mut self, book: Book, chapter: Option<u32>, now_ms: u64) {
        let changed = self.book != Some(book);
        self.book = Some(book);
        match chapter {
            Some(c) if c > 0 => self.chapter = Some(c),
            _ if changed => self.chapter = None,
            _ => {}
        }
        self.updated_at = now_ms;
    }

    pub fn is_valid(&self, now_ms: u64) -> bool {
        self.book.is_some() && now_ms.saturating_sub(self.updated_at) < self.expire_ms()
    }

    /// The book and chapter in play now, or nothing if it has lapsed.
    pub fn active(&self, now_ms: u64) -> Active {
        if self.is_valid(now_ms) { Active { book: self.book, chapter: self.chapter } } else { Active::default() }
    }

    pub fn reset(&mut self) {
        *self = ReadingContext::new();
    }
}
