//! Replays recorded sermons through the engine and scores what it decides.
//!
//! Inputs are the exact interim/final sequence the Node pipeline was fed
//! (`parity/golden/*.json`). Two clocks:
//!
//! - `--clock audio` (default): each segment at its real position in the
//!   sermon — the windows measured in seconds behave as they do in a service.
//! - `--clock golden`: the compressed wall clock the Node recording ran at,
//!   for comparing decision by decision with Node (`--parity`).
//!
//! Scoring follows `server/eval/run_eval.js`: a ground-truth verse counts
//! when it reaches the screen within 30 s of where it was said; a verse sent
//! that the sermon never cites is a wrong send (verses of a chapter cited
//! with no verse are exempt for 10 minutes); a miss offered in Possible
//! Matches is counted separately; suggestions per hour measure busyness.
//!
//! Only the dev sermons are replayed unless `--held-out` is given
//! (docs/RUST_REWRITE_PLAN.md, "Evaluation split").
//!
//!   cargo run --release -p kairo-detect --bin replay -- [--clock golden] [--parity] [--held-out] [--verbose] [name ...]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use kairo_bible::{Book, Corpus, VerseId, VerseRef};
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use serde_json::Value;

const DEV: &[&str] = &["enough-is-enough", "oyedepo-kingdomlaw", "shiloh2025-impartation", "2026-09-26-identity-in-christ-1", "2026-09-26-identity-in-christ-2", "2026-09-26-identity-in-christ-3"];
const HELD_OUT: &[&str] = &["oyedepo-meditation", "shiloh2025-love-mystery"];
const MATCH_WINDOW_MS: i64 = 30_000;
const BARE_CHAPTER_EXEMPT_MS: i64 = 10 * 60 * 1000;
/// Speaking pace assumed where a recording has no audio times (live runs).
const MS_PER_WORD: u64 = 400;

struct Decision {
    cue: usize,
    at_ms: i64,
    target: Target,
    method: String,
    verses: Vec<VerseId>,
    corrected: bool,
}

struct GroundTruth {
    keys: Vec<VerseId>,
    bare: Option<(Book, u16)>,
    at_ms: i64,
}

#[derive(Default)]
struct Score {
    found: usize,
    total: usize,
    sent: usize,
    wrong: usize,
    offered: usize,
    suggestions: usize,
    hours: f64,
    wrong_list: Vec<String>,
    missed_list: Vec<String>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn load_knowledge() -> Knowledge {
    let dir = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).expect("kjv");
    let nlt = corpus.load_map_nlt(&dir.join("map.json")).expect("nlt");
    let niv = corpus.load_flat("NIV", &dir.join("translations/NIV.json")).expect("niv");
    Knowledge::build(corpus, Wordings { own: None, also: vec![nlt, niv] })
}

/// "1 John 5:4", "Psalms 23:1-3" → its first verse.
fn parse_ref(k: &Knowledge, s: &str) -> Option<VerseId> {
    let (book, cv) = s.rsplit_once(' ')?;
    let book = Book::from_name(book).or_else(|| Book::from_name(&format!("{book}s"))).or_else(|| Book::from_name(book.trim_end_matches('s')))?;
    let (c, v) = cv.split_once(':')?;
    let v = v.split(['-', ',']).next()?;
    k.id(VerseRef::new(book, c.parse().ok()?, v.parse().ok()?))
}

