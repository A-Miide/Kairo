//! Checks against the Bible files in `databases/bibles`.

use std::path::PathBuf;

use kairo_bible::{Book, Corpus, VerseRef};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles")
}

fn r(book: &str, chapter: u16, verse: u16) -> VerseRef {
    VerseRef::new(Book::from_name(book).unwrap(), chapter, verse)
}

#[test]
fn the_whole_kjv_is_there() {
    let c = Corpus::load(&data_dir()).unwrap();
    assert_eq!(c.len(), 31_102);
    assert_eq!(c.chapter_count(Book::from_name("Psalms").unwrap()), 150);
    assert_eq!(c.verse_count(Book::from_name("Psalms").unwrap(), 119), Some(176));
    // Verses map.json dropped as duplicate wording — they must exist here.
    for v in [r("Luke", 13, 5), r("Psalms", 107, 15), r("Psalms", 107, 31), r("Revelation", 3, 22), r("Mark", 9, 46), r("Numbers", 7, 21)] {
        let id = c.id(v).unwrap_or_else(|| panic!("{v} missing"));
        assert!(!c.kjv().text(id).unwrap().is_empty(), "{v} has no text");
    }
}

/// Standard KJV numbering (`scripts/fix-kjv-versification.py` restored it):
/// a verse missing or split shifts every later verse in its chapter.
#[test]
fn kjv_numbering_is_standard() {
    let c = Corpus::load(&data_dir()).unwrap();
    let count = |b: &str, ch: u16| c.verse_count(Book::from_name(b).unwrap(), ch);
    for (b, ch, n) in [("Matthew", 2, 23), ("Matthew", 22, 46), ("Matthew", 26, 75), ("Mark", 4, 41), ("Mark", 7, 37), ("Mark", 8, 38), ("1 Samuel", 20, 42), ("1 Kings", 22, 53), ("3 John", 1, 14), ("Revelation", 12, 17)] {
        assert_eq!(count(b, ch), Some(n), "{b} {ch}");
    }
    let text = |v| c.kjv().text(c.id(v).unwrap()).unwrap().to_string();
    assert!(text(r("Matthew", 26, 38)).contains("exceeding sorrowful"));
    assert!(text(r("Matthew", 26, 39)).contains("let this cup pass from me"));
    assert!(text(r("Mark", 7, 27)).contains("Let the children first be filled"));
    assert!(text(r("Matthew", 22, 2)).starts_with("The kingdom of heaven is like unto a certain king"));
    assert!(text(r("Revelation", 13, 1)).starts_with("And I stood upon the sand of the sea, and saw a beast"));
}

#[test]
fn kjv_text_is_clean() {
    let c = Corpus::load(&data_dir()).unwrap();
    let text = |v| c.kjv().text(c.id(v).unwrap()).unwrap().to_string();
    assert_eq!(text(r("Psalms", 23, 1)), "The LORD is my shepherd; I shall not want.");
    assert_eq!(text(r("Psalms", 119, 1)), "Blessed are the undefiled in the way, who walk in the law of the LORD.");
    for id in c.ids() {
        let t = c.kjv().text(id).unwrap();
        assert!(!t.contains(['{', '}', '[', ']']), "{}: {t}", c.verse_ref(id));
    }
}

#[test]
fn bsb_covers_the_bible() {
    let c = Corpus::load(&data_dir()).unwrap();
    let bsb = c.load_flat("BSB", &data_dir().join("translations/BSB.json")).unwrap();
    let missing: Vec<_> = bsb.missing().map(|id| c.verse_ref(id)).collect();
    assert!(missing.len() < 30, "BSB lacks {} KJV verses: {:?}", missing.len(), &missing[..missing.len().min(10)]);
    assert!(bsb.text(c.id(r("John", 3, 16)).unwrap()).unwrap().contains("He gave His one and only Son"));
}
