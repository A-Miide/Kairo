//! What a church's services taught Kairo (engine/learned.rs) changes only what it should: the
//! preacher's word for a book is read as the book, and a verse taken down is offered, not sent,
//! when only its wording brings it up again. A citation by reference is never held back.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::{Book, Corpus, VerseRef};
use kairo_detect::engine::{Config, Effect, Engine, Event, Learned, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Arc::new(Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] }))
});

/// The verses sent to the screen, and those offered, as "Book c:v".
fn outcome(learned: Learned, lines: &[&str]) -> (Vec<String>, Vec<String>) {
    let mut e = Engine::new(K.clone(), Config { offline: true, learned: Arc::new(learned), ..Config::default() });
    let mut t = 10_000;
    let (mut shown, mut offered) = (Vec::new(), Vec::new());
    for text in lines {
        t += 380 * text.split_whitespace().count() as u64;
        let o = e.handle(Event::Speech(Segment { text: text.to_string(), is_final: true, speech_final: true, continues_live: false }), t);
        for eff in o.effects {
            if let Effect::Show { verses, target, .. } = eff {
                let list = if target == Target::Screen { &mut shown } else { &mut offered };
                for v in verses {
                    let r = K.verse_ref(v);
                    list.push(format!("{} {}:{}", r.book.name(), r.chapter, r.verse));
                }
            }
        }
    }
    (shown, offered)
}

fn verse(book: &str, c: u16, v: u16) -> kairo_bible::VerseId {
    K.id(VerseRef::new(Book::from_name(book).unwrap(), c, v)).unwrap()
}

#[test]
fn the_preachers_word_for_a_book_is_read_as_the_book() {
    // A mishearing nothing built in knows ("San" for Psalm is, normalize.rs): a preacher whose
    // "Psalm" the recogniser writes "Zom".
    let said = ["Zom one ten verse one to three"];
    let before = outcome(Learned::default(), &said);
    assert!(!before.0.iter().any(|v| v.starts_with("Psalms 110")), "unlearned, it isn't Psalm 110: {before:?}");
    let learned = Learned::new(HashMap::from([("zom".into(), Book::from_name("Psalms").unwrap())]), HashSet::new(), HashSet::new());
    let after = outcome(learned, &said);
    assert!(after.0.iter().any(|v| v.starts_with("Psalms 110:1")), "learned, Psalm 110 goes up: {after:?}");
}

#[test]
fn a_verse_taken_down_is_offered_when_only_its_wording_brings_it_up() {
    let read = ["For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life"];
    let before = outcome(Learned::default(), &read);
    assert!(before.0.contains(&"John 3:16".to_string()), "read aloud, it goes up: {before:?}");
    let held = Learned::new(HashMap::new(), HashSet::new(), HashSet::from([verse("John", 3, 16)]));
    let after = outcome(held.clone(), &read);
    assert!(!after.0.contains(&"John 3:16".to_string()) && after.1.contains(&"John 3:16".to_string()), "taken down before, it's offered: {after:?}");
    let cited = outcome(held, &["turn with me to John chapter 3 verse 16"]);
    assert!(cited.0.contains(&"John 3:16".to_string()), "cited by reference, it still goes up: {cited:?}");
}
