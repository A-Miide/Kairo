//! One vector per verse, stored as int8 with a per-verse scale: a quarter of
//! float32's size (91 MB → ~23 MB for 768 dims) with cosine scores that move
//! by well under 0.01.
//!
//! Search is two passes, as the Node engine did: score every verse on the
//! leading `prefilter_dims` (Matryoshka models front-load meaning there), keep
//! a shortlist, then rescore the shortlist on every dimension.

use std::io::{Read, Write};
use std::path::Path;

use kairo_bible::VerseId;
use serde::{Deserialize, Serialize};

use crate::SemanticError;

const MAGIC: &[u8; 8] = b"KAIROVX1";

/// Recorded with the vectors so a query is only ever compared against vectors
/// made the same way.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IndexMeta {
    pub embedder: String,
    pub dims: usize,
    pub count: usize,
    /// Translations fused into each verse's vector, in order.
    pub translations: Vec<String>,
    pub built_at: String,
}

pub struct VerseIndex {
    meta: IndexMeta,
    scales: Vec<f32>,
    data: Vec<i8>,
    prefilter_dims: usize,
    /// norm of each verse's leading `prefilter_dims`, dequantized
    prefix_norms: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub verse: VerseId,
    pub score: f32,
}

const PREFILTER_DIMS: usize = 256;
const PREFILTER_KEEP: usize = 48;

impl VerseIndex {
    /// Quantizes unit vectors (one per verse, in `VerseId` order).
    pub fn from_vectors(meta: IndexMeta, vectors: &[Vec<f32>]) -> VerseIndex {
        let dims = meta.dims;
        let mut scales = Vec::with_capacity(vectors.len());
        let mut data = Vec::with_capacity(vectors.len() * dims);
        for v in vectors {
            assert_eq!(v.len(), dims, "vector width");
            let max = v.iter().fold(0f32, |m, x| m.max(x.abs())).max(f32::MIN_POSITIVE);
            scales.push(max / 127.0);
            data.extend(v.iter().map(|x| (x / max * 127.0).round().clamp(-127.0, 127.0) as i8));
        }
        VerseIndex::assemble(meta, scales, data)
    }

    fn assemble(meta: IndexMeta, scales: Vec<f32>, data: Vec<i8>) -> VerseIndex {
        let dims = meta.dims;
        let prefilter_dims = PREFILTER_DIMS.min(dims);
        let prefix_norms = (0..meta.count)
            .map(|i| {
                let row = &data[i * dims..i * dims + prefilter_dims];
                let s: f32 = row.iter().map(|&q| (q as f32) * (q as f32)).sum();
                (s.sqrt() * scales[i]).max(f32::MIN_POSITIVE)
            })
            .collect();
        VerseIndex { meta, scales, data, prefilter_dims, prefix_norms }
    }

    pub fn meta(&self) -> &IndexMeta {
        &self.meta
    }

    pub fn len(&self) -> usize {
        self.meta.count
    }

    pub fn is_empty(&self) -> bool {
        self.meta.count == 0
    }

    fn dot(&self, i: usize, q: &[f32], dims: usize) -> f32 {
        let row = &self.data[i * self.meta.dims..i * self.meta.dims + dims];
        let mut acc = 0f32;
        for (a, &b) in q[..dims].iter().zip(row) {
            acc += a * b as f32;
        }
        acc * self.scales[i]
    }

    /// The `limit` nearest verses to a unit query vector.
    pub fn search(&self, q: &[f32], limit: usize) -> Vec<Hit> {
        self.search_many(&[q.to_vec()], limit).pop().unwrap_or_default()
    }

    /// Several queries in one pass over the index.
    pub fn search_many(&self, queries: &[Vec<f32>], limit: usize) -> Vec<Vec<Hit>> {
        let p = self.prefilter_dims;
        let q_norms: Vec<f32> = queries
            .iter()
            .map(|q| q[..p].iter().map(|x| x * x).sum::<f32>().sqrt().max(f32::MIN_POSITIVE))
            .collect();
        let mut short: Vec<Vec<Hit>> = vec![Vec::with_capacity(PREFILTER_KEEP + 1); queries.len()];
        for i in 0..self.meta.count {
            for (k, q) in queries.iter().enumerate() {
                let cos = self.dot(i, q, p) / (self.prefix_norms[i] * q_norms[k]);
                push_top(&mut short[k], PREFILTER_KEEP, Hit { verse: VerseId(i as u32), score: cos });
            }
        }
        short
            .into_iter()
            .zip(queries)
            .map(|(list, q)| {
                let mut top = Vec::with_capacity(limit + 1);
                for h in list {
                    let score = self.dot(h.verse.index(), q, self.meta.dims);
                    push_top(&mut top, limit, Hit { verse: h.verse, score });
                }
                top
            })
            .collect()
    }

