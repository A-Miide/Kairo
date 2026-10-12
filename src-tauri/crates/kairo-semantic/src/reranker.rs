//! Cross-encoder reranker: reads the spoken text and a verse together and
//! scores how well they match, 0–1 (sigmoid of the model's one logit). Slower
//! than vector search, so it only sees the shortlist vector search returns.

use std::path::PathBuf;
use std::sync::Mutex;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use tokenizers::{EncodeInput, Tokenizer};

use crate::embedder::threads;
use crate::SemanticError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RerankerConfig {
    pub name: String,
    pub model: PathBuf,
    pub tokenizer: PathBuf,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub threads: usize,
}

fn default_max_tokens() -> usize {
    256
}

pub struct Reranker {
    cfg: RerankerConfig,
    tokenizer: Tokenizer,
    session: Mutex<Session>,
    inputs: Vec<String>,
    pad_id: u32,
}

impl Reranker {
    pub fn load(cfg: RerankerConfig) -> Result<Reranker, SemanticError> {
        let tokenizer = Tokenizer::from_file(&cfg.tokenizer).map_err(|e| SemanticError::Tokenizer(e.to_string()))?;
        let pad_id = tokenizer.get_padding().map(|p| p.pad_id).or_else(|| tokenizer.token_to_id("[PAD]")).unwrap_or(0);
        let session = Session::builder()
            .and_then(|b| Ok(b.with_optimization_level(GraphOptimizationLevel::Level3)?))
            .and_then(|b| Ok(b.with_intra_threads(threads(cfg.threads))?))
            .and_then(|b| Ok(b.with_inter_threads(1)?))
            .and_then(|mut b| b.commit_from_file(&cfg.model))
            .map_err(SemanticError::ort)?;
        let inputs = session.inputs().iter().map(|i| i.name().to_string()).collect();
        Ok(Reranker { cfg, tokenizer, session: Mutex::new(session), inputs, pad_id })
    }

    pub fn config(&self) -> &RerankerConfig {
        &self.cfg
    }

    /// One score per passage for the same query.
    pub fn score(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>, SemanticError> {
        let queries = vec![query; passages.len()];
        self.score_pairs(&queries, passages)
    }

    /// One score per (query, passage) pair.
    pub fn score_pairs(&self, queries: &[&str], passages: &[&str]) -> Result<Vec<f32>, SemanticError> {
        assert_eq!(queries.len(), passages.len(), "one query per passage");
        if passages.is_empty() {
            return Ok(Vec::new());
        }
        let pairs: Vec<EncodeInput> = queries.iter().zip(passages).map(|(q, p)| (q.to_string(), p.to_string()).into()).collect();
        let encodings = self.tokenizer.encode_batch(pairs, true).map_err(|e| SemanticError::Tokenizer(e.to_string()))?;
        let n = encodings.len();
        let len = encodings.iter().map(|e| e.get_ids().len().min(self.cfg.max_tokens)).max().unwrap_or(1).max(1);
        let mut ids = vec![self.pad_id as i64; n * len];
        let mut mask = vec![0i64; n * len];
        let mut types = vec![0i64; n * len];
        for (r, enc) in encodings.iter().enumerate() {
            for j in 0..enc.get_ids().len().min(len) {
                ids[r * len + j] = enc.get_ids()[j] as i64;
                mask[r * len + j] = 1;
                types[r * len + j] = enc.get_type_ids()[j] as i64;
            }
        }
        let shape = [n, len];
        let mut feed: Vec<(String, ort::value::DynValue)> = Vec::new();
        for name in &self.inputs {
            let data = match name.as_str() {
                "input_ids" => ids.clone(),
                "attention_mask" => mask.clone(),
                "token_type_ids" => types.clone(),
                other => return Err(SemanticError::Model(format!("unexpected reranker input `{other}`"))),
            };
            feed.push((name.clone(), Tensor::from_array((shape, data)).map_err(SemanticError::ort)?.into_dyn()));
        }
        let mut session = self.session.lock().expect("reranker session poisoned");
        let outputs = session.run(feed).map_err(SemanticError::ort)?;
        let (shape, logits) = outputs[0].try_extract_tensor::<f32>().map_err(SemanticError::ort)?;
        let per_row = if shape.len() > 1 { shape[1] as usize } else { 1 };
        Ok((0..n).map(|r| sigmoid(logits[r * per_row])).collect())
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}
