//! A passage being read stays on screen when its own words are also found, word for word, in a verse of another
//! book. Live 13 and 18 Nov and the last test, 19 Nov 2026: Ephesians 1:19 was up ("…according to the greatness of
//! his power which he wrought in Christ") and Psalm 79:11, which has "according to the greatness of thy power",
//! replaced it.

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

/// The speech as the live recogniser gives it: one word more every ~0.57 s, then final. Every verse that goes to the
/// screen, in order, as "Book c:v".
fn screens(offline: bool, step: usize, sentence: &str) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config { offline, ..Config::default() });
    let words: Vec<&str> = sentence.split_whitespace().collect();
    let mut out = Vec::new();
    let mut t = 1_000_000;
    let mut n = 0;
    while n < words.len() {
        n = (n + step).min(words.len());
        t += 575 * step as u64;
        let last = n == words.len();
        let seg = Segment { text: words[..n].join(" "), is_final: last, speech_final: last, continues_live: false };
        for eff in e.handle(Event::Speech(seg), t).effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.extend(verses.iter().map(|v| { let r = K.verse_ref(*v); format!("{} {}:{}", r.book.name(), r.chapter, r.verse) }));
            }
        }
    }
    out
}

const SAID: &str = "how can a man walk in the power of God same thing The Bible says the eyes of your understanding will be flooded with light to know the hope of his calling the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when he raised it from the dead";

#[test]
fn the_passage_being_read_is_not_replaced_by_a_text_match_in_another_book() {
    for offline in [false, true] {
        for step in [1, 2] {
            let shown = screens(offline, step, SAID);
            assert!(shown.iter().any(|s| s.starts_with("Ephesians 1:")), "the passage goes up: {shown:?}");
            assert!(!shown.iter().any(|s| s.starts_with("Psalms 79")), "Psalm 79:11 only shares the words, it stays off the screen: offline {offline}, step {step}: {shown:?}");
        }
    }
}
