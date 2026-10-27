//! What's found must not depend on where the recogniser cut the speech into
//! segments (`detection-spec/08-word-buffer.md`): the same words, split many
//! ways, put the same verses on screen.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::{Book, Corpus, VerseRef};
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Arc::new(Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] }))
});

const MS_PER_WORD: u64 = 380;

/// One segment: its words, whether it's a final, and whether the live text runs on past it (the
/// offline engine locking words while the sentence continues).
type Seg = (String, bool, bool);

/// The verses sent to the screen, and those offered in Possible Matches, as "Book c:v".
fn outcome(segments: &[Seg]) -> (Vec<String>, Vec<String>) {
    // Words locked while the sentence runs on: the offline engine, whose newest live word may
    // still be growing.
    let offline = segments.iter().any(|s| s.2);
    let mut e = Engine::new(K.clone(), Config { offline, ..Config::default() });
    let mut t = 10_000;
    let (mut shown, mut offered) = (Vec::new(), Vec::new());
    for (text, fin, runs_on) in segments {
        t += MS_PER_WORD * text.split_whitespace().count().max(1) as u64;
        let o = e.handle(Event::Speech(Segment { text: text.clone(), is_final: *fin, speech_final: *fin && !runs_on, continues_live: *runs_on }), t);
        for eff in o.effects {
            if let Effect::Show { verses, target, .. } = eff {
                let list = if target == Target::Screen { &mut shown } else { &mut offered };
                for v in verses {
                    let r = K.verse_ref(v);
                    let l = format!("{} {}:{}", r.book.name(), r.chapter, r.verse);
                    if !list.contains(&l) {
                        list.push(l);
                    }
                }
            }
        }
    }
    (shown, offered)
}

/// Every screen send in order, a correction marked with what it corrects.
fn sends(segments: &[Seg]) -> Vec<String> {
    let mut e = Engine::new(K.clone(), Config::default());
    let (mut t, mut out) = (10_000, Vec::new());
    let label = |v| { let r = K.verse_ref(v); format!("{} {}:{}", r.book.name(), r.chapter, r.verse) };
    for (text, fin, _) in segments {
        t += MS_PER_WORD * text.split_whitespace().count().max(1) as u64;
        for eff in e.handle(Event::Speech(Segment { text: text.clone(), is_final: *fin, speech_final: *fin, continues_live: false }), t).effects {
            if let Effect::Show { verses, target: Target::Screen, corrected_from, .. } = eff {
                out.push(format!("{}{}", label(verses[0]), corrected_from.map_or(String::new(), |c| format!(" (corrects {})", label(c)))));
            }
        }
    }
    out
}

/// A `say --segments` script: a line starting "~ " is an interim, any other a final.
fn script(s: &str) -> Vec<Seg> {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| l.strip_prefix("~ ").map_or((l.to_string(), true, false), |i| (i.to_string(), false, false))).collect()
}

/// Interims growing every two words, then the final.
fn utterance(words: &[&str]) -> Vec<Seg> {
    let mut out: Vec<Seg> = (2..words.len()).step_by(2).map(|n| (words[..n].join(" "), false, false)).collect();
    out.push((words.join(" "), true, false));
    out
}

/// As the offline engine hears it (`kairo-server` offline.rs `Transcriber::pump`): the live
/// words after the locked ones as interims, all but the newest two locked once `lock` words are
/// waiting, the rest a final at the pause.
fn offline(words: &[&str], lock: usize) -> Vec<Seg> {
    let (mut out, mut locked) = (Vec::new(), 0);
    for heard in 1..=words.len() {
        let up_to = heard.saturating_sub(2);
        if up_to >= locked + lock {
            out.push((words[locked..up_to].join(" "), true, true));
            locked = up_to;
        }
        out.push((words[locked..heard].join(" "), false, false));
    }
    out.push((words[locked..].join(" "), true, false));
    out
}

/// The same words cut every way a recogniser might: one utterance; two utterances split at
/// each word; and the lagging cut — the first part heard only in an interim, the final carrying
/// the rest.
fn splits(text: &str) -> Vec<(String, Vec<Seg>)> {
    let w: Vec<&str> = text.split_whitespace().collect();
    let mut out = vec![("whole".to_string(), utterance(&w))];
    out.extend((2..=6).map(|lock| (format!("offline, locking {lock} at a time"), offline(&w, lock))));
    for k in 1..w.len() {
        let mut two = utterance(&w[..k]);
        two.extend(utterance(&w[k..]));
        out.push((format!("final | final at {k}"), two));
        let mut lagged = vec![(w[..k].join(" "), false, false)];
        lagged.extend(utterance(&w[k..]));
        out.push((format!("interim | final at {k}"), lagged));
    }
    out
}

