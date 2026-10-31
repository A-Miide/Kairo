//! Part of the Core ML encoder's weights kept int8 on the CPU, by rewriting the model's program
//! (`model.mil`, 1 MB) where it's installed. Its weights (`weight.bin`, 590 MB) are untouched and
//! nothing is downloaded.
//!
//! Why: Core ML compiles a model for the CPU with every compressed weight expanded to 16 bits
//! (int8 and int4, per channel or per block, and palettes all alike: macOS 27.2, 2026-10-31), so
//! the encoder's 590 MB of int8 becomes 1.1 GB that the engine maps and locks (main.swift,
//! pinCompiledModel). Here the weights the rewrite covers are written as ordinary operations
//! instead: the int8 weight, gathered through indices that depend on an input (so the compiler
//! can't fold it to 16 bits ahead of time), cast as each chunk is decoded, multiplied in and scaled
//! after. kairo-eval's `compact-encoder` writes it for other cut points, to measure.
//!
//! Which weights: every feed-forward module's first linear and the second in layers 0-11, 72 of
//! the 193 int8 linears. Measured on an 8 GB M1, macOS 27.2 (2026-10-31), the dev sermons through
//! offline-transcribe, replayed on the 4.4.7 detector (found of 82 / wrong):
//!   as published:            1156 MB locked, 7.3x real time, 66 / 2
//!   these 72:                 868 MB,        5.6-5.7x,       67 / 2
//!   feed-forward first (48):  964 MB,        5.6-6.1x,       67 / 2
//!   all feed-forward (96):    773 MB,        5.3-5.4x,       66 / 3
//!   all linears (193):        673 MB,        4.3-4.7x,       65 / 3
//! Word error against Deepgram the same for all. Held out (shiloh2025-love-mystery, scored once):
//! these 72 found 23/30, 3 wrong, as published 24/30, 3 wrong, word error 10.41% both.
//!
//! Only the exact file published (installer.rs COREML_EN) is rewritten, only into the exact file
//! measured, and only once this Mac has checked it: Core ML's compiler differs between macOS
//! releases, and an encoder that loads but computes wrong passes FluidAudio's load check (output
//! not all zero). So the rewrite is first put together beside the encoder (its weights linked, not
//! copied), and the engine runs both on the same input (main.swift, compareEncoders); it goes in
//! only if they agree. The published one is kept beside it and comes back if the engine can't load
//! the rewrite.

use std::path::{Path, PathBuf};

use regex::Regex;
use sha2::{Digest, Sha256};

/// `model.mil` as published (installer.rs COREML_EN), and as rewritten.
const PUBLISHED: &str = "4b903ee4f401d81add07d1252192a486bd07154174818902f03bca38b2f293f6";
const REWRITTEN: &str = "dfc81adc55fefd8bc96b1e87142bf2fe85096b66b97bf0c6733373c3834f7c1a";

/// The weights rewritten.
pub const SHIPPED: &str = r"feed_forward._linear1|module_layers_([0-9]|1[01])_feed_forward._linear2";

/// The engine's comparison of the two encoders takes 4-8 s on an 8 GB M1 (compiling both, if
/// they aren't yet); past this something is wrong, and it's tried again at the next launch.
const COMPARE_WAITS: std::time::Duration = std::time::Duration::from_secs(300);

/// The Core ML engine's compiled models (named after its program, kairo-coreml-speech): the
/// published encoder's 1.1 GB is no use once it's rewritten.
const COMPILED: &str = "Library/Caches/kairo-coreml-speech/com.apple.e5rt.e5bundlecache";

fn program(dir: &Path) -> PathBuf {
    dir.join("encoder/encoder_int8.mlmodelc/model.mil")
}

/// The published program, kept outside the compiled model's folder.
fn published(dir: &Path) -> PathBuf {
    dir.join("encoder/model.mil.published")
}

/// The rewrite put together beside the encoder, for the engine to compare.
fn trial(dir: &Path) -> PathBuf {
    dir.join("encoder/rewritten.mlmodelc")
}