fn label(k: &Knowledge, v: VerseId) -> String {
    let r = k.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

fn ground_truth(k: &Knowledge, fixture: &Value) -> Vec<GroundTruth> {
    let mut out = Vec::new();
    for g in fixture["groundTruth"].as_array().into_iter().flatten() {
        let Some(book) = g["book"].as_str().and_then(|b| Book::from_name(b).or_else(|| Book::from_name(&format!("{b}s")))) else { continue };
        let ch = g["chapter"].as_u64().unwrap_or(0) as u16;
        let at_ms = g["startMs"].as_i64().unwrap_or(0);
        let id = |v: u64| k.id(VerseRef::new(book, ch, v as u16));
        let keys: Vec<VerseId> = if let Some(v) = g["verse"].as_u64() {
            id(v).into_iter().collect()
        } else if let (Some(a), Some(b)) = (g["verseStart"].as_u64(), g["verseEnd"].as_u64()) {
            (a..=b).filter_map(id).collect()
        } else if let Some(r) = g["ranges"].as_array() {
            r.iter().flat_map(|x| (x["start"].as_u64().unwrap_or(1)..=x["end"].as_u64().unwrap_or(0)).filter_map(id)).collect()
        } else {
            Vec::new()
        };
        let bare = keys.is_empty().then_some((book, ch));
        out.push(GroundTruth { keys, bare, at_ms });
    }
    out
}

fn score(k: &Knowledge, gt: &[GroundTruth], decisions: &[Decision], last_ms: i64) -> Score {
    let screen: Vec<&Decision> = decisions.iter().filter(|d| d.target == Target::Screen).collect();
    let mut s = Score { hours: (last_ms.max(1) as f64 / 3_600_000.0).max(1.0), ..Default::default() };
    let mut used: HashSet<usize> = HashSet::new();
    let mut missed: Vec<&GroundTruth> = Vec::new();
    for g in gt.iter().filter(|g| !g.keys.is_empty()) {
        s.total += 1;
        let hit = screen.iter().enumerate().find(|(i, d)| !used.contains(i) && g.keys.contains(&d.verses[0]) && (d.at_ms - g.at_ms).abs() <= MATCH_WINDOW_MS);
        match hit {
            Some((i, _)) => {
                used.insert(i);
                s.found += 1;
            }
            None => missed.push(g),
        }
    }
    let all_keys: HashSet<VerseId> = gt.iter().flat_map(|g| g.keys.iter().copied()).collect();
    let in_bare = |d: &Decision| {
        let r = k.verse_ref(d.verses[0]);
        gt.iter().any(|g| g.bare == Some((r.book, r.chapter)) && d.at_ms >= g.at_ms && d.at_ms - g.at_ms <= BARE_CHAPTER_EXEMPT_MS)
    };
    s.sent = screen.len();
    for (i, d) in screen.iter().enumerate() {
        if !used.contains(&i) && !all_keys.contains(&d.verses[0]) && !in_bare(d) {
            s.wrong += 1;
            s.wrong_list.push(format!("{:>6.1}m {} ({}{})", d.at_ms as f64 / 60_000.0, label(k, d.verses[0]), d.method, if d.corrected { ", corrected" } else { "" }));
        }
    }
    let suggestions: Vec<&Decision> = decisions.iter().filter(|d| d.target == Target::Suggestions).collect();
    s.suggestions = suggestions.len();
    for g in &missed {
        let offered = suggestions.iter().any(|d| g.keys.contains(&d.verses[0]) && (d.at_ms - g.at_ms).abs() <= MATCH_WINDOW_MS);
        if offered {
            s.offered += 1;
        }
        s.missed_list.push(format!("{:>6.1}m {}{}", g.at_ms as f64 / 60_000.0, label(k, g.keys[0]), if offered { " (offered)" } else { "" }));
    }
    s
}

/// Each recorded input at its time on the chosen clock.
fn timeline(golden: &Value, audio: bool) -> Vec<(u64, usize, String, bool)> {
    let cues = golden["cues"].as_array().unwrap();
    let inputs = golden["inputs"].as_array().unwrap();
    // Cue start and end on the audio clock (synthesised from word counts where there's no audio).
    let mut spans: Vec<(u64, u64)> = Vec::with_capacity(cues.len());
    let mut synth = 0u64;
    for (i, c) in cues.iter().enumerate() {
        let words = c["text"].as_str().unwrap_or("").split_whitespace().count() as u64;
        let start = c["audioMs"].as_u64().unwrap_or(synth);
        let end = cues.get(i + 1).and_then(|n| n["audioMs"].as_u64()).unwrap_or(start + words.max(1) * MS_PER_WORD);
        spans.push((start, end.max(start + 1)));
        synth = start + words.max(1) * MS_PER_WORD;
    }
    inputs
        .iter()
        .map(|x| {
            let cue = x["cue"].as_u64().unwrap() as usize;
            let text = x["text"].as_str().unwrap().to_string();
            let fin = x["isFinal"].as_bool().unwrap();
            let t = if audio {
                let total = cues[cue]["text"].as_str().unwrap_or("").split_whitespace().count().max(1) as f64;
                let said = text.split_whitespace().count() as f64;
                let (a, b) = spans[cue];
                a + ((b - a) as f64 * 0.95 * (said / total)) as u64
            } else {
                x["t"].as_u64().unwrap()
            };
            (t, cue, text, fin)
        })
        .collect()
}

fn cue_ms(golden: &Value) -> Vec<i64> {
    let mut synth = 0i64;
    golden["cues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let words = c["text"].as_str().unwrap_or("").split_whitespace().count() as i64;
            let at = c["audioMs"].as_i64().unwrap_or(synth);
            synth = at + words.max(1) * MS_PER_WORD as i64;
            at
        })
        .collect()
}

