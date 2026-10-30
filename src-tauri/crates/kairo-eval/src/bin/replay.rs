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
//! `--offline` replays the offline engine's transcriptions instead
//! (`parity/offline/*.json`, made by `offline-transcribe`), on their own clock.
//!
//! Scoring follows `eval/run_eval.js`: a ground-truth verse counts
//! when it reaches the screen within 30 s of where it was said; a verse sent
//! that the sermon never cites is a wrong send (verses of a chapter cited
//! with no verse are exempt for 10 minutes); a miss offered in Possible
//! Matches is counted separately; suggestions per hour measure busyness.
//! A verse the preacher only paraphrased (`kind: "paraphrase"`) makes an
//! offer right but isn't expected on screen: paraphrases are only offered.
//!
//! Only the dev sermons are replayed unless `--held-out` is given
//! (docs/RUST_REWRITE_PLAN.md, "Evaluation split").
//!
//! Meaning-based detection runs when `--meaning <embedder.json>` names an
//! embedder config (in `kairo-semantic/bench`); `--reranker <config>`,
//! `--index KJV,NLT` and `--thresholds <file.json>` choose the rest.
//!
//!   cargo run --release -p kairo-eval --bin replay -- [--clock golden] [--parity] [--held-out] [--verbose] [--trace a..b] [--meaning bge-small.json] [name ...]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use kairo_bible::{Book, VerseId, VerseRef};
use kairo_detect::engine::{Config, Effect, Engine, Event, MeaningThresholds, Output, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_eval::{answer, knowledge, meaning, root};
use kairo_semantic::MeaningService;
use serde_json::{json, Value};

const DEV: &[&str] = &["enough-is-enough", "oyedepo-kingdomlaw", "shiloh2025-impartation", "2026-09-26-identity-in-christ-1", "2026-09-26-identity-in-christ-2", "2026-09-26-identity-in-christ-3"];
const HELD_OUT: &[&str] = &["oyedepo-meditation", "shiloh2025-love-mystery"];
const MATCH_WINDOW_MS: i64 = 30_000;
/// A suggestion is right when the sermon cites or reads that verse within this of it.
const SUGGESTION_RIGHT_MS: i64 = 60_000;
/// …or is a verse of a cited or read passage: same chapter, this close, this soon.
const PASSAGE_MS: i64 = 180_000;
const PASSAGE_VERSES: u16 = 5;
const BARE_CHAPTER_EXEMPT_MS: i64 = 10 * 60 * 1000;
/// Possible Matches hides anything scored under this (app.js SUGGESTION_DISPLAY_MIN,
/// PARAPHRASE_DISPLAY_MIN), so the operator only sees offers at or above it.
const DISPLAY_MIN: f32 = 0.80;
const PARAPHRASE_DISPLAY_MIN: f32 = 0.695;
/// Speaking pace assumed where a recording has no audio times (live runs).
const MS_PER_WORD: u64 = 400;

struct Decision {
    cue: usize,
    at_ms: i64,
    target: Target,
    method: String,
    verses: Vec<VerseId>,
    corrected: bool,
    score: f32,
}

struct GroundTruth {
    keys: Vec<VerseId>,
    bare: Option<(Book, u16)>,
    at_ms: i64,
    /// Said in the preacher's own words: right as an offer, not counted as found or missed.
    paraphrase: bool,
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
    /// Method → (found, wrong).
    by_method: std::collections::BTreeMap<String, (usize, usize)>,
    /// Method → (suggestions offered, of them the verse being cited or read then).
    offered_by: std::collections::BTreeMap<String, (usize, usize)>,
    /// Suggestions that are the verse being cited or read around then.
    suggestions_right: usize,
    /// …or a verse of the passage being cited or read then.
    suggestions_in_passage: usize,
    /// Of the suggestions, those Possible Matches shows; of them, right; of them, noise.
    shown: (usize, usize, usize),
    noise_list: Vec<String>,
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
            // The fixtures spell them verseStart/verseEnd (start/end accepted too).
            let n = |x: &Value, a: &str, b: &str| x[a].as_u64().or_else(|| x[b].as_u64());
            r.iter().flat_map(|x| (n(x, "verseStart", "start").unwrap_or(1)..=n(x, "verseEnd", "end").unwrap_or(0)).filter_map(id)).collect()
        } else {
            Vec::new()
        };
        let bare = keys.is_empty().then_some((book, ch));
        out.push(GroundTruth { keys, bare, at_ms, paraphrase: g["kind"] == "paraphrase" });
    }
    out
}

