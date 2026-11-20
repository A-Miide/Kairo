//! Wrong sends found by the detector audit of 19 Nov 2026, each reproduced on the engine before it was fixed, and
//! the everyday citations next to them that must keep working.

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

/// Finals after a pause of `gap` ms, each grown word by word at 400 ms a word; every verse sent to the screen.
fn screens(finals: &[(u64, &str)]) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config::default());
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
                if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                    out.extend(verses.iter().map(|v| { let r = K.verse_ref(*v); format!("{} {}:{}", r.book.name(), r.chapter, r.verse) }));
                }
            }
        }
    }
    out
}

#[test]
fn a_book_abbreviation_said_alone_is_an_everyday_word() {
    let shown = screens(&[(0, "is this mic on can you hear me at the back"), (2000, "chapter five verse two")]);
    assert!(!shown.iter().any(|s| s.starts_with("Micah")), "\"mic\" is not Micah: {shown:?}");
}

#[test]
fn a_book_abbreviation_with_numbers_is_still_a_citation() {
    let shown = screens(&[(0, "Phil four thirteen I can do all things through Christ")]);
    assert!(shown.iter().any(|s| s == "Philippians 4:13"), "{shown:?}");
}

#[test]
fn a_chapter_the_book_lacks_is_not_cut_into_chapter_and_verse() {
    let shown = screens(&[(0, "John twenty two verse three")]);
    assert!(!shown.iter().any(|s| s.starts_with("John 20")), "John 22 is not John 20:2 or 20:3: {shown:?}");
}

#[test]
fn a_spoken_chapter_and_verse_still_split_as_said() {
    let shown = screens(&[(0, "Romans eight twenty eight and we know that all things work together for good")]);
    assert!(shown.iter().any(|s| s == "Romans 8:28"), "{shown:?}");
}

#[test]
fn back_in_alone_points_to_the_past_and_returns_nothing() {
    let shown = screens(&[
        (0, "Romans ten ten for with the heart man believeth unto righteousness"),
        (60000, "now let us look at the life of Abraham who believed God"),
        (8000, "as Paul said back in Romans ten faith cometh by hearing and hearing by the word of God"),
    ]);
    assert_eq!(shown.iter().filter(|s| *s == "Romans 10:10").count(), 1, "10:10 not put back: {shown:?}");
    assert!(shown.iter().any(|s| s == "Romans 10:17"), "{shown:?}");
}

#[test]
fn going_back_to_a_chapter_still_returns_its_verse() {
    let shown = screens(&[
        (0, "Romans ten ten for with the heart man believeth unto righteousness"),
        (60000, "now let us look at the life of Abraham who left his home"),
        (8000, "let us go back to Romans ten"),
    ]);
    assert_eq!(shown.iter().filter(|s| *s == "Romans 10:10").count(), 2, "10:10 back up: {shown:?}");
}

#[test]
fn psalm_one_hundred_said_in_pieces_does_not_flash_verse_one() {
    let shown = screens(&[(0, "turn to Psalm one nineteen verse one oh five thy word is a lamp unto my feet")]);
    assert!(!shown.iter().any(|s| s == "Psalms 119:1"), "no 119:1 flash: {shown:?}");
    assert!(shown.iter().any(|s| s == "Psalms 119:105"), "{shown:?}");
}

#[test]
fn psalm_verse_one_still_goes_up() {
    let shown = screens(&[(0, "Psalm one verse one blessed is the man that walketh not in the counsel of the ungodly")]);
    assert!(shown.iter().any(|s| s == "Psalms 1:1"), "{shown:?}");
}

