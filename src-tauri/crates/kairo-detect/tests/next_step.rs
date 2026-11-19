//! A verse announced as the next step of a reading, in the same breath as the citation, is a
//! range call (owner, 2026-10-31): "Genesis chapter 21 verse 1 and then we go to verse 6" puts
//! 21:1 up and holds 21:6 for when it's read. A range follows the reader a verse at a time
//! (owner, 2026-11-01): its verses never go up together.

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

const SARAH: &str = "and Sarah said God hath made me to laugh so that all that hear will laugh with me";

#[test]
fn a_verse_announced_next_in_the_same_breath_joins_the_citation() {
    for offline in [false, true] {
        for said in ["Genesis chapter 21 verse 1 and then we go to verse 6", "Genesis chapter 21 verse 1, next verse 6"] {
            // 21:1 goes up, and 21:6 only when it's read: the range follows the reader.
            let s = screens(offline, &[said]);
            assert_eq!(s, ["Genesis 21:1"], "offline {offline}, «{said}»: {s:?}");
            let s = screens(offline, &[said, SARAH]);
            assert_eq!(s, ["Genesis 21:1", "Genesis 21:6"], "offline {offline}, «{said}» then 21:6 read: {s:?}");
        }
    }
}

#[test]
fn what_already_worked_is_unchanged() {
    // One show of the range, the first verse up: the screen takes its first verse, the rest follow.
    assert_eq!(screens(false, &["Genesis chapter 21 verse 1 and verse 6"]), ["Genesis 21:1, Genesis 21:6"]);
    assert_eq!(screens(false, &["Genesis chapter 21 verse 1.", "And then we go to verse 6."]), ["Genesis 21:1", "Genesis 21:6"]);
    assert_eq!(screens(false, &["Romans 8:28, then we go to Romans 8:31"]), ["Romans 8:28", "Romans 8:31"]);
    assert_eq!(screens(false, &["John 3:16 then we will talk about love"]), ["John 3:16"]);
}

#[test]
fn a_short_range_follows_the_reader_a_verse_at_a_time() {
    // Live, 2026-10-31: "Psalm twenty two will read twenty seven and twenty eight" put both up
    // together and kept both up through the reading, while a longer range stepped through
    // (owner, 2026-11-01: it has to follow along). The call is the range, its first verse up; 22:28
    // goes up on its own once 22:27 has been read to its end.
    const CALL: &str = "Our final read for now, Psalm twenty two will read twenty seven and twenty eight. I'd like us to read together in concert.";
    const V27: &str = "all the ends of the world shall remember and turn unto the LORD and all the kindreds of the nations shall worship before thee";
    const V28: &str = "for the kingdom is the LORD'S and he is the governor among the nations";
    assert_eq!(screens(false, &[CALL]), ["Psalms 22:27, Psalms 22:28"]);
    assert_eq!(screens(false, &[CALL, V27]), ["Psalms 22:27, Psalms 22:28", "Psalms 22:28"]);
    assert_eq!(screens(false, &[CALL, V27, V28]), ["Psalms 22:27, Psalms 22:28", "Psalms 22:28"]);
    // Not before it: the second verse isn't put up while the first is still being read.
    assert_eq!(screens(false, &[CALL, "all the ends of the world shall remember"]), ["Psalms 22:27, Psalms 22:28"]);
}

#[test]
fn a_range_called_then_extended_follows_every_verse_called() {
    // "verses 1 and 2, and then we go to verse 6": the range is 21:1, 21:2 and 21:6, read in turn.
    const V1: &str = "and the LORD visited Sarah as he had said and the LORD did unto Sarah as he had spoken";
    const V2: &str = "for Sarah conceived and bare Abraham a son in his old age at the set time of which God had spoken to him";
    let s = screens(false, &["Genesis chapter 21 verses 1 and 2 and then we go to verse 6", V1, V2, SARAH]);
    assert_eq!(s.iter().skip(1).map(String::as_str).collect::<Vec<_>>(), ["Genesis 21:2", "Genesis 21:6"], "{s:?}");
}

#[test]
fn the_same_verse_cited_again_at_once_goes_up_once() {
    assert_eq!(screens(false, &["John 3:16", "John 3:16"]), ["John 3:16"]);
}
