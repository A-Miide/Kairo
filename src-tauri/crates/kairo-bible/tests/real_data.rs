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
    assert_eq!(c.len(), 31_100);
    assert_eq!(c.chapter_count(Book::from_name("Psalms").unwrap()), 150);
    assert_eq!(c.verse_count(Book::from_name("Psalms").unwrap(), 119), Some(176));
    // Verses map.json dropped as duplicate wording — they must exist here.
    for v in [r("Luke", 13, 5), r("Psalms", 107, 15), r("Psalms", 107, 31), r("Revelation", 3, 22), r("Mark", 9, 46), r("Numbers", 7, 21)] {
        let id = c.id(v).unwrap_or_else(|| panic!("{v} missing"));
        assert!(!c.kjv().text(id).unwrap().is_empty(), "{v} has no text");
    }
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
fn nlt_gaps_are_known() {
    let c = Corpus::load(&data_dir()).unwrap();
    let nlt = c.load_map_nlt(&data_dir().join("map.json")).unwrap();
    let psalms = Book::from_name("Psalms").unwrap();
    let missing: Vec<_> = nlt.missing().map(|id| c.verse_ref(id)).collect();
    assert_eq!(missing.iter().filter(|v| v.book == psalms).count(), 2461, "all of Psalms");
    assert!(nlt.text(c.id(r("John", 3, 16)).unwrap()).unwrap().contains("loved the world so much"));
}
