//! The word matchers on verses drawn from the whole Bible: read exactly, and
//! read with the slips real speech has. A verse's identical-wording twin
//! counts as found — no words can tell them apart.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Instant;

use kairo_bible::{Corpus, VerseId};
use kairo_detect::lexical::{verbatim, Lexicon, StreamMatcher, Wordings};

struct Fixture {
    corpus: Corpus,
    lex: Lexicon,
}

static F: LazyLock<Fixture> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    let corpus = Corpus::load(&dir).unwrap();
    let nlt = corpus.load_map_nlt(&dir.join("map.json")).unwrap();
    let t = Instant::now();
    let lex = Lexicon::build(&corpus, Wordings { own: None, also: vec![nlt] });
    eprintln!("lexicon built in {:.1}s: {} words, {} anchors, {} verses with twins", t.elapsed().as_secs_f32(), lex.vocab().len(), lex.anchor_count(), lex.verses_with_twins());
    Fixture { corpus, lex }
});

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn same_or_twin(found: VerseId, want: VerseId) -> bool {
    if found == want {
        return true;
    }
    let kjv = F.corpus.kjv();
    kjv.text(found) == kjv.text(want) || F.lex.twins(want).iter().any(|t| t.verse == found && t.shared >= 3)
}

/// Verses of a readable length (the matchers need six words to work with).
fn sample(n: usize, seed: u64) -> Vec<VerseId> {
    let mut rng = Rng(seed);
    let ids: Vec<VerseId> = F.corpus.ids().collect();
    let mut out = Vec::new();
    while out.len() < n {
        let v = ids[rng.below(ids.len())];
        let words = F.corpus.kjv().text(v).unwrap().split_whitespace().count();
        if (8..=40).contains(&words) {
            out.push(v);
        }
    }
    out
}

#[test]
fn verbatim_finds_verses_read_aloud() {
    let mut rng = Rng(0xbeef_1234_5678_9abc);
    let (mut exact_ok, mut slip_ok, mut n) = (0, 0, 0);
    let mut misses = Vec::new();
    for v in sample(1500, 0x1234_5678_9abc_def1) {
        n += 1;
        let text = F.corpus.kjv().text(v).unwrap();
        let top = verbatim(&F.lex, text, 6, 3).first().map(|h| h.verse);
        if top.is_some_and(|t| same_or_twin(t, v)) {
            exact_ok += 1;
        } else if misses.len() < 8 {
            misses.push(format!("exact {}: {:?}", F.corpus.verse_ref(v), top.map(|t| F.corpus.verse_ref(t))));
        }
        // A realistic slip: one word dropped, or "the" read as "a".
        let mut words: Vec<&str> = text.split_whitespace().collect();
        if rng.below(2) == 0 {
            let i = 1 + rng.below(words.len() - 1);
            words.remove(i);
        } else if let Some(i) = words.iter().position(|w| w.eq_ignore_ascii_case("the")) {
            words[i] = "a";
        }
        let slipped = words.join(" ");
        let top = verbatim(&F.lex, &slipped, 6, 3).first().map(|h| h.verse);
        if top.is_some_and(|t| same_or_twin(t, v)) {
            slip_ok += 1;
        } else if misses.len() < 16 {
            misses.push(format!("slip {}: {:?} ← {slipped:?}", F.corpus.verse_ref(v), top.map(|t| F.corpus.verse_ref(t))));
        }
    }
    let exact = exact_ok as f32 / n as f32;
    let slip = slip_ok as f32 / n as f32;
    println!("verbatim, read exactly: {:.1}% · with a slip: {:.1}%\n{}", exact * 100.0, slip * 100.0, misses.join("\n"));
    assert!(exact >= 0.99, "exact readings found {:.1}%", exact * 100.0);
    assert!(slip >= 0.97, "slipped readings found {:.1}%", slip * 100.0);
}

#[test]
fn streaming_confirms_verses_as_they_are_read() {
    let (mut ok, mut n) = (0, 0);
    let mut misses = Vec::new();
    for v in sample(800, 0x0dd_ba11_c0ff_ee00) {
        n += 1;
        let mut m = StreamMatcher::new();
        let mut confirmed = Vec::new();
        for (i, w) in F.corpus.kjv().text(v).unwrap().split_whitespace().enumerate() {
            confirmed.extend(m.push(&F.lex, w, i as u64 * 300).confirmed.into_iter().map(|c| c.verse));
        }
        if confirmed.iter().any(|c| same_or_twin(*c, v)) {
            ok += 1;
        } else if misses.len() < 10 {
            misses.push(format!("{}: {:?}", F.corpus.verse_ref(v), confirmed.iter().map(|c| F.corpus.verse_ref(*c)).collect::<Vec<_>>()));
        }
    }
    let rate = ok as f32 / n as f32;
    println!("streaming confirmed {:.1}% of verses read aloud\n{}", rate * 100.0, misses.join("\n"));
    // Short or common-worded verses legitimately never carry enough weight to confirm alone.
    assert!(rate >= 0.80, "confirmed {:.1}%", rate * 100.0);
}

#[test]
fn filler_never_confirms() {
    for filler in [
        "praise the lord praise the lord praise the lord hallelujah",
        "in the name of jesus christ we give you the glory amen",
        "our father in the lord we thank you for this day",
    ] {
        let mut m = StreamMatcher::new();
        let mut confirmed = Vec::new();
        for (i, w) in filler.split_whitespace().enumerate() {
            confirmed.extend(m.push(&F.lex, w, i as u64 * 300).confirmed);
        }
        assert!(confirmed.is_empty(), "{filler:?} confirmed {:?}", confirmed.iter().map(|c| F.corpus.verse_ref(c.verse)).collect::<Vec<_>>());
    }
}

#[test]
fn identical_wording_verses_are_twins() {
    let id = |b: &str, c: u16, v: u16| F.corpus.id(kairo_bible::VerseRef::new(kairo_bible::Book::from_name(b).unwrap(), c, v)).unwrap();
    let pairs = [(id("Mark", 9, 46), id("Mark", 9, 48)), (id("Revelation", 3, 13), id("Revelation", 3, 22))];
    for (a, b) in pairs {
        assert!(F.lex.twins(a).iter().any(|t| t.verse == b), "{} / {}", F.corpus.verse_ref(a), F.corpus.verse_ref(b));
    }
}
