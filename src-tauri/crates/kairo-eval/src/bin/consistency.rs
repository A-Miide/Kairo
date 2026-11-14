//! The same words must give the same verses, however the recogniser splits them and however fast
//! the computer is. Replays every recorded sermon as heard, then again with the in-progress text
//! split differently (steps skipped, as a busy or slower recogniser does), the inputs a little
//! early or late, and the meaning models' answers arriving a few steps late. Any screen send that
//! differs from the first replay is printed.
//!
//!   cargo run --release -p kairo-eval --bin consistency -- [--variants 20] [--meaning bge-small.json] [name ...]
//!
//! Exits non-zero when any variant differs.

use std::collections::VecDeque;
use std::sync::Arc;

use kairo_detect::engine::{Config, Effect, Engine, Event, MeaningThresholds, Request, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_eval::{answer, knowledge, meaning, root};
use kairo_semantic::MeaningService;
use serde_json::Value;

const MS_PER_WORD: u64 = 380;

/// (time, text, final, utterance end, continues live)
type Input = (u64, String, bool, bool, bool);

/// The recording's inputs on the audio clock (as `replay` does).
fn timeline(golden: &Value) -> Vec<Input> {
    let cues = golden["cues"].as_array().unwrap();
    let mut spans: Vec<(u64, u64)> = Vec::with_capacity(cues.len());
    let mut synth = 0u64;
    for (i, c) in cues.iter().enumerate() {
        let words = c["text"].as_str().unwrap_or("").split_whitespace().count() as u64;
        let start = c["audioMs"].as_u64().unwrap_or(synth);
        let end = cues.get(i + 1).and_then(|n| n["audioMs"].as_u64()).unwrap_or(start + words.max(1) * MS_PER_WORD);
        spans.push((start, end.max(start + 1)));
        synth = start + words.max(1) * MS_PER_WORD;
    }
    golden["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            let cue = x["cue"].as_u64().unwrap() as usize;
            let text = x["text"].as_str().unwrap().to_string();
            let fin = x["isFinal"].as_bool().unwrap();
            let t = x["audioMs"].as_u64().unwrap_or_else(|| {
                let total = cues[cue]["text"].as_str().unwrap_or("").split_whitespace().count().max(1) as f64;
                let said = text.split_whitespace().count() as f64;
                let (a, b) = spans[cue];
                a + ((b - a) as f64 * 0.95 * (said / total)) as u64
            });
            (t, text, fin, x["speechFinal"].as_bool().unwrap_or(fin), x["continues"].as_bool().unwrap_or(false))
        })
        .collect()
}

/// A small, seeded random source (the variants must be the same on every run and platform).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// How a variant differs from the recording.
#[derive(Clone, Copy)]
struct Shake {
    /// Percent of in-progress steps the recogniser skips.
    skip: u64,
    /// Up to this many ms early or late (keeping order).
    jitter: u64,
    /// Meaning answers arrive up to this many inputs late.
    answer_lag: u64,
}

