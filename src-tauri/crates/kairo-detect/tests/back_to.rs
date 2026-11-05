//! "Go back to Romans 10": a chapter cited earlier in the service is returned to at the verse
//! called there then, not where its reading ended (owner, 2026-11-04).

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::{Corpus, VerseId};
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Arc::new(Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] }))
});

const MS_PER_WORD: u64 = 380;

fn label(v: VerseId) -> String {
    let r = K.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

/// Every screen send in order. A line starting "~ " is an interim, any other a final.
fn sends(script: &str) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config::default());
    let (mut t, mut out) = (10_000, Vec::new());
    for line in script.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (text, fin) = line.strip_prefix("~ ").map_or((line, true), |i| (i, false));
        t += MS_PER_WORD * text.split_whitespace().count().max(1) as u64;
        for eff in e.handle(Event::Speech(Segment { text: text.to_string(), is_final: fin, speech_final: fin, continues_live: false }), t).effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.push(label(verses[0]));
            }
        }
    }
    out
}

/// Romans 10:10 called, read on to 10:12, then a detour to John 3:16.
const CALLED_THEN_READ_ON: &str = "
    Romans ten verse ten
    for with the heart man believeth unto righteousness and with the mouth confession is made unto salvation
    for the scripture saith whosoever believeth on him shall not be ashamed
    for there is no difference between the Jew and the Greek for the same Lord over all is rich unto all that call upon him
    and you know John three sixteen
    for God so loved the world that he gave his only begotten son
";

#[test]
fn back_to_a_chapter_brings_back_the_verse_called_there() {
    let before = sends(CALLED_THEN_READ_ON);
    assert!(before.contains(&"Romans 10:12".to_string()) && before.last().is_some_and(|l| l == "John 3:16"), "{before:?}");
    for cue in ["let's go back to Romans ten", "now back to Romans ten", "return to Romans chapter ten", "come back again to Romans ten"] {
        let s = sends(&format!("{CALLED_THEN_READ_ON}\n{cue}\nthis is what I want you to see"));
        assert_eq!(s.len(), before.len() + 1, "{cue}: {s:?}");
        assert_eq!(s.last().map(String::as_str), Some("Romans 10:10"), "{cue}: {s:?}");
    }
}

#[test]
fn back_to_a_range_is_its_first_verse() {
    let s = sends(
        "Romans ten verses nine to thirteen
         that if thou shalt confess with thy mouth the Lord Jesus and shalt believe in thine heart
         and you know John three sixteen
         for God so loved the world that he gave his only begotten son
         let's go back to Romans ten
         this is what I want you to see",
    );
    assert_eq!(s.last().map(String::as_str), Some("Romans 10:9"), "{s:?}");
}

#[test]
fn heard_growing_in_interims_it_goes_up_once() {
    let s = sends(&format!(
        "{CALLED_THEN_READ_ON}
         ~ let's go back
         ~ let's go back to Romans
         ~ let's go back to Romans ten
         ~ let's go back to Romans ten this is
         let's go back to Romans ten this is what I want you to see"
    ));
    assert_eq!(s.iter().filter(|x| *x == "Romans 10:10").count(), 2, "called, then returned to once: {s:?}");
    assert_eq!(s.last().map(String::as_str), Some("Romans 10:10"), "{s:?}");
}

#[test]
fn back_to_a_chapter_not_cited_this_service_is_as_before() {
    // Romans 10 never cited: the return cue changes nothing.
    let earlier = "and you know John three sixteen
         for God so loved the world that he gave his only begotten son";
    let with_cue = sends(&format!("{earlier}\nlet's go back to Romans ten\nthis is what I want you to see"));
    let without = sends(&format!("{earlier}\nlet's go to Romans ten\nthis is what I want you to see"));
    assert_eq!(with_cue, without);
    assert!(!with_cue.iter().any(|s| s.starts_with("Romans")), "{with_cue:?}");
    // …and a chapter cited before but named without a return cue isn't returned to either.
    let s = sends(&format!("{CALLED_THEN_READ_ON}\nin Romans ten Paul is writing to the church\nthis is what I want you to see"));
    assert_eq!(s, sends(CALLED_THEN_READ_ON), "{s:?}");
}

#[test]
fn a_verse_named_after_the_return_takes_over() {
    let s = sends(&format!("{CALLED_THEN_READ_ON}\nlet's go back to Romans ten\nlook at verse thirteen"));
    let back = s.iter().rposition(|x| x == "Romans 10:10").expect("returned to 10:10");
    assert_eq!(s.last().map(String::as_str), Some("Romans 10:13"), "{s:?}");
    assert!(back + 1 == s.len() - 1, "{s:?}");
    // In the same breath, the verse named is the one.
    let s = sends(&format!("{CALLED_THEN_READ_ON}\ngo back to Romans ten verse thirteen"));
    assert_eq!(s.last().map(String::as_str), Some("Romans 10:13"), "{s:?}");
    assert_eq!(s.len(), sends(CALLED_THEN_READ_ON).len() + 1, "10:10 not sent on the way: {s:?}");
}