/// Written when the engine found the rewrite computing differently, or the published encoder
/// loaded where the rewrite didn't: it isn't tried again.
fn declined(dir: &Path) -> PathBuf {
    dir.join("encoder/.kairo-rewrite-declined")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Puts `bytes` at `path` whole or not at all: written beside the model's folder, then renamed.
fn replace(dir: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join("encoder/model.mil.partial");
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// What this Mac made of the rewrite, run beside the published encoder.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// The same outputs, within the engine's tolerance.
    Agrees,
    /// Different outputs: this Mac keeps the published encoder.
    Differs,
    /// No answer (the engine missing, killed, too slow): asked again at the next launch.
    Unknown,
}

/// Asks the engine (`exe`) how far apart the two encoders' outputs are (main.swift, `--compare`).
pub fn compare_with_engine(exe: &Path, published: &Path, rewritten: &Path) -> Verdict {
    let Ok(mut child) = std::process::Command::new(exe)
        .arg("--compare")
        .args([published, rewritten])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return Verdict::Unknown;
    };
    let deadline = std::time::Instant::now() + COMPARE_WAITS;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let mut said = String::new();
    if let Some(mut out) = child.stdout.take() {
        use std::io::Read;
        let _ = out.read_to_string(&mut said);
    }
    eprintln!("[Offline] the rewritten encoder against the published one: {}", said.trim().strip_prefix("K1 compare ").unwrap_or("no answer"));
    match status.and_then(|s| s.code()) {
        Some(0) => Verdict::Agrees,
        Some(1) => Verdict::Differs,
        _ => Verdict::Unknown,
    }
}

