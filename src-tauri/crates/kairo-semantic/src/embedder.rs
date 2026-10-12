//! Text → unit vector, for any ONNX embedding model.
//!
//! Models differ in three ways, all captured by `EmbedderConfig`:
//! - how a sentence vector comes out (`Pooling`): some models export their own
//!   pooled, projected vector (EmbeddingGemma's `sentence_embedding`); others
//!   give per-token states to pool by mean, first token or last token;
//! - the prompt each side was trained with (EmbeddingGemma: `task: search
//!   result | query: ` for queries, `title: none | text: ` for documents);
//! - which inputs the graph takes (`token_type_ids`, `position_ids`).

use std::path::PathBuf;
use std::sync::Mutex;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;

use crate::{normalize, SemanticError};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "output")]
pub enum Pooling {
    /// The graph already outputs a pooled sentence vector under this name.
    Output(String),
    /// Mean of `last_hidden_state` over real tokens.
    Mean,
    /// `last_hidden_state` of the first token.
    Cls,
    /// `last_hidden_state` of the last real token (decoder-style embedders).
    LastToken,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedderConfig {
    /// A short name for reports and index metadata.
    pub name: String,
    pub model: PathBuf,
    pub tokenizer: PathBuf,
    pub pooling: Pooling,
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default)]
    pub document_prefix: String,
    /// Keep only the first N dimensions (Matryoshka models). 0 = all.
    #[serde(default)]
    pub truncate_dims: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    /// Inference threads. 0 = half the cores: audio, transcription and
    /// outputs run on the same machine and starved when inference took them
    /// all (the Node server learned this live).
    #[serde(default)]
    pub threads: usize,
}

fn default_max_tokens() -> usize {
    256
}

pub struct Embedder {
    cfg: EmbedderConfig,
    tokenizer: Tokenizer,
    session: Mutex<Session>,
    inputs: Vec<String>,
    pad_id: u32,
    dims: usize,
}

impl Embedder {
    pub fn load(cfg: EmbedderConfig) -> Result<Embedder, SemanticError> {
        let tokenizer = Tokenizer::from_file(&cfg.tokenizer).map_err(|e| SemanticError::Tokenizer(e.to_string()))?;
        let pad_id = tokenizer
            .get_padding()
            .map(|p| p.pad_id)
            .or_else(|| tokenizer.token_to_id("<pad>"))
            .or_else(|| tokenizer.token_to_id("[PAD]"))
            .unwrap_or(0);
        let session = Session::builder()
            .and_then(|b| Ok(b.with_optimization_level(GraphOptimizationLevel::Level3)?))
            .and_then(|b| Ok(b.with_intra_threads(threads(cfg.threads))?))
            .and_then(|b| Ok(b.with_inter_threads(1)?))
            .and_then(|mut b| b.commit_from_file(&cfg.model))
            .map_err(SemanticError::ort)?;
        let inputs = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let mut e = Embedder { cfg, tokenizer, session: Mutex::new(session), inputs, pad_id, dims: 0 };
        e.dims = e.run(&["probe".to_string()])?[0].len();
        Ok(e)
    }

    pub fn config(&self) -> &EmbedderConfig {
        &self.cfg
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub fn embed_queries(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SemanticError> {
        self.run(&texts.iter().map(|t| format!("{}{}", self.cfg.query_prefix, t)).collect::<Vec<_>>())
    }

    pub fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SemanticError> {
        self.run(&texts.iter().map(|t| format!("{}{}", self.cfg.document_prefix, t)).collect::<Vec<_>>())
    }

    fn run(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| SemanticError::Tokenizer(e.to_string()))?;
        let n = encodings.len();
        let len = encodings.iter().map(|e| e.get_ids().len().min(self.cfg.max_tokens)).max().unwrap_or(1).max(1);
        let mut ids = vec![self.pad_id as i64; n * len];
        let mut mask = vec![0i64; n * len];
        let mut types = vec![0i64; n * len];
        let mut positions = vec![0i64; n * len];
        let mut lengths = Vec::with_capacity(n);
        for (r, enc) in encodings.iter().enumerate() {
            let k = enc.get_ids().len().min(len);
            for j in 0..k {
                ids[r * len + j] = enc.get_ids()[j] as i64;
                mask[r * len + j] = 1;
                types[r * len + j] = enc.get_type_ids()[j] as i64;
                positions[r * len + j] = j as i64;
            }
            lengths.push(k);
        }
        let shape = [n, len];
        let mut feed: Vec<(String, ort::value::DynValue)> = Vec::new();
        for name in &self.inputs {
            let data = match name.as_str() {
                "input_ids" => ids.clone(),
                "attention_mask" => mask.clone(),
                "token_type_ids" => types.clone(),
                "position_ids" => positions.clone(),
                other => return Err(SemanticError::Model(format!("unexpected model input `{other}`"))),
            };
            feed.push((name.clone(), Tensor::from_array((shape, data)).map_err(SemanticError::ort)?.into_dyn()));
        }

        let mut session = self.session.lock().expect("embedder session poisoned");
        let outputs = session.run(feed).map_err(SemanticError::ort)?;
        let mut vectors = match &self.cfg.pooling {
            Pooling::Output(name) => {
                let out = outputs.get(name).ok_or_else(|| SemanticError::Model(format!("model has no output `{name}`")))?;
                let (shape, data) = out.try_extract_tensor::<f32>().map_err(SemanticError::ort)?;
                let d = shape[shape.len() - 1] as usize;
                (0..n).map(|r| data[r * d..(r + 1) * d].to_vec()).collect::<Vec<_>>()
            }
            pooling => {
                let out = outputs
                    .get("last_hidden_state")
                    .ok_or_else(|| SemanticError::Model("model has no `last_hidden_state` output".into()))?;
                let (shape, data) = out.try_extract_tensor::<f32>().map_err(SemanticError::ort)?;
                let (t, d) = (shape[1] as usize, shape[2] as usize);
                let row = |r: usize, j: usize| &data[(r * t + j) * d..(r * t + j + 1) * d];
                (0..n)
                    .map(|r| match pooling {
                        Pooling::Cls => row(r, 0).to_vec(),
                        Pooling::LastToken => row(r, lengths[r].saturating_sub(1)).to_vec(),
                        _ => {
                            let mut v = vec![0f32; d];
                            for j in 0..lengths[r] {
                                for (a, b) in v.iter_mut().zip(row(r, j)) {
                                    *a += b;
                                }
                            }
                            let k = lengths[r].max(1) as f32;
                            v.iter_mut().for_each(|a| *a /= k);
                            v
                        }
                    })
                    .collect()
            }
        };
        drop(outputs);
        drop(session);
        for v in &mut vectors {
            if self.cfg.truncate_dims > 0 && self.cfg.truncate_dims < v.len() {
                v.truncate(self.cfg.truncate_dims);
            }
            normalize(v);
        }
        Ok(vectors)
    }
}

pub(crate) fn threads(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }
    std::thread::available_parallelism().map(|n| (n.get() / 2).max(1)).unwrap_or(1)
}
