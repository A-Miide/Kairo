//! Book names that are everyday words ("numbers", "mark", "acts", "job", "judges", "kings") said as
//! words, not citations; the same books cited still go up (detector probe, 23 Nov 2026).

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

#[test]
fn everyday_words_are_not_citations() {
    for s in [
        "I want you to mark two things in this passage",
        "and the job one has to do is pray",
        "his acts two thousand years ago changed everything",
        "the numbers three and four tell the story",
        "the kings two of them came against Israel",
        "the judges five of them sat in the gate",
    ] {
        assert!(sends(s).is_empty(), "{s}: {:?}", sends(s));
    }
}

#[test]
fn the_same_books_cited_still_go_up() {
    for (s, want) in [
        ("Mark chapter two verse five", "Mark 2:5"),
        ("Acts two verse thirty eight", "Acts 2:38"),
        ("Numbers six twenty four", "Numbers 6:24"),
        ("in the book of Numbers chapter six verse twenty four", "Numbers 6:24"),
    ] {
        assert_eq!(sends(s).first().map(String::as_str), Some(want), "{s}");
    }
}
