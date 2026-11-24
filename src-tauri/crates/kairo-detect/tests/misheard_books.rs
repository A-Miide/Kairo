//! What the offline recogniser hears instead of a book's name, read back as that book only where a
//! citation follows (reference/normalize.rs `MISHEARD_BOOKS`), lined up against Deepgram's
//! transcripts of the same sermons (23 Nov 2026).

use std::path::PathBuf;
use std::sync::LazyLock;

use kairo_bible::Corpus;
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::{Active, Lang, Verses};

static K: LazyLock<Knowledge> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] })
});

fn cite(text: &str) -> Option<String> {
    K.parser(Lang::En).parse_one(text, Active::default()).map(|c| match c.verses {
        Verses::One(v) => format!("{} {}:{v}", c.book.name(), c.chapter),
        Verses::Chapter => format!("{} {}", c.book.name(), c.chapter),
        other => format!("{} {}:{other:?}", c.book.name(), c.chapter),
    })
}

#[test]
fn a_misheard_book_before_a_citation_is_that_book() {
    for (heard, want) in [
        ("I'm thirty verse five", "Psalms 30:5"),
        ("some seventy verse one", "Psalms 70:1"),
        ("drew two verse twenty eight", "Joel 2:28"),
        ("look four verse eighteen", "Luke 4:18"),
        ("genitrow is one verse one", "Genesis 1:1"),
        ("Psalm thirty plus five", "Psalms 30:5"),
    ] {
        assert_eq!(cite(heard).as_deref(), Some(want), "{heard}");
    }
}

#[test]
fn the_same_words_in_ordinary_talk_are_left_alone() {
    for heard in ["I'm thirty years old", "look four times at it", "some seventy people came", "he drew two lines"] {
        assert_eq!(cite(heard), None, "{heard}");
    }
}

#[test]
fn another_language_saying_in_chapter_is_not_a_psalm() {
    // German "im Kapitel 12" reaches the reader as "im chapter 12" (de-reger-roemer12).
    let k = K.parser(Lang::De).parse_one("wir lesen im Kapitel 12", Active::default());
    assert!(k.as_ref().is_none_or(|c| c.book.name() != "Psalms"), "{k:?}");
}
