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

/// The KJV, the NLT and the NIV — what the app matches for an English service.
pub fn translations() -> (Corpus, Translation, Translation) {
    let corpus = Corpus::load(&bibles()).expect("kjv");
    let nlt = corpus.load_map_nlt(&bibles().join("map.json")).expect("nlt");
    let niv = corpus.load_flat("NIV", &bibles().join("translations/NIV.json")).expect("niv");
    (corpus, nlt, niv)
}

pub fn knowledge() -> Knowledge {
    let (corpus, nlt, niv) = translations();
    Knowledge::build(corpus, Wordings { own: None, also: vec![nlt, niv] })
}

fn bench_config(name: &str) -> PathBuf {
    let p = Path::new(name);
    if p.is_absolute() { p.to_path_buf() } else { root().join("src-tauri/crates/kairo-semantic/bench").join(name) }
}

/// The meaning models: an embedder config and optional cross-encoder config
/// (files in `kairo-semantic/bench`), the verse index over `index_codes`
/// (built and cached on first use).
pub fn meaning(embedder: &str, reranker: Option<&str>, index_codes: &[&str]) -> MeaningService {
    let (corpus, nlt, niv) = translations();
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
            "NLT" => nlt.clone(),
            "NIV" => niv.clone(),
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
    MeaningService::new(embedder, index, reranker, corpus.kjv().clone(), vec![nlt, niv])
}

/// The engine's request, answered.
pub fn answer(m: &MeaningService, r: &Request) -> Answer {
    match r {
        Request::Paraphrase { id, windows, limit } => Answer::Paraphrase {
            id: *id,
            candidates: m
                .paraphrase(windows, *limit)
                .unwrap_or_default()
                .into_iter()
                .map(|f| MeaningHit { verse: f.verse, cos: f.cos, window: f.window, rerank: f.rerank, rerank_modern: f.rerank_modern })
                .collect(),
        },
        Request::Scoped { id, text, verses, limit } => Answer::Scoped { id: *id, hits: m.scoped(text, verses, *limit).unwrap_or_default() },
        Request::Compare { id, texts, verses } => Answer::Compare { id: *id, scores: m.compare(texts, verses).unwrap_or_default() },
    }
}
