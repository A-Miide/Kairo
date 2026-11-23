//! Numbers inside the words of a verse being read or quoted ("forty days and forty nights", "seventy
//! times seven") are reading, not verses called; a number said as a verse still is (23 Nov 2026).

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

fn check(script: &str, want: &[&str]) {
    let s = sends(script);
    assert_eq!(s, want, "{script}");
}

#[test]
fn numbers_read_in_a_verse_are_not_verses() {
    check(
        "turn to Matthew chapter four verse one\nthen was Jesus led up of the spirit into the wilderness\nand when he had fasted forty days and forty nights he was afterward an hungred",
        &["Matthew 4:1", "Matthew 4:2"],
    );
    check(
        "turn to Matthew eighteen verse twenty one\nLord how oft shall my brother sin against me\nI say not unto thee until seven times but until seventy times seven",
        &["Matthew 18:21", "Matthew 18:22"],
    );
    check("Revelation chapter seven verse four\nand I heard the number of them which were sealed and there were sealed an hundred and forty and four thousand", &["Revelation 7:4"]);
}

#[test]
fn a_chapter_then_its_verse_after_a_pause_is_that_verse() {
    // "Isaiah chapter fifty" then "four…": 50:4, not 50:1 before the verse (church PC, 15 Nov 2026).
    check("Isaiah chapter fifty\n~ four the Lord God hath given me\nfour the Lord God hath given me the tongue of the learned", &["Isaiah 50:4"]);
}

#[test]
fn a_verse_called_by_number_still_goes_up() {
    check("John chapter three verse sixteen\nfor God so loved the world\nand in verse seventeen", &["John 3:16", "John 3:17"]);
}