/// Every word of a long stretch is settled once, in order, when the recogniser merges two words inside the part
/// already settled ("can not" → "cannot"): counted by position, the first word after the settled part was skipped.
#[test]
fn a_word_merged_inside_the_settled_part_skips_nothing() {
    let first = "my brothers and sisters we really can not stay silent about what the Lord has done for us in this house over the many years we have walked together as one family under his care";
    let revised = first.replace("can not", "cannot") + " and today I want us to remember every single blessing that came our way through the storms and through the long nights of waiting on him";
    let mut e = Engine::new(K.clone(), Config::default());
    let mut settled: Vec<String> = Vec::new();
    let mut now = 1_000_000u64;
    let mut feed = |text: &str, from: usize, settled: &mut Vec<String>, now: &mut u64| {
        let words: Vec<&str> = text.split_whitespace().collect();
        for n in from..=words.len() {
            *now += 400;
            let seg = Segment { text: words[..n].join(" "), is_final: false, speech_final: false, continues_live: false };
            for eff in e.handle(Event::Speech(seg), *now).effects {
                if let Effect::Settled { text } = eff {
                    settled.push(text);
                }
            }
        }
    };
    feed(first, 1, &mut settled, &mut now);
    assert!(!settled.is_empty(), "the long stretch locked a part");
    feed(&revised, first.split_whitespace().count() - 1, &mut settled, &mut now);
    assert!(settled.len() >= 2, "a second part locked: {settled:?}");
    let letters = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let got = letters(&settled.join(" "));
    assert!(letters(&revised).starts_with(&got), "settled in order, nothing skipped or repeated:\n{settled:#?}");
}

#[test]
fn a_book_swapped_for_a_sound_alike_is_only_offered() {
    // James has five chapters, John twenty-one: the chapter said fits Joshua and Job, which only sound alike.
    for said in ["James six verse one", "John twenty two verse three"] {
        let shown = screens(&[(0, said)]);
        assert!(shown.is_empty(), "{said}: a guessed book is offered, not sent: {shown:?}");
    }
}

#[test]
fn a_number_a_book_does_not_have_picks_none_of_them() {
    let shown = screens(&[(0, "third Timothy two verse one")]);
    assert!(shown.is_empty(), "no Timothy has a third letter: {shown:?}");
    let shown = screens(&[(0, "second Timothy two verse fifteen study to shew thyself approved unto God")]);
    assert!(shown.iter().any(|s| s == "2 Timothy 2:15"), "{shown:?}");
}

#[test]
fn chapter_comma_verse_names_another_chapter_of_the_book_being_read() {
    let shown = screens(&[(0, "John one one in the beginning was the word"), (4000, "now John 3, 16 for God so loved the world")]);
    assert!(shown.iter().any(|s| s == "John 3:16"), "{shown:?}");
}

#[test]
fn a_count_of_verses_is_not_a_chapter() {
    let shown = screens(&[
        (0, "let me read four verses fifteen through eighteen"),
        (3000, "for God so loved the world that he gave his only begotten son that whosoever believeth in him should not perish"),
    ]);
    assert!(shown.iter().any(|s| s == "John 3:16"), "the quote isn't held by a count of verses: {shown:?}");
}

#[test]
fn psalm_one_nineteen_split_across_segments_does_not_flash_verse_one() {
    let shown = screens(&[(0, "turn to Psalm one nineteen verse one"), (900, "oh five thy word is a lamp unto my feet")]);
    assert!(!shown.iter().any(|s| s == "Psalms 119:1"), "no 119:1 flash: {shown:?}");
    assert!(shown.iter().any(|s| s == "Psalms 119:105"), "{shown:?}");
}

#[test]
fn psalm_one_nineteen_verse_one_read_on_still_goes_up() {
    let shown = screens(&[(0, "turn to Psalm one nineteen verse one"), (900, "blessed are the undefiled in the way who walk in the law of the Lord")]);
    assert!(shown.iter().any(|s| s == "Psalms 119:1"), "{shown:?}");
}

#[test]
fn a_year_before_a_translation_name_is_not_a_citation_in_it() {
    use kairo_detect::reference::translation_command::translation_command;
    assert_eq!(translation_command("In 1604 King James commissioned a new Bible"), None);
    assert_eq!(translation_command("John 3:16 in the King James"), Some("KJV"));
}
