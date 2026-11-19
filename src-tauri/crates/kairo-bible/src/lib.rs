//! Kairo's Bible data: canonical books, the one verse-cleaning rule, and the
//! corpus every detection path and the semantic index share.

pub mod book;
pub mod corpus;
pub mod text;

pub use book::Book;
pub use corpus::{Corpus, Translation, VerseId, VerseRef};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum BibleError {
    #[error("reading {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {detail}")]
    Format { path: PathBuf, detail: String },
}
