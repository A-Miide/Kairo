//! A slide deck followed by the Bible engine's reading rules (engine/deck.rs).

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::Corpus;
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Arc::new(Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] }))
});


fn engine() -> Engine {
    Engine::new(K.clone(), Config::default())
}
fn say(e: &mut Engine, text: &str, is_final: bool, now: u64) -> Vec<usize> {
    let seg = Segment { text: text.into(), is_final, speech_final: is_final, continues_live: false };
    e.handle(Event::Speech(seg), now).effects.into_iter().filter_map(|f| if let Effect::DeckMoved { index, .. } = f { Some(index) } else { None }).collect()
}
fn deck(e: &mut Engine) {
    let slides = ["Your faith won't build itself", "Faith comes by hearing the word", "Shiloh 2026 far above all", "Give thanks always"];
    e.handle(Event::DeckSet { deck: "d".into(), slides: slides.iter().map(|s| s.to_string()).collect(), current: 0 }, 0);
}

#[test]
fn the_next_slide_goes_up_on_its_first_words() {
    let mut e = engine();
    deck(&mut e);
    // Partway through the sentence, before the pause: interims move it as a range's are.
    assert_eq!(say(&mut e, "your faith wont build itself church faith comes by", false, 5_000), vec![1]);
}

#[test]
fn words_of_a_later_slide_dont_move_it_alone() {
    let mut e = engine();
    deck(&mut e);
    assert!(say(&mut e, "we give thanks", true, 5_000).is_empty());
}

#[test]
fn a_word_only_the_slide_has_moves_it() {
    let mut e = engine();
    deck(&mut e);
    e.handle(Event::DeckAt(1), 1_000);
    // "Shiloh" is no Bible word: one of the slide's most telling. Two of its own words move it.
    assert_eq!(say(&mut e, "faith comes by hearing and this year at shiloh we go far above", true, 5_000), vec![2]);
}

#[test]
fn one_telling_word_of_the_next_slide_is_not_enough() {
    let mut e = engine();
    deck(&mut e);
    e.handle(Event::DeckAt(1), 1_000);
    assert!(say(&mut e, "faith comes by hearing and they went to shiloh", true, 5_000).is_empty());
}

#[test]
fn the_operator_picking_a_slide_moves_the_deck_without_a_move_back() {
    let mut e = engine();
    deck(&mut e);
    e.handle(Event::DeckAt(2), 1_000);
    assert_eq!(say(&mut e, "far above all give thanks always", false, 5_000), vec![3]);
    e.handle(Event::DeckClear, 6_000);
    assert!(say(&mut e, "your faith wont build itself faith comes by", false, 9_000).is_empty());
}