/// The screen sends, in order, for one way of hearing the sermon.
fn run(k: &Arc<Knowledge>, golden: &Value, inputs: &[Input], m: Option<&MeaningService>, thresholds: MeaningThresholds, shake: Option<(Shake, u64)>) -> Vec<String> {
    let lang = golden["lang"].as_str().map(kairo_detect::reference::Lang::from_code).unwrap_or_default();
    let mut engine = Engine::new(k.clone(), Config { meaning: m.is_some(), meaning_thresholds: thresholds, offline: golden["offline"].as_bool().unwrap_or(false), lang, ..Config::default() });
    let mut rng = Rng(shake.map_or(1, |s| s.1));
    let mut sends = Vec::new();
    let mut wakes: Vec<(u64, Event)> = Vec::new();
    // Requests waiting for their answer: (inputs still to go before it may land, request, the input it was asked at).
    let mut pending: VecDeque<(u64, Request, u64)> = VecDeque::new();
    let input = std::cell::Cell::new(0u64);
    let mut last_t = 0u64;
    let label = |v| {
        let r = k.verse_ref(v);
        format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
    };
    let take = |o: kairo_detect::engine::Output, wakes: &mut Vec<(u64, Event)>, pending: &mut VecDeque<(u64, Request, u64)>, rng: &mut Rng, sends: &mut Vec<String>| {
        for e in o.effects {
            match e {
                Effect::Show { verses, target: Target::Screen, .. } if !verses.is_empty() => sends.push(label(verses[0])),
                Effect::WakeAt { id, at_ms } => wakes.push((at_ms, Event::Timer(id))),
                _ => {}
            }
        }
        if m.is_some() {
            for r in o.requests {
                let lag = shake.map_or(0, |s| rng.below(s.0.answer_lag + 1));
                pending.push_back((lag, r, input.get()));
            }
        }
    };
    for (i, (t, text, fin, speech_final, continues)) in inputs.iter().enumerate() {
        input.set(i as u64);
        let (shake_now, _) = match shake {
            Some((s, _)) => (Some(s), ()),
            None => (None, ()),
        };
        // The recogniser skipped this in-progress step (never the last one before a final).
        if let Some(s) = shake_now {
            let before_final = inputs.get(i + 1).is_some_and(|n| n.2);
            if !fin && !before_final && rng.below(100) < s.skip {
                continue;
            }
        }
        let jitter = shake_now.map_or(0, |s| rng.below(2 * s.jitter + 1)) as i64 - shake_now.map_or(0, |s| s.jitter) as i64;
        let now = ((*t as i64 + jitter).max(last_t as i64)) as u64;
        // Answers due land before a finished stretch (never before words still being said), in the
        // order they were asked, at the time of the input before (detect.rs settle); one landing may
        // ask again.
        // …or once owed more than OWED_INPUTS inputs.
        if *fin || pending.iter().any(|(_, _, at)| (i as u64).saturating_sub(*at) > kairo_server::detect::OWED_INPUTS) {
            while let Some(m) = m {
                let Some(pos) = pending.iter().position(|(lag, _, _)| *lag == 0) else { break };
                let (_, r, _) = pending.remove(pos).unwrap();
                let o = engine.handle(Event::Answer(answer(m, &r)), last_t);
                take(o, &mut wakes, &mut pending, &mut rng, &mut sends);
            }
        }
        last_t = now;
        wakes.sort_by_key(|w| w.0);
        while wakes.first().is_some_and(|w| w.0 <= now) {
            let (at, ev) = wakes.remove(0);
            let o = engine.handle(ev, at);
            take(o, &mut wakes, &mut pending, &mut rng, &mut sends);
        }
        let o = engine.handle(Event::Speech(Segment { text: text.clone(), is_final: *fin, speech_final: *speech_final, continues_live: *continues }), now);
        take(o, &mut wakes, &mut pending, &mut rng, &mut sends);
        pending.iter_mut().for_each(|(lag, _, _)| *lag = lag.saturating_sub(1));
    }
    sends
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let variants: u64 = opt("--variants").and_then(|s| s.parse().ok()).unwrap_or(20);
    const TAKES_VALUE: &[&str] = &["--variants", "--meaning"];
    let names: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && TAKES_VALUE.contains(&args[i - 1].as_str()))).map(|(_, a)| a.clone()).collect();
    let dir = root().join("parity/golden");
    let mut all: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()?.path().file_stem()?.to_str().map(str::to_string)).collect();
    all.sort();
    let chosen: Vec<String> = if names.is_empty() { all } else { names };

    let k = Arc::new(knowledge());
    let m = opt("--meaning").map(|e| meaning(&e, Some("minilm.json"), &["KJV"]));
    let thresholds = opt("--meaning").and_then(|e| std::fs::read(root().join("src-tauri/crates/kairo-semantic/bench").join(e)).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["name"].as_str().map(MeaningThresholds::for_embedder)).unwrap_or_default();

    let shakes = [
        ("steps skipped", Shake { skip: 40, jitter: 0, answer_lag: 0 }),
        ("early or late", Shake { skip: 0, jitter: 400, answer_lag: 0 }),
        ("answers late", Shake { skip: 0, jitter: 0, answer_lag: 3 }),
        ("all at once", Shake { skip: 40, jitter: 400, answer_lag: 3 }),
    ];
    let (mut differ, mut tried, mut sends_total) = (0u64, 0u64, 0usize);
    let mut by_kind = vec![0u64; shakes.len()];
    for name in &chosen {
        let Ok(raw) = std::fs::read_to_string(dir.join(format!("{name}.json"))) else { continue };
        let golden: Value = serde_json::from_str(&raw).unwrap();
        let inputs = timeline(&golden);
        let base = run(&k, &golden, &inputs, m.as_ref(), thresholds, None);
        sends_total += base.len();
        let mut bad = 0;
        for (si, (kind, s)) in shakes.iter().enumerate() {
            if s.answer_lag > 0 && m.is_none() {
                continue;
            }
            for seed in 1..=variants {
                tried += 1;
                let got = run(&k, &golden, &inputs, m.as_ref(), thresholds, Some((*s, seed * 7919 + si as u64)));
                if got != base {
                    differ += 1;
                    by_kind[si] += 1;
                    bad += 1;
                    if bad <= 2 {
                        let extra: Vec<&String> = got.iter().filter(|v| !base.contains(v)).collect();
                        let missing: Vec<&String> = base.iter().filter(|v| !got.contains(v)).collect();
                        println!("  {name} [{kind} #{seed}]: {} sends vs {}; new {extra:?}; missing {missing:?}", got.len(), base.len());
                    }
                }
            }
        }
        println!("{name:<40} {} sends, {bad} variants differ", base.len());
    }
    println!();
    for (i, (kind, s)) in shakes.iter().enumerate() {
        if s.answer_lag > 0 && m.is_none() {
            continue;
        }
        println!("{kind:<14} {} differ", by_kind[i]);
    }
    println!("{differ} of {tried} variants differ from the recording ({sends_total} screen sends in {} sermons)", chosen.len());
    if differ > 0 {
        std::process::exit(1);
    }
}
