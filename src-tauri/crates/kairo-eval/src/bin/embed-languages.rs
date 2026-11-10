//! Which verse does a meaning model find from a verse's text in another language? Embeds the whole KJV as
//! documents, then each language pack's text of a sample of verses as queries, and reports how often the
//! right KJV verse comes first (and in the first five), with the model's size, memory and speed. For choosing
//! one meaning model for every language the app speaks (docs/ARCHITECTURE.md: one model per job).
//!
//! `cargo run --release -p kairo-eval --bin embed-languages -- <embedder.json> [--sample 600] [--langs es,fr,...] [--cache file.bin]`
//! `… <embedder.json> --tokens [--ids-out ids.json]` lists the vocabulary the Bibles use instead.
//! The config is a kairo-semantic EmbedderConfig (absolute paths work). Languages are the packs in
//! `databases/bibles/packs`, plus `en` (the BSB, what the English path reads).

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use kairo_bible::{Corpus, VerseId};
use kairo_eval::root;
use kairo_semantic::{Embedder, EmbedderConfig};

/// This process's resident memory in MB (ps, so no platform code here).
fn rss_mb() -> f64 {
    let out = Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().ok();
    out.and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.trim().parse::<f64>().ok()).map_or(0.0, |kb| kb / 1024.0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let Some(cfg_path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: embed-languages <embedder.json> [--sample N] [--langs es,fr] [--cache file]");
        std::process::exit(2);
    };
    let sample: usize = opt("--sample").and_then(|s| s.parse().ok()).unwrap_or(600);
    let langs: Vec<String> = opt("--langs").map_or_else(|| "en,es,fr,de,pt,sw,yo,ig,ha".split(',').map(String::from).collect(), |l| l.split(',').map(String::from).collect());
    let cfg: EmbedderConfig = serde_json::from_slice(&std::fs::read(cfg_path).expect("config")).expect("config json");
    let size_mb = std::fs::metadata(&cfg.model).map_or(0.0, |m| m.len() as f64 / 1e6) + std::fs::metadata(&cfg.tokenizer).map_or(0.0, |m| m.len() as f64 / 1e6);
    // `--tokens`: how many distinct tokens the Bibles use (a model's vocabulary can be cut to these).
    if args.iter().any(|a| a == "--tokens") {
        count_tokens(&cfg, opt("--ids-out").map(PathBuf::from));
        return;
    }
    let rss_before = rss_mb();
    let embedder = Embedder::load(cfg.clone()).expect("embedder");
    let rss_loaded = rss_mb();
    println!("{}: files {:.0} MB, {} dims; memory before load {:.0} MB, after load {:.0} MB (+{:.0})", cfg.name, size_mb, embedder.dims(), rss_before, rss_loaded, rss_loaded - rss_before);

    let dir = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).expect("kjv");
    let kjv = corpus.kjv().clone();
    let ids: Vec<VerseId> = corpus.ids().collect();
    let dims = embedder.dims();

    // The KJV as documents (cached: it is the slow part).
    let cache = opt("--cache").map(PathBuf::from);
    let mut docs: Vec<f32> = Vec::new();
    if let Some(c) = cache.as_ref().filter(|c| c.exists()) {
        let bytes = std::fs::read(c).expect("cache");
        docs = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    }
    if docs.len() != ids.len() * dims {
        docs.clear();
        let t = Instant::now();
        let texts: Vec<&str> = ids.iter().map(|v| kjv.text(*v).unwrap_or("")).collect();
        for (n, batch) in texts.chunks(64).enumerate() {
            for v in embedder.embed_documents(batch).expect("embed") {
                docs.extend(v);
            }
            if n % 100 == 0 {
                eprintln!("[docs] {}/{}", (n * 64).min(texts.len()), texts.len());
            }
        }
        println!("embedded {} KJV verses in {:.0}s; peak-ish memory now {:.0} MB", ids.len(), t.elapsed().as_secs_f32(), rss_mb());
        if let Some(c) = &cache {
            let bytes: Vec<u8> = docs.iter().flat_map(|f| f.to_le_bytes()).collect();
            std::fs::write(c, bytes).ok();
        }
    }

    let step = (ids.len() / sample).max(1);
    let picks: Vec<usize> = (0..ids.len()).step_by(step).take(sample).collect();
    println!("{:<4} {:>8} {:>8} {:>10}", "lang", "top1", "top5", "ms/query");
    let mut totals = (0usize, 0usize, 0usize);
    for lang in &langs {
        let tr = if lang == "en" {
            corpus.load_flat("BSB", &dir.join("translations/BSB.json"))
        } else {
            corpus.load_flat(lang, &dir.join(format!("packs/{lang}.json")))
        };
        let Ok(tr) = tr else { println!("{lang:<4} (no text)"); continue };
        let queries: Vec<(usize, &str)> = picks.iter().filter_map(|&i| tr.text(ids[i]).filter(|t| t.split_whitespace().count() >= 4).map(|t| (i, t))).collect();
        let t = Instant::now();
        let (mut top1, mut top5) = (0usize, 0usize);
        for batch in queries.chunks(32) {
            let texts: Vec<&str> = batch.iter().map(|q| q.1).collect();
            let vecs = embedder.embed_queries(&texts).expect("embed");
            for ((truth, _), q) in batch.iter().zip(vecs) {
                // Cosine over normalised vectors: the dot product; keep the five best.
                let mut best: Vec<(f32, usize)> = Vec::with_capacity(6);
                for (d, row) in docs.chunks_exact(dims).enumerate() {
                    let s: f32 = row.iter().zip(&q).map(|(a, b)| a * b).sum();
                    if best.len() < 5 || s > best[best.len() - 1].0 {
                        best.push((s, d));
                        best.sort_by(|a, b| b.0.total_cmp(&a.0));
                        best.truncate(5);
                    }
                }
                top1 += (best[0].1 == *truth) as usize;
                top5 += best.iter().any(|b| b.1 == *truth) as usize;
            }
        }
        let n = queries.len().max(1);
        let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("{lang:<4} {:>7.1}% {:>7.1}% {:>9.0}  (n={n})", 100.0 * top1 as f64 / n as f64, 100.0 * top5 as f64 / n as f64, ms);
        totals = (totals.0 + top1, totals.1 + top5, totals.2 + n);
    }
    println!("all  {:>7.1}% {:>7.1}%   memory at end {:.0} MB", 100.0 * totals.0 as f64 / totals.2.max(1) as f64, 100.0 * totals.1 as f64 / totals.2.max(1) as f64, rss_mb());
}

