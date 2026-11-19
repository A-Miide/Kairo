//! Does what the sermon has been about tell a right meaning offer from noise?
//!
//! Reads `replay` output made with `OFFERS=paraphrase` (its `offer RIGHT|noise`
//! lines), and for each offer embeds the speech of the minutes before it (the
//! sermon's topic, not the words that found the verse) and scores the verse
//! against it: cosine, the verse's rank in the topic's own search, and whether
//! its chapter is among the topic's nearest chapters.
//!
//!   cargo run --release -p kairo-eval --bin offer-topic -- <replay output> [--meaning gemma-legacy.json] [--from-s 180] [--to-s 15]

use std::collections::HashSet;

use kairo_bible::{Book, VerseRef};
use kairo_eval::{meaning, root};
use serde_json::Value;

struct Offer {
    sermon: String,
    at_ms: i64,
    verse: String,
    right: bool,
}

const SEARCH_DEPTH: usize = 400;
const NEAR_CHAPTERS: usize = 10;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let input = args.get(1).filter(|a| !a.starts_with("--")).expect("usage: offer-topic <replay output>");
    let from_ms = arg("--from-s").and_then(|s| s.parse::<i64>().ok()).unwrap_or(180) * 1000;
    let to_ms = arg("--to-s").and_then(|s| s.parse::<i64>().ok()).unwrap_or(15) * 1000;
    let embedder = arg("--meaning").unwrap_or_else(|| "gemma-legacy.json".into());

    let offers = parse_offers(&std::fs::read_to_string(input).expect("replay output"));
    eprintln!("[offer-topic] {} offers ({} right)", offers.len(), offers.iter().filter(|o| o.right).count());
    let m = meaning(&embedder, None, &["KJV"]);
    let corpus = kairo_bible::Corpus::load(&root().join("databases/bibles")).expect("kjv");

    let mut rows: Vec<(bool, f32)> = Vec::new();
    println!("{:<6} {:>7} {:<34} {:<22} {:>6} {:>6} {:>5}", "", "min", "sermon", "verse", "cos", "rank", "chap");
    for o in &offers {
        let golden: Value = serde_json::from_slice(&std::fs::read(root().join(format!("parity/golden/{}.json", o.sermon))).expect("golden")).expect("golden");
        let topic: String = golden["cues"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["audioMs"].as_i64().is_some_and(|t| t >= o.at_ms - from_ms && t <= o.at_ms - to_ms))
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let Some(v) = parse_ref(&o.verse).and_then(|r| corpus.id(r)) else {
            eprintln!("[offer-topic] can't read {}", o.verse);
            continue;
        };
        if topic.split_whitespace().count() < 20 {
            continue;
        }
        let cos = m.compare(&[topic.clone()], &[v]).ok().and_then(|s| s[0].0).unwrap_or(0.0);
        let near = m.search(&topic, SEARCH_DEPTH).unwrap_or_default();
        let rank = near.iter().position(|(x, _)| *x == v).map(|i| i + 1);
        let r = corpus.verse_ref(v);
        let mut chapters: Vec<(Book, u16)> = Vec::new();
        let mut seen = HashSet::new();
        for (x, _) in &near {
            let xr = corpus.verse_ref(*x);
            if seen.insert((xr.book, xr.chapter)) {
                chapters.push((xr.book, xr.chapter));
            }
        }
        let chap = chapters.iter().take(NEAR_CHAPTERS).any(|c| *c == (r.book, r.chapter));
        println!(
            "{:<6} {:>6.1}m {:<34} {:<22} {:>6.3} {:>6} {:>5}",
            if o.right { "RIGHT" } else { "noise" },
            o.at_ms as f64 / 60_000.0,
            o.sermon,
            o.verse,
            cos,
            rank.map_or("-".into(), |r| r.to_string()),
            if chap { "yes" } else { "" }
        );
        rows.push((o.right, cos));
    }
    let auc = auc(&rows);
    println!("\ntopic cosine, right vs noise: AUC {auc:.2} over {} offers ({} right)", rows.len(), rows.iter().filter(|r| r.0).count());
}

/// `offer` lines belong to the sermon whose summary line follows them.
fn parse_offers(text: &str) -> Vec<Offer> {
    let mut out = Vec::new();
    let mut pending: Vec<(i64, String, bool)> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("offer ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            // Node's recorded decisions are scored too, with no score of their own: skip them.
            if parts.len() >= 4 && parts[1] != "0.000" {
                let right = parts[0] == "RIGHT";
                let mins: f64 = parts[2].trim_end_matches('m').parse().unwrap_or(0.0);
                pending.push(((mins * 60_000.0) as i64, parts[3..].join(" "), right));
            }
        } else if !line.starts_with(' ') && !pending.is_empty() {
            if let Some(name) = line.split_whitespace().next() {
                if root().join(format!("parity/golden/{name}.json")).exists() {
                    out.extend(pending.drain(..).map(|(at_ms, verse, right)| Offer { sermon: name.to_string(), at_ms, verse, right }));
                }
            }
        }
    }
    out
}

fn parse_ref(s: &str) -> Option<VerseRef> {
    let (book, cv) = s.rsplit_once(' ')?;
    let (c, v) = cv.split_once(':')?;
    Some(VerseRef::new(Book::from_name(book)?, c.parse().ok()?, v.split(['-', ',']).next()?.parse().ok()?))
}

fn auc(rows: &[(bool, f32)]) -> f64 {
    let pos: Vec<f32> = rows.iter().filter(|r| r.0).map(|r| r.1).collect();
    let neg: Vec<f32> = rows.iter().filter(|r| !r.0).map(|r| r.1).collect();
    if pos.is_empty() || neg.is_empty() {
        return f64::NAN;
    }
    let mut wins = 0.0;
    for p in &pos {
        for n in &neg {
            wins += if p > n { 1.0 } else if p == n { 0.5 } else { 0.0 };
        }
    }
    wins / (pos.len() * neg.len()) as f64
}
