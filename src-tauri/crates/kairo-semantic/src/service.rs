//! Answers the detection engine's meaning requests: which verses a stretch
//! of speech means, within the whole Bible or a few chapters, and how well a
//! few named verses fit it.

use std::collections::HashMap;
use std::sync::Mutex;

use kairo_bible::{Translation, VerseId};

use crate::embedder::Embedder;
use crate::index::VerseIndex;
use crate::reranker::Reranker;
use crate::SemanticError;

/// Verses each window brings to the shortlist.
const PER_WINDOW: usize = 6;

/// A verse found by meaning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub verse: VerseId,
    /// Best cosine over the windows.
    pub cos: f32,
    /// The window that found it.
    pub window: usize,
    /// Cross-encoder: the KJV wording, and a modern one.
    pub rerank: Option<f32>,
    pub rerank_modern: Option<f32>,
}

pub struct MeaningService {
    embedder: Embedder,
    index: VerseIndex,
    reranker: Option<Reranker>,
    kjv: Translation,
    /// Modern wordings the cross-encoder reads beside the KJV, first that has the verse.
    modern: Vec<Translation>,
    /// Query vectors already computed (speech windows repeat while a sentence grows).
    cache: Mutex<HashMap<String, Vec<f32>>>,
}

const CACHE_MAX: usize = 4_096;

impl MeaningService {
    pub fn new(embedder: Embedder, index: VerseIndex, reranker: Option<Reranker>, kjv: Translation, modern: Vec<Translation>) -> MeaningService {
        MeaningService { embedder, index, reranker, kjv, modern, cache: Mutex::new(HashMap::new()) }
    }

    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some()
    }

    fn queries(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SemanticError> {
        let mut cache = self.cache.lock().unwrap();
        let missing: Vec<&str> = texts.iter().copied().filter(|t| !cache.contains_key(*t)).collect();
        if !missing.is_empty() {
            let vecs = self.embedder.embed_queries(&missing)?;
            if cache.len() + missing.len() > CACHE_MAX {
                cache.clear();
            }
            for (t, v) in missing.iter().zip(vecs) {
                cache.insert(t.to_string(), v);
            }
        }
        Ok(texts.iter().map(|t| cache[*t].clone()).collect())
    }

    fn kjv_text(&self, v: VerseId) -> &str {
        self.kjv.text(v).unwrap_or("")
    }

    fn modern_text(&self, v: VerseId) -> &str {
        self.modern.iter().find_map(|t| t.text(v)).unwrap_or_else(|| self.kjv_text(v))
    }

    /// The verses the windows mean, best window per verse, with cross-encoder
    /// scores for the shortlist.
    pub fn paraphrase(&self, windows: &[String], limit: usize) -> Result<Vec<Found>, SemanticError> {
        if windows.is_empty() {
            return Ok(Vec::new());
        }
        let texts: Vec<&str> = windows.iter().map(String::as_str).collect();
        let qs = self.queries(&texts)?;
        let mut pool: HashMap<VerseId, (f32, usize)> = HashMap::new();
        for (w, hits) in self.index.search_many(&qs, PER_WINDOW).into_iter().enumerate() {
            for h in hits {
                let e = pool.entry(h.verse).or_insert((h.score, w));
                if h.score > e.0 {
                    *e = (h.score, w);
                }
            }
        }
        let mut top: Vec<Found> = pool.into_iter().map(|(verse, (cos, window))| Found { verse, cos, window, rerank: None, rerank_modern: None }).collect();
        top.sort_by(|a, b| b.cos.total_cmp(&a.cos).then(a.verse.cmp(&b.verse)));
        top.truncate(limit);
        if let Some(rr) = &self.reranker {
            let mut queries = Vec::with_capacity(top.len() * 2);
            let mut passages = Vec::with_capacity(top.len() * 2);
            for f in &top {
                queries.extend([texts[f.window], texts[f.window]]);
                passages.extend([self.kjv_text(f.verse), self.modern_text(f.verse)]);
            }
            let scores = rr.score_pairs(&queries, &passages)?;
            for (i, f) in top.iter_mut().enumerate() {
                f.rerank = scores.get(2 * i).copied();
                f.rerank_modern = scores.get(2 * i + 1).copied();
            }
        }
        Ok(top)
    }

    /// The closest verses in meaning in the whole Bible.
    pub fn search(&self, text: &str, limit: usize) -> Result<Vec<(VerseId, f32)>, SemanticError> {
        let q = self.queries(&[text])?;
        Ok(self.index.search(&q[0], limit).into_iter().map(|h| (h.verse, h.score)).collect())
    }

    /// The closest verses in meaning among `verses`.
    pub fn scoped(&self, text: &str, verses: &[VerseId], limit: usize) -> Result<Vec<(VerseId, f32)>, SemanticError> {
        let q = self.queries(&[text])?;
        Ok(self.index.search_within(&q[0], verses, limit).into_iter().map(|h| (h.verse, h.score)).collect())
    }

    /// How well each verse fits the speech: best cosine over the texts, and
    /// the cross-encoder's best over the texts and both wordings.
    pub fn compare(&self, texts: &[String], verses: &[VerseId]) -> Result<Vec<(Option<f32>, Option<f32>)>, SemanticError> {
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let qs = self.queries(&refs)?;
        let cos: Vec<Option<f32>> = verses.iter().map(|v| qs.iter().map(|q| self.index.score(q, *v)).reduce(f32::max)).collect();
        let rr: Vec<Option<f32>> = match &self.reranker {
            Some(r) if !texts.is_empty() => {
                let (mut queries, mut passages) = (Vec::new(), Vec::new());
                for t in &refs {
                    for v in verses {
                        queries.extend([*t, *t]);
                        passages.extend([self.kjv_text(*v), self.modern_text(*v)]);
                    }
                }
                let scores = r.score_pairs(&queries, &passages)?;
                let per_text = verses.len() * 2;
                (0..verses.len())
                    .map(|n| (0..refs.len()).flat_map(|t| [scores[t * per_text + 2 * n], scores[t * per_text + 2 * n + 1]]).reduce(f32::max))
                    .collect()
            }
            _ => vec![None; verses.len()],
        };
        Ok(cos.into_iter().zip(rr).collect())
    }
}
