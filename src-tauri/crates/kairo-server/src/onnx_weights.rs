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
//!
//! Better still, [`prepack`]: ONNX Runtime rearranges ("prepacks") each weight
//! into the layout its maths routines read fastest, and keeps that copy on the
//! heap. Saved once to the weights file, the prepacked copies are mapped too,
//! so decoding costs what it always did (turning prepacking off instead costs
//! about 15% more CPU).

use std::io::Write;
use std::path::{Path, PathBuf};

/// The weights file written beside `encoder.int8.onnx`.
pub const WEIGHTS: &str = "encoder.int8.weights";
/// Tensors smaller than this stay in the graph (scales, shapes, small constants).
const MIN_EXTERNAL: usize = 4096;
/// Each tensor starts on its own page in the weights file.
const ALIGN: u64 = 16384;

/// A graph with its weights beside it is a few MB; the encoder with its weights is 650.
const MAX_GRAPH: u64 = 64 << 20;

/// Whether `model` is a small graph whose weights live in a file beside it
/// ([`WEIGHTS`], or prepacked ones from [`prepack`]).
pub fn external(model: &Path) -> bool {
    let len = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
    let graph = len(model);
    graph > 0 && graph < MAX_GRAPH && weights_files(model).iter().any(|w| len(w) > graph)
}

/// Every `<model stem>.*weights` file beside `model`.
fn weights_files(model: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (model.parent(), model.file_stem().and_then(|s| s.to_str())) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(stem) && n.ends_with(".weights")))
        .map(|e| e.path())
        .collect()
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
        let mut out = Weights { file: Some(std::io::BufWriter::new(std::fs::File::create(&weights_tmp).map_err(|e| e.to_string())?)), at: 0, moved: 0 };
        let graph = model_proto(&map, &mut out).ok_or("the model file isn't a readable ONNX model")?;
        if out.moved == 0 {
            return Err("the model has no weights to move".into());
        }
        out.file.take().ok_or("no weights file")?.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
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

/// What a prepacked weights file is only good for: prepacked bytes depend on the
/// ONNX Runtime build (this app version) and the processor (a model folder copied
/// to a new computer). When either changes, ONNX Runtime would quietly prepack
/// onto the heap again, so the weights are prepacked afresh.
pub fn stamp() -> String {
    format!("KAIRO {} {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH, cpu_name())
}

fn cpu_name() -> String {
    #[cfg(target_os = "macos")]
    let name = std::process::Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    #[cfg(windows)]
    let name = std::env::var("PROCESSOR_IDENTIFIER").ok();
    #[cfg(not(any(target_os = "macos", windows)))]
    let name = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|c| c.lines().find(|l| l.starts_with("model name")).map(str::to_string));
    name.map(|n| n.trim().to_string()).unwrap_or_default()
}

