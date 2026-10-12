//! Speak a plain-text transcript into the engine and print every decision —
//! for checking what a service would have shown from what was said.
//!
//! `cargo run --release -p kairo-eval --bin say -- transcript.txt [--meaning gemma-legacy.json] [--explain]`
//!
//! Words arrive at speaking pace; an utterance ends at a sentence end (or
//! after 18 words), the newest words arriving as interim results first.

use std::sync::Arc;

use kairo_bible::VerseId;
use kairo_detect::engine::{Config, Effect, Engine, Event, Output, Request, Segment};
use kairo_detect::knowledge::Knowledge;
use kairo_eval::{answer, knowledge, meaning};
use kairo_semantic::MeaningService;

const MS_PER_WORD: u64 = 380;
const INTERIM_EVERY: usize = 3;
const MAX_UTTERANCE_WORDS: usize = 18;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let explain = args.iter().any(|a| a == "--explain");
    let path = args.first().expect("a transcript file");
    let text = std::fs::read_to_string(path).expect("transcript");
    let k = Arc::new(knowledge());
    let m = opt("--meaning").map(|e| meaning(&e, Some("minilm.json"), &["KJV"]));
    let mut engine = Engine::new(k.clone(), Config { explain, meaning: m.is_some(), ..Config::default() });

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut utterances: Vec<Vec<&str>> = vec![Vec::new()];
    for w in &words {
        let u = utterances.last_mut().unwrap();
        u.push(w);
        if w.ends_with(['.', '?', '!']) || u.len() >= MAX_UTTERANCE_WORDS {
            utterances.push(Vec::new());
        }
    }

    let mut t = 0u64;
    let mut wakes: Vec<(u64, Event)> = Vec::new();
    for u in utterances.iter().filter(|u| !u.is_empty()) {
        for n in 1..=u.len() {
            t += MS_PER_WORD;
            wakes.sort_by_key(|w| w.0);
            while wakes.first().is_some_and(|w| w.0 <= t) {
                let (at, ev) = wakes.remove(0);
                let o = engine.handle(ev, at);
                let mut pending = Vec::new();
                report(&k, o, m.is_some(), &mut wakes, &mut pending);
                drain(&mut engine, &k, m.as_ref(), at, &mut wakes, pending);
            }
            let fin = n == u.len();
            if !fin && n % INTERIM_EVERY != 0 {
                continue;
            }
            let said = u[..n].join(" ");
            if fin {
                println!("[{:>5.1}s] {said}", t as f64 / 1000.0);
            }
            let o = engine.handle(Event::Speech(Segment { text: said, is_final: fin, speech_final: fin, continues_live: false }), t);
            let mut pending = Vec::new();
            report(&k, o, m.is_some(), &mut wakes, &mut pending);
            drain(&mut engine, &k, m.as_ref(), t, &mut wakes, pending);
        }
        t += 600;
    }
}

/// The meaning models answer at once.
fn drain(engine: &mut Engine, k: &Knowledge, m: Option<&MeaningService>, t: u64, wakes: &mut Vec<(u64, Event)>, mut pending: Vec<Request>) {
    while let (Some(m), Some(r)) = (m, pending.pop()) {
        let o = engine.handle(Event::Answer(answer(m, &r)), t);
        report(k, o, true, wakes, &mut pending);
    }
}

fn label(k: &Knowledge, v: VerseId) -> String {
    let r = k.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

fn report(k: &Knowledge, o: Output, meaning: bool, wakes: &mut Vec<(u64, Event)>, pending: &mut Vec<Request>) {
    for n in &o.notes {
        println!("          .. {n}");
    }
    for e in o.effects {
        match e {
            Effect::Show { verses, method, score, target, corrected_from } => println!(
                "          -> {target:?} {} {:.2} {}{}",
                method.wire(),
                score,
                verses.iter().map(|v| label(k, *v)).collect::<Vec<_>>().join(", "),
                corrected_from.map_or(String::new(), |c| format!(" (corrects {})", label(k, c)))
            ),
            Effect::WakeAt { id, at_ms } => wakes.push((at_ms, Event::Timer(id))),
            _ => {}
        }
    }
    if meaning {
        pending.extend(o.requests);
    }
}
