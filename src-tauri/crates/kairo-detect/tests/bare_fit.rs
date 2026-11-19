//! Numbers cited without a book ("four verse four") take their book from the words said right after them, when one
//! verse at those numbers fits clearly. The preacher's own wording is too loose for a quote match: "who is in you is
//! greater than the devils in the world" is 1 John 4:4 (19 Nov 2026, offline model heard "what sound mine four verse
//! four"). Words far past the quote must never pick a book (Revelation 4:4 came up for "…have said the world where
//! you are is under power" when the window was too long).

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

/// The speech word by word at the offline model's pace, the last step final; every verse that goes to the screen.
fn screens(sentence: &str) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config { offline: true, ..Config::default() });
    let words: Vec<&str> = sentence.split_whitespace().collect();
    let mut out = Vec::new();
    for n in 1..=words.len() {
        let last = n == words.len();
        let seg = Segment { text: words[..n].join(" "), is_final: last, speech_final: last, continues_live: false };
        for eff in e.handle(Event::Speech(seg), 1_000_000 + 570 * n as u64).effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.extend(verses.iter().map(|v| { let r = K.verse_ref(*v); format!("{} {}:{}", r.book.name(), r.chapter, r.verse) }));
            }
        }
    }
    out
}

#[test]
fn the_words_after_bare_numbers_name_the_book_when_one_verse_fits() {
    let shown = screens("a spirit of fear that the boldness and what sound mine four verse four say who is in you is greater than the devils in the world. But hear this");
    assert!(shown.iter().any(|s| s == "1 John 4:4"), "1 John 4:4 goes up: {shown:?}");
}

#[test]
fn speech_far_past_the_numbers_never_picks_the_book() {
    let shown = screens("so we come to chapter four verse four and then we talk for a while about something else entirely today the church of the living God has been given power over all the world where you are is under control");
    assert!(shown.is_empty(), "bare numbers with no quote after them stay quiet: {shown:?}");
}