fn replay(k: &Arc<Knowledge>, golden: &Value, audio: bool) -> (Vec<Decision>, f64) {
    let mut engine = Engine::new(k.clone(), Config::default());
    let cue_at = cue_ms(golden);
    let mut out = Vec::new();
    let mut wakes: Vec<(u64, Event)> = Vec::new();
    let started = Instant::now();
    let collect = |o: kairo_detect::engine::Output, cue: usize, wakes: &mut Vec<(u64, Event)>, out: &mut Vec<Decision>| {
        for e in o.effects {
            match e {
                Effect::Show { verses, method, target, corrected_from, .. } => {
                    out.push(Decision { cue, at_ms: cue_at[cue.min(cue_at.len() - 1)], target, method: method.wire().to_string(), verses, corrected: corrected_from.is_some() })
                }
                Effect::WakeAt { id, at_ms } => wakes.push((at_ms, Event::Timer(id))),
                _ => {}
            }
        }
    };
    for (t, cue, text, fin) in timeline(golden, audio) {
        // Timers due before this input fire first.
        wakes.sort_by_key(|w| w.0);
        while wakes.first().is_some_and(|w| w.0 <= t) {
            let (at, ev) = wakes.remove(0);
            let o = engine.handle(ev, at);
            collect(o, cue, &mut wakes, &mut out);
        }
        let o = engine.handle(Event::Speech(Segment { text, is_final: fin, speech_final: fin, continues_live: false }), t);
        collect(o, cue, &mut wakes, &mut out);
    }
    (out, started.elapsed().as_secs_f64())
}

