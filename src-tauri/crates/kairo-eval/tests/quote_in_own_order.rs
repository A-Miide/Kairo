//! A verse quoted in the preacher's own order, with no reference. Said clause by clause in a different
//! order from the text ("believe in your heart the Lord Jesus, confess with your mouth that God raised
//! him from the dead, you shall be saved" for Romans 10:9), the words matched in order cover too little
//! of the verse to pass the word-for-word bar alone; a second method landing the same verse is what
//! sends it (live-clip test, 2026-11-09). A few words of it, with no second method, are not enough.

use std::sync::{Arc, LazyLock};

use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;

// The knowledge the app runs with (the modern wordings are what a preacher's "your heart" meets).
static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| Arc::new(kairo_eval::knowledge()));

/// Everything sent to the screen as `said` arrives in steps of (ms, words so far), each step an interim
/// as the offline engine delivers it.
fn screens(said: &str, steps: &[(u64, usize)]) -> Vec<String> {
    let words: Vec<&str> = said.split_whitespace().collect();
    let mut e = Engine::new(K.clone(), Config { offline: true, ..Config::default() });
    let mut out = Vec::new();
    for &(t, n) in steps {
        let seg = Segment { text: words[..n.min(words.len())].join(" "), is_final: false, speech_final: false, continues_live: true };
        for eff in e.handle(Event::Speech(seg), t).effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.extend(verses.iter().map(|v| {
                    let r = K.verse_ref(*v);
                    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
                }));
            }
        }
    }
    out
}

/// The pace of a real clip: the offline engine's interims, the words said by each.
const STEPS: [(u64, usize); 20] = [
    (1200, 3), (1680, 4), (2880, 8), (3360, 10), (3960, 14), (4560, 15), (5040, 17), (5640, 20), (6240, 22), (6720, 24),
    (7320, 27), (7920, 28), (8400, 31), (9000, 34), (9600, 38), (10080, 42), (10680, 43), (11280, 44), (11760, 45), (12360, 47),
];

#[test]
fn a_verse_in_the_preachers_order_goes_up_when_two_methods_agree() {
    let said = "If I ask you, how is a man born again? How is a man saved? You say the Bible says believe in your heart the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit";
    assert_eq!(screens(said, &STEPS), vec!["Romans 10:9"]);
}

#[test]
fn a_few_words_of_it_do_not() {
    assert!(screens("You say the Bible says believe in your heart the Lord Jesus", &[(2000, 4), (3000, 8), (4000, 12)]).is_empty());
}

/// The same clip's first 30 s, cue by cue as the engine delivered it: (the cue's words, then each
/// interim as (ms, words so far, final)).
const CLIP: [(&str, &[(u64, usize, bool)]); 4] = [
    ("If I ask you, how is a man born again? How is a man saved? You say the Bible says believe in your heart the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our answer", &[(1200, 3, false), (1680, 4, false), (2880, 8, false), (3360, 10, false), (3960, 14, false), (4560, 15, false), (5040, 17, false), (5640, 20, false), (6240, 22, false), (6720, 24, false), (7320, 27, false), (7920, 28, false), (8400, 31, false), (9000, 34, false), (9600, 38, false), (10080, 42, false), (10680, 43, false), (11280, 44, false), (11760, 45, false), (12360, 47, false), (12960, 50, false), (13440, 52, false), (14040, 53, false), (14640, 55, false), (15720, 60, false), (16320, 62, false), (16800, 63, false), (17400, 64, false), (18000, 65, false), (19080, 65, true)]),
    ("when it comes to salvation. Is that true", &[(19680, 4, false), (20160, 5, false), (20760, 8, false), (21840, 8, true)]),
    ("if I now ask you", &[(22440, 3, false), (23040, 5, false), (24120, 5, true)]),
    ("how can a man walk in the power of God same thing? The Bible says the eyes of your understanding will be flooded with light to know the hope of", &[(24720, 4, false), (25200, 6, false), (25800, 10, false), (26400, 12, false), (27480, 15, false), (28080, 19, false), (28560, 22, false), (29160, 25, false), (30240, 30, false)]),
];

/// Each verse sent to the screen with the time it went up.
fn screens_timed() -> Vec<(u64, String)> {
    let mut e = Engine::new(K.clone(), Config { offline: true, ..Config::default() });
    let mut out = Vec::new();
    for (text, steps) in CLIP {
        let words: Vec<&str> = text.split_whitespace().collect();
        for &(t, n, fin) in steps {
            let seg = Segment { text: words[..n.min(words.len())].join(" "), is_final: fin, speech_final: fin, continues_live: !fin };
            for eff in e.handle(Event::Speech(seg), t).effects {
                if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                    out.extend(verses.iter().map(|v| {
                        let r = K.verse_ref(*v);
                        (t, format!("{} {}:{}", r.book.name(), r.chapter, r.verse))
                    }));
                }
            }
        }
    }
    out
}

/// A verse that went up a few seconds ago is not a second opinion against the next quote: Romans 10:9 on
/// screen must not hold Ephesians 1:18 back until its words are nearly all said (29 s, not 32 s).
#[test]
fn a_verse_just_shown_does_not_veto_the_next_quote() {
    let shown = screens_timed();
    assert_eq!(shown.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["Romans 10:9", "Ephesians 1:18"]);
    assert!(shown[1].0 <= 29_500, "Ephesians 1:18 went up at {} ms", shown[1].0);
}
