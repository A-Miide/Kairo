//! Follows a slide deck through recorded speech with the detection engine (kairo-detect
//! engine/deck.rs), for `scripts/slide-follow-eval.js --mode engine`.
//!
//! `deck-replay <slides.json> <deliveries.json>`: the slides' words (a JSON array of strings) and
//! the speech as the app receives it (`[{ now, text, isFinal }]`, ms). Prints the moves as JSON:
//! `[{ "to": slide, "at": ms }]`.

use std::sync::Arc;

use kairo_detect::engine::{Config, Effect, Engine, Event, Segment};
use kairo_eval::knowledge;
use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(slides), Some(speech)) = (args.first(), args.get(1)) else {
        eprintln!("usage: deck-replay <slides.json> <deliveries.json>");
        std::process::exit(2);
    };
    let read = |p: &str| -> Value { serde_json::from_slice(&std::fs::read(p).expect("readable file")).expect("JSON") };
    let slides: Vec<String> = read(slides).as_array().expect("slides").iter().map(|s| s.as_str().unwrap_or("").to_string()).collect();
    let speech = read(speech);
    let mut e = Engine::new(Arc::new(knowledge()), Config { explain: true, ..Config::default() });
    e.handle(Event::DeckSet { deck: "eval".into(), slides, current: 0 }, 1);
    let mut moves = Vec::new();
    // DECK_TRACE=1: the engine's notes for every segment on stderr, to see why a slide waited.
    let trace = std::env::var_os("DECK_TRACE").is_some();
    // A segment with no time of its own (some transcripts have them) is heard when the last one was.
    let mut clock = 2u64;
    for d in speech.as_array().expect("deliveries") {
        clock = d["now"].as_f64().map_or(clock, |t| (t as u64).max(clock));
        let now = clock;
        let is_final = d["isFinal"].as_bool().unwrap_or(false);
        let seg = Segment { text: d["text"].as_str().unwrap_or("").to_string(), is_final, speech_final: is_final, continues_live: false };
        let text = seg.text.clone();
        let out = e.handle(Event::Speech(seg), now);
        let why = out.notes.iter().find(|n| n.starts_with("deck moves")).cloned().unwrap_or_default();
        if trace {
            for n in &out.notes {
                eprintln!("{:>6.1}s  {n}", now as f64 / 1000.0);
            }
        }
        for f in out.effects {
            if let Effect::DeckMoved { index, .. } = f {
                moves.push(json!({ "to": index, "at": now, "why": why, "heard": text }));
            }
        }
    }
    println!("{}", Value::Array(moves));
}