/// `encoder.int8.<unique>.weights`: a new name each time, so replacing the graph
/// is the one step that switches from one weights file to the next.
fn prepacked_name(model: &Path, stamp: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let unique = format!("{stamp} {} {now}", std::process::id());
    let hash: String = Sha256::digest(unique.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
    Some(format!("{}.{hash}.weights", model.file_stem()?.to_str()?))
}

/// `encoder.int8.prepacked`: the stamp, then the name of the weights file the graph uses.
fn stamp_path(model: &Path) -> Option<PathBuf> {
    Some(model.parent()?.join(format!("{}.prepacked", model.file_stem()?.to_str()?)))
}

/// Whether `model` is prepacked for this `stamp` (see [`prepack`]).
pub fn prepacked(model: &Path, stamp: &str) -> bool {
    let (Some(stamp_file), Some(dir)) = (stamp_path(model), model.parent()) else { return false };
    let Ok(saved) = std::fs::read_to_string(stamp_file) else { return false };
    let mut lines = saved.lines();
    lines.next() == Some(stamp) && lines.next().is_some_and(|name| dir.join(name).exists()) && external(model)
}

/// Replaces `model` with ONNX Runtime's optimized graph, its weights (as they
/// are and prepacked) in `encoder.int8.<stamp hash>.weights` beside it. ONNX
/// Runtime still reads the weights as they are when it loads (it prepacks each
/// one to recognise its saved copy), but those pages are mapped, and dropped
/// again by the system when memory is short. Costs about 530 MB more disk.
///
/// The graph is replaced last, in one step: a run cut short leaves the model
/// working as it was. Must run in the program that will load the model (the
/// prepacked bytes are this program's ONNX Runtime's).
pub fn prepack(model: &Path, stamp: &str) -> Result<(), String> {
    use ort::session::{builder::GraphOptimizationLevel, Session};
    let dir = model.parent().ok_or("model has no folder")?;
    let name = prepacked_name(model, stamp).ok_or("model has no name")?;
    let stamp_file = stamp_path(model).ok_or("model has no name")?;
    let work = dir.join(format!(".prepack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir(&work).map_err(|e| e.to_string())?;
    // Prepacking a model already prepacked once (another app version or computer): ONNX
    // Runtime would use the saved copies it recognises and save none, so it is given the
    // graph without them (beside the weights it refers to).
    let source = dir.join(format!(".prepack-source-{}.onnx", std::process::id()));
    let result = (|| {
        let graph = work.join(model.file_name().ok_or("model has no name")?);
        let from = if external(model) {
            let bytes = std::fs::read(model).map_err(|e| e.to_string())?;
            let mut w = Weights { file: None, at: 0, moved: 0 };
            let stripped = model_proto(&bytes, &mut w).ok_or("the model file isn't a readable ONNX model")?;
            std::fs::write(&source, stripped).map_err(|e| e.to_string())?;
            source.as_path()
        } else {
            model
        };
        // The session is dropped at the end of the statement, unmapping the old
        // weights before they're replaced (Windows can't replace a mapped file).
        let save = || -> ort::Result<()> {
            Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_intra_threads(1)?
                .with_optimized_model_path(&graph)?
                .with_config_entry("session.optimized_model_external_initializers_file_name", &name)?
                .with_config_entry("session.optimized_model_external_initializers_min_size_in_bytes", MIN_EXTERNAL.to_string())?
                .with_config_entry("session.save_external_prepacked_constant_initializers", "1")?
                .commit_from_file(from)?;
            Ok(())
        };
        save().map_err(|e| e.to_string())?;
        if !work.join(&name).exists() || !graph.exists() {
            return Err("ONNX Runtime saved no prepacked model".into());
        }
        std::fs::rename(work.join(&name), dir.join(&name)).map_err(|e| e.to_string())?;
        std::fs::rename(&graph, model).map_err(|e| e.to_string())?;
        std::fs::write(&stamp_file, format!("{stamp}\n{name}\n")).map_err(|e| e.to_string())?;
        // The weights the old graph used (still mapped by a running engine on Windows: left for next time).
        for w in weights_files(model).into_iter().filter(|w| !w.ends_with(&name)) {
            let _ = std::fs::remove_file(w);
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_file(&source);
    result
}

/// The weights the encoder reads while decoding, locked in memory (see [`pin`]);
/// unlocked when dropped.
#[cfg(target_os = "macos")]
pub struct Pinned {
    _map: memmap2::Mmap,
    pub bytes: u64,
}

/// Locks the weights `model` reads every chunk (its prepacked copies, and the
/// weights that have none) into memory, for as long as the result is kept.
///
/// Mapped from a file, the weights are clean pages: the first thing macOS takes
/// back when memory runs short, before it compresses anything, so the encoder
/// read them from the disk again every chunk. Measured on an 8 GB M1 under a live
/// test's memory pressure (2026-10-27, 5 minutes live): 3.3 million page faults
/// and four times the audio dropped of the weights on the heap; locked, the same
/// as on the heap, and locked pages can't be compressed or swapped either.
#[cfg(target_os = "macos")]
pub fn pin(model: &Path) -> Result<Pinned, String> {
    let graph = std::fs::read(model).map_err(|e| e.to_string())?;
    let (location, ranges) = hot_ranges(&graph).ok_or("the model keeps no weights beside it")?;
    let file = std::fs::File::open(model.parent().ok_or("model has no folder")?.join(location)).map_err(|e| e.to_string())?;
    // SAFETY: only read; a weights file is replaced, never written in place.
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| e.to_string())?;
    // SAFETY: sysconf has no preconditions.
    let page = (unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).max(4096) as u64;
    let mut bytes = 0;
    for (offset, len) in ranges {
        let start = offset / page * page;
        let end = offset.saturating_add(len).div_ceil(page).saturating_mul(page).min(map.len() as u64);
        if start >= end {
            return Err("a weight lies outside its file".into());
        }
        // SAFETY: start..end lies inside the mapping, which outlives the lock.
        if unsafe { libc::mlock(map.as_ptr().add(start as usize).cast(), (end - start) as usize) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        bytes += end - start;
    }
    Ok(Pinned { _map: map, bytes })
}

/// The weights file a graph's initializers live in, and the byte ranges in it
/// read while decoding: each prepacked copy (external_data "prepacked…":
/// `<key>|<offset>;<length>;<alignment>|…`), else the weight as it is.
fn hot_ranges(graph: &[u8]) -> Option<(String, Vec<(u64, u64)>)> {
    let model = fields(graph)?;
    let g = model.iter().find(|f| f.number == 7 && f.wire == 2)?.payload;
    let (mut location, mut ranges) = (None, Vec::new());
    for t in fields(g)?.iter().filter(|f| f.number == 5 && f.wire == 2) {
        let mut entries: Vec<(String, String)> = Vec::new();
        for e in fields(t.payload)?.iter().filter(|f| f.number == 13 && f.wire == 2) {
            let kv = fields(e.payload)?;
            let text = |n: u64| kv.iter().find(|f| f.number == n).map_or(String::new(), |f| String::from_utf8_lossy(f.payload).into_owned());
            entries.push((text(1), text(2)));
        }
        let get = |k: &str| entries.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        let Some(loc) = get("location") else { continue };
        location.get_or_insert_with(|| loc.to_string());
        let prepacked: Vec<&str> = entries.iter().filter(|(k, _)| k.starts_with("prepacked")).map(|(_, v)| v.as_str()).collect();
        if prepacked.is_empty() {
            ranges.push((get("offset").and_then(|v| v.parse().ok()).unwrap_or(0), get("length")?.parse().ok()?));
        }
        for blob in prepacked.iter().flat_map(|p| p.split('|').skip(1)) {
            let mut f = blob.split(';');
            ranges.push((f.next()?.parse().ok()?, f.next()?.parse().ok()?));
        }
    }
    Some((location?, ranges))
}

/// Where moved weights go; without a file, a rewrite only drops saved prepacked copies.
struct Weights {
    file: Option<std::io::BufWriter<std::fs::File>>,
    at: u64,
    moved: usize,
}

impl Weights {
    /// Appends one tensor's bytes, page-aligned; returns its offset.
    fn put(&mut self, bytes: &[u8]) -> Option<u64> {
        let file = self.file.as_mut()?;
        let pad = (ALIGN - self.at % ALIGN) % ALIGN;
        file.write_all(&vec![0; pad as usize]).ok()?;
        let offset = self.at + pad;
        file.write_all(bytes).ok()?;
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
    if w.file.is_none() {
        // external_data entries named "prepacked…" are ONNX Runtime's saved prepacked copies.
        let prepacked = |f: &Field| f.number == 13 && fields(f.payload).is_some_and(|e| e.iter().any(|k| k.number == 1 && k.payload.starts_with(b"prepacked")));
        if !fs.iter().any(prepacked) {
            return Some(buf.to_vec());
        }
        return Some(fs.iter().filter(|f| !prepacked(f)).flat_map(|f| f.whole.iter().copied()).collect());
    }
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

    #[test]
    fn finds_the_weights_read_while_decoding() {
        let entry = |k: &str, v: &str| {
            let mut e = Vec::new();
            put_bytes(&mut e, 1, k.as_bytes());
            put_bytes(&mut e, 2, v.as_bytes());
            e
        };
        let tensor = |entries: &[(&str, &str)]| {
            let mut t = Vec::new();
            for (k, v) in entries {
                put_bytes(&mut t, 13, &entry(k, v));
            }
            t
        };
        let mut graph = Vec::new();
        put_bytes(&mut graph, 5, &tensor(&[("location", "w.weights"), ("offset", "0"), ("length", "100"), ("prepacked_0", "Op+1|4096;50;0|8192;60;0")]));
        put_bytes(&mut graph, 5, &tensor(&[("location", "w.weights"), ("offset", "16384"), ("length", "70")]));
        put_bytes(&mut graph, 5, &tensor(&[]));
        let mut model = Vec::new();
        put_bytes(&mut model, 7, &graph);
        assert_eq!(hot_ranges(&model), Some(("w.weights".into(), vec![(4096, 50), (8192, 60), (16384, 70)])));
    }

    #[test]
    fn drops_saved_prepacked_copies_only() {
        let entry = |k: &str, v: &str| {
            let mut e = Vec::new();
            put_bytes(&mut e, 1, k.as_bytes());
            put_bytes(&mut e, 2, v.as_bytes());
            e
        };
        let mut t = Vec::new();
        put_bytes(&mut t, 8, b"w");
        put_bytes(&mut t, 13, &entry("location", "encoder.int8.1234.weights"));
        put_bytes(&mut t, 13, &entry("prepacked_0", "MatMul+1|0;64;0"));
        put_varint(&mut t, 14 << 3);
        put_varint(&mut t, 1);
        let mut graph = Vec::new();
        put_bytes(&mut graph, 5, &t);
        let mut model = Vec::new();
        put_bytes(&mut model, 7, &graph);

        let out = model_proto(&model, &mut Weights { file: None, at: 0, moved: 0 }).unwrap();
        let g = fields(fields(&out).unwrap()[0].payload).unwrap();
        let kept: Vec<&[u8]> = fields(g[0].payload).unwrap().iter().filter(|f| f.number == 13).map(|f| f.payload).collect();
        assert_eq!(kept, [&entry("location", "encoder.int8.1234.weights")[..]]);
    }
}
