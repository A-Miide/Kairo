//! Bake-off for the meaning-based layer: how well an embedder (+ fusion, +
//! reranker) finds the verse a preacher paraphrased, and how cleanly it keeps
//! ordinary sermon talk from looking like scripture.
//!
//! Cases: `eval/paraphrase_cases.json` (written, gapped, negatives) and
//! real paraphrases from the sermon fixtures — selected exactly as
//! `eval/paraphrase_bench.js` selects them.
//!
//!   cargo run --release -p kairo-semantic --bin semantic_bench -- \
//!     --embedder bench/embeddinggemma.json [--translations KJV,NLT] \
//!     [--reranker bench/minilm.json] [--cases eval/paraphrase_cases_hard.json] [--list]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use kairo_bible::{Book, Corpus, Translation, VerseId, VerseRef};
use kairo_semantic::{Builder, Embedder, EmbedderConfig, Reranker, RerankerConfig, VerseIndex};
use serde::Deserialize;

struct Case {
    group: &'static str,
    said: String,
    refs: Vec<VerseId>,
}

#[derive(Deserialize)]
struct CaseFile {
    positives: Vec<WrittenCase>,
    #[serde(default)]
    gapped: Vec<WrittenCase>,
    negatives: Vec<String>,
}

#[derive(Deserialize)]
struct WrittenCase {
    said: String,
    refs: Vec<String>,
}

#[derive(Deserialize)]
struct Fixture {
    transcript: Vec<Cue>,
    #[serde(rename = "groundTruth")]
    ground_truth: Vec<Truth>,
}

#[derive(Deserialize)]
struct Cue {
    #[serde(rename = "startMs")]
    start_ms: i64,
    text: String,
}

#[derive(Deserialize)]
struct Truth {
    book: String,
    chapter: u16,
    verse: Option<u16>,
    kind: String,
    #[serde(rename = "startMs")]
    start_ms: i64,
    similarity: Option<f64>,
    #[serde(default)]
    bootstrapped: bool,
}

