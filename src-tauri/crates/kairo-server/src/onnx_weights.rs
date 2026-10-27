//! Moving an ONNX model's weights out of the graph into a file beside it
//! (ONNX "external data"), so ONNX Runtime maps them from disk instead of
//! copying them onto the heap.
//!
//! Why: sherpa-onnx hands ONNX Runtime the model's path and the runtime reads
//! the whole file into memory, so the offline encoder's 650 MB of weights
//! lived in anonymous memory. On an 8 GB church laptop playing video that
//! memory gets compressed and swapped (measured 2026-10-26: 728 MB footprint,
//! 54 MB resident), and a streaming transducer touches every weight every
//! chunk, so decoding fell behind. Mapped from a file, the weights are clean
//! pages: they don't count against the process, are never compressed or
//! written to swap, and are re-read from the disk only if the system drops them.
//!
//! The rewrite works on the protobuf wire format and touches nothing but the
//! tensors' `raw_data`: every other field is copied byte for byte, so the
//! graph is exactly the one sherpa-onnx shipped.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The weights file written beside `encoder.int8.onnx`.
pub const WEIGHTS: &str = "encoder.int8.weights";
/// Tensors smaller than this stay in the graph (scales, shapes, small constants).
const MIN_EXTERNAL: usize = 4096;
/// Each tensor starts on its own page in the weights file.
const ALIGN: u64 = 16384;

/// Whether `model` already keeps its weights in [`WEIGHTS`].
pub fn external(model: &Path) -> bool {
    // The weights file is written before the graph replaces the original, so a run cut
    // short in between leaves a model still larger than its (stale) weights file.
    let len = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
    weights_path(model).is_some_and(|w| len(&w) > len(model))
}

fn weights_path(model: &Path) -> Option<PathBuf> {
    Some(model.parent()?.join(WEIGHTS))
}

/// Rewrites `model` in place so its weights live in [`WEIGHTS`] beside it.
/// The original is replaced only once both new files are complete: an
/// interrupted run leaves the original model as it was.
pub fn externalize(model: &Path) -> Result<(), String> {
    let weights = weights_path(model).ok_or("model has no folder")?;
    let tmp = |p: &Path| {
        let mut name = p.as_os_str().to_owned();
        name.push(format!(".tmp-{}", std::process::id()));
        PathBuf::from(name)
    };
    let (graph_tmp, weights_tmp) = (tmp(model), tmp(&weights));
    let result = (|| {
        let src = std::fs::File::open(model).map_err(|e| e.to_string())?;
        // SAFETY: the model is only read, and nothing else writes it while KAIRO runs.
        let map = unsafe { memmap2::Mmap::map(&src) }.map_err(|e| e.to_string())?;
        let mut out = Weights { file: std::io::BufWriter::new(std::fs::File::create(&weights_tmp).map_err(|e| e.to_string())?), at: 0, moved: 0 };
        let graph = model_proto(&map, &mut out).ok_or("the model file isn't a readable ONNX model")?;
        if out.moved == 0 {
            return Err("the model has no weights to move".into());
        }
        out.file.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
        let mut g = std::fs::File::create(&graph_tmp).map_err(|e| e.to_string())?;
        g.write_all(&graph).and_then(|_| g.sync_all()).map_err(|e| e.to_string())?;
        drop(map); // Windows can't replace a file that is still mapped
        std::fs::rename(&weights_tmp, &weights).map_err(|e| e.to_string())?;
        std::fs::rename(&graph_tmp, model).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&graph_tmp);
        let _ = std::fs::remove_file(&weights_tmp);
    }
    result
}

struct Weights {
    file: std::io::BufWriter<std::fs::File>,
    at: u64,
    moved: usize,
}

impl Weights {
    /// Appends one tensor's bytes, page-aligned; returns its offset.
    fn put(&mut self, bytes: &[u8]) -> Option<u64> {
        let pad = (ALIGN - self.at % ALIGN) % ALIGN;
        self.file.write_all(&vec![0; pad as usize]).ok()?;
        let offset = self.at + pad;
        self.file.write_all(bytes).ok()?;
        self.at = offset + bytes.len() as u64;
        self.moved += 1;
        Some(offset)
    }
}

/// One field of a protobuf message: its number, wire type, the whole field's
/// bytes, and (for a length-delimited field) its payload.
struct Field<'a> {
    number: u64,
    wire: u64,
    whole: &'a [u8],
    payload: &'a [u8],
    /// A varint field's value.
    value: u64,
}

fn varint(buf: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*i)?;
        *i += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b < 0x80 {
            return Some(v);
        }
    }
    None
}

