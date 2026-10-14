//! Carries the meaning thresholds from one embedder to another without
//! touching the answer key: both embedders score the same few thousand
//! windows of real sermon speech (dev sermons), and each threshold moves to
//! the score the new embedder gives at the same rank — the same share of
//! speech clears it.
//!
//!   cargo run --release -p kairo-eval --bin calibrate -- gemma-legacy.json gemma.json KJV

use kairo_detect::engine::MeaningThresholds;
use kairo_eval::{meaning, root};

const DEV: &[&str] = &["enough-is-enough", "oyedepo-kingdomlaw", "shiloh2025-impartation"];
const WINDOW_WORDS: usize = 24;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (from, to) = (&args[0], &args[1]);
    let codes: Vec<&str> = args.get(2).map_or("KJV", String::as_str).split(',').collect();
    let mut windows: Vec<String> = Vec::new();
    for name in DEV {
        let f: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("eval/fixtures").join(format!("{name}.json"))).unwrap()).unwrap();
        let mut words: Vec<String> = Vec::new();
        for c in f["transcript"].as_array().unwrap() {
            words.extend(c["text"].as_str().unwrap().split_whitespace().map(str::to_string));
            windows.push(words[words.len().saturating_sub(WINDOW_WORDS)..].join(" "));
        }
    }
    eprintln!("{} windows", windows.len());
    let top1 = |cfg: &str| -> Vec<f32> {
        let m = meaning(cfg, None, &codes);
        let mut v: Vec<f32> = windows.iter().map(|w| m.search(w, 1).unwrap().first().map_or(0.0, |h| h.1)).collect();
        v.sort_by(f32::total_cmp);
        v
    };
    let (a, b) = (top1(from), top1(to));
    // The scoped search runs within a few chapters: its scores are calibrated
    // on the scoped searches a replay actually made (`SCOPED_LOG`).
    let scoped_queries: Vec<(String, Vec<kairo_bible::VerseId>)> = std::env::var("SCOPED_LOG")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|raw| {
            raw.lines()
                .map(|l| {
                    let v: serde_json::Value = serde_json::from_str(l).unwrap();
                    (v["text"].as_str().unwrap().to_string(), v["verses"].as_array().unwrap().iter().map(|x| kairo_bible::VerseId(x.as_u64().unwrap() as u32)).collect())
                })
                .collect()
        })
        .unwrap_or_default();
    let scoped_top = |cfg: &str| -> Vec<f32> {
        let m = meaning(cfg, None, &codes);
        let mut v: Vec<f32> = scoped_queries.iter().map(|(t, vs)| m.scoped(t, vs, 1).unwrap().first().map_or(0.0, |h| h.1)).collect();
        v.sort_by(f32::total_cmp);
        v
    };
    let (sa, sb) = if scoped_queries.is_empty() { (a.clone(), b.clone()) } else { (scoped_top(from), scoped_top(to)) };
    eprintln!("{} scoped searches", scoped_queries.len());
    let map_scoped = |t: f32| -> f32 {
        let p = sa.partition_point(|x| *x < t) as f64 / sa.len() as f64;
        (sb[((p * sb.len() as f64) as usize).min(sb.len() - 1)] * 1000.0).round() / 1000.0
    };
    let map = |t: f32| -> f32 {
        let rank = a.partition_point(|x| *x < t);
        let p = rank as f64 / a.len() as f64;
        let i = ((p * b.len() as f64) as usize).min(b.len() - 1);
        (b[i] * 1000.0).round() / 1000.0
    };
    let l = MeaningThresholds::LEGACY;
    let out = MeaningThresholds {
        offer: (l.offer.0, map(l.offer.1), l.offer.2),
        offer_in_play: (l.offer_in_play.0, map(l.offer_in_play.1), l.offer_in_play.2),
        offer_no_rerank: (map(l.offer_no_rerank.0), l.offer_no_rerank.1),
        offer_modern: (l.offer_modern.0, map(l.offer_modern.1), l.offer_modern.2, l.offer_modern.3),
        send: (l.send.0, map(l.send.1), l.send.2, l.send.3),
        scoped: (map_scoped(l.scoped.0), l.scoped.1),
    };
    for t in [0.60, 0.75, 0.78, 0.80, 0.82, 0.84] {
        let rank = a.partition_point(|x| *x < t);
        println!("{from} {t:.2} (top {:.1}% of speech) → {to} {:.3}", 100.0 - rank as f64 * 100.0 / a.len() as f64, map(t));
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