/// The distinct tokens the KJV, BSB and every pack use, out of the tokenizer's whole vocabulary: the
/// embedding table (most of a multilingual model's size and memory) only needs these rows.
fn count_tokens(cfg: &EmbedderConfig, out: Option<PathBuf>) {
    let tok = tokenizers::Tokenizer::from_file(&cfg.tokenizer).expect("tokenizer");
    let dir = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).expect("kjv");
    let mut used = std::collections::BTreeSet::new();
    let note = |name: &str, texts: Vec<String>, used: &mut std::collections::BTreeSet<u32>| {
        let before = used.len();
        for batch in texts.chunks(256) {
            let enc = tok.encode_batch(batch.to_vec(), false).expect("encode");
            for e in enc {
                used.extend(e.get_ids().iter().copied());
            }
        }
        println!("{name:<4} adds {:>6} tokens (total {})", used.len() - before, used.len());
    };
    let ids: Vec<VerseId> = corpus.ids().collect();
    let kjv = corpus.kjv().clone();
    note("kjv", ids.iter().filter_map(|v| kjv.text(*v).map(String::from)).collect(), &mut used);
    let mut sets: Vec<(String, kairo_bible::Translation)> = Vec::new();
    if let Ok(t) = corpus.load_flat("BSB", &dir.join("translations/BSB.json")) {
        sets.push(("bsb".into(), t));
    }
    for l in ["es", "fr", "de", "pt", "sw", "yo", "ig", "ha"] {
        if let Ok(t) = corpus.load_flat(l, &dir.join(format!("packs/{l}.json"))) {
            sets.push((l.into(), t));
        }
    }
    for (name, t) in &sets {
        note(name, ids.iter().filter_map(|v| t.text(*v).map(String::from)).collect(), &mut used);
    }
    if let Some(path) = out {
        let ids: Vec<String> = used.iter().map(u32::to_string).collect();
        std::fs::write(path, format!("[{}]", ids.join(","))).ok();
    }
    println!("vocabulary {} tokens; the Bibles use {} ({:.1}%)", tok.get_vocab_size(true), used.len(), 100.0 * used.len() as f64 / tok.get_vocab_size(true) as f64);
}
