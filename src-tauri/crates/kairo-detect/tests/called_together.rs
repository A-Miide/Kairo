//! Two verses called in one breath: the first goes up, the others wait as called until they are read
//! (engine/called.rs). From the owner's service of 22 Nov 2026.

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

/// Finals after a pause of `gap` ms, each grown word by word at 400 ms a word: every verse sent to the
/// screen ("John 3:16") and every verse called ("called John 3:16"), in order.
fn run(finals: &[(u64, &str)]) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config::default());
    let label = |v| { let r = K.verse_ref(v); format!("{} {}:{}", r.book.name(), r.chapter, r.verse) };
    let mut out = Vec::new();
    let mut now = 1_000_000u64;
    for (gap, text) in finals {
        now += gap;
        let words: Vec<&str> = text.split_whitespace().collect();
        for n in 1..=words.len() {
            now += 400;
            let last = n == words.len();
            let seg = Segment { text: words[..n].join(" "), is_final: last, speech_final: last, continues_live: false };
            for eff in e.handle(Event::Speech(seg), now).effects {
                match eff {
                    Effect::Show { verses, target: Target::Screen, .. } => out.push(label(verses[0])),
                    Effect::Called { verse } => out.push(format!("called {}", label(verse))),
                    _ => {}
                }
            }
        }
    }
    out
}

const JONAH: &str = "they that observe lying vanities forsake their own mercy. When you observe lying vanity you forsake your own mercy";
const CORINTHIANS: &str = "now thanks be unto God which always causeth us to triumph in Christ";

#[test]
fn two_called_in_one_sentence_the_second_waits_for_its_reading() {
    let got = run(&[
        (0, "Jonah chapter two verse eight and also Second Corinthians two verse fourteen"),
        (1500, "I want to talk briefly on what are titled the glory of the church. The glory of the church."),
        (3000, JONAH),
        (4000, CORINTHIANS),
    ]);
    assert_eq!(got, ["Jonah 2:8", "called 2 Corinthians 2:14", "2 Corinthians 2:14"]);
}

#[test]
fn a_pause_between_the_two_citations_is_still_called_together() {
    let got = run(&[(0, "Jonah chapter two verse eight"), (1200, "and also Second Corinthians two verse fourteen"), (3000, JONAH), (4000, CORINTHIANS)]);
    assert_eq!(got, ["Jonah 2:8", "called 2 Corinthians 2:14", "2 Corinthians 2:14"]);
}

#[test]
fn a_verse_cited_after_the_first_was_read_goes_up_as_ever() {
    let got = run(&[(0, "Jonah chapter two verse eight"), (500, JONAH), (1000, "turn with me to Second Corinthians two verse fourteen")]);
    assert_eq!(got, ["Jonah 2:8", "2 Corinthians 2:14"]);
}

#[test]
fn a_correction_or_the_same_chapter_is_not_called_together() {
    assert_eq!(run(&[(0, "John three sixteen sorry Romans five eight")]).last().map(String::as_str), Some("Romans 5:8"));
    assert_eq!(run(&[(0, "John three sixteen and verse seventeen")]), ["John 3:16", "John 3:17"]);
}
