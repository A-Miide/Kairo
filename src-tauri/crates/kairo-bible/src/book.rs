//! The 66 books of the Protestant canon, in canonical order, by the names the
//! rest of Kairo uses ("Psalms", "Song of Solomon", "1 John").

use std::fmt;

pub const BOOK_NAMES: [&str; 66] = [
    "Genesis", "Exodus", "Leviticus", "Numbers", "Deuteronomy",
    "Joshua", "Judges", "Ruth", "1 Samuel", "2 Samuel",
    "1 Kings", "2 Kings", "1 Chronicles", "2 Chronicles", "Ezra",
    "Nehemiah", "Esther", "Job", "Psalms", "Proverbs",
    "Ecclesiastes", "Song of Solomon", "Isaiah", "Jeremiah", "Lamentations",
    "Ezekiel", "Daniel", "Hosea", "Joel", "Amos",
    "Obadiah", "Jonah", "Micah", "Nahum", "Habakkuk",
    "Zephaniah", "Haggai", "Zechariah", "Malachi",
    "Matthew", "Mark", "Luke", "John", "Acts",
    "Romans", "1 Corinthians", "2 Corinthians", "Galatians", "Ephesians",
    "Philippians", "Colossians", "1 Thessalonians", "2 Thessalonians", "1 Timothy",
    "2 Timothy", "Titus", "Philemon", "Hebrews", "James",
    "1 Peter", "2 Peter", "1 John", "2 John", "3 John",
    "Jude", "Revelation",
];

/// A book by its canonical position (0 = Genesis, 65 = Revelation).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Book(u8);

impl Book {
    pub const COUNT: usize = 66;

    pub fn from_index(i: usize) -> Option<Book> {
        (i < Self::COUNT).then_some(Book(i as u8))
    }

    /// Exact canonical name only. Spoken and abbreviated forms belong to the
    /// reference parser, not here.
    pub fn from_name(name: &str) -> Option<Book> {
        BOOK_NAMES.iter().position(|n| *n == name).map(|i| Book(i as u8))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn name(self) -> &'static str {
        BOOK_NAMES[self.0 as usize]
    }

    pub fn is_new_testament(self) -> bool {
        self.0 >= 39
    }

    pub fn all() -> impl Iterator<Item = Book> {
        (0..Self::COUNT as u8).map(Book)
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for b in Book::all() {
            assert_eq!(Book::from_name(b.name()), Some(b));
        }
        assert_eq!(Book::from_name("Psalms").unwrap().index(), 18);
        assert!(Book::from_name("Matthew").unwrap().is_new_testament());
        assert!(!Book::from_name("Malachi").unwrap().is_new_testament());
        assert_eq!(Book::from_name("Psalm"), None);
    }
}
