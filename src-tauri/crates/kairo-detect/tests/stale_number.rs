//! A bare number only reads on from a verse that is up right now, and only forwards: "which was later
//! reduced to five", minutes after Revelation 19:10, is a sum of money, not verse 5 (church PC, 15 Nov 2026).

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


const TALK: &str = "and so the testimony goes on and the people were glad and the church gave thanks to God for everything He had done in their midst";

fn minutes_of_talk(minutes: u64) -> String {
    // MS_PER_WORD is the script clock; a line of talk is about 25 words.
    let lines = minutes * 60_000 / (MS_PER_WORD * 25) + 1;
    vec![TALK; lines as usize].join("\n")
}

#[test]
fn a_number_minutes_after_a_citation_is_not_a_verse() {
    let s = sends(&format!("The scripture says in the book of Revelation chapter nineteen verse ten\n{}\nthey claimed they were owed which was later reduced to five", minutes_of_talk(4)));
    assert!(s.iter().all(|v| v != "Revelation 19:5"), "{s:?}");
}

#[test]
fn a_number_before_the_verse_being_read_is_not_a_verse() {
    let s = sends("The scripture says in the book of Revelation chapter nineteen verse ten\nand the amount was reduced to five");
    assert!(s.iter().all(|v| v != "Revelation 19:5"), "{s:?}");
}

#[test]
fn a_number_called_soon_after_still_reads_on() {
    let s = sends("The scripture says in the book of Revelation chapter nineteen verse ten\nand then it goes to twelve");
    assert!(s.contains(&"Revelation 19:12".to_string()), "{s:?}");
}
