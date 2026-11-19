//! Verses worded alike are told apart by a word only one of them has. Luke 22:20 on screen, then "this is
//! my blood of the new testament, which is shed for many for the remission of sins" read word for word:
//! that is Matthew 26:28 (not Mark 14:24, which lacks "remission of sins"), and it goes up even though
//! its twin Luke 22:20 is the passage in play (live-clip audit, 2026-11-09). Luke's own words said again
//! stay Luke's.

use std::sync::{Arc, LazyLock};

use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| Arc::new(kairo_eval::knowledge()));

const LUKE: &str = "Jesus took the cup and said this cup is the new testament in my blood which is shed for you";
const MATTHEW: &str = "For this is my blood of the new testament which is shed for many for the remission of sins";

/// Every verse put on screen as the lines are read at speaking pace, three words at a time as interims.
fn screens(lines: &[&str]) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config { offline: true, ..Config::default() });
    let mut out = Vec::new();
    let mut t = 10_000;
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        for n in 1..=words.len() {
            t += 380;
            let fin = n == words.len();
            if !fin && n % 3 != 0 {
                continue;
            }
            let seg = Segment { text: words[..n].join(" "), is_final: fin, speech_final: fin, continues_live: !fin };
            for eff in e.handle(Event::Speech(seg), t).effects {
                if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                    out.extend(verses.iter().map(|v| {
                        let r = K.verse_ref(*v);
                        format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
                    }));
                }
            }
        }
        t += 1_000;
    }
    out
}

#[test]
fn its_own_words_put_the_twin_up_after_the_other_is_on_screen() {
    assert_eq!(screens(&[LUKE, MATTHEW]), ["Luke 22:20", "Matthew 26:28"]);
}

#[test]
fn the_same_verse_read_again_stays_the_same_verse() {
    assert_eq!(screens(&[LUKE, LUKE]), ["Luke 22:20"]);
}