fn decision_key(k: &Knowledge, cue: usize, target: &str, method: &str, refs: &[String]) -> String {
    let _ = k;
    format!("{cue}|{target}|{method}|{}", refs.join("+"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let audio = !args.windows(2).any(|w| w[0] == "--clock" && w[1] == "golden");
    let parity = args.iter().any(|a| a == "--parity");
    let verbose = args.iter().any(|a| a == "--verbose");
    let held_out = args.iter().any(|a| a == "--held-out");
    let names: Vec<&str> = args.iter().filter(|a| !a.starts_with("--") && *a != "golden" && *a != "audio").map(String::as_str).collect();
    let chosen: Vec<&str> = if !names.is_empty() {
        names
    } else if held_out {
        DEV.iter().chain(HELD_OUT).copied().collect()
    } else {
        DEV.to_vec()
    };

    let t0 = Instant::now();
    let k = Arc::new(load_knowledge());
    eprintln!("[replay] knowledge built in {:.1}s; clock: {}", t0.elapsed().as_secs_f64(), if audio { "audio" } else { "golden (Node's)" });

    let (mut rust_total, mut node_total) = (Score::default(), Score::default());
    let mut parity_totals = (0usize, 0usize, 0usize);
    println!("{:<34} {:>13} {:>9} {:>9} {:>9}   {:>13} {:>9} {:>9} {:>9}", "", "Rust found", "wrong", "offered", "sugg/h", "Node found", "wrong", "offered", "sugg/h");
    for name in chosen {
        let golden_path = root().join("parity/golden").join(format!("{name}.json"));
        let Ok(raw) = std::fs::read_to_string(&golden_path) else {
            eprintln!("[replay] no recording for {name}");
            continue;
        };
        let golden: Value = serde_json::from_str(&raw).unwrap();
        let (decisions, secs) = replay(&k, &golden, audio);
        let fixture_path = root().join("server/eval/fixtures").join(format!("{name}.json"));
        let fixture: Option<Value> = std::fs::read_to_string(&fixture_path).ok().and_then(|s| serde_json::from_str(&s).ok());
        let cue_at = cue_ms(&golden);
        let last_ms = *cue_at.last().unwrap_or(&0);

        // Node's run, as recorded (stable + run 0's unstable decisions).
        let node: Vec<Decision> = golden["stable"]
            .as_array()
            .unwrap()
            .iter()
            .chain(golden["unstable"].as_array().unwrap().iter().filter(|o| o["run"].as_u64() == Some(0)))
            .filter_map(|o| {
                let cue = o["cue"].as_u64()? as usize;
                let verses: Vec<VerseId> = o["refs"].as_array()?.iter().filter_map(|r| parse_ref(&k, r.as_str()?)).collect();
                if verses.is_empty() {
                    return None;
                }
                let target = if o["target"] == "viewer" { Target::Screen } else { Target::Suggestions };
                Some(Decision { cue, at_ms: cue_at[cue.min(cue_at.len() - 1)], target, method: o["method"].as_str()?.to_string(), verses, corrected: o.get("correctedFrom").is_some() })
            })
            .collect();

        match &fixture {
            Some(f) => {
                let gt = ground_truth(&k, f);
                let (r, n) = (score(&k, &gt, &decisions, last_ms), score(&k, &gt, &node, last_ms));
                println!(
                    "{:<34} {:>6}/{:<6} {:>9} {:>9} {:>9.0}   {:>6}/{:<6} {:>9} {:>9} {:>9.0}   ({:.2}s)",
                    name, r.found, r.total, format!("{}/{}", r.wrong, r.sent), r.offered, r.suggestions as f64 / r.hours, n.found, n.total, format!("{}/{}", n.wrong, n.sent), n.offered, n.suggestions as f64 / n.hours, secs
                );
                if verbose {
                    for w in &r.wrong_list {
                        println!("    wrong   {w}");
                    }
                    for m in &r.missed_list {
                        println!("    missed  {m}");
                    }
                }
                for (t, s) in [(&mut rust_total, r), (&mut node_total, n)] {
                    t.found += s.found;
                    t.total += s.total;
                    t.sent += s.sent;
                    t.wrong += s.wrong;
                    t.offered += s.offered;
                    t.suggestions += s.suggestions;
                    t.hours += s.hours;
                }
            }
            None => {
                let on = decisions.iter().filter(|d| d.target == Target::Screen).count();
                let node_on = node.iter().filter(|d| d.target == Target::Screen).count();
                println!("{name:<34} {on:>6} sent (no ground truth)              {node_on:>6} sent   ({secs:.2}s)");
            }
        }

        if parity {
            let to_key = |d: &Decision| decision_key(&k, d.cue, if d.target == Target::Screen { "viewer" } else { "suggestions" }, &d.method, &d.verses.iter().map(|v| label(&k, *v)).collect::<Vec<_>>());
            let mut rust: HashMap<String, usize> = HashMap::new();
            for d in &decisions {
                *rust.entry(to_key(d)).or_default() += 1;
            }
            let stable: Vec<Decision> = golden["stable"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|o| {
                    let verses: Vec<VerseId> = o["refs"].as_array()?.iter().filter_map(|r| parse_ref(&k, r.as_str()?)).collect();
                    let target = if o["target"] == "viewer" { Target::Screen } else { Target::Suggestions };
                    Some(Decision { cue: o["cue"].as_u64()? as usize, at_ms: 0, target, method: o["method"].as_str()?.to_string(), verses, corrected: false })
                })
                .collect();
            let (mut same, mut missing): (usize, Vec<String>) = (0, Vec::new());
            for d in &stable {
                let key = to_key(d);
                match rust.get_mut(&key) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        same += 1;
                    }
                    _ => missing.push(key),
                }
            }
            let by_cue = |k: &String| k.split('|').next().and_then(|c| c.parse::<usize>().ok());
            let mut extra: Vec<String> = rust.into_iter().flat_map(|(k, n)| std::iter::repeat_n(k, n)).collect();
            extra.sort_by_key(by_cue);
            missing.sort_by_key(by_cue);
            println!("    parity: {same}/{} of Node's stable decisions reproduced; {} missing, {} extra", stable.len(), missing.len(), extra.len());
            if verbose {
                for m in &missing {
                    println!("      - {m}");
                }
                for e in &extra {
                    println!("      + {e}");
                }
            }
            parity_totals.0 += same;
            parity_totals.1 += stable.len();
            parity_totals.2 += extra.len();
        }
    }
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 };
    println!();
    for (who, s) in [("Rust", &rust_total), ("Node", &node_total)] {
        println!(
            "{who}: found {}/{} ({:.1}%), precision {:.1}% ({} wrong of {} sent), missed but offered {}/{}, {:.0} suggestions/hour",
            s.found,
            s.total,
            pct(s.found, s.total),
            100.0 - pct(s.wrong, s.sent),
            s.wrong,
            s.sent,
            s.offered,
            s.total - s.found,
            s.suggestions as f64 / s.hours.max(1e-9)
        );
    }
    if parity {
        println!("Parity: {}/{} of Node's stable decisions reproduced ({:.1}%), {} extra", parity_totals.0, parity_totals.1, pct(parity_totals.0, parity_totals.1), parity_totals.2);
    }
    let _ = Path::new(".");
}
