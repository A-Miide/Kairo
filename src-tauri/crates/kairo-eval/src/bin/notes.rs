//! Sermon notes for the dev sermons, built as the live app builds them: the
//! engine replays each sermon, and what it put on screen (and how) feeds the
//! notes. Writes a PDF per sermon and prints the notes as text.
//!
//!   cargo run --release -p kairo-eval --bin notes -- <outdir> [--meaning gemma-legacy.json] [name ...]

use std::sync::Arc;

use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_eval::{knowledge, meaning, root};
use kairo_server::notes::{self, How, Line, Session, Shown};
use serde_json::Value;

const DEV: &[&str] = &["enough-is-enough", "oyedepo-kingdomlaw", "shiloh2025-impartation"];
const T0: u64 = 1_791_900_000_000;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::path::PathBuf::from(&args[0]);
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let names: Vec<&str> = args[1..].iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && args[*i].starts_with("--"))).map(|(_, a)| a.as_str()).filter(|a| !a.ends_with(".json")).collect();
    let names = if names.is_empty() { DEV.to_vec() } else { names };
    let k = Arc::new(knowledge());
    let m = opt("--meaning").map(|e| meaning(&e, None, &["KJV"]));
    std::fs::create_dir_all(&out).unwrap();
    for name in names {
        let f: Value = serde_json::from_str(&std::fs::read_to_string(root().join("eval/fixtures").join(format!("{name}.json"))).unwrap()).unwrap();
        let cues = f["transcript"].as_array().unwrap();
        let mut engine = Engine::new(k.clone(), Config::default());
        let mut session = Session::default();
        for (i, c) in cues.iter().enumerate() {
            let text = c["text"].as_str().unwrap_or("").to_string();
            let at = c["startMs"].as_u64().unwrap_or(0);
            let next = cues.get(i + 1).and_then(|n| n["startMs"].as_u64()).unwrap_or(at + 5_000);
            let o = engine.handle(Event::Speech(Segment { text: text.clone(), is_final: true, speech_final: true, continues_live: false }), next.saturating_sub(100));
            for e in o.effects {
                match e {
                    Effect::Show { verses, method, target: Target::Screen, .. } => {
                        for v in verses {
                            session.shown.push(Shown { verse: v, at: T0 + at, how: How::of(method.wire()) });
                        }
                    }
                    Effect::RangeSet { verses, .. } => session.ranges.push((verses, T0 + at)),
                    _ => {}
                }
            }
            session.lines.push(Line { text, at: T0 + at });
        }
        let t = std::time::Instant::now();
        let n = notes::build(&k, &session, m.as_ref().map(|m| m as &dyn notes::Meaning));
        let pdf = notes::render_pdf(&k, &n, "Sunday, October 11, 2026");
        std::fs::write(out.join(format!("{name}.pdf")), pdf).unwrap();
        println!("=== {name} ({:.1}s) — {} | {:?} min | {} passages", t.elapsed().as_secs_f32(), n.title, n.minutes, n.passage_count);
        println!("  themes: {}  |  people: {}", n.themes.join(", "), n.people.join(", "));
        for p in &n.main_text {
            println!("  TEXT {}", notes::reference(&k, p));
        }
        for s in &n.sections {
            println!("  [{:?}] {}", s.kind, s.heading.chars().take(140).collect::<String>());
            for q in &s.quotes {
                println!("      \"{}\"", q.text.chars().take(150).collect::<String>());
            }
            for how in [How::Read, How::Cited, How::Alluded] {
                let of: Vec<&notes::Passage> = s.scriptures.iter().filter(|p| p.how == how).collect();
                let r = notes::compact(&k, &of);
                if !r.is_empty() {
                    println!("      {how:?}: {}", r.join(", "));
                }
            }
        }
        for p in &n.prayer_points {
            println!("  PRAY {}", p.text);
        }
    }
}