const SHORTLIST: usize = 20;
const NEGATIVE_BUDGET: f64 = 0.05;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let list = args.iter().any(|a| a == "--list");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize()?;
    let bench_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench");
    let resolve = |p: &str| if Path::new(p).is_absolute() { PathBuf::from(p) } else { bench_dir.join(p) };

    let bibles = root.join("databases/bibles");
    let corpus = Corpus::load(&bibles)?;
    let codes: Vec<String> = arg("--translations").unwrap_or_else(|| "KJV".into()).split(',').map(|s| s.trim().to_uppercase()).collect();
    let translations: Vec<Translation> = codes.iter().map(|c| load_translation(&corpus, &bibles, c)).collect::<Result<_, _>>()?;

    let mut ecfg: EmbedderConfig = serde_json::from_slice(&std::fs::read(resolve(&arg("--embedder").ok_or("--embedder <config.json> is required")?))?)?;
    ecfg.model = root.join(&ecfg.model);
    ecfg.tokenizer = root.join(&ecfg.tokenizer);
    let t = Instant::now();
    let embedder = Embedder::load(ecfg)?;
    eprintln!("[bench] {} loaded in {:.1}s ({} dims)", embedder.config().name, t.elapsed().as_secs_f32(), embedder.dims());

    let cache_dir = root.join("eval-results/semantic-cache");
    let index_path = cache_dir.join(format!("{}.{}.kvx", embedder.config().name, codes.join("+")));
    let index = match VerseIndex::load(&index_path) {
        Ok(i) => i,
        Err(_) => {
            let builder = Builder { embedder: &embedder, corpus: &corpus, cache_dir: cache_dir.clone() };
            let refs: Vec<&Translation> = translations.iter().collect();
            let t = Instant::now();
            let mut last = Instant::now();
            let idx = builder.build(&refs, |code, done, total| {
                if last.elapsed().as_secs() >= 15 || done == total {
                    last = Instant::now();
                    let rate = done as f32 / t.elapsed().as_secs_f32().max(0.001);
                    eprintln!("[embed {code}] {done}/{total} ({rate:.0}/s)");
                }
            })?;
            idx.save(&index_path)?;
            idx
        }
    };

    let reranker = match arg("--reranker") {
        Some(p) => {
            let mut rcfg: RerankerConfig = serde_json::from_slice(&std::fs::read(resolve(&p))?)?;
            rcfg.model = root.join(&rcfg.model);
            rcfg.tokenizer = root.join(&rcfg.tokenizer);
            Some(Reranker::load(rcfg)?)
        }
        None => None,
    };

    let (cases, negatives) = load_cases(&root, &corpus)?;
    let kjv = corpus.kjv();

    // Scores for every case: the vector ranking, and the reranked shortlist.
    let mut query_ms = Vec::new();
    // `--window N [--step M]`: the speech is also searched in sliding windows of N words; a verse
    // scores as its best window (the paraphrase is usually a part of what was said around it).
    let window: usize = arg("--window").and_then(|s| s.parse().ok()).unwrap_or(0);
    let step: usize = arg("--step").and_then(|s| s.parse().ok()).unwrap_or(window.max(2) / 2);
    let mut score_case = |said: &str| -> Result<(Vec<(VerseId, f32)>, Vec<(VerseId, f32)>), Box<dyn std::error::Error>> {
        let t = Instant::now();
        let mut queries: Vec<String> = vec![said.to_string()];
        let words: Vec<&str> = said.split_whitespace().collect();
        if window > 0 && words.len() > window + step {
            let mut i = 0;
            while i + window <= words.len() {
                queries.push(words[i..i + window].join(" "));
                i += step;
            }
            queries.push(words[words.len() - window..].join(" "));
        }
        let refs: Vec<&str> = queries.iter().map(String::as_str).collect();
        let embedded = embedder.embed_queries(&refs)?;
        // verse → (best score, the query text that found it)
        let mut best: HashMap<VerseId, (f32, usize)> = HashMap::new();
        for (qi, q) in embedded.iter().enumerate() {
            for h in index.search(q, SHORTLIST) {
                let e = best.entry(h.verse).or_insert((f32::NEG_INFINITY, qi));
                if h.score > e.0 {
                    *e = (h.score, qi);
                }
            }
        }
        let mut merged: Vec<(VerseId, f32, usize)> = best.into_iter().map(|(v, (s, q))| (v, s, q)).collect();
        merged.sort_by(|a, b| b.1.total_cmp(&a.1));
        merged.truncate(SHORTLIST);
        query_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let vector: Vec<(VerseId, f32)> = merged.iter().map(|h| (h.0, h.1)).collect();
        let reranked = match &reranker {
            Some(r) => {
                // Each candidate against the words that found it.
                let mut v: Vec<(VerseId, f32)> = Vec::with_capacity(merged.len());
                for (verse, _, qi) in &merged {
                    let score = r.score(&queries[*qi], &[kjv.text(*verse).unwrap_or("")])?[0];
                    v.push((*verse, score));
                }
                v.sort_by(|a, b| b.1.total_cmp(&a.1));
                v
            }
            None => Vec::new(),
        };
        Ok((vector, reranked))
    };

    // `--dump <file>`: each case with its top candidates (the ranker's own order), for a judge to read.
    let mut dump: Vec<serde_json::Value> = Vec::new();
    let candidates_json = |ranked: &[(VerseId, f32)], vector: &[(VerseId, f32)]| -> Vec<serde_json::Value> {
        ranked.iter().take(5).map(|(v, s)| {
            let vs = vector.iter().find(|(x, _)| x == v).map_or(0.0, |x| x.1);
            serde_json::json!({ "ref": corpus.verse_ref(*v).to_string(), "text": kjv.text(*v).unwrap_or(""), "rr": s, "vec": vs })
        }).collect()
    };
    let mut by_group: HashMap<&str, Vec<(usize, f32, usize, f32)>> = HashMap::new(); // (vec rank, vec score at #1 if right, rr rank, rr score)
    for c in &cases {
        let (vector, reranked) = score_case(&c.said)?;
        let rank = |l: &[(VerseId, f32)]| l.iter().position(|(v, _)| c.refs.contains(v)).map_or(usize::MAX, |i| i + 1);
        let (vr, rr) = (rank(&vector), rank(&reranked));
        let top_if_right = |l: &[(VerseId, f32)], r: usize| if r == 1 { l[0].1 } else { f32::NEG_INFINITY };
        by_group.entry(c.group).or_default().push((vr, top_if_right(&vector, vr), rr, top_if_right(&reranked, rr)));
        dump.push(serde_json::json!({ "group": c.group, "said": c.said, "truth": c.refs.iter().map(|v| corpus.verse_ref(*v).to_string()).collect::<Vec<_>>(), "candidates": candidates_json(if reranked.is_empty() { &vector } else { &reranked }, &vector) }));
        if list && (vr > 1 || (reranker.is_some() && rr > 1)) {
            let show = |l: &[(VerseId, f32)]| l.iter().take(3).map(|(v, s)| format!("{} {s:.2}", corpus.verse_ref(*v))).collect::<Vec<_>>().join(", ");
            println!("  [{}] want {} | vec #{} rr #{} | vec: {} | rr: {} | \"{}\"",
                c.group, corpus.verse_ref(c.refs[0]), fmt_rank(vr), fmt_rank(rr), show(&vector), show(&reranked), truncate(&c.said, 90));
        }
    }
    let mut neg_vec = Vec::new();
    let mut neg_rr = Vec::new();
    for said in &negatives {
        let (vector, reranked) = score_case(said)?;
        neg_vec.push(vector.first().map_or(0.0, |x| x.1));
        neg_rr.push(reranked.first().map_or(0.0, |x| x.1));
        dump.push(serde_json::json!({ "group": "negative", "said": said, "truth": [], "candidates": candidates_json(if reranked.is_empty() { &vector } else { &reranked }, &vector) }));
    }
    if let Some(path) = arg("--dump") {
        std::fs::write(&path, serde_json::to_string(&dump)?)?;
        eprintln!("[bench] {} cases written to {path}", dump.len());
    }

    // Thresholds that let through at most NEGATIVE_BUDGET of ordinary talk.
    let th_vec = budget_threshold(&neg_vec);
    let th_rr = budget_threshold(&neg_rr);

    println!("\n== {} | fused: {} | reranker: {} ==", embedder.config().name, codes.join("+"), reranker.as_ref().map_or("none", |r| r.config().name.as_str()));
    println!("{:<9} {:>4}  {:>6} {:>6} {:>6} {:>6}  {:>9}   {:>6} {:>6}  {:>9}", "group", "n", "vec@1", "vec@3", "vec@10", "MRR", "offered*", "rr@1", "rr@3", "offered*");
    let mut total = (0usize, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for group in ["written", "gapped", "real"] {
        let Some(rows) = by_group.get(group) else { continue };
        let n = rows.len() as f64;
        let at = |k: usize, sel: fn(&(usize, f32, usize, f32)) -> usize| rows.iter().filter(|r| sel(r) <= k).count() as f64 / n;
        let mrr = rows.iter().map(|r| if r.0 == usize::MAX { 0.0 } else { 1.0 / r.0 as f64 }).sum::<f64>() / n;
        let off_vec = rows.iter().filter(|r| r.1 >= th_vec).count() as f64 / n;
        let off_rr = rows.iter().filter(|r| r.3 >= th_rr).count() as f64 / n;
        let pct = |x: f64| format!("{:.0}%", x * 100.0);
        let rr_cols = if reranker.is_some() {
            format!("{:>6} {:>6}  {:>9}", pct(at(1, |r| r.2)), pct(at(3, |r| r.2)), pct(off_rr))
        } else {
            String::new()
        };
        println!("{:<9} {:>4}  {:>6} {:>6} {:>6} {:>6.3}  {:>9}   {}", group, rows.len(), pct(at(1, |r| r.0)), pct(at(3, |r| r.0)), pct(at(10, |r| r.0)), mrr, pct(off_vec), rr_cols);
        total.0 += rows.len();
        total.1 += at(1, |r| r.0) * n;
        total.2 += off_vec * n;
        total.3 += at(1, |r| r.2) * n;
        total.4 += off_rr * n;
    }
    let n = total.0 as f64;
    println!("{:<9} {:>4}  {:>6.0}%{:>29.0}%   {:>5.0}%{:>18.0}%", "all", total.0, total.1 / n * 100.0, total.2 / n * 100.0, total.3 / n * 100.0, total.4 / n * 100.0);
    println!("* offered = right verse at #1 AND above the threshold that offers ≤{:.0}% of {} negatives (vec ≥ {th_vec:.3}, rr ≥ {th_rr:.3})", NEGATIVE_BUDGET * 100.0, negatives.len());
    query_ms.sort_by(|a, b| a.total_cmp(b));
    println!("query embed+search: p50 {:.0} ms, p95 {:.0} ms", query_ms[query_ms.len() / 2], query_ms[query_ms.len() * 95 / 100]);
    Ok(())
}

