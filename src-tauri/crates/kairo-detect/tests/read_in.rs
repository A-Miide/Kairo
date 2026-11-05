//! The translation being read goes on screen (read_in.rs): verses read in a row word for word in
//! one wording tell the server which, never one verse alone, and never a verse put up on what the
//! preacher's own words meant. Switching (only when it isn't the one shown) and going back at the
//! next passage are the server's (kairo-server spoken.rs).

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::Corpus;
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

/// The KJV and the BSB, the wordings the app ships.
static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    let corpus = Corpus::load(&dir).unwrap();
    let bsb = corpus.load_flat("BSB", &dir.join("translations/BSB.json")).unwrap();
    Arc::new(Knowledge::build(corpus, Wordings { own: None, also: vec![bsb] }))
});

const MS_PER_WORD: u64 = 380;

/// The verses sent to the screen, and each translation the passage was told to be read in, with
/// the verse up then.
fn run(finals: &str) -> (Vec<String>, Vec<(String, String)>) {
    let mut e = Engine::new(K.clone(), Config::default());
    let label = |v| { let r = K.verse_ref(v); format!("{} {}:{}", r.book.name(), r.chapter, r.verse) };
    let (mut t, mut up, mut read_in) = (10_000, Vec::new(), Vec::new());
    for text in finals.lines().map(str::trim).filter(|l| !l.is_empty()) {
        t += MS_PER_WORD * text.split_whitespace().count() as u64;
        for eff in e.handle(Event::Speech(Segment { text: text.to_string(), is_final: true, speech_final: true, continues_live: false }), t).effects {
            match eff {
                Effect::Show { verses, target: Target::Screen, .. } => up.push(label(verses[0])),
                Effect::ReadIn { verse, translation } => read_in.push((translation, label(verse))),
                _ => {}
            }
        }
    }
    (up, read_in)
}

const BSB_9_5: &str = "we have sinned and done wrong we have acted wickedly and rebelled we have turned away from your commandments and ordinances";
const BSB_9_6: &str = "we have not listened to your servants the prophets who spoke in your name to our kings leaders fathers and all the people of the land";
const BSB_9_7: &str = "to you O Lord belongs righteousness but this day we are covered with shame the men of Judah the people of Jerusalem";

#[test]
fn two_verses_read_in_a_row_in_the_bsb_show_it() {
    let (up, read_in) = run(&format!("Daniel nine verse five\n{BSB_9_5}\n{BSB_9_6}\n{BSB_9_7}"));
    assert!(up.contains(&"Daniel 9:6".to_string()), "up {up:?}");
    assert_eq!(read_in, [("BSB".to_string(), "Daniel 9:6".to_string())], "told once, on the second verse; up {up:?}");
}

#[test]
fn one_verse_read_in_the_bsb_is_not_enough() {
    let (up, read_in) = run(&format!("Daniel nine verse five\n{BSB_9_5}\nand that is the heart of the prayer my friends"));
    assert_eq!(up, ["Daniel 9:5"]);
    assert!(read_in.is_empty(), "{read_in:?}");
}

#[test]
fn verses_read_apart_are_not_a_reading() {
    // 9:5 and then 9:7, both in the BSB's words: two quotes from the chapter, not a reading of it.
    let (up, read_in) = run(&format!("Daniel nine verse five\n{BSB_9_5}\nnow look at verse seven\n{BSB_9_7}"));
    assert!(up.contains(&"Daniel 9:7".to_string()), "up {up:?}");
    assert!(read_in.is_empty(), "{read_in:?}");
}

#[test]
fn the_kjv_read_is_never_told_as_the_bsb() {
    let (up, read_in) = run(
        "Daniel nine verse five
         we have sinned and have committed iniquity and have done wickedly and have rebelled even by departing from thy precepts and from thy judgments
         neither have we hearkened unto thy servants the prophets which spake in thy name to our kings our princes and our fathers and to all the people of the land",
    );
    assert!(up.contains(&"Daniel 9:6".to_string()), "up {up:?}");
    assert!(read_in.iter().all(|(code, _)| code == "KJV"), "{read_in:?}");
}
