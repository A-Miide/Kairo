//! Replays a live run from the app's debug.log exactly: every input the engine took (speech, meaning
//! answers, timers, the operator's sends), at the time it took it, with the settings it had. The
//! decisions must come out as the live app made them; any difference is printed.
//!
//!   cargo run --release -p kairo-eval --bin replay-log -- [debug.log] [--run N] [--explain 09:21:00..09:21:05] [--twice]
//!
//! - `debug.log`: defaults to the app's (macOS app data).
//! - `--run N`: which run of the app (1 = the first in the log, -1 = the last; default -1).
//! - `--explain a..b`: print why each decision went where it did, for inputs between these clock times.
//! - `--twice`: replay twice and check both give the same decisions (the engine is deterministic).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kairo_bible::{Book, Corpus, VerseId};
use kairo_detect::engine::{Config, Effect, Engine, Event, Learned, MeaningThresholds, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::Lang;
use kairo_eval::root;
use serde_json::Value;

/// One line of the log that matters here.
enum Line {
    Config(Value),
    Input { now: u64, at: u64, event: Event },
    /// What the live app decided: (clock time, verses, target, method).
    Detect { at: u64, verses: Vec<String>, target: String, method: String },
}

fn app_log() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME")).join("Library/Application Support/com.kairo.scripture/debug.log")
}

fn app_data(log: &Path) -> PathBuf {
    log.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The log's lines, split into runs of the app (a run starts where the engine starts with `now` 0).
fn runs(log: &Path) -> Vec<Vec<Line>> {
    let text = std::fs::read_to_string(log).expect("read debug.log");
    let mut runs: Vec<Vec<Line>> = Vec::new();
    for l in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        let at = v["t"].as_u64().unwrap_or(0);
        let d = &v["data"];
        match v["event"].as_str().unwrap_or("") {
            "server:engine-config" => {
                if d["now"].as_u64() == Some(0) && d["fresh"].as_bool() == Some(true) {
                    runs.push(Vec::new());
                }
                if let Some(r) = runs.last_mut() {
                    r.push(Line::Config(d.clone()));
                }
            }
            "server:engine" => {
                let (Some(now), Ok(event)) = (d["now"].as_u64(), serde_json::from_value::<Event>(d["event"].clone())) else { continue };
                if let Some(r) = runs.last_mut() {
                    r.push(Line::Input { now, at, event });
                }
            }
            "server:detect" => {
                let verses = d["verses"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                if let Some(r) = runs.last_mut() {
                    r.push(Line::Detect { at, verses, target: d["target"].as_str().unwrap_or("").into(), method: d["method"].as_str().unwrap_or("").into() });
                }
            }
            _ => {}
        }
    }
    runs
}

/// The knowledge the live app built ("en", "en:file:NIV", "es:pack").
fn knowledge(from: &str, data: &Path) -> Knowledge {
    let db = root().join("databases/bibles");
    let corpus = Corpus::load(&db).expect("kjv");
    let mut parts = from.split(':');
    let lang = Lang::from_code(parts.next().unwrap_or("en"));
    let wordings = match (parts.next(), parts.next()) {
        (Some("file"), Some(code)) => Wordings { own: Some(corpus.load_flat(code, &data.join("translations").join(format!("{code}.json"))).expect("the church's Bible file")), also: vec![] },
        (Some("pack"), _) => {
            let code = from.get(..2).unwrap_or("en");
            let downloaded = data.join("packs").join(format!("{code}.json"));
            let path = if downloaded.is_file() { downloaded } else { db.join("packs").join(format!("{code}.json")) };
            let t = corpus.load_flat(code, &path).expect("language pack");
            Wordings { own: Some(kairo_detect::lexical::text::for_matching(&t, lang)), also: vec![] }
        }
        (Some("api"), _) => panic!("knowledge from an API.Bible translation ({from}) isn't replayable yet"),
        // English: the KJV and the BSB, as the app matches them.
        _ => Wordings { own: None, also: vec![kairo_eval::translations().1] },
    };
    let anchors_in = if wordings.own.is_some() { lang } else { Lang::En };
    Knowledge::build_for(corpus, wordings, anchors_in)
}

fn config(c: &Value, k: &Knowledge) -> Config {
    let ids = |key: &str| -> HashSet<VerseId> { c[key].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| VerseId(n as u32))).collect()).unwrap_or_default() };
    let book_words: HashMap<String, Book> = c["bookWords"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| Some((p[0].as_str()?.to_string(), Book::from_name(p[1].as_str()?)?))).collect())
        .unwrap_or_default();
    let _ = k;
    Config {
        lang: serde_json::from_value(c["lang"].clone()).unwrap_or_default(),
        auto_send: c["autoSend"].as_bool().unwrap_or(true),
        auto_correct: c["autoCorrect"].as_bool().unwrap_or(true),
        offline: c["offline"].as_bool().unwrap_or(false),
        meaning: c["meaning"].as_bool().unwrap_or(false),
        explain: false,
        meaning_thresholds: serde_json::from_value::<MeaningThresholds>(c["thresholds"].clone()).unwrap_or(MeaningThresholds::LEGACY),
        learned: Arc::new(Learned::new(book_words, ids("familiar"), ids("takenDown"))),
    }
}

