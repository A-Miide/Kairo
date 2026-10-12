//! Builds the verse index: every verse embedded in each chosen translation,
//! and a verse's vectors fused into one (the mean of its unit vectors,
//! renormalized). KJV wording and a modern translation's wording of the same
//! verse land near each other, so the fused vector sits where a preacher's
//! own words for it are likely to fall.
//!
//! Each translation's raw vectors are cached as float32, so trying another
//! fusion doesn't re-embed the Bible.

use std::path::{Path, PathBuf};

use kairo_bible::{Corpus, Translation};

use crate::embedder::Embedder;
use crate::index::{IndexMeta, VerseIndex};
use crate::{normalize, SemanticError};

const BATCH: usize = 32;

pub struct Builder<'a> {
    pub embedder: &'a Embedder,
    pub corpus: &'a Corpus,
    /// Where per-translation float32 vectors are cached.
    pub cache_dir: PathBuf,
}

impl Builder<'_> {
    /// Unit vectors for every verse of one translation (`None` where the
    /// translation has no text), from the cache when present.
    pub fn translation_vectors(
        &self,
        t: &Translation,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<Vec<Option<Vec<f32>>>, SemanticError> {
        let path = self.cache_path(&t.code);
        if let Some(v) = read_cache(&path, self.corpus.len(), self.embedder.dims())? {
            return Ok(v);
        }
        let ids: Vec<_> = self.corpus.ids().filter(|id| t.text(*id).is_some()).collect();
        let mut out: Vec<Option<Vec<f32>>> = vec![None; self.corpus.len()];
        for (done, chunk) in ids.chunks(BATCH).enumerate() {
            let texts: Vec<&str> = chunk.iter().map(|id| t.text(*id).unwrap()).collect();
            for (id, v) in chunk.iter().zip(self.embedder.embed_documents(&texts)?) {
                out[id.index()] = Some(v);
            }
            progress(((done + 1) * BATCH).min(ids.len()), ids.len());
        }
        write_cache(&path, &out, self.embedder.dims())?;
        Ok(out)
    }

    /// The fused index over `translations` (the first is the anchor; every
    /// verse has it).
    pub fn build(
        &self,
        translations: &[&Translation],
        mut progress: impl FnMut(&str, usize, usize),
    ) -> Result<VerseIndex, SemanticError> {
        let dims = self.embedder.dims();
        let mut fused = vec![vec![0f32; dims]; self.corpus.len()];
        for t in translations {
            let vecs = self.translation_vectors(t, |d, n| progress(&t.code, d, n))?;
            for (acc, v) in fused.iter_mut().zip(vecs) {
                if let Some(v) = v {
                    acc.iter_mut().zip(&v).for_each(|(a, b)| *a += b);
                }
            }
        }
        for v in &mut fused {
            normalize(v);
        }
        let meta = IndexMeta {
            embedder: self.embedder.config().name.clone(),
            dims,
            count: self.corpus.len(),
            translations: translations.iter().map(|t| t.code.clone()).collect(),
            built_at: now_utc(),
        };
        Ok(VerseIndex::from_vectors(meta, &fused))
    }

    fn cache_path(&self, code: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.{}.f32", self.embedder.config().name, code))
    }
}

/// Cache file: per verse a presence byte, then `dims` little-endian floats if present.
fn write_cache(path: &Path, vecs: &[Option<Vec<f32>>], dims: usize) -> Result<(), SemanticError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| SemanticError::io(dir, e))?;
    }
    let mut bytes = Vec::with_capacity(vecs.len() * (1 + dims * 4));
    for v in vecs {
        match v {
            Some(v) => {
                bytes.push(1);
                v.iter().for_each(|x| bytes.extend_from_slice(&x.to_le_bytes()));
            }
            None => bytes.push(0),
        }
    }
    std::fs::write(path, bytes).map_err(|e| SemanticError::io(path, e))
}

fn read_cache(path: &Path, count: usize, dims: usize) -> Result<Option<Vec<Option<Vec<f32>>>>, SemanticError> {
    let Ok(bytes) = std::fs::read(path) else { return Ok(None) };
    let mut out = Vec::with_capacity(count);
    let mut i = 0;
    while i < bytes.len() && out.len() < count {
        let present = bytes[i] == 1;
        i += 1;
        if present {
            let end = i + dims * 4;
            if end > bytes.len() {
                return Ok(None);
            }
            out.push(Some(bytes[i..end].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()));
            i = end;
        } else {
            out.push(None);
        }
    }
    Ok((out.len() == count && i == bytes.len()).then_some(out))
}

fn now_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}
