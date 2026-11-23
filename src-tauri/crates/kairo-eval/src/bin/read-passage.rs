//! Cites a passage and reads it aloud, verse by verse at speaking pace, then reports what Kairo did:
//! how soon each verse went up after the verse before it was read to its end, which ones went up
//! while the verse before was still being read (early) or never, and any verse
//! from outside the passage that interrupted the reading. Many passages in each Bible version, so a
//! rule is judged on the whole Bible, not one passage.
//!
//! `cargo run --release -p kairo-eval --bin read-passage -- [--passages 300] [--seed N] [--list] [--trace "Mark 14:49"] [--bibles <folder>]`
//! `--trace`: the engine's notes while the passage starting at that verse is read.
//! Versions: the KJV, the BSB and every Bible installed in the Kairo app on this computer (`--bibles`,
//! default its translations folder), each matched as the app matches them; and the BSB again with
//! Kairo not given it ("BSB unseen"), standing in for a version Kairo has never seen.

use std::path::PathBuf;
use std::sync::Arc;

use kairo_bible::{Book, Corpus, Translation, VerseId};
use kairo_detect::engine::{Config, Effect, Engine, Event, Output, Segment, Target, TimerId};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_eval::root;

const MS_PER_WORD: u64 = 380;
const INTERIM_EVERY: usize = 3;
/// The offline engine locks words in at a pause; a long verse is said in chunks this long.
const CHUNK_WORDS: usize = 8;
const VERSE_GAP_MS: u64 = 700;
const SETTLE_MS: u64 = 4_000;
/// A verse up more than this after the verse before it ended is late.
const LATE_MS: u64 = 3_000;

struct Passage {
    book: Book,
    chapter: u16,
    verses: Vec<VerseId>,
}

#[derive(Default)]
struct Tally {
    passages: usize,
    verses: usize,
    delays: Vec<u64>,
    late: usize,
    early: usize,
    never: usize,
    outside: Vec<String>,
}

fn label(k: &Knowledge, v: VerseId) -> String {
    let r = k.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

/// A small, repeatable random sequence (no extra crate).
struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn passages(corpus: &Corpus, n: usize, seed: u64) -> Vec<Passage> {
    let ids: Vec<VerseId> = corpus.ids().collect();
    let mut rng = Rng(seed.max(1));
    let mut out = Vec::new();
    while out.len() < n {
        let start = ids[rng.next(ids.len())];
        let r = corpus.verse_ref(start);
        let len = 4 + rng.next(3);
        let count = corpus.verse_count(r.book, r.chapter).unwrap_or(0);
        if u16::from(r.verse) + len as u16 - 1 > count {
            continue;
        }
        let at = ids.iter().position(|v| *v == start).unwrap();
        out.push(Passage { book: r.book, chapter: r.chapter, verses: ids[at..at + len].to_vec() });
    }
    out
}

struct Run<'a> {
    e: Engine,
    k: &'a Knowledge,
    wakes: Vec<(u64, TimerId)>,
    /// Screen sends: verse and when.
    sent: Vec<(VerseId, u64)>,
    traced: bool,
}

impl Run<'_> {
    fn take(&mut self, o: Output, t: u64) {
        if self.traced {
            for n in &o.notes {
                println!("    {:>6.1}s  {n}", (t - 10_000) as f64 / 1000.0);
            }
        }
        for eff in o.effects {
            match eff {
                Effect::Show { verses, target: Target::Screen, .. } => self.sent.push((verses[0], t)),
                Effect::WakeAt { id, at_ms } => self.wakes.push((at_ms, id)),
                _ => {}
            }
        }
    }
    fn fire(&mut self, until: u64) {
        self.wakes.sort_by_key(|w| w.0);
        while self.wakes.first().is_some_and(|w| w.0 <= until) {
            let (when, id) = self.wakes.remove(0);
            let o = self.e.handle(Event::Timer(id), when);
            self.take(o, when);
        }
    }
    /// Says `text` from `t` at speaking pace: interims as it grows, finals at each chunk's end.
    fn say(&mut self, text: &str, mut t: u64) -> u64 {
        let words: Vec<&str> = text.split_whitespace().collect();
        for chunk in words.chunks(CHUNK_WORDS) {
            for n in 1..=chunk.len() {
                t += MS_PER_WORD;
                self.fire(t);
                let fin = n == chunk.len();
                if !fin && n % INTERIM_EVERY != 0 {
                    continue;
                }
                let seg = Segment { text: chunk[..n].join(" "), is_final: fin, speech_final: fin, continues_live: false };
                if self.traced && fin {
                    println!("    {:>6.1}s  heard \"{}\"", (t - 10_000) as f64 / 1000.0, chunk.join(" "));
                }
                let o = self.e.handle(Event::Speech(seg), t);
                self.take(o, t);
            }
        }
        t
    }
}

