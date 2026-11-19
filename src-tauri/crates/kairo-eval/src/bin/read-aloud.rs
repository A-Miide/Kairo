//! Reads verses aloud to the detector, a verse at a time at speaking pace with no reference said, and
//! reports what Kairo did: put the right verse on screen (and how far into the verse), only offered it,
//! missed it, or put the wrong verse up. A much larger test than six sermons, in any Bible version or
//! language the packs hold, so a rule can be judged on thousands of verses.
//!
//! `cargo run --release -p kairo-eval --bin read-aloud -- [--verses 400] [--only KJV,BSB,es,...] [--dir <folder of flat <CODE>.json versions>] [--out lines.jsonl] [--seed N]`
//! Versions: the KJV and BSB (the wordings Kairo matches), the language packs (es fr de pt sw yo ig ha: the
//! detector reads that pack as the service language's own Bible), and any flat `<CODE>.json` in `--dir`
//! (English versions the detector has never seen, to measure how well it copes with another wording).

use std::path::PathBuf;
use std::sync::Arc;

use kairo_bible::{Corpus, Translation, VerseId};
use kairo_detect::engine::{Config, Effect, Engine, Event, MeaningThresholds, Output, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::Lang;
use kairo_eval::{answer, meaning, root};
use kairo_semantic::MeaningService;

const MS_PER_WORD: u64 = 380;
const INTERIM_EVERY: usize = 3;
const MIN_WORDS: usize = 8;

#[derive(Default, Clone)]
struct Tally {
    n: usize,
    screen: usize,
    offered: usize,
    missed: usize,
    wrong: usize,
    wrong_twin: usize,
    /// Fraction of the verse spoken when it went up (screen sends only).
    at: Vec<f32>,
    wrong_list: Vec<String>,
    missed_list: Vec<String>,
}

fn label(k: &Knowledge, v: VerseId) -> String {
    let r = k.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

/// How long after the last word the detector is given to settle (its timers and meaning searches finish).
const SETTLE_MS: u64 = 4_000;

/// What the detector did with one reading: the first verse put on screen and how far into the verse that was,
/// and whether the true verse was at least offered.
struct Reading {
    screen: Option<(Vec<VerseId>, f32)>,
    offered: bool,
}

/// Reads one verse; timers fire when due and meaning searches are answered at once, as in a live session.
fn read(k: &Arc<Knowledge>, lang: Lang, truth: VerseId, text: &str, m: Option<&MeaningService>, thresholds: MeaningThresholds) -> Reading {
    let mut e = Engine::new(k.clone(), Config { offline: true, lang, meaning: m.is_some(), meaning_thresholds: thresholds, ..Config::default() });
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut t = 10_000u64;
    let mut wakes: Vec<(u64, kairo_detect::engine::TimerId)> = Vec::new();
    let mut r = Reading { screen: None, offered: false };
    // Takes one output: notes what it showed, queues its timers, answers its meaning searches.
    fn take(e: &mut Engine, o: Output, t: u64, truth: VerseId, at: f32, r: &mut Reading, wakes: &mut Vec<(u64, kairo_detect::engine::TimerId)>, m: Option<&MeaningService>) {
        let mut pending = vec![o];
        while let Some(o) = pending.pop() {
            for eff in o.effects {
                match eff {
                    Effect::Show { verses, target, .. } => match target {
                        Target::Screen if r.screen.is_none() => r.screen = Some((verses, at)),
                        Target::Suggestions if verses.contains(&truth) => r.offered = true,
                        _ => {}
                    },
                    Effect::WakeAt { id, at_ms } => wakes.push((at_ms, id)),
                    _ => {}
                }
            }
            if let Some(m) = m {
                for req in o.requests {
                    let ans = answer(m, &req);
                    // `MEANING_DUMP=1`: one line per meaning search: version-free, the top verse's cosine and lead, right or not.
                    if std::env::var("MEANING_DUMP").is_ok() {
                        if let (kairo_detect::engine::Answer::Paraphrase { candidates, .. }, kairo_detect::engine::Request::Paraphrase { windows, .. }) = (&ans, &req) {
                            let mut c: Vec<_> = candidates.iter().collect();
                            c.sort_by(|a, b| b.cos.total_cmp(&a.cos));
                            if let Some(top) = c.first() {
                                let second = c.iter().find(|x| x.verse != top.verse).map_or(0.0, |x| x.cos);
                                let lex = windows.get(top.window).map_or(0.0, |w| e.shared_wording(w, top.verse));
                                eprintln!("D\t{}\t{:.4}\t{:.4}\t{:.2}\t{:.1}", u8::from(top.verse == truth), top.cos, top.cos - second, at, lex);
                            }
                        }
                    }
                    pending.push(e.handle(Event::Answer(ans), t));
                }
            }
        }
    }
    let fire = |e: &mut Engine, until: u64, r: &mut Reading, at: f32, wakes: &mut Vec<(u64, kairo_detect::engine::TimerId)>| {
        wakes.sort_by_key(|w| w.0);
        while wakes.first().is_some_and(|w| w.0 <= until) {
            let (when, id) = wakes.remove(0);
            let o = e.handle(Event::Timer(id), when);
            take(e, o, when, truth, at, r, wakes, m);
        }
    };
    for n in 1..=words.len() {
        t += MS_PER_WORD;
        let at = n as f32 / words.len() as f32;
        fire(&mut e, t, &mut r, at, &mut wakes);
        let fin = n == words.len();
        if !fin && n % INTERIM_EVERY != 0 {
            continue;
        }
        let seg = Segment { text: words[..n].join(" "), is_final: fin, speech_final: fin, continues_live: !fin };
        let o = e.handle(Event::Speech(seg), t);
        take(&mut e, o, t, truth, at, &mut r, &mut wakes, m);
    }
    fire(&mut e, t + SETTLE_MS, &mut r, 1.0, &mut wakes);
    r
}

/// The English knowledge with the BSB and these extra wordings (a flat `<CODE>.json` each in `extra`), except `skip`.
fn english_with(codes: &[String], skip: &str, extra: &std::path::Path, dir: &std::path::Path) -> Knowledge {
    let corpus = Corpus::load(dir).expect("kjv");
    let mut also: Vec<Translation> = Vec::new();
    if !skip.eq_ignore_ascii_case("BSB") {
        if let Ok(t) = corpus.load_flat("BSB", &dir.join("translations/BSB.json")) {
            also.push(t);
        }
    }
    for c in codes.iter().filter(|c| !c.eq_ignore_ascii_case(skip)) {
        if let Ok(t) = corpus.load_flat(c, &extra.join(format!("{c}.json"))) {
            also.push(t);
        }
    }
    Knowledge::build(corpus, Wordings { own: None, also })
}

/// This process's resident memory in MB.
fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().ok();
    out.and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.trim().parse::<f64>().ok()).map_or(0.0, |kb| kb / 1024.0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let per: usize = opt("--verses").and_then(|s| s.parse().ok()).unwrap_or(400);
    let only: Option<Vec<String>> = opt("--only").map(|o| o.split(',').map(String::from).collect());
    let seed: usize = opt("--seed").and_then(|s| s.parse().ok()).unwrap_or(0);
    let out = opt("--out").map(PathBuf::from);
    // `--meaning` reads with the meaning model on, as the app ships it (English: bge-small + the MiniLM reranker).
    let service: Option<MeaningService> = args.iter().any(|a| a == "--meaning").then(|| meaning(&opt("--embedder").unwrap_or_else(|| "bge-small.json".into()), Some(&opt("--reranker").unwrap_or_else(|| "minilm.json".into())), &["KJV"]));
    // `--meaning-other <embedder.json>`: the multilingual meaning model for the service languages other than English
    // (no cross-encoder), with `--thresholds-other <json>`; a `--dir` file named like `esBES.json` is a version in
    // that language read to a service whose Bible is the pack.
    let service_other: Option<MeaningService> = opt("--meaning-other").map(|e| meaning(&e, None, &["KJV"]));
    let thresholds_other: MeaningThresholds = opt("--thresholds-other").map(|p| serde_json::from_slice(&std::fs::read(p).expect("thresholds")).expect("thresholds json")).unwrap_or(MeaningThresholds::LEGACY);
    let thresholds: MeaningThresholds = opt("--thresholds").map(|p| serde_json::from_slice(&std::fs::read(p).expect("thresholds")).expect("thresholds json")).unwrap_or(MeaningThresholds::BGE_SMALL);
    // `--also WEB,ASV`: extra wordings (from --dir) the detector matches beside the KJV and BSB, left out
    // whenever that very version is the one being read, so it is tested on wording it has not been given.
    let also: Vec<String> = opt("--also").map(|o| o.split(',').map(String::from).collect()).unwrap_or_default();
    let extra_dir = opt("--dir").map(PathBuf::from);
    let want = |code: &str| only.as_ref().is_none_or(|o| o.iter().any(|c| c.eq_ignore_ascii_case(code)));

    let dir = root().join("databases/bibles");
    // `--mem`: what the knowledge costs in memory, plain and with a language pack's wording as well.
    if args.iter().any(|a| a == "--mem") {
        let r0 = rss_mb();
        let plain = kairo_eval::knowledge();
        let r1 = rss_mb();
        let c = Corpus::load(&dir).expect("kjv");
        let pack = c.load_flat("es", &dir.join("packs/es.json")).expect("es");
        let with_own = Knowledge::build(c, Wordings { own: Some(pack), also: vec![] });
        let r2 = rss_mb();
        println!("start {r0:.0} MB; KJV+BSB knowledge {r1:.0} MB (+{:.0}); a second knowledge with the Spanish pack as its own wording {r2:.0} MB (+{:.0})", r1 - r0, r2 - r1);
        drop((plain, with_own));
        return;
    }
    let corpus = Corpus::load(&dir).expect("kjv");
    let ids: Vec<VerseId> = corpus.ids().collect();
    let k_en = Arc::new(kairo_eval::knowledge());

    // (code, language, knowledge, text source)
    let mut jobs: Vec<(String, Lang, Arc<Knowledge>, Translation)> = Vec::new();
    if want("KJV") {
        let k = if also.is_empty() { k_en.clone() } else { Arc::new(english_with(&also, "KJV", extra_dir.as_deref().unwrap_or(&dir), &dir)) };
        jobs.push(("KJV".into(), Lang::En, k, corpus.kjv().clone()));
    }
    if want("BSB") {
        if let Ok(t) = corpus.load_flat("BSB", &dir.join("translations/BSB.json")) {
            let k = if also.is_empty() { k_en.clone() } else { Arc::new(english_with(&also, "BSB", extra_dir.as_deref().unwrap_or(&dir), &dir)) };
            jobs.push(("BSB".into(), Lang::En, k, t));
        }
    }
    if let Some(extra) = opt("--dir").map(PathBuf::from) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&extra).map(|d| d.filter_map(|f| f.ok().map(|f| f.path())).collect()).unwrap_or_default();
        files.sort();
        for f in files.iter().filter(|f| f.extension().is_some_and(|x| x == "json")) {
            let code = f.file_stem().unwrap().to_string_lossy().to_string();
            // A version in a service language: the pack is the service's Bible, this is what the church reads.
            let in_lang = code.get(..2).filter(|l| l.bytes().all(|b| b.is_ascii_lowercase()) && code.len() > 2 && code.as_bytes()[2].is_ascii_uppercase());
            if let Some(l) = in_lang.filter(|_| want(&code)) {
                if let (Ok(t), Ok(pack)) = (corpus.load_flat(&code, f), corpus.load_flat(l, &dir.join(format!("packs/{l}.json")))) {
                    let k = Arc::new(Knowledge::build(Corpus::load(&dir).expect("kjv"), Wordings { own: Some(pack), also: vec![] }));
                    jobs.push((code.clone(), Lang::from_code(l), k, t));
                }
                continue;
            }
            if want(&code) {
                if let Ok(t) = corpus.load_flat(&code, f) {
                    // `--own`: the church's Bible built into the detector as its own wording, as the server does.
                    let k = if args.iter().any(|a| a == "--own") {
                        Arc::new(Knowledge::build(Corpus::load(&dir).expect("kjv"), Wordings { own: Some(t.clone()), also: vec![] }))
                    } else if also.is_empty() { k_en.clone() } else { Arc::new(english_with(&also, &code, extra_dir.as_deref().unwrap(), &dir)) };
                    jobs.push((code, Lang::En, k, t));
                }
            }
        }
    }
    for code in ["es", "fr", "de", "pt", "sw", "yo", "ig", "ha"] {
        if !want(code) {
            continue;
        }
        let Ok(pack) = corpus.load_flat(code, &dir.join(format!("packs/{code}.json"))) else { continue };
        // `--no-own`: the detector as the app builds it today, with no language pack wording (own: None),
        // reading the pack's text aloud in that service language.
        let k = if args.iter().any(|a| a == "--no-own") {
            k_en.clone()
        } else {
            let c2 = Corpus::load(&dir).expect("kjv");
            Arc::new(Knowledge::build(c2, Wordings { own: Some(pack.clone()), also: vec![] }))
        };
        jobs.push((code.into(), Lang::from_code(code), k, pack));
    }

    println!("{:<7} {:>5} {:>8} {:>9} {:>9} {:>8} {:>10} {:>9}   (words said before it went up, as a share of the verse)", "version", "n", "on screen", "offered", "missed", "wrong", "wrong-twin", "median at");
    let mut lines: Vec<String> = Vec::new();
    for (code, lang, k, tr) in &jobs {
        // The verses worth reading: long enough, in this version.
        let pool: Vec<VerseId> = ids.iter().copied().filter(|v| tr.text(*v).is_some_and(|t| t.split_whitespace().count() >= MIN_WORDS)).collect();
        let step = (pool.len() / per).max(1);
        let picks: Vec<VerseId> = pool.iter().copied().skip(seed % step).step_by(step).take(per).collect();
        let mut tally = Tally::default();
        for v in picks {
            let text = tr.text(v).unwrap_or("");
            let Reading { screen, offered } = read(k, *lang, v, text, if *lang == Lang::En { service.as_ref() } else { service_other.as_ref() }, if *lang == Lang::En { thresholds } else { thresholds_other });
            tally.n += 1;
            let verdict = match &screen {
                Some((shown, at)) if shown.contains(&v) => {
                    tally.screen += 1;
                    tally.at.push(*at);
                    "screen"
                }
                Some((shown, _)) => {
                    let twin = k.lexicon.twins(v).iter().any(|t| shown.contains(&t.verse));
                    if twin {
                        tally.wrong_twin += 1;
                    } else {
                        tally.wrong += 1;
                        tally.wrong_list.push(format!("{} -> {}", label(k, v), shown.iter().map(|s| label(k, *s)).collect::<Vec<_>>().join(", ")));
                    }
                    "wrong"
                }
                None if offered => {
                    tally.offered += 1;
                    "offered"
                }
                None => {
                    tally.missed += 1;
                    tally.missed_list.push(label(k, v));
                    "missed"
                }
            };
            lines.push(format!("{{\"version\":\"{code}\",\"verse\":\"{}\",\"result\":\"{verdict}\",\"at\":{}}}", label(k, v), screen.as_ref().map_or(-1.0, |s| s.1 as f64)));
        }
        let pct = |x: usize| 100.0 * x as f64 / tally.n.max(1) as f64;
        tally.at.sort_by(f32::total_cmp);
        let median = tally.at.get(tally.at.len() / 2).copied().map_or("-".to_string(), |m| format!("{:.0}%", m * 100.0));
        println!("{:<7} {:>5} {:>7.1}% {:>8.1}% {:>8.1}% {:>7.1}% {:>9.1}% {:>9}", code, tally.n, pct(tally.screen), pct(tally.offered), pct(tally.missed), pct(tally.wrong), pct(tally.wrong_twin), median);
        for w in tally.wrong_list.iter().take(6) {
            println!("          wrong: {w}");
        }
    }
    if let Some(p) = out {
        std::fs::write(p, lines.join("\n")).ok();
    }
    }
