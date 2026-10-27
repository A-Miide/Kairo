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

/// One segment: its words, and whether it's a final.
type Seg = (String, bool);

/// The verses sent to the screen, and those offered in Possible Matches, as "Book c:v".
fn outcome(segments: &[Seg]) -> (Vec<String>, Vec<String>) {
    let mut e = Engine::new(K.clone(), Config::default());
    let mut t = 10_000;
    let (mut shown, mut offered) = (Vec::new(), Vec::new());
    for (text, fin) in segments {
        t += MS_PER_WORD * text.split_whitespace().count().max(1) as u64;
        let o = e.handle(Event::Speech(Segment { text: text.clone(), is_final: *fin, speech_final: *fin, continues_live: false }), t);
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

fn screen(segments: &[Seg]) -> Vec<String> {
    outcome(segments).0
}

/// A `say --segments` script: a line starting "~ " is an interim, any other a final.
fn script(s: &str) -> Vec<Seg> {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| l.strip_prefix("~ ").map_or((l.to_string(), true), |i| (i.to_string(), false))).collect()
}

/// Interims growing every two words, then the final.
fn utterance(words: &[&str]) -> Vec<Seg> {
    let mut out: Vec<Seg> = (2..words.len()).step_by(2).map(|n| (words[..n].join(" "), false)).collect();
    out.push((words.join(" "), true));
    out
}

/// The same words cut every way a recogniser might: one utterance; two utterances split at
/// each word; and the lagging cut — the first part heard only in an interim, the final carrying
/// the rest.
fn splits(text: &str) -> Vec<(String, Vec<Seg>)> {
    let w: Vec<&str> = text.split_whitespace().collect();
    let mut out = vec![("whole".to_string(), utterance(&w))];
    for k in 1..w.len() {
        let mut two = utterance(&w[..k]);
        two.extend(utterance(&w[k..]));
        out.push((format!("final | final at {k}"), two));
        let mut lagged = vec![(w[..k].join(" "), false)];
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
    for (i, (name, segs)) in splits(text).into_iter().enumerate() {
        if i > 0 && skip((i + 1) / 2) {
            continue;
        }
        let (shown, offers) = outcome(&segs);
        if shown != up || !offered.iter().all(|o| offers.contains(o)) {
            bad.push(format!("{name}: up {shown:?}, offered {offers:?}"));
        }
    }
    assert!(bad.is_empty(), "want {up:?} up and {offered:?} offered however «{text}» is split; wrong for:\n{}", bad.join("\n"));
}

// Psalm 67:3 and 67:5 are worded alike: the chapter's verse is offered, not sent (twins), but
// it must be found whichever segment carried the reference.
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
    assert!(up.is_empty() && offered.contains(&v("Psalms", 67, 3)), "up {up:?}, offered {offered:?}");
    let whole = outcome(&[(PSALM_67.to_string(), true)]);
    assert_eq!((up, offered.contains(&v("Psalms", 67, 3))), (whole.0, whole.1.contains(&v("Psalms", 67, 3))));
}

#[test]
fn psalm_67_however_it_is_split() {
    // A boundary inside "sixty | seven" is a different number heard: not asked of the engine.
    same_however_split(PSALM_67, &[], &[v("Psalms", 67, 3)], |k| k == 2);
}

#[test]
fn a_full_citation_however_it_is_split() {
    same_however_split("turn with me to John chapter three verse sixteen for God so loved the world", &[v("John", 3, 16)], &[], |_| false);
}

#[test]
fn a_chapter_then_its_verse_quoted_however_split() {
    same_however_split("open your Bibles to Romans chapter eight we know that all things work together for good to them that love God", &[v("Romans", 8, 28)], &[], |_| false);
}