fn score(k: &Knowledge, gt: &[GroundTruth], decisions: &[Decision], last_ms: i64) -> Score {
    let screen: Vec<&Decision> = decisions.iter().filter(|d| d.target == Target::Screen).collect();
    let mut s = Score { hours: (last_ms.max(1) as f64 / 3_600_000.0).max(1.0), ..Default::default() };
    let mut used: HashSet<usize> = HashSet::new();
    let mut missed: Vec<&GroundTruth> = Vec::new();
    for g in gt.iter().filter(|g| !g.keys.is_empty() && !g.paraphrase) {
        s.total += 1;
        let hit = screen.iter().enumerate().find(|(i, d)| !used.contains(i) && g.keys.contains(&d.verses[0]) && (d.at_ms - g.at_ms).abs() <= MATCH_WINDOW_MS);
        // …or a later verse of a short range sent whole, still up when it's read: the engine keeps
        // it up together (SHOWN_TOGETHER_MAX) instead of sending that verse on its own.
        let together = || {
            screen.iter().enumerate().find(|(i, d)| {
                (2..=kairo_detect::engine::SHOWN_TOGETHER_MAX).contains(&d.verses.len())
                    && d.verses[1..].iter().any(|v| g.keys.contains(v))
                    && d.at_ms <= g.at_ms + MATCH_WINDOW_MS
                    && screen.get(i + 1).is_none_or(|next| next.at_ms >= g.at_ms - MATCH_WINDOW_MS)
            })
        };
        match hit.or_else(together) {
            Some((i, d)) => {
                used.insert(i);
                s.found += 1;
                s.by_method.entry(d.method.clone()).or_default().0 += 1;
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
            s.by_method.entry(d.method.clone()).or_default().1 += 1;
            s.wrong_list.push(format!("{:>6.1}m {} ({}{})", d.at_ms as f64 / 60_000.0, label(k, d.verses[0]), d.method, if d.corrected { ", corrected" } else { "" }));
        }
    }
    let suggestions: Vec<&Decision> = decisions.iter().filter(|d| d.target == Target::Suggestions).collect();
    s.suggestions = suggestions.len();
    for d in &suggestions {
        let right = gt.iter().any(|g| g.keys.contains(&d.verses[0]) && (d.at_ms - g.at_ms).abs() <= SUGGESTION_RIGHT_MS) || in_bare(d);
        let r = k.verse_ref(d.verses[0]);
        let in_passage = right
            || gt.iter().any(|g| {
                (d.at_ms - g.at_ms).abs() <= PASSAGE_MS
                    && g.keys.iter().any(|x| {
                        let x = k.verse_ref(*x);
                        x.book == r.book && x.chapter == r.chapter && x.verse.abs_diff(r.verse) <= PASSAGE_VERSES
                    })
            });
        if in_passage {
            s.suggestions_in_passage += 1;
        }
        let e = s.offered_by.entry(d.method.clone()).or_default();
        e.0 += 1;
        if std::env::var("OFFERS").is_ok_and(|m| m == d.method) {
            println!("    offer {} {:.3} {:>6.1}m {}", if right { "RIGHT" } else { "noise" }, d.score, d.at_ms as f64 / 60_000.0, label(k, d.verses[0]));
        }
        let shown = d.score >= if d.method == "paraphrase" { PARAPHRASE_DISPLAY_MIN } else { DISPLAY_MIN };
        if shown {
            s.shown.0 += 1;
        }
        if right {
            e.1 += 1;
            s.suggestions_right += 1;
            s.shown.1 += shown as usize;
        } else if !in_passage {
            s.shown.2 += shown as usize;
            s.noise_list.push(format!("{:>6.1}m {} ({}, {:.2}{})", d.at_ms as f64 / 60_000.0, label(k, d.verses[0]), d.method, d.score, if shown { "" } else { ", hidden" }));
        }
    }
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
/// One input: when, which cue, the text, final, utterance end, continues live.
type Input = (u64, usize, String, bool, bool, bool);

fn timeline(golden: &Value, audio: bool) -> Vec<Input> {
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
            let speech_final = x["speechFinal"].as_bool().unwrap_or(fin);
            let continues = x["continues"].as_bool().unwrap_or(false);
            // Recorded on the audio clock already (an offline transcription).
            let t = if let Some(t) = x["audioMs"].as_u64() {
                t
            } else if audio {
                let total = cues[cue]["text"].as_str().unwrap_or("").split_whitespace().count().max(1) as f64;
                let said = text.split_whitespace().count() as f64;
                let (a, b) = spans[cue];
                a + ((b - a) as f64 * 0.95 * (said / total)) as u64
            } else {
                x["t"].as_u64().unwrap()
            };
            (t, cue, text, fin, speech_final, continues)
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

/// Cues to print every input and effect for (`--trace 60..75`).
fn trace_range() -> Option<(usize, usize)> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--trace")?;
    let (a, b) = args.get(i + 1)?.split_once("..")?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn replay(k: &Arc<Knowledge>, golden: &Value, audio: bool, m: Option<&MeaningService>, thresholds: MeaningThresholds) -> (Vec<Decision>, f64) {
    let trace = trace_range();
    // A recording can say which language the service was in ("lang": "es"), as Settings does.
    let lang = golden["lang"].as_str().map(kairo_detect::reference::Lang::from_code).unwrap_or_default();
    let mut engine = Engine::new(k.clone(), Config { explain: trace.is_some(), meaning: m.is_some(), meaning_thresholds: thresholds, offline: golden["offline"].as_bool().unwrap_or(false), lang, ..Config::default() });
    // A scenario can start with a song slide live ("songLive"): the operator sent lyrics, not scripture.
    if golden["songLive"].as_bool().unwrap_or(false) {
        engine.handle(Event::NonScriptureLive, 0);
    }
    let cue_at = cue_ms(golden);
    let mut out = Vec::new();
    let mut wakes: Vec<(u64, Event)> = Vec::new();
    let started = Instant::now();
    let collect = |o: Output, cue: usize, wakes: &mut Vec<(u64, Event)>, out: &mut Vec<Decision>| {
        for e in o.effects {
            match e {
                Effect::Show { verses, method, target, corrected_from, score } => {
                    out.push(Decision { cue, at_ms: cue_at[cue.min(cue_at.len() - 1)], target, method: method.wire().to_string(), verses, corrected: corrected_from.is_some(), score })
                }
                Effect::WakeAt { id, at_ms } => wakes.push((at_ms, Event::Timer(id))),
                _ => {}
            }
        }
    };
    // `MUSIC_DIR=<dir>`: the sound classifier's frames for this recording (<dir>/<source>.csv,
    // from `music-check`) switch singing on and off as the app would.
    let mut singing: std::collections::VecDeque<(u64, bool)> = std::env::var("MUSIC_DIR")
        .ok()
        .and_then(|dir| std::fs::read_to_string(std::path::Path::new(&dir).join(format!("{}.csv", golden["source"].as_str().unwrap_or("")))).ok())
        .map(|csv| {
            let mut gate = kairo_detect::music::MusicGate::default();
            csv.lines()
                .skip(1)
                .filter_map(|l| {
                    let f: Vec<f32> = l.split(',').filter_map(|x| x.parse().ok()).collect();
                    (f.len() == 4).then(|| gate.push(f[1], f[3]).map(|on| ((f[0] * 1000.0) as u64, on))).flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    for (t, cue, text, fin, speech_final, continues_live) in timeline(golden, audio) {
        while singing.front().is_some_and(|s| s.0 <= t) {
            let (at, on) = singing.pop_front().unwrap();
            if std::env::var("MUSIC_LOG").is_ok() {
                eprintln!("[music] {} singing {}", golden["source"].as_str().unwrap_or(""), if on { format!("on at {:.1}m", at as f64 / 60000.0) } else { format!("off at {:.1}m", at as f64 / 60000.0) });
            }
            engine.handle(Event::Singing(on), at);
        }
        // Timers due before this input fire first.
        wakes.sort_by_key(|w| w.0);
        while wakes.first().is_some_and(|w| w.0 <= t) {
            let (at, ev) = wakes.remove(0);
            let o = engine.handle(ev, at);
            collect(o, cue, &mut wakes, &mut out);
        }
        let traced = trace.is_some_and(|(a, b)| cue >= a && cue <= b);
        if traced {
            println!("  [{cue} t={t}] {} \"{text}\"", if fin { "FINAL" } else { "interim" });
        }
        let o = engine.handle(Event::Speech(Segment { text, is_final: fin, speech_final, continues_live }), t);
        if traced {
            for n in &o.notes {
                println!("      .. {n}");
            }
            for e in &o.effects {
                match e {
                    Effect::Show { verses, method, score, target, corrected_from } => println!(
                        "      -> {target:?} {} {:.3} {}{}",
                        method.wire(),
                        score,
                        verses.iter().map(|v| label(k, *v)).collect::<Vec<_>>().join(", "),
                        corrected_from.map_or(String::new(), |c| format!(" (corrects {})", label(k, c)))
                    ),
                    other => println!("      -> {other:?}"),
                }
            }
        }
        let mut requests = o.requests.clone();
        collect(o, cue, &mut wakes, &mut out);
        // The models answer at once (their latency is the runtime's concern).
        while let (Some(m), Some(r)) = (m, requests.pop()) {
            if let (Ok(path), kairo_detect::engine::Request::Scoped { text, verses, .. }) = (std::env::var("SCOPED_LOG"), &r) {
                use std::io::Write;
                let line = serde_json::json!({ "text": text, "verses": verses.iter().map(|v| v.0).collect::<Vec<_>>() });
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
                writeln!(f, "{line}").unwrap();
            }
            let a = answer(m, &r);
            // `MEANING_LOG=<file>`: every meaning search and what it found, one JSON line each.
            if let (Ok(path), kairo_detect::engine::Request::Paraphrase { windows, .. }, kairo_detect::engine::Answer::Paraphrase { candidates, .. }) = (std::env::var("MEANING_LOG"), &r, &a) {
                use std::io::Write;
                let hits: Vec<Value> = candidates.iter().map(|h| json!({ "verse": label(k, h.verse), "cos": h.cos, "rr": h.rerank, "rrm": h.rerank_modern, "window": h.window })).collect();
                let line = json!({ "sermon": golden["source"].as_str().unwrap_or(""), "t": t, "windows": windows, "hits": hits });
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
                writeln!(f, "{line}").unwrap();
            }
            let o = engine.handle(Event::Answer(a), t);
            if traced {
                for n in &o.notes {
                    println!("      .. {n}");
                }
                for e in &o.effects {
                    println!("      -> {e:?}");
                }
            }
            requests.extend(o.requests.iter().cloned());
            collect(o, cue, &mut wakes, &mut out);
        }
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
    // Transcribed by the offline engine (`offline-transcribe`) instead of Deepgram.
    let offline = args.iter().any(|a| a == "--offline");
    const TAKES_VALUE: &[&str] = &["--clock", "--trace", "--meaning", "--index", "--reranker", "--thresholds"];
    let names: Vec<&str> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && TAKES_VALUE.contains(&args[i - 1].as_str())))
        .map(|(_, a)| a.as_str())
        .collect();
    let chosen: Vec<&str> = if !names.is_empty() {
        names
    } else if held_out {
        DEV.iter().chain(HELD_OUT).copied().collect()
    } else {
        DEV.to_vec()
    };

    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let t0 = Instant::now();
    let k = Arc::new(knowledge());
    let m = opt("--meaning").map(|e| {
        let codes = opt("--index").unwrap_or_else(|| "KJV".into());
        let codes: Vec<&str> = codes.split(',').collect();
        meaning(&e, Some(opt("--reranker").as_deref().unwrap_or("minilm.json")), &codes)
    });
    let thresholds: MeaningThresholds = opt("--thresholds")
        .map(|p| serde_json::from_slice(&std::fs::read(root().join("src-tauri/crates/kairo-semantic/bench").join(p)).expect("thresholds")).expect("thresholds"))
        // Otherwise the ones the app uses with this embedder.
        .unwrap_or_else(|| {
            let name = opt("--meaning").and_then(|e| std::fs::read(root().join("src-tauri/crates/kairo-semantic/bench").join(e)).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["name"].as_str().map(str::to_string));
            name.map_or_else(MeaningThresholds::default, |n| MeaningThresholds::for_embedder(&n))
        });
    eprintln!("[replay] knowledge built in {:.1}s; clock: {}", t0.elapsed().as_secs_f64(), if audio { "audio" } else { "golden (Node's)" });

    let (mut rust_total, mut node_total) = (Score::default(), Score::default());
    let mut parity_totals = (0usize, 0usize, 0usize);
    println!("{:<34} {:>13} {:>9} {:>9} {:>9}   {:>13} {:>9} {:>9} {:>9}", "", "Rust found", "wrong", "offered", "sugg/h", "Node found", "wrong", "offered", "sugg/h");
    for name in chosen {
        let golden_path = root().join(if offline { "parity/offline" } else { "parity/golden" }).join(format!("{name}.json"));
        let Ok(raw) = std::fs::read_to_string(&golden_path) else {
            eprintln!("[replay] no recording for {name}");
            continue;
        };
        let golden: Value = serde_json::from_str(&raw).unwrap();
        let (decisions, secs) = replay(&k, &golden, audio, m.as_ref(), thresholds);
        // "enough-is-enough.beam": a variant transcription of the same sermon.
        let sermon = name.split('.').next().unwrap_or(name);
        let fixture_path = root().join("eval/fixtures").join(format!("{sermon}.json"));
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
                Some(Decision { cue, at_ms: cue_at[cue.min(cue_at.len() - 1)], target, method: o["method"].as_str()?.to_string(), verses, corrected: o.get("correctedFrom").is_some(), score: 0.0 })
            })
            .collect();

        // SENDS_JSON=<dir>: what reached the screen, when — the slide-follow
        // eval (scripts/slide-follow-eval.js) feeds these to the follower.
        if let Ok(dir) = std::env::var("SENDS_JSON") {
            let sends: Vec<Value> = decisions
                .iter()
                .filter(|d| d.target == Target::Screen)
                .map(|d| json!({ "atMs": d.at_ms, "verses": d.verses.iter().map(|v| label(&k, *v)).collect::<Vec<_>>() }))
                .collect();
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.json")), serde_json::to_string(&sends).unwrap_or_default());
        }

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
                    if args.iter().any(|a| a == "--noise") {
                        for n in &r.noise_list {
                            println!("    noise   {n}");
                        }
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
                    t.suggestions_right += s.suggestions_right;
                    t.suggestions_in_passage += s.suggestions_in_passage;
                    t.shown.0 += s.shown.0;
                    t.shown.1 += s.shown.1;
                    t.shown.2 += s.shown.2;
                    for (m, (n, r)) in s.offered_by {
                        let e = t.offered_by.entry(m).or_default();
                        e.0 += n;
                        e.1 += r;
                    }
                    for (m, (f, w)) in s.by_method {
                        let e = t.by_method.entry(m).or_default();
                        e.0 += f;
                        e.1 += w;
                    }
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
                    Some(Decision { cue: o["cue"].as_u64()? as usize, at_ms: 0, target, method: o["method"].as_str()?.to_string(), verses, corrected: false, score: 0.0 })
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
    for (who, s) in [("Rust", &rust_total), ("Node", &node_total)] {
        println!("{who} by method (found/wrong): {}", s.by_method.iter().map(|(m, (f, w))| format!("{m} {f}/{w}")).collect::<Vec<_>>().join(", "));
        println!(
            "{who} Possible Matches: {:.0}/hour, {:.0}% the verse cited, {:.0}% in the passage, {:.1} noise/hour — by method (offered/right): {}",
            s.suggestions as f64 / s.hours,
            pct(s.suggestions_right, s.suggestions),
            pct(s.suggestions_in_passage, s.suggestions),
            (s.suggestions - s.suggestions_in_passage) as f64 / s.hours,
            s.offered_by.iter().map(|(m, (n, r))| format!("{m} {n}/{r}")).collect::<Vec<_>>().join(", ")
        );
        println!(
            "{who} Possible Matches shown (score >= {DISPLAY_MIN:.2}, paraphrase {PARAPHRASE_DISPLAY_MIN:.2}): {} offered, {} the verse cited, {} noise ({:.1}/hour)",
            s.shown.0,
            s.shown.1,
            s.shown.2,
            s.shown.2 as f64 / s.hours.max(1e-9)
        );
    }
    if parity {
        println!("Parity: {}/{} of Node's stable decisions reproduced ({:.1}%), {} extra", parity_totals.0, parity_totals.1, pct(parity_totals.0, parity_totals.1), parity_totals.2);
    }
}