fn read(k: &Arc<Knowledge>, p: &Passage, text: &dyn Fn(VerseId) -> String, tally: &mut Tally, list: bool, trace: Option<&str>) {
    let traced = trace.is_some_and(|t| t == label(k, p.verses[0]));
    let mut run = Run { e: Engine::new(k.clone(), Config { offline: true, explain: traced, ..Config::default() }), k, wakes: Vec::new(), sent: Vec::new(), traced };
    let first = k.verse_ref(p.verses[0]).verse;
    let last = k.verse_ref(*p.verses.last().unwrap()).verse;
    let mut t = run.say(&format!("turn with me to {} chapter {} verses {} to {}", p.book.name(), p.chapter, first, last), 10_000);
    t += 1_500;
    let (mut starts, mut ends) = (Vec::new(), Vec::new());
    for v in &p.verses {
        starts.push(t);
        let end = run.say(&text(*v), t);
        ends.push(end);
        t = end + VERSE_GAP_MS;
    }
    run.fire(t + SETTLE_MS);
    tally.passages += 1;
    for (i, v) in p.verses.iter().enumerate().skip(1) {
        tally.verses += 1;
        let before_ended = ends[i - 1];
        let end = starts.get(i + 1).copied().unwrap_or(u64::MAX);
        // The send that put it up for its own reading: the last one before the next verse began.
        match run.sent.iter().filter(|(x, at)| x == v && *at < end).last() {
            None => tally.never += 1,
            Some((_, at)) if *at + 500 < before_ended => tally.early += 1,
            Some((_, at)) => {
                let d = at.saturating_sub(before_ended);
                tally.delays.push(d);
                if d > LATE_MS {
                    tally.late += 1;
                    if list {
                        println!("  late: {} {:.1}s", label(k, *v), d as f64 / 1000.0);
                    }
                }
            }
        }
    }
    for (v, at) in &run.sent {
        if !p.verses.contains(v) {
            let line = format!("{} during {} (at {:.1}s)", label(run.k, *v), label(run.k, p.verses[0]), (*at - 10_000) as f64 / 1000.0);
            if list {
                println!("  outside: {line}");
            }
            tally.outside.push(line);
        }
    }
}

fn report(name: &str, t: &mut Tally) {
    t.delays.sort_unstable();
    let pct = |q: f64| t.delays.get(((t.delays.len() as f64 - 1.0) * q) as usize).map_or(0.0, |d| *d as f64 / 1000.0);
    println!(
        "{name:<12} passages {:>4}  verses {:>5}  median {:.1}s  90th {:.1}s  late(>3s) {:>4}  early {:>3}  never {:>3}  outside verses up {:>3}",
        t.passages, t.verses, pct(0.5), pct(0.9), t.late, t.early, t.never, t.outside.len()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let val = |f: &str, d: u64| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (n, seed, list) = (val("--passages", 300) as usize, val("--seed", 7), args.iter().any(|a| a == "--list"));
    let trace = args.iter().position(|a| a == "--trace").and_then(|i| args.get(i + 1)).map(String::as_str);
    // The Bibles installed in the Kairo app on this computer (flat `<CODE>.json`), as the app matches them.
    let installed: PathBuf = args.iter().position(|a| a == "--bibles").and_then(|i| args.get(i + 1)).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/com.kairo.scripture/translations")
    });
    let dir: PathBuf = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).expect("kjv");
    let mut files: Vec<(String, PathBuf)> = vec![("BSB".into(), dir.join("translations/BSB.json"))];
    let mut found: Vec<PathBuf> = std::fs::read_dir(&installed).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    found.sort();
    files.extend(found.into_iter().filter_map(|p| Some((p.file_stem()?.to_str()?.to_uppercase(), p))));
    let load = |c: &Corpus, skip: &str| -> Vec<Translation> { files.iter().filter(|(code, _)| code != skip).filter_map(|(code, p)| c.load_flat(code, p).ok()).collect() };
    let texts: Vec<(String, Translation)> = files.iter().filter_map(|(code, p)| Some((code.clone(), corpus.load_flat(code, p).ok()?))).collect();
    println!("versions: KJV {}", texts.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>().join(" "));
    let known = Arc::new(Knowledge::build(Corpus::load(&dir).expect("kjv"), Wordings { own: None, also: load(&corpus, "") }));
    // The BSB with Kairo not given it: a version Kairo has never seen.
    let unseen = Arc::new(Knowledge::build(Corpus::load(&dir).expect("kjv"), Wordings { own: None, also: load(&corpus, "BSB") }));
    let ps = passages(&corpus, n, seed);
    let kjv = |v: VerseId| corpus.kjv().text(v).unwrap_or("").to_string();
    let mut runs: Vec<(String, &Arc<Knowledge>, Box<dyn Fn(VerseId) -> String + '_>)> = vec![("KJV".into(), &known, Box::new(kjv))];
    for (code, t) in &texts {
        runs.push((code.clone(), &known, Box::new(move |v: VerseId| t.text(v).unwrap_or("").to_string())));
    }
    if let Some((_, bsb)) = texts.iter().find(|(c, _)| c == "BSB") {
        runs.push(("BSB unseen".into(), &unseen, Box::new(move |v: VerseId| bsb.text(v).unwrap_or("").to_string())));
    }
    for (name, k, text) in &runs {
        let mut t = Tally::default();
        if list {
            println!("{name}");
        }
        for p in &ps {
            read(k, p, text.as_ref(), &mut t, list, trace);
        }
        report(name, &mut t);
    }
}
