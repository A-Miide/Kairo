//! Shared setup for the evaluation tools: the Bible, the engine's knowledge,
//! and the meaning models, loaded the way the app loads them.

use std::path::{Path, PathBuf};

use kairo_bible::{Corpus, Translation};
use kairo_detect::engine::{Answer, MeaningHit, Request};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_semantic::{Builder, Embedder, EmbedderConfig, MeaningService, Reranker, RerankerConfig, VerseIndex};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn bibles() -> PathBuf {
    root().join("databases/bibles")
}

/// The KJV and the BSB — what the app matches for an English service.
pub fn translations() -> (Corpus, Translation) {
    let corpus = Corpus::load(&bibles()).expect("kjv");
    let bsb = corpus.load_flat("BSB", &bibles().join("translations/BSB.json")).expect("bsb");
    (corpus, bsb)
}

/// The modern wordings matched beside the KJV, to measure what they add: none with
/// `KAIRO_EVAL_KJV_ONLY` set, others in its place with `KAIRO_EVAL_MODERN=WEB=/path.json,…`
/// (flat verse files, like `translations/BSB.json`).
fn modern(corpus: &Corpus, bsb: Translation) -> Vec<Translation> {
    if std::env::var_os("KAIRO_EVAL_KJV_ONLY").is_some() {
        return vec![];
    }
    let Ok(list) = std::env::var("KAIRO_EVAL_MODERN") else { return vec![bsb] };
    list.split(',')
        .map(|item| {
            let (code, path) = item.split_once('=').expect("CODE=path");
            corpus.load_flat(code, Path::new(path)).expect("modern translation")
        })
        .collect()
}

pub fn knowledge() -> Knowledge {
    let (corpus, bsb) = translations();
    let also = modern(&corpus, bsb);
    Knowledge::build(corpus, Wordings { own: None, also })
}

fn bench_config(name: &str) -> PathBuf {
    let p = Path::new(name);
    if p.is_absolute() { p.to_path_buf() } else { root().join("src-tauri/crates/kairo-semantic/bench").join(name) }
}

/// The meaning models: an embedder config and optional cross-encoder config
/// (files in `kairo-semantic/bench`), the verse index over `index_codes`
/// (built and cached on first use).
pub fn meaning(embedder: &str, reranker: Option<&str>, index_codes: &[&str]) -> MeaningService {
    let (corpus, bsb) = translations();
    let mut ecfg: EmbedderConfig = serde_json::from_slice(&std::fs::read(bench_config(embedder)).expect("embedder config")).expect("embedder config");
    ecfg.model = root().join(&ecfg.model);
    ecfg.tokenizer = root().join(&ecfg.tokenizer);
    let embedder = Embedder::load(ecfg).expect("embedder");
    let cache_dir = root().join("eval-results/semantic-cache");
    let index_path = cache_dir.join(format!("{}.{}.kvx", embedder.config().name, index_codes.join("+")));
    let index = VerseIndex::load(&index_path).ok().filter(|i| i.len() == corpus.len()).unwrap_or_else(|| {
        let kjv = corpus.kjv().clone();
        let pick = |c: &str| match c {
            "KJV" => kjv.clone(),
            "BSB" => bsb.clone(),
            other => panic!("no translation {other}"),
        };
        let ts: Vec<Translation> = index_codes.iter().map(|c| pick(c)).collect();
        let refs: Vec<&Translation> = ts.iter().collect();
        let b = Builder { embedder: &embedder, corpus: &corpus, cache_dir: cache_dir.clone() };
        let idx = b.build(&refs, |code, done, total| eprintln!("[embed {code}] {done}/{total}")).expect("index");
        idx.save(&index_path).expect("save index");
        idx
    });
    let reranker = reranker.map(|r| {
        let mut rcfg: RerankerConfig = serde_json::from_slice(&std::fs::read(bench_config(r)).expect("reranker config")).expect("reranker config");
        rcfg.model = root().join(&rcfg.model);
        rcfg.tokenizer = root().join(&rcfg.tokenizer);
        Reranker::load(rcfg).expect("reranker")
    });
    MeaningService::new(embedder, index, reranker, corpus.kjv().clone(), modern(&corpus, bsb))
}

/// The engine's request, answered.
pub fn answer(m: &MeaningService, r: &Request) -> Answer {
    match r {
        Request::Paraphrase { id, windows, limit, rerank_from } => Answer::Paraphrase {
            id: *id,
            // `MEANING_LIMIT=<n>`: a longer shortlist for the cross-encoder than the engine asks for.
            candidates: m
                .paraphrase(windows, std::env::var("MEANING_LIMIT").ok().and_then(|n| n.parse().ok()).unwrap_or(*limit), *rerank_from)
                .unwrap_or_default()
                .into_iter()
                .map(|f| MeaningHit { verse: f.verse, cos: f.cos, window: f.window, rerank: f.rerank, rerank_modern: f.rerank_modern })
                .collect(),
        },
        Request::Scoped { id, text, verses, limit } => Answer::Scoped { id: *id, hits: m.scoped(text, verses, *limit).unwrap_or_default() },
        Request::Compare { id, texts, verses } => Answer::Compare { id: *id, scores: m.compare(texts, verses).unwrap_or_default() },
    }
}