fn fields(buf: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        let start = i;
        let key = varint(buf, &mut i)?;
        let (number, wire) = (key >> 3, key & 7);
        let mut value = 0;
        let payload = match wire {
            0 => {
                value = varint(buf, &mut i)?;
                &buf[i..i]
            }
            1 | 5 => {
                i += if wire == 1 { 8 } else { 4 };
                buf.get(i..i)?
            }
            2 => {
                let len = usize::try_from(varint(buf, &mut i)?).ok()?;
                let p = buf.get(i..i.checked_add(len)?)?;
                i += len;
                p
            }
            _ => return None, // groups: not used by ONNX
        };
        out.push(Field { number, wire, whole: buf.get(start..i)?, payload, value });
    }
    Some(out)
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_bytes(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
    put_varint(out, number << 3 | 2);
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// Copies a message, rewriting the length-delimited fields `rewrite` names.
fn message(buf: &[u8], w: &mut Weights, rewrite: fn(u64) -> Option<Kind>) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(buf.len().min(1 << 20));
    for f in fields(buf)? {
        match (f.wire, rewrite(f.number)) {
            (2, Some(kind)) => {
                let inner = match kind {
                    Kind::Graph => graph_proto(f.payload, w)?,
                    Kind::Node => message(f.payload, w, node_fields)?,
                    Kind::Attribute => message(f.payload, w, attribute_fields)?,
                    Kind::Tensor => tensor_proto(f.payload, w)?,
                };
                put_bytes(&mut out, f.number, &inner);
            }
            _ => out.extend_from_slice(f.whole),
        }
    }
    Some(out)
}

#[derive(Clone, Copy)]
enum Kind {
    Graph,
    Node,
    Attribute,
    Tensor,
}

// Field numbers from onnx.proto.
fn model_proto(buf: &[u8], w: &mut Weights) -> Option<Vec<u8>> {
    message(buf, w, |n| (n == 7).then_some(Kind::Graph)) // ModelProto.graph
}
fn graph_proto(buf: &[u8], w: &mut Weights) -> Option<Vec<u8>> {
    message(buf, w, |n| match n {
        1 => Some(Kind::Node),   // GraphProto.node
        5 => Some(Kind::Tensor), // GraphProto.initializer
        _ => None,
    })
}
fn node_fields(n: u64) -> Option<Kind> {
    (n == 5).then_some(Kind::Attribute) // NodeProto.attribute
}
fn attribute_fields(n: u64) -> Option<Kind> {
    match n {
        5 | 10 => Some(Kind::Tensor), // AttributeProto.t, .tensors
        6 | 11 => Some(Kind::Graph),  // AttributeProto.g, .graphs
        _ => None,
    }
}

/// A tensor with its `raw_data` (field 9) moved out: `external_data` (13)
/// says where, `data_location` (14) = EXTERNAL.
fn tensor_proto(buf: &[u8], w: &mut Weights) -> Option<Vec<u8>> {
    let fs = fields(buf)?;
    let raw = fs.iter().find(|f| f.number == 9 && f.wire == 2).map(|f| f.payload);
    // data_location is often written out even when it is DEFAULT (0).
    let already = fs.iter().any(|f| f.number == 14 && f.wire == 0 && f.value == 1);
    let Some(raw) = raw.filter(|r| r.len() >= MIN_EXTERNAL && !already) else {
        return Some(buf.to_vec());
    };
    let offset = w.put(raw)?;
    let mut out = Vec::with_capacity(buf.len() - raw.len() + 96);
    for f in fs.iter().filter(|f| !matches!(f.number, 9 | 13 | 14)) {
        out.extend_from_slice(f.whole);
    }
    for (k, v) in [("location", WEIGHTS.to_string()), ("offset", offset.to_string()), ("length", raw.len().to_string())] {
        let mut entry = Vec::new();
        put_bytes(&mut entry, 1, k.as_bytes());
        put_bytes(&mut entry, 2, v.as_bytes());
        put_bytes(&mut out, 13, &entry);
    }
    put_varint(&mut out, 14 << 3);
    put_varint(&mut out, 1);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tensor(name: &str, raw: &[u8]) -> Vec<u8> {
        let mut t = Vec::new();
        put_varint(&mut t, 1 << 3); // dims = [len]
        put_varint(&mut t, raw.len() as u64);
        put_varint(&mut t, 2 << 3); // data_type = UINT8
        put_varint(&mut t, 2);
        put_bytes(&mut t, 8, name.as_bytes());
        put_bytes(&mut t, 9, raw);
        put_varint(&mut t, 14 << 3); // data_location = DEFAULT, written out as exporters do
        put_varint(&mut t, 0);
        t
    }

    #[test]
    fn moves_large_tensors_and_keeps_everything_else() {
        let big: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        let mut graph = Vec::new();
        put_bytes(&mut graph, 2, b"g"); // name
        put_bytes(&mut graph, 5, &tensor("w", &big));
        put_bytes(&mut graph, 5, &tensor("s", &[1, 2, 3]));
        let mut model = Vec::new();
        put_varint(&mut model, 1 << 3); // ir_version
        put_varint(&mut model, 8);
        put_bytes(&mut model, 7, &graph);

        let dir = std::env::temp_dir().join(format!("kairo-onnx-weights-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("encoder.int8.onnx");
        std::fs::write(&path, &model).unwrap();
        externalize(&path).unwrap();

        let out = std::fs::read(&path).unwrap();
        let weights = std::fs::read(dir.join(WEIGHTS)).unwrap();
        assert_eq!(&weights[..big.len()], &big[..]);
        let top = fields(&out).unwrap();
        assert_eq!(top[0].whole, &model[..2], "ir_version copied as is");
        let g = fields(top[1].payload).unwrap();
        let w = fields(g[1].payload).unwrap();
        assert!(!w.iter().any(|f| f.number == 9), "raw data moved out");
        let loc: Vec<u64> = w.iter().filter(|f| f.number == 14).map(|f| f.value).collect();
        assert_eq!(loc, [1], "marked external, once");
        assert!(external(&path), "seen as done");
        assert_eq!(g[2].payload, &tensor("s", &[1, 2, 3])[..], "small tensor kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