    /// Exact scores over just these verses — a passage or the chapters a named
    /// person appears in, where the Bible-wide top-K would miss the verse.
    pub fn search_within(&self, q: &[f32], verses: &[VerseId], limit: usize) -> Vec<Hit> {
        let mut top = Vec::with_capacity(limit + 1);
        for &v in verses {
            if v.index() < self.meta.count {
                push_top(&mut top, limit, Hit { verse: v, score: self.dot(v.index(), q, self.meta.dims) });
            }
        }
        top
    }

    /// Cosine of one verse to a query.
    pub fn score(&self, q: &[f32], verse: VerseId) -> f32 {
        self.dot(verse.index(), q, self.meta.dims)
    }

    pub fn save(&self, path: &Path) -> Result<(), SemanticError> {
        let meta = serde_json::to_vec(&self.meta).map_err(|e| SemanticError::Index(e.to_string()))?;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| SemanticError::io(path, e))?);
        let mut w = |b: &[u8]| f.write_all(b).map_err(|e| SemanticError::io(path, e));
        w(MAGIC)?;
        w(&(meta.len() as u32).to_le_bytes())?;
        w(&meta)?;
        for s in &self.scales {
            w(&s.to_le_bytes())?;
        }
        w(&self.data.iter().map(|&b| b as u8).collect::<Vec<u8>>())?;
        f.flush().map_err(|e| SemanticError::io(path, e))
    }

    pub fn load(path: &Path) -> Result<VerseIndex, SemanticError> {
        let mut f = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| SemanticError::io(path, e))?);
        let mut read = |n: usize| -> Result<Vec<u8>, SemanticError> {
            let mut b = vec![0u8; n];
            f.read_exact(&mut b).map_err(|e| SemanticError::io(path, e))?;
            Ok(b)
        };
        if read(8)? != MAGIC {
            return Err(SemanticError::Index(format!("{} is not a Kairo verse index", path.display())));
        }
        let meta_len = u32::from_le_bytes(read(4)?.try_into().unwrap()) as usize;
        let meta: IndexMeta = serde_json::from_slice(&read(meta_len)?).map_err(|e| SemanticError::Index(e.to_string()))?;
        let scales = read(meta.count * 4)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let data = read(meta.count * meta.dims)?.into_iter().map(|b| b as i8).collect();
        Ok(VerseIndex::assemble(meta, scales, data))
    }
}

/// Keeps `top` sorted best-first and at most `limit` long.
fn push_top(top: &mut Vec<Hit>, limit: usize, h: Hit) {
    if limit == 0 || (top.len() >= limit && h.score <= top[top.len() - 1].score) {
        return;
    }
    let pos = top.partition_point(|x| x.score >= h.score);
    top.insert(pos, h);
    top.truncate(limit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize;

    fn meta(count: usize, dims: usize) -> IndexMeta {
        IndexMeta { embedder: "test".into(), dims, count, translations: vec!["KJV".into()], built_at: String::new() }
    }

    fn unit(seed: u64, dims: usize) -> Vec<f32> {
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut v: Vec<f32> = (0..dims)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % 2001) as f32 / 1000.0 - 1.0
            })
            .collect();
        normalize(&mut v);
        v
    }

    #[test]
    fn int8_scores_track_float_scores() {
        let dims = 768;
        let vecs: Vec<_> = (0..500).map(|i| unit(i, dims)).collect();
        let idx = VerseIndex::from_vectors(meta(500, dims), &vecs);
        let q = unit(9999, dims);
        for (i, v) in vecs.iter().enumerate().step_by(37) {
            let exact: f32 = q.iter().zip(v).map(|(a, b)| a * b).sum();
            let approx = idx.score(&q, VerseId(i as u32));
            assert!((exact - approx).abs() < 0.01, "verse {i}: {exact} vs {approx}");
        }
    }

    #[test]
    fn a_verse_finds_itself_first() {
        let dims = 768;
        let vecs: Vec<_> = (0..2000).map(|i| unit(i, dims)).collect();
        let idx = VerseIndex::from_vectors(meta(2000, dims), &vecs);
        for i in [0usize, 17, 1999] {
            let hits = idx.search(&vecs[i], 5);
            assert_eq!(hits[0].verse, VerseId(i as u32));
            assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
        }
        let within = idx.search_within(&vecs[42], &[VerseId(1), VerseId(42), VerseId(7)], 2);
        assert_eq!(within[0].verse, VerseId(42));
        assert_eq!(within.len(), 2);
    }

    #[test]
    fn saves_and_loads() {
        let dims = 64;
        let vecs: Vec<_> = (0..10).map(|i| unit(i, dims)).collect();
        let idx = VerseIndex::from_vectors(meta(10, dims), &vecs);
        let path = std::env::temp_dir().join(format!("kairo-index-test-{}.kvx", std::process::id()));
        idx.save(&path).unwrap();
        let back = VerseIndex::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back.meta(), idx.meta());
        assert_eq!(back.search(&vecs[3], 1)[0].verse, VerseId(3));
    }
}
