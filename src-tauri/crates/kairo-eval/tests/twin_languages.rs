//! A verse read after its twin goes up in every language pack: Luke 22:20 up, then Matthew 26:28 read word
//! for word in that language. Texts come from each pack itself, so no wording is assumed. Before 2026-11-09
//! French and Yoruba only offered it (the words that tell twins apart kept a-z only, so "rémission" and
//! Yoruba's tone-marked words were cut away) and Portuguese put Mark 14:24, the first half of Matthew
//! 26:28, up halfway through it.

use std::sync::Arc;

use kairo_bible::{Book, Corpus, VerseRef};
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::Lang;

fn read(e: &mut Engine, k: &Knowledge, text: &str, t: &mut u64) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::new();
    for n in 1..=words.len() {
        *t += 380;
        let fin = n == words.len();
        if !fin && n % 3 != 0 {
            continue;
        }
        let seg = Segment { text: words[..n].join(" "), is_final: fin, speech_final: fin, continues_live: !fin };
        let o = e.handle(Event::Speech(seg), *t);
        if std::env::var_os("TWIN_TRACE").is_some() {
            for n in &o.notes {
                eprintln!("    {n}");
            }
        }
        for eff in o.effects {
            if let Effect::Show { verses, target: Target::Screen, .. } = eff {
                out.extend(verses.iter().map(|v| {
                    let r = k.verse_ref(*v);
                    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
                }));
            }
        }
    }
    *t += 1_000;
    out
}

/// The languages among `codes` where the twin did not go up as it should (`TWIN_TRACE=1` prints each
/// decision, `TWIN_ONLY=fr` runs one language).
fn failing(codes: &[&'static str]) -> Vec<&'static str> {
    let mut failures = Vec::new();
    let only = std::env::var("TWIN_ONLY").ok();
    for &code in codes.iter().filter(|c| only.as_deref().is_none_or(|o| o == **c)) {
        let dir = kairo_eval::root().join("databases/bibles");
        let corpus = Corpus::load(&dir).unwrap();
        let pack = corpus.load_flat(code, &dir.join(format!("packs/{code}.json"))).unwrap();
        let text = |b: Book, c: u16, v: u16| -> String {
            let id = corpus.id(VerseRef::new(b, c, v)).unwrap();
            pack.text(id).unwrap_or("").to_string()
        };
        let (luke, matthew) = (text(Book::from_name("Luke").unwrap(), 22, 20), text(Book::from_name("Matthew").unwrap(), 26, 28));
        let k = Arc::new(Knowledge::build(corpus, Wordings { own: Some(pack), also: vec![] }));
        let mut e = Engine::new(k.clone(), Config { offline: true, explain: std::env::var_os("TWIN_TRACE").is_some(), lang: Lang::from_code(code), ..Config::default() });
        let mut t = 10_000;
        let first = read(&mut e, &k, &luke, &mut t);
        let second = read(&mut e, &k, &matthew, &mut t);
        eprintln!("{code}: luke -> {first:?}, matthew -> {second:?}");
        if first != ["Luke 22:20"] || second != ["Matthew 26:28"] {
            failures.push(code);
        }
    }
    failures
}

#[test]
fn a_twin_read_after_its_twin_goes_up_in_every_language() {
    assert_eq!(failing(&["es", "fr", "de", "pt", "sw", "yo", "ig", "ha"]), Vec::<&str>::new());
}
