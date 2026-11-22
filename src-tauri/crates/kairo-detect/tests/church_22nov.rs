//! Wrong and slow sends from the church PC's 22 Nov 2026 service, replayed from its log's own
//! transcript: each line of `church_22nov/*.txt` is "<ms> <F|I> <text>", a final or an interim.

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

fn label(v: VerseId) -> String {
    let r = K.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

/// Every screen send in order.
fn sends(script: &str) -> Vec<String> {
    // The church PC runs the offline speech engine.
    let mut e = Engine::new(K.clone(), Config { offline: true, ..Config::default() });
    let mut out = Vec::new();
    for line in script.lines().filter(|l| !l.is_empty()) {
        let mut parts = line.splitn(3, ' ');
        let (Some(ms), Some(kind), text) = (parts.next(), parts.next(), parts.next().unwrap_or("")) else { continue };
        let fin = kind == "F";
        let t = 10_000 + ms.parse::<u64>().unwrap();
        for eff in e.handle(Event::Speech(Segment { text: text.to_string(), is_final: fin, speech_final: fin, continues_live: false }), t).effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.push(label(verses[0]));
            }
        }
    }
    out
}

/// The sends after the first send of `verse`.
fn after(s: &[String], verse: &str) -> Vec<String> {
    s.iter().skip_while(|v| *v != verse).skip(1).cloned().collect()
}


/// "The Lord with the sound of a trumpet" is Psalm 47:5 being read, not Psalm 150:3: a weak quote
/// of the same words isn't a second opinion.
#[test]
fn the_same_words_again_are_not_a_second_opinion() {
    let s = sends(include_str!("church_22nov/psalm-47.txt"));
    assert!(s.contains(&"Psalms 47:5".to_string()), "{s:?}");
    assert!(!s.contains(&"Psalms 150:3".to_string()), "{s:?}");
}

/// "Things we say will begin to come to pass": "things" is three words away from the phrase, so
/// it doesn't make Luke 21:28 ("when these things begin to come to pass").
#[test]
fn a_common_phrase_with_a_word_far_before_it_is_not_a_quote() {
    let s = sends(include_str!("church_22nov/comes-to-pass.txt"));
    assert!(!s.contains(&"Luke 21:28".to_string()), "{s:?}");
}

/// "Isaiah fifty two verse one", then 53:1 read: the verse read goes up, not John 12:38 (which
/// quotes it), and without waiting for the operator.
#[test]
fn the_cited_verse_in_the_next_chapter_corrects_the_citation() {
    let s = sends(include_str!("church_22nov/isaiah-53.txt"));
    assert!(s.contains(&"Isaiah 53:1".to_string()), "{s:?}");
    assert!(!s.contains(&"John 12:38".to_string()), "{s:?}");
}

/// Reading on to 28:12-13 doesn't send 28:2 back up on its shared wording ("hearken unto the voice
/// of the LORD thy God").
#[test]
fn a_passage_being_read_does_not_go_back_on_shared_wording() {
    let s = sends(include_str!("church_22nov/deuteronomy-28.txt"));
    let later = after(&s, "Deuteronomy 28:12");
    assert!(s.contains(&"Deuteronomy 28:12".to_string()), "{s:?}");
    assert!(!later.contains(&"Deuteronomy 28:2".to_string()), "{s:?}");
}

/// Matthew 8:26 cited and read, then a story about it: 8:28 needs its own words, not the story's.
#[test]
fn a_single_cited_verse_does_not_skip_on_without_its_own_words() {
    let s = sends(include_str!("church_22nov/matthew-8.txt"));
    assert!(s.contains(&"Matthew 8:26".to_string()), "{s:?}");
    assert!(!s.contains(&"Matthew 8:28".to_string()), "{s:?}");
}