fn v(book: &str, c: u16, n: u16) -> String {
    let r = VerseRef::new(Book::from_name(book).unwrap(), c, n);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

/// Every split puts `up` on screen and nothing else, and offers at least `offered`.
/// `skip(k)`: a final boundary after word `k` changes what was said (a number cut in two).
fn same_however_split(text: &str, up: &[String], offered: &[String], skip: impl Fn(usize) -> bool) {
    let mut bad = Vec::new();
    for (name, segs) in splits(text) {
        if name.split(" at ").nth(1).and_then(|k| k.parse().ok()).is_some_and(&skip) {
            continue;
        }
        let (shown, offers) = outcome(&segs);
        if shown != up || !offered.iter().all(|o| offers.contains(o)) {
            bad.push(format!("{name}: up {shown:?}, offered {offers:?}"));
        }
    }
    assert!(bad.is_empty(), "want {up:?} up and {offered:?} offered however «{text}» is split; wrong for:\n{}", bad.join("\n"));
}

// Psalm 67:3 and 67:5 are worded alike, a refrain: read word for word in the chapter just named,
// the first goes up and the other is offered — whichever segment carried the reference.
const PSALM_67: &str = "Psalm sixty seven it says let the people praise thee O God let all the people praise thee";

#[test]
fn psalm_67_as_the_live_log_split_it() {
    // Live log 2026-10-26: the reference only ever in an interim.
    let segs = script(
        "~ Psalm sixty seven
         ~ Psalm sixty seven it
         says let the people praise thee O God let all the people praise thee",
    );
    let (up, offered) = outcome(&segs);
    assert!(up == [v("Psalms", 67, 3)] && offered.contains(&v("Psalms", 67, 5)), "up {up:?}, offered {offered:?}");
}

#[test]
fn psalm_67_however_it_is_split() {
    // A boundary inside "sixty | seven" is a different number heard: not asked of the engine.
    same_however_split(PSALM_67, &[v("Psalms", 67, 3)], &[v("Psalms", 67, 5)], |k| k == 2);
}

const PSALM_107_REFRAIN: &str = "Oh that men would praise the LORD for his goodness, and for his wonderful works to the children of men!";

#[test]
fn a_refrain_read_through_its_chapter_goes_up_where_the_reading_is() {
    // Psalm 107:8, 15, 21 and 31 are the same words: each time it's read, the next one.
    let mut segs = script("Psalm one hundred and seven");
    for _ in 0..3 {
        segs.extend(utterance(&PSALM_107_REFRAIN.split_whitespace().collect::<Vec<_>>()));
        segs.push(("and then he goes on to tell what the Lord did for them".to_string(), true, false));
    }
    let (up, offered) = outcome(&segs);
    assert_eq!(up, [v("Psalms", 107, 8), v("Psalms", 107, 15), v("Psalms", 107, 21)], "offered {offered:?}");
}

#[test]
fn the_same_words_in_other_chapters_stay_a_choice() {
    // Psalm 107:1, 118:1 and 136:1: no chapter in play, so which passage it is isn't guessed.
    let (up, _) = outcome(&utterance(&"O give thanks unto the LORD; for he is good: for his mercy endureth for ever.".split_whitespace().collect::<Vec<_>>()));
    assert!(up.is_empty(), "up {up:?}");
}

#[test]
fn a_full_citation_however_it_is_split() {
    same_however_split("turn with me to John chapter three verse sixteen for God so loved the world", &[v("John", 3, 16)], &[], |_| false);
}

#[test]
fn a_chapter_then_its_verse_quoted_however_split() {
    same_however_split("open your Bibles to Romans chapter eight we know that all things work together for good to them that love God", &[v("Romans", 8, 28)], &[], |_| false);
}

fn read(segs: &mut Vec<Seg>, text: &str) {
    segs.extend(utterance(&text.split_whitespace().collect::<Vec<_>>()));
}

#[test]
fn the_verse_read_after_a_refrain_tells_which_it_was() {
    // 67:3 goes up on the refrain's words; "Then shall the earth yield her increase" is 67:6, so the
    // refrain was 67:5.
    let mut segs = script("Psalm sixty seven");
    read(&mut segs, "let the people praise thee O God let all the people praise thee.");
    read(&mut segs, "Then shall the earth yield her increase");
    let s = sends(&segs);
    assert_eq!(s.first().map(String::as_str), Some("Psalms 67:3"), "{s:?}");
    assert!(s.contains(&"Psalms 67:5 (corrects Psalms 67:3)".to_string()), "{s:?}");

    // …and "O let the nations be glad" is 67:4: it was 67:3.
    let mut segs = script("Psalm sixty seven");
    read(&mut segs, "let the people praise thee O God let all the people praise thee.");
    read(&mut segs, "O let the nations be glad and sing for joy");
    let s = sends(&segs);
    assert!(s.first().is_some_and(|f| f == "Psalms 67:3") && !s.iter().any(|x| x.contains("67:5")), "{s:?}");
}

#[test]
fn a_chapter_read_in_pieces_keeps_waiting_for_its_verse() {
    // The live miss (2026-10-26): 67:5–6 read in pieces after "Psalm sixty seven" heard only in an
    // interim. Each later final re-joined that interim must not count as naming the chapter afresh:
    // the chapter keeps waiting, and "then the earth shall yield" is 67:6.
    let segs = script(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../eval/segmentation/psalm-67-read-in-pieces.txt")).unwrap());
    let s = sends(&segs);
    assert!(s.last().is_some_and(|l| l.starts_with("Psalms 67:6")) && s.iter().all(|x| x.starts_with("Psalms 67:5") || x.starts_with("Psalms 67:6")), "{s:?}");
}
