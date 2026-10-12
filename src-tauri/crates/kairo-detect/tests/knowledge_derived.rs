//! Knowledge the engine derives from the Bible itself instead of keeping
//! hand-made lists.

use std::path::PathBuf;
use std::sync::LazyLock;

use kairo_bible::{Book, Corpus};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Knowledge> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] })
});

fn ch(b: &str, c: u16) -> (Book, u16) {
    (Book::from_name(b).unwrap(), c)
}

#[test]
fn parallel_passages_are_confusable_formulas_are_not() {
    for (a, b) in [(ch("Jeremiah", 17), ch("Psalms", 1)), (ch("Psalms", 14), ch("Psalms", 53)), (ch("Psalms", 18), ch("2 Samuel", 22)), (ch("Psalms", 70), ch("Psalms", 40))] {
        assert!(K.confusable(a, b) && K.confusable(b, a), "{a:?} / {b:?}");
    }
    assert!(!K.confusable(ch("Exodus", 30), ch("Leviticus", 4)), "a shared formula verse isn't shared wording");
    assert!(!K.confusable(ch("John", 3), ch("John", 3)));
}

#[test]
fn proper_names_come_from_capitalization() {
    assert_eq!(K.name_chapters("isaac").map(<[_]>::len), Some(52));
    assert!(K.name_chapters("zacchaeus").is_some_and(|c| c == [ch("Luke", 19)]));
    for not_a_name in ["lord", "god", "jesus", "the", "and"] {
        assert!(!K.is_name(not_a_name), "{not_a_name}");
    }
    let top = K.frequent_names(10);
    assert!(top.contains(&"david") && top.contains(&"moses"), "{top:?}");
}
