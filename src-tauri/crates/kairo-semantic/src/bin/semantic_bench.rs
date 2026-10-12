//! Bake-off for the meaning-based layer: how well an embedder (+ fusion, +
//! reranker) finds the verse a preacher paraphrased, and how cleanly it keeps
//! ordinary sermon talk from looking like scripture.
//!
//! Cases: `server/eval/paraphrase_cases.json` (written, gapped, negatives) and
//! real paraphrases from the sermon fixtures — selected exactly as
//! `server/eval/paraphrase_bench.js` selects them.
//!
//!   cargo run --release -p kairo-semantic --bin semantic_bench -- \
//!     --embedder bench/embeddinggemma.json [--translations KJV,NLT] \
//!     [--reranker bench/minilm.json] [--list]

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
    let mut score_case = |said: &str| -> Result<(Vec<(VerseId, f32)>, Vec<(VerseId, f32)>), Box<dyn std::error::Error>> {
        let t = Instant::now();
        let q = embedder.embed_queries(&[said])?.remove(0);
        let hits = index.search(&q, SHORTLIST);
        query_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let vector: Vec<(VerseId, f32)> = hits.iter().map(|h| (h.verse, h.score)).collect();
        let reranked = match &reranker {
            Some(r) => {
                let texts: Vec<&str> = hits.iter().map(|h| kjv.text(h.verse).unwrap_or("")).collect();
                let scores = r.score(said, &texts)?;
                let mut v: Vec<(VerseId, f32)> = hits.iter().map(|h| h.verse).zip(scores).collect();
                v.sort_by(|a, b| b.1.total_cmp(&a.1));
                v
            }
            None => Vec::new(),
        };
        Ok((vector, reranked))
    };

    let mut by_group: HashMap<&str, Vec<(usize, f32, usize, f32)>> = HashMap::new(); // (vec rank, vec score at #1 if right, rr rank, rr score)
    for c in &cases {
        let (vector, reranked) = score_case(&c.said)?;
        let rank = |l: &[(VerseId, f32)]| l.iter().position(|(v, _)| c.refs.contains(v)).map_or(usize::MAX, |i| i + 1);
        let (vr, rr) = (rank(&vector), rank(&reranked));
        let top_if_right = |l: &[(VerseId, f32)], r: usize| if r == 1 { l[0].1 } else { f32::NEG_INFINITY };
        by_group.entry(c.group).or_default().push((vr, top_if_right(&vector, vr), rr, top_if_right(&reranked, rr)));
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
    let file: CaseFile = serde_json::from_slice(&std::fs::read(root.join("server/eval/paraphrase_cases.json"))?)?;
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
    let dir = root.join("server/eval/fixtures");
    let mut names: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    names.sort();
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
