//! A verse announced as the next step of a reading, in the same breath as the citation, is a
//! range call (owner, 2026-10-31): "Genesis chapter 21 verse 1 and then we go to verse 6" puts
//! 21:1 up, then 21:1 with 21:6.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::Corpus;
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Arc::new(Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] }))
});

/// Each screen change, as "Book c:v, Book c:v".
fn screens(offline: bool, lines: &[&str]) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config { offline, ..Config::default() });
    let mut t = 10_000;
    let mut out = Vec::new();
    for text in lines {
        t += 380 * text.split_whitespace().count() as u64;
        let o = e.handle(Event::Speech(Segment { text: text.to_string(), is_final: true, speech_final: true, continues_live: false }), t);
        for eff in o.effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.push(verses.iter().map(|v| {
                    let r = K.verse_ref(*v);
                    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
                }).collect::<Vec<_>>().join(", "));
            }
        }
    }
    out
}

#[test]
fn a_verse_announced_next_in_the_same_breath_joins_the_citation() {
    for offline in [false, true] {
        let s = screens(offline, &["Genesis chapter 21 verse 1 and then we go to verse 6"]);
        assert_eq!(s.last().map(String::as_str), Some("Genesis 21:1, Genesis 21:6"), "offline {offline}: {s:?}");
        let s = screens(offline, &["Genesis chapter 21 verse 1, next verse 6"]);
        assert_eq!(s.last().map(String::as_str), Some("Genesis 21:1, Genesis 21:6"), "offline {offline}: {s:?}");
    }
}

#[test]
fn what_already_worked_is_unchanged() {
    assert_eq!(screens(false, &["Genesis chapter 21 verse 1 and verse 6"]), ["Genesis 21:1, Genesis 21:6"]);
    assert_eq!(screens(false, &["Genesis chapter 21 verse 1.", "And then we go to verse 6."]), ["Genesis 21:1", "Genesis 21:6"]);
    assert_eq!(screens(false, &["Romans 8:28, then we go to Romans 8:31"]), ["Romans 8:28", "Romans 8:31"]);
    assert_eq!(screens(false, &["John 3:16 then we will talk about love"]), ["John 3:16"]);
}
