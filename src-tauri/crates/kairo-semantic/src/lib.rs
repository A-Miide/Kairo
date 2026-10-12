//! Meaning-based verse search: an embedder for any ONNX embedding model, a
//! quantized one-vector-per-verse index (optionally fused across
//! translations), and a cross-encoder reranker for the shortlist.

pub mod build;
pub mod embedder;
pub mod index;
pub mod reranker;
pub mod service;

pub use build::Builder;
pub use embedder::{Embedder, EmbedderConfig, Pooling};
pub use index::{Hit, IndexMeta, VerseIndex};
pub use reranker::{Reranker, RerankerConfig};
pub use service::{Found, MeaningService};

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SemanticError {
    #[error("onnx runtime: {0}")]
    Ort(String),
    #[error("tokenizer: {0}")]
    Tokenizer(String),
    #[error("model: {0}")]
    Model(String),
    #[error("index: {0}")]
    Index(String),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
}

impl SemanticError {
    pub(crate) fn ort<E: std::fmt::Display>(e: E) -> Self {
        SemanticError::Ort(e.to_string())
    }

    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        SemanticError::Io { path: path.to_path_buf(), source }
    }
}

/// Scales a vector to unit length (left as is if it's all zeros).
pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}