fn clock(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    // Local time of day, as the log's readers see it.
    let local = std::process::Command::new("date").args(["-r", &secs.to_string(), "+%H:%M:%S"]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    format!("{}.{:03}", local.trim(), ms % 1000)
}

/// A clock time ("09:21:00") on the day of `ms`, as epoch ms.
fn at_time(day_ms: u64, hms: &str) -> u64 {
    let secs = (day_ms / 1000) as i64;
    let day = std::process::Command::new("date").args(["-r", &secs.to_string(), "+%Y-%m-%d"]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    let out = std::process::Command::new("date").args(["-j", "-f", "%Y-%m-%d %H:%M:%S", &format!("{} {hms}", day.trim()), "+%s"]).output().expect("date");
    String::from_utf8(out.stdout).ok().and_then(|s| s.trim().parse::<u64>().ok()).expect("a time like 09:21:00") * 1000
}

/// Every decision the engine makes for this run: (clock time, verses, target, method).
fn replay(lines: &[Line], data: &Path, explain: Option<(u64, u64)>) -> Vec<(u64, Vec<String>, String, String)> {
    let mut engine: Option<(Engine, Arc<Knowledge>)> = None;
    let mut built = String::new();
    let mut out = Vec::new();
    for l in lines {
        match l {
            Line::Config(c) => {
                let from = c["builtFrom"].as_str().filter(|s| !s.is_empty()).unwrap_or("en").to_string();
                let k = match &engine {
                    Some((_, k)) if from == built => k.clone(),
                    _ => Arc::new(knowledge(&from, data)),
                };
                built = from;
                let mut cfg = config(c, &k);
                cfg.explain = explain.is_some();
                match (&mut engine, c["fresh"].as_bool() == Some(true)) {
                    (Some((e, _)), false) => *e.config_mut() = cfg,
                    _ => engine = Some((Engine::new(k.clone(), cfg), k)),
                }
            }
            Line::Input { now, at, event } => {
                let Some((e, k)) = &mut engine else { continue };
                let o = e.handle(event.clone(), *now);
                let shown = explain.is_some_and(|(a, b)| *at >= a && *at <= b);
                if shown {
                    let what = match event {
                        Event::Speech(s) => format!("{} \"…{}\"", if s.is_final { "FINAL" } else { "interim" }, s.text.chars().rev().take(70).collect::<Vec<_>>().into_iter().rev().collect::<String>()),
                        Event::Answer(a) => format!("answer {a:?}").chars().take(300).collect(),
                        other => format!("{other:?}"),
                    };
                    println!("{} now={now} {what}", clock(*at));
                    for n in &o.notes {
                        println!("      .. {n}");
                    }
                }
                for eff in o.effects {
                    if let Effect::Show { verses, method, target, .. } = eff {
                        if target == Target::Drop || verses.is_empty() {
                            continue;
                        }
                        let refs: Vec<String> = verses.iter().map(|v| {
                            let r = k.verse_ref(*v);
                            format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
                        }).collect();
                        let t = if target == Target::Screen { "screen" } else { "suggestions" };
                        if shown {
                            println!("      -> {t} {} {}", method.wire(), refs.join(", "));
                        }
                        out.push((*at, refs, t.to_string(), method.wire().to_string()));
                    }
                }
            }
            Line::Detect { .. } => {}
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let log = args.iter().enumerate().find(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--run", "--explain"].contains(&args[i - 1].as_str()))).map(|(_, a)| PathBuf::from(a)).unwrap_or_else(app_log);
    let data = app_data(&log);
    let all = runs(&log);
    if all.is_empty() {
        eprintln!("No recorded runs in {} (the app records its engine inputs from 4.6.2 on).", log.display());
        std::process::exit(1);
    }
    let n: i64 = opt("--run").and_then(|s| s.parse().ok()).unwrap_or(-1);
    let idx = if n < 0 { all.len() as i64 + n } else { n - 1 };
    let lines = all.get(idx.max(0) as usize).expect("no such run");
    let first_at = lines.iter().find_map(|l| match l { Line::Input { at, .. } => Some(*at), _ => None }).unwrap_or(0);
    let explain = opt("--explain").and_then(|r| r.split_once("..").map(|(a, b)| (at_time(first_at, a), at_time(first_at, b))));
    println!("Run {} of {} ({} inputs from {})", idx + 1, all.len(), lines.iter().filter(|l| matches!(l, Line::Input { .. })).count(), clock(first_at));

    let ours = replay(lines, &data, explain);
    let live: Vec<(u64, Vec<String>, String, String)> = lines
        .iter()
        .filter_map(|l| match l {
            Line::Detect { at, verses, target, method } if target != "range-step" => {
                // Auto-send off or Director mode changes where a send lands, not the decision.
                // Hidden (an offer of a kind wrong too often) is still the engine's offer.
                let t = match target.as_str() {
                    "staged" => "screen",
                    "hidden" => "suggestions",
                    other => other,
                };
                Some((*at, verses.clone(), t.to_string(), method.clone()))
            }
            _ => None,
        })
        .collect();
    let key = |d: &(u64, Vec<String>, String, String)| format!("{} {} {}", d.2, d.3, d.1.join(", "));
    let (a, b): (Vec<String>, Vec<String>) = (live.iter().map(key).collect(), ours.iter().map(key).collect());
    if a == b {
        println!("Same {} decisions as the live app.", a.len());
    } else {
        println!("Live and replay differ ({} live, {} replay):", a.len(), b.len());
        let width = a.len().max(b.len());
        for i in 0..width {
            let (l, r) = (a.get(i).cloned().unwrap_or_default(), b.get(i).cloned().unwrap_or_default());
            let at = live.get(i).or(ours.get(i)).map_or(String::new(), |d| clock(d.0));
            println!("{} {at}  live: {l:<50} replay: {r}", if l == r { " " } else { "≠" });
        }
    }
    if args.iter().any(|a| a == "--twice") {
        let again = replay(lines, &data, None);
        let c: Vec<String> = again.iter().map(key).collect();
        println!("{}", if c == b { "Twice: identical." } else { "Twice: DIFFERENT — the engine isn't deterministic." });
    }
}