fn load_translation(corpus: &Corpus, bibles: &Path, code: &str) -> Result<Translation, Box<dyn std::error::Error>> {
    Ok(match code {
        "KJV" => {
            // The anchor: KJV text straight from the corpus.
            let path = bibles.join("translations/KJV.json");
            if path.exists() { corpus.load_flat("KJV", &path)? } else { kjv_copy(corpus) }
        }
        "NLT" => corpus.load_map_nlt(&bibles.join("map.json"))?,
        other => corpus.load_flat(other, &bibles.join(format!("translations/{other}.json")))?,
    })
}

fn kjv_copy(corpus: &Corpus) -> Translation {
    corpus.kjv().clone()
}

fn load_cases(root: &Path, corpus: &Corpus) -> Result<(Vec<Case>, Vec<String>), Box<dyn std::error::Error>> {
    // `--cases <file>` (relative to the repo): another case set, e.g. eval/paraphrase_cases_hard.json.
    let cases_path = std::env::args().collect::<Vec<_>>().windows(2).find(|w| w[0] == "--cases").map(|w| w[1].clone()).unwrap_or_else(|| "eval/paraphrase_cases.json".into());
    let file: CaseFile = serde_json::from_slice(&std::fs::read(root.join(&cases_path))?)?;
    let parse = |s: &str| -> Option<VerseId> {
        let (book, cv) = s.rsplit_once(' ')?;
        let (c, v) = cv.split_once(':')?;
        let book = Book::from_name(book).or_else(|| Book::from_name(&format!("{book}s")))?;
        corpus.id(VerseRef::new(book, c.parse().ok()?, v.parse().ok()?))
    };
    let mut cases = Vec::new();
    for (group, list) in [("written", &file.positives), ("gapped", &file.gapped)] {
        for c in list {
            let refs: Vec<VerseId> = c.refs.iter().filter_map(|r| parse(r)).collect();
            if refs.is_empty() {
                eprintln!("[bench] skipping case with unknown refs {:?}", c.refs);
                continue;
            }
            cases.push(Case { group, said: c.said.clone(), refs });
        }
    }
    // Development sermons only; the held-out pair is scored once, at the end
    // (docs/RUST_REWRITE_PLAN.md, "Evaluation split").
    let dir = root.join("eval/fixtures");
    let held_out = std::env::args().any(|a| a == "--held-out");
    let split: &[&str] = if held_out {
        &["oyedepo-meditation", "shiloh2025-love-mystery"]
    } else {
        &["enough-is-enough", "oyedepo-kingdomlaw", "shiloh2025-impartation"]
    };
    let names: Vec<_> = split.iter().map(|n| dir.join(format!("{n}.json"))).collect();
    for path in names {
        let fx: Fixture = serde_json::from_slice(&std::fs::read(&path)?)?;
        for g in &fx.ground_truth {
            let Some(verse) = g.verse else { continue };
            if g.kind != "quote" || g.bootstrapped || g.similarity.is_none_or(|s| s >= 0.6) {
                continue;
            }
            let said: String = fx.transcript.iter().filter(|c| c.start_ms >= g.start_ms - 3000 && c.start_ms <= g.start_ms + 10000).map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ");
            if said.split_whitespace().count() < 6 {
                continue;
            }
            if let Some(id) = parse(&format!("{} {}:{}", g.book, g.chapter, verse)) {
                cases.push(Case { group: "real", said, refs: vec![id] });
            }
        }
    }
    Ok((cases, file.negatives))
}

fn budget_threshold(neg: &[f32]) -> f32 {
    if neg.is_empty() {
        return 0.0;
    }
    let mut s = neg.to_vec();
    s.sort_by(|a, b| b.total_cmp(a));
    let allowed = (neg.len() as f64 * NEGATIVE_BUDGET).floor() as usize;
    // Strictly above the (allowed+1)-th highest negative.
    s[allowed.min(s.len() - 1)] + 1e-6
}

fn fmt_rank(r: usize) -> String {
    if r == usize::MAX { "-".into() } else { r.to_string() }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n).collect::<String>() + "…" }
}