/// The compiled encoder at `from` again at `to`, with `program` for its program: the weights
/// hard-linked (the same disk space), the rest copied.
fn put_together(from: &Path, to: &Path, program: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            put_together(&src, &dst, program)?;
        } else if entry.file_name() == "model.mil" {
            std::fs::write(&dst, program)?;
        } else if entry.file_name() == "weight.bin" {
            std::fs::hard_link(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// Rewrites the encoder in `dir` if it's the published one, the rewrite wasn't declined here, and
/// `compare` (the engine's, [`compare_with_engine`]) finds the rewrite computing the same: whether
/// it did.
pub fn rewrite_installed(dir: &Path, compare: impl FnOnce(&Path, &Path) -> Verdict) -> bool {
    let path = program(dir);
    let Ok(text) = std::fs::read_to_string(&path) else { return false };
    if sha256(text.as_bytes()) != PUBLISHED || declined(dir).exists() {
        return false;
    }
    let out = rewrite(&text);
    if sha256(out.as_bytes()) != REWRITTEN {
        eprintln!("[Offline] the encoder's rewrite didn't come out as measured: left as published");
        return false;
    }
    let installed = path.parent().unwrap_or(dir);
    let _ = std::fs::remove_dir_all(trial(dir));
    let verdict = match put_together(installed, &trial(dir), &out) {
        Ok(()) => compare(installed, &trial(dir)),
        Err(e) => {
            eprintln!("[Offline] the rewritten encoder couldn't be put together to check: {e}");
            Verdict::Unknown
        }
    };
    let _ = std::fs::remove_dir_all(trial(dir));
    // Compiled for the comparison, under the trial's path: no use now either way.
    forget_compiled();
    match verdict {
        Verdict::Agrees => {}
        Verdict::Differs => {
            eprintln!("[Offline] the rewritten encoder computes differently on this Mac: the published one stays");
            decline(dir);
            return false;
        }
        Verdict::Unknown => return false,
    }
    if let Err(e) = replace(dir, &published(dir), text.as_bytes()).and_then(|()| replace(dir, &path, out.as_bytes())) {
        eprintln!("[Offline] the encoder couldn't be rewritten: {e}");
        return false;
    }
    eprintln!("[Offline] the encoder's feed-forward weights now stay int8 (about 290 MB less memory)");
    true
}

/// The published encoder back, whatever is in its place (the rewrite, or a file cut short): whether
/// it was put back.
pub fn restore(dir: &Path) -> bool {
    let Ok(text) = std::fs::read(published(dir)) else { return false };
    if sha256(&text) != PUBLISHED || std::fs::read(program(dir)).is_ok_and(|b| b == text) {
        return false;
    }
    if let Err(e) = replace(dir, &program(dir), &text) {
        eprintln!("[Offline] the published encoder couldn't be put back: {e}");
        return false;
    }
    forget_compiled();
    true
}

/// The rewrite isn't tried again in `dir`: the published encoder loaded where it didn't.
pub fn decline(dir: &Path) {
    let _ = std::fs::write(declined(dir), "");
}

/// Core ML compiles the model again at the next load: a few seconds on the CPU. (Not from tests:
/// they'd take this Mac's.)
fn forget_compiled() {
    if cfg!(test) {
        return;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let _ = std::fs::remove_dir_all(Path::new(&home).join(COMPILED));
    }
}

/// The rewrite of the published `model.mil`: the weights in [`SHIPPED`].
pub fn rewrite(text: &str) -> String {
    rewrite_with(text, &Regex::new(SHIPPED).unwrap())
}

/// The rewrite of the int8 linears whose weight's name matches `only`.
pub fn rewrite_with(text: &str, only: &Regex) -> String {
    const BLOB: &str = r#"tensor<string, \[\]>\("[^"]+"\), offset = tensor<uint64, \[\]>\(\d+\)"#;
    let cx = Regex::new(&format!(
        r#"^(\s*)tensor<fp16, \[([0-9, ]+)\]> (\S+) = constexpr_affine_dequantize\(\)\[axis = tensor<int32, \[\]>\((-?\d+)\), name = tensor<string, \[\]>\("[^"]+"\), quantized_data = (tensor<int8, \[[0-9, ]+\]>\(BLOBFILE\(path = {BLOB}\)\)), scale = (tensor<fp16, \[\d+\]>\(BLOBFILE\(path = {BLOB}\)\)), zero_point = (tensor<int8, \[\d+\]>\(BLOBFILE\(path = {BLOB}\)\))\];$"#
    ))
    .unwrap();
    let lin = Regex::new(r#"^(\s*)tensor<fp16, \[([0-9, ]+)\]> (\S+) = linear\(bias = (\S+), weight = (\S+), x = (\S+)\)\[name = tensor<string, \[\]>\("([^"]+)"\)\];$"#).unwrap();
    let def = Regex::new(r"^\s*tensor<fp16, \[([0-9, ]+)\]> (\S+) = ").unwrap();

    struct Weight {
        rows: usize,
        cols: usize,
        data: String,
        scale: String,
    }
    let lines: Vec<&str> = text.split('\n').collect();
    // The published weights are symmetric int8 along their rows (every zero point 0): checked by
    // the hash, not here.
    let mut q = std::collections::HashMap::new();
    let mut shapes = std::collections::HashMap::new();
    for line in &lines {
        if let Some(m) = cx.captures(line) {
            let dims: Vec<usize> = m[2].split(',').filter_map(|d| d.trim().parse().ok()).collect();
            if &m[4] == "0" && dims.len() == 2 {
                q.insert(m[3].to_string(), Weight { rows: dims[0], cols: dims[1], data: m[5].to_string(), scale: m[6].to_string() });
            }
        }
        if let Some(m) = def.captures(line) {
            shapes.insert(m[2].to_string(), m[1].to_string());
        }
    }
    let used: std::collections::HashSet<String> = lines
        .iter()
        .filter_map(|l| lin.captures(l))
        .filter(|m| q.contains_key(&m[5]) && only.is_match(&m[5]) && shapes.contains_key(&m[6]))
        .map(|m| m[5].to_string())
        .collect();
    let rows: std::collections::BTreeSet<usize> = used.iter().map(|w| q[w].rows).collect();

    let s = |name: &str| format!(r#"tensor<string, []>("{name}")"#);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + used.len() * 10);
    let (mut n, mut started) = (0, false);
    for line in &lines {
        if cx.captures(line).is_some_and(|m| used.contains(&m[3])) {
            continue; // written where it's used
        }
        if !started && line.contains("= const()") {
            let ind = &line[..line.len() - line.trim_start().len()];
            // Zero, as far as the compiler knows: an input times zero. Indices built on it keep each
            // weight's gather, and so its int8, until the model runs.
            out.push(format!("{ind}tensor<int32, []> kq_axis0 = const()[name = {}, val = tensor<int32, []>(0)];", s("kq_axis0")));
            out.push(format!("{ind}tensor<int32, [1]> kq_zero = mul(x = cache_len, y = kq_axis0)[name = {}];", s("kq_zero")));
            out.push(format!("{ind}tensor<bool, []> kq_false = const()[name = {}, val = tensor<bool, []>(false)];", s("kq_false")));
            out.push(format!("{ind}tensor<bool, []> kq_true = const()[name = {}, val = tensor<bool, []>(true)];", s("kq_true")));
            out.push(format!(r#"{ind}tensor<string, []> kq_fp16 = const()[name = {}, val = tensor<string, []>("fp16")];"#, s("kq_fp16")));
            for r in &rows {
                let range = (0..*r).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
                out.push(format!("{ind}tensor<int32, [{r}]> kq_range_{r} = const()[name = {}, val = tensor<int32, [{r}]>([{range}])];", s(&format!("kq_range_{r}"))));
                out.push(format!("{ind}tensor<int32, [{r}]> kq_idx_{r} = add(x = kq_range_{r}, y = kq_zero)[name = {}];", s(&format!("kq_idx_{r}"))));
            }
            started = true;
        }
        if let Some(m) = lin.captures(line).filter(|m| used.contains(&m[5])) {
            let (ind, oshape, var, bias, w, x, name) = (&m[1], &m[2], &m[3], &m[4], &m[5], &m[6], &m[7]);
            let Weight { rows: o, cols: i, data, scale } = &q[w];
            let p = format!("kq_{n}");
            n += 1;
            // x · Wᵀ, scaled per output channel. x · q runs ~1/scale larger than x · w and overflows
            // 16 bits, so x is scaled by the layer's smallest scale first (no sum is larger than the
            // original's) and the rest after.
            out.push(format!("{ind}tensor<int8, [{o}, {i}]> {p}_q = const()[name = {}, val = {data}];", s(&format!("{p}_q"))));
            out.push(format!("{ind}tensor<fp16, [{o}]> {p}_s = const()[name = {}, val = {scale}];", s(&format!("{p}_s"))));
            out.push(format!("{ind}tensor<int8, [{o}, {i}]> {p}_g = gather(axis = kq_axis0, batch_dims = kq_axis0, indices = kq_idx_{o}, validate_indices = kq_false, x = {p}_q)[name = {}];", s(&format!("{p}_g"))));
            out.push(format!("{ind}tensor<fp16, [{o}, {i}]> {p}_w = cast(dtype = kq_fp16, x = {p}_g)[name = {}];", s(&format!("{p}_w"))));
            out.push(format!("{ind}tensor<fp16, []> {p}_c = reduce_min(keep_dims = kq_false, x = {p}_s)[name = {}];", s(&format!("{p}_c"))));
            out.push(format!("{ind}tensor<fp16, [{o}]> {p}_post = real_div(x = {p}_s, y = {p}_c)[name = {}];", s(&format!("{p}_post"))));
            out.push(format!("{ind}tensor<fp16, [{}]> {p}_x = mul(x = {x}, y = {p}_c)[name = {}];", shapes[x], s(&format!("{p}_x"))));
            out.push(format!("{ind}tensor<fp16, [{oshape}]> {p}_mm = matmul(transpose_x = kq_false, transpose_y = kq_true, x = {p}_x, y = {p}_w)[name = {}];", s(&format!("{p}_mm"))));
            out.push(format!("{ind}tensor<fp16, [{oshape}]> {p}_sc = mul(x = {p}_mm, y = {p}_post)[name = {}];", s(&format!("{p}_sc"))));
            out.push(format!("{ind}tensor<fp16, [{oshape}]> {var} = add(x = {p}_sc, y = {bias})[name = {}];", s(name)));
            continue;
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOB: &str = r#"BLOBFILE(path = tensor<string, []>("@model_path/weights/weight.bin"), offset = tensor<uint64, []>"#;

    fn weight(name: &str, o: usize, i: usize, at: usize) -> String {
        format!(
            r#"            tensor<fp16, [{o}, {i}]> {name} = constexpr_affine_dequantize()[axis = tensor<int32, []>(0), name = tensor<string, []>("{name}"), quantized_data = tensor<int8, [{o}, {i}]>({BLOB}({at}))), scale = tensor<fp16, [{o}]>({BLOB}({}))), zero_point = tensor<int8, [{o}]>({BLOB}(64)))];"#,
            at + 512
        )
    }

    fn linear(var: &str, w: &str, x: &str, o: usize) -> String {
        format!(r#"            tensor<fp16, [1, 7, {o}]> {var} = linear(bias = b_{o}, weight = {w}, x = {x})[name = tensor<string, []>("{var}")];"#)
    }

    /// A program in the published one's form, small: one feed-forward weight the rewrite covers,
    /// one of a later layer's second linear and one of attention, which it doesn't.
    fn small() -> String {
        [
            "program(1.0)".to_string(),
            "{".into(),
            "    func main<ios17>(tensor<int32, [1]> cache_len, tensor<fp32, [1, 7, 8]> mel) {".into(),
            r#"            tensor<string, []> dt = const()[name = tensor<string, []>("dt"), val = tensor<string, []>("fp16")];"#.into(),
            r#"            tensor<fp16, [1, 7, 8]> x_1 = cast(dtype = dt, x = mel)[name = tensor<string, []>("x_1")];"#.into(),
            weight("module_layers_3_feed_forward1_linear1_weight_to_fp16_quantized", 4, 8, 128),
            weight("module_layers_20_feed_forward2_linear2_weight_to_fp16_quantized", 8, 8, 1024),
            weight("module_layers_3_self_attn_linear_q_weight_to_fp16_quantized", 8, 8, 2048),
            linear("y_1", "module_layers_3_feed_forward1_linear1_weight_to_fp16_quantized", "x_1", 4),
            linear("y_2", "module_layers_20_feed_forward2_linear2_weight_to_fp16_quantized", "x_1", 8),
            linear("y_3", "module_layers_3_self_attn_linear_q_weight_to_fp16_quantized", "x_1", 8),
            "    } -> (y_1, y_2, y_3);".into(),
            "}".into(),
        ]
        .join("\n")
    }

    #[test]
    fn rewrites_only_the_weights_chosen() {
        let src = small();
        let out = rewrite(&src);
        // The covered weight: an int8 const from the same blob, gathered, cast, multiplied and scaled.
        assert!(!out.contains("module_layers_3_feed_forward1_linear1_weight_to_fp16_quantized = constexpr"));
        assert!(out.contains(&format!("tensor<int8, [4, 8]> kq_0_q = const()[name = tensor<string, []>(\"kq_0_q\"), val = tensor<int8, [4, 8]>({BLOB}(128)))];")));
        assert!(out.contains("kq_0_g = gather(axis = kq_axis0, batch_dims = kq_axis0, indices = kq_idx_4,"));
        assert!(out.contains("tensor<fp16, [1, 7, 8]> kq_0_x = mul(x = x_1, y = kq_0_c)"));
        assert!(out.contains(r#"tensor<fp16, [1, 7, 4]> y_1 = add(x = kq_0_sc, y = b_4)[name = tensor<string, []>("y_1")];"#));
        // Indices for its rows only, before the first constant; the zero comes from an input.
        let idx = out.find("kq_idx_4 = add(x = kq_range_4, y = kq_zero)").unwrap();
        assert!(idx < out.find("dt = const()").unwrap() && !out.contains("kq_idx_8"));
        assert!(out.contains("kq_zero = mul(x = cache_len, y = kq_axis0)"));
        // The rest as they were.
        for line in src.lines().filter(|l| l.contains("layers_20_") || l.contains("self_attn") || l.contains("y_2") || l.contains("y_3")) {
            assert!(out.contains(line), "changed: {line}");
        }
        // Everything chosen, with the second weight's rows too.
        let all = rewrite_with(&src, &Regex::new("feed_forward").unwrap());
        assert!(all.contains("kq_idx_8") && all.contains("kq_1_q") && !all.contains("kq_2_q"));
    }

    /// A compiled model's folder in the published one's form, made up: its program, its weights.
    fn model_folder(dir: &Path, program_text: &[u8]) {
        let encoder = program(dir).parent().unwrap().to_path_buf();
        std::fs::create_dir_all(encoder.join("weights")).unwrap();
        std::fs::create_dir_all(encoder.join("analytics")).unwrap();
        std::fs::write(program(dir), program_text).unwrap();
        std::fs::write(encoder.join("coremldata.bin"), b"interface").unwrap();
        std::fs::write(encoder.join("analytics/coremldata.bin"), b"analytics").unwrap();
        std::fs::write(encoder.join("weights/weight.bin"), b"weights").unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kairo-coreml-encoder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_trial_is_the_encoder_with_its_weights_linked() {
        let dir = scratch("trial");
        model_folder(&dir, b"published");
        let from = program(&dir).parent().unwrap().to_path_buf();
        put_together(&from, &trial(&dir), "rewritten").unwrap();
        let t = trial(&dir);
        assert_eq!(std::fs::read_to_string(t.join("model.mil")).unwrap(), "rewritten");
        assert_eq!(std::fs::read(t.join("analytics/coremldata.bin")).unwrap(), b"analytics");
        use std::os::unix::fs::MetadataExt;
        let (a, b) = (std::fs::metadata(from.join("weights/weight.bin")).unwrap(), std::fs::metadata(t.join("weights/weight.bin")).unwrap());
        assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()), "weights copied, not linked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_engines_answer_is_read_from_how_it_ends() {
        let dir = scratch("answer");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = |name: &str, body: &str| {
            let exe = dir.join(name);
            std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            exe
        };
        let ask = |exe: &Path| compare_with_engine(exe, Path::new("a"), Path::new("b"));
        assert_eq!(ask(&engine("agrees", "echo 'K1 compare 0.02'; exit 0")), Verdict::Agrees);
        assert_eq!(ask(&engine("differs", "echo 'K1 compare 2.1'; exit 1")), Verdict::Differs);
        // Couldn't load a model (2), killed, or not there: no answer.
        assert_eq!(ask(&engine("failed", "exit 2")), Verdict::Unknown);
        assert_eq!(ask(&engine("killed", "kill -9 $$")), Verdict::Unknown);
        assert_eq!(ask(&dir.join("missing")), Verdict::Unknown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_files_are_left_alone() {
        let dir = scratch("other");
        model_folder(&dir, b"something else");
        // Not the published program: never compared, left alone, nothing to restore.
        assert!(!rewrite_installed(&dir, |_, _| panic!("compared")) && !restore(&dir));
        // A published copy beside it that isn't the real one: not put back.
        std::fs::write(published(&dir), b"not the published program").unwrap();
        assert!(!restore(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With the model (KAIRO_OFFLINE_MODEL, else models/coreml-nemotron-en in the source tree, as
    /// offline-transcribe finds it): the rewrite is the file measured, goes in only on the engine's
    /// say-so, and restores and declines as it should; with the engine built (src-tauri/bin), the
    /// engine's own comparison agrees. Needs the model, so it runs only when asked (`-- --ignored`).
    #[test]
    #[ignore = "needs the Core ML model: KAIRO_OFFLINE_MODEL=<model folder> cargo test -p kairo-server -- --ignored"]
    fn the_published_encoder_rewritten_is_the_one_measured() {
        let src = std::env::var_os("KAIRO_OFFLINE_MODEL").map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../models/coreml-nemotron-en"), PathBuf::from);
        let text = std::fs::read(program(&src)).expect("the Core ML model: set KAIRO_OFFLINE_MODEL");
        assert_eq!(sha256(&text), PUBLISHED);
        assert_eq!(sha256(rewrite(std::str::from_utf8(&text).unwrap()).as_bytes()), REWRITTEN);

        // The installed encoder, its weights linked.
        let dir = scratch("real");
        put_together(program(&src).parent().unwrap(), program(&dir).parent().unwrap(), std::str::from_utf8(&text).unwrap()).unwrap();
        let is = |sha: &str| sha256(&std::fs::read(program(&dir)).unwrap()) == sha;
        // No answer from the engine: left as published, asked again next time.
        assert!(!rewrite_installed(&dir, |_, _| Verdict::Unknown) && is(PUBLISHED) && !declined(&dir).exists() && !trial(&dir).exists());
        // It agrees: rewritten, once.
        assert!(rewrite_installed(&dir, |_, _| Verdict::Agrees) && !rewrite_installed(&dir, |_, _| Verdict::Agrees) && is(REWRITTEN));
        assert!(!dir.join("encoder/model.mil.partial").exists() && !trial(&dir).exists());
        // Cut short (a crash mid-write): the published one comes back whole.
        std::fs::write(program(&dir), &text[..1000]).unwrap();
        assert!(restore(&dir) && is(PUBLISHED));
        // It differs: left as published, and not tried again.
        assert!(!rewrite_installed(&dir, |_, _| Verdict::Differs) && is(PUBLISHED) && declined(&dir).exists());
        assert!(!rewrite_installed(&dir, |_, _| Verdict::Agrees));

        // The engine itself, as a copy of its own (its compiled models go in a cache folder named
        // after it, removed after).
        let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/kairo-coreml-speech");
        if built.exists() {
            let exe = dir.join("kairo-coreml-speech-test");
            std::fs::copy(&built, &exe).unwrap();
            let _ = std::fs::remove_file(declined(&dir));
            assert!(rewrite_installed(&dir, |a, b| compare_with_engine(&exe, a, b)) && is(REWRITTEN));
            if let Some(home) = std::env::var_os("HOME") {
                let _ = std::fs::remove_dir_all(Path::new(&home).join("Library/Caches/kairo-coreml-speech-test"));
            }
        } else {
            eprintln!("the engine isn't built (scripts/build-coreml-speech.sh): its comparison not tried");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
