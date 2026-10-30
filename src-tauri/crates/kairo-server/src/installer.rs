//! Downloading the offline speech models: resumable, retried, checked
//! against a known checksum, then unpacked into place — progress reported
//! line by line (NDJSON) as the UI expects. The translation models
//! (`mt.rs`) download through the same checked fetch.

use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::Shared;

struct Spec {
    url: &'static str,
    fallback: &'static str,
    sha256: &'static str,
    top_dir: &'static str,
    dir: &'static str,
    version: &'static str,
    archive: &'static str,
    approx_mb: u64,
}

const EN: Spec = Spec {
    url: crate::kairo_download!("offline-model-v1/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2"),
    fallback: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2",
    sha256: "78e2b79fcf7271553a74402a76b771b09ea40117a39566a79f52235b23db6358",
    top_dir: "sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25",
    dir: "sherpa-streaming-en",
    version: "v1",
    archive: "sherpa-en.tar.bz2",
    approx_mb: 465,
};
const MULTI: Spec = Spec {
    url: crate::kairo_download!("offline-model-v1/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11.tar.bz2"),
    fallback: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11.tar.bz2",
    sha256: "c6bf5e0df765f9d5b43bc9e0536d4b4b3e7d40bdf5ecf13e45f134c51c05ae3a",
    top_dir: "sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11",
    dir: "sherpa-streaming-multi",
    version: "v1",
    archive: "sherpa-multi.tar.bz2",
    approx_mb: 475,
};
/// English on Apple Silicon: the same model as Core ML, for the Neural Engine (offline::coreml),
/// file by file from FluidAudio's conversion, pinned to one revision and checked file by file.
/// The mel front-end is computed in the engine, so the conversion's preprocessor isn't needed.
const COREML_EN: CoreMlSpec = CoreMlSpec {
    base: "https://huggingface.co/FluidInference/nemotron-speech-streaming-en-0.6b-coreml/resolve/e673531caa6d25ab7baf5a8c14c9b99ba1551838/nemotron_coreml_560ms/",
    dir: "coreml-nemotron-en",
    version: "v1",
    files: &[
        ("decoder.mlmodelc/analytics/coremldata.bin", 243, "bca0e343fa0d698de8bdb22c66fd377b79104f606b1bfffc90931fe2161029b8"),
        ("decoder.mlmodelc/coremldata.bin", 493, "4d0549ac72a38abdcbe7927f2b7d488386d6b96f1f5e837d93ba9e379de04f4e"),
        ("decoder.mlmodelc/model.mil", 13093, "8298f16d3926565b77732b549fa9c64fedebb2e5a29a76e33d0b2e970e93ae69"),
        ("decoder.mlmodelc/weights/weight.bin", 14429952, "d9758e49f9b2b9a0462c933256663d4174209739e62f0ddc14e4147bc50e7818"),
        ("decoder_joint.mlmodelc/analytics/coremldata.bin", 243, "d46ecf790a728d91b8962ec62e597980e288e66c775e45429bf734b54c658ffc"),
        ("decoder_joint.mlmodelc/coremldata.bin", 514, "d9a7f7cfece967a69dca59b3fc3cc7d1ecff93ad2815ac3ba6b47e057db22172"),
        ("decoder_joint.mlmodelc/model.mil", 17146, "517bb31cf87eb8aa80adaa8c3fee2df0fd7d7dd0c8791e615c5023886c39d572"),
        ("decoder_joint.mlmodelc/weights/weight.bin", 17876866, "96cac67a1352e45a2418284779dbd5efe5865159b7b92f328247e5ed31485f52"),
        ("encoder/encoder_int8.mlmodelc/analytics/coremldata.bin", 243, "ef51dbab5c6ce310ce1a537538a4df6b88f13ad5c45c0acaddca055503896daa"),
        ("encoder/encoder_int8.mlmodelc/coremldata.bin", 608, "864a357ae2eb7d6b38ced73d5f23c908096a256516afa498c6730778767ec404"),
        ("encoder/encoder_int8.mlmodelc/model.mil", 968920, "4b903ee4f401d81add07d1252192a486bd07154174818902f03bca38b2f293f6"),
        ("encoder/encoder_int8.mlmodelc/weights/weight.bin", 589057600, "8ced31411f7903eaec52de85e94c7847228f3e44028d8f0040800e5603943a40"),
        ("joint.mlmodelc/analytics/coremldata.bin", 243, "478804308534c7f9cab1030aa9ce19ddf672fcd15deba633fe3ed5da8f53b103"),
        ("joint.mlmodelc/coremldata.bin", 401, "46363626fd89730ade6e6461e8be808c7fade198fb0df63db83fb00ed033abb2"),
        ("joint.mlmodelc/model.mil", 5064, "73bcc51b58ff73c7620e31a2c32dc5ef4939e8ec2a77ec22242303422bfafba4"),
        ("joint.mlmodelc/weights/weight.bin", 3446978, "d1d45514a91da39f24e264712a9f8b5a2a90e9cbce04bad3f3f74242bea106d5"),
        ("metadata.json", 438, "e8d93e5afa1e5c87257f6e31a5c336058eeba7d6cacd36ba80fbf9758c3916dc"),
        ("tokenizer.json", 18374, "a87b5b8a0e0a989fa7db3121f279ce6af70e337feb55ab553b8d0658db70b9d3"),
    ],
};

struct CoreMlSpec {
    base: &'static str,
    dir: &'static str,
    version: &'static str,
    /// Path in the model folder, size, SHA-256.
    files: &'static [(&'static str, u64, &'static str)],
}

pub(crate) fn coreml_files() -> impl Iterator<Item = &'static str> {
    COREML_EN.files.iter().map(|f| f.0)
}

const ATTEMPTS: u32 = 6;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PRUNE: [&str; 2] = ["test_wavs", "README.md"];

static INSTALLING: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
/// How far each running install is (percent), for whoever asks — Settings
/// shows a download the tour started, not just "in progress".
static PROGRESS: std::sync::Mutex<Vec<(&'static str, i64)>> = std::sync::Mutex::new(Vec::new());

pub fn installing(model: &str) -> bool {
    INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&model)
}

/// A running install's last reported percent.
pub fn progress(model: &str) -> Option<i64> {
    PROGRESS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|(m, _)| *m == model).map(|(_, p)| *p)
}

fn set_progress(model: &'static str, pct: Option<i64>) {
    let mut p = PROGRESS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    p.retain(|(m, _)| *m != model);
    if let Some(pct) = pct {
        p.push((model, pct));
    }
}

pub(crate) type Progress = tokio::sync::mpsc::UnboundedSender<Value>;

/// Bytes received so far and the total, as a download runs.
pub(crate) type OnBytes<'a> = &'a (dyn Fn(u64, u64) + Send + Sync);

async fn download(client: &reqwest::Client, url: &str, dest: &Path, total: u64, on: OnBytes<'_>) -> Result<(), String> {
    let start = tokio::fs::metadata(dest).await.map_or(0, |m| m.len());
    if total > 0 && start >= total {
        on(total, total);
        return Ok(());
    }
    let mut req = client.get(url);
    if start > 0 {
        req = req.header(header::RANGE, format!("bytes={start}-"));
    }
    let mut resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let mut received = start;
    let mut file = if status == 206 {
        tokio::fs::OpenOptions::new().append(true).open(dest).await.map_err(|e| e.to_string())?
    } else if status == 200 {
        received = 0; // the server ignored the range: start over
        tokio::fs::File::create(dest).await.map_err(|e| e.to_string())?
    } else if status == 416 && start > 0 {
        return Ok(());
    } else {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(format!("HTTP {status}"));
    };
    let total = if total > 0 { total } else { received + resp.content_length().unwrap_or(0) };
    loop {
        let chunk = tokio::time::timeout(IDLE_TIMEOUT, resp.chunk()).await.map_err(|_| format!("Download stalled (no activity for {}s)", IDLE_TIMEOUT.as_secs()))?.map_err(|e| e.to_string())?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        on(received, total);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    on(received, total.max(received));
    Ok(())
}

/// A plain download of a file whose size isn't known in advance (no checksum:
/// the caller verifies it another way, e.g. ndi_runtime's signature checks).
pub(crate) async fn download_plain(client: &reqwest::Client, url: &str, dest: &Path, on: OnBytes<'_>) -> Result<(), String> {
    download(client, url, dest, 0, on).await
}

/// Downloads `url` to `dest` (resuming, retrying) and checks its SHA-256;
/// a file that doesn't match is deleted.
pub(crate) async fn fetch_verified(client: &reqwest::Client, url: &str, dest: &Path, total: u64, sha256: &str, on: OnBytes<'_>, progress: &Progress) -> Result<(), String> {
    let mut last_err = None;
    for attempt in 1..=ATTEMPTS {
        match download(client, url, dest, total, on).await {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                if attempt < ATTEMPTS {
                    let _ = progress.send(json!({ "phase": "retry", "attempt": attempt, "maxAttempts": ATTEMPTS, "error": e }));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                last_err = Some(e);
            }
        }
    }
    if let Some(e) = last_err {
        return Err(format!("Download failed after {ATTEMPTS} attempts: {e}"));
    }
    if sha256_file(dest).await? != sha256 {
        let _ = tokio::fs::remove_file(dest).await;
        return Err("The downloaded file did not match the expected one (checksum). Nothing was installed — try again.".into());
    }
    Ok(())
}

/// Download progress as the UI reads it: whole percents, each sent once.
pub(crate) fn percent_reporter(progress: Progress) -> impl Fn(u64, u64) + Send + Sync {
    let last = std::sync::atomic::AtomicI64::new(-1);
    move |received, total| {
        if total == 0 {
            return;
        }
        let pct = (received.min(total) * 100 / total) as i64;
        if last.swap(pct, std::sync::atomic::Ordering::Relaxed) != pct {
            let _ = progress.send(json!({ "phase": "download", "pct": pct, "received": received, "total": total }));
        }
    }
}

async fn sha256_file(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn install(app: &Shared, s: &Spec, progress: &Progress) -> Result<std::path::PathBuf, String> {
    let dir = app.cfg.models_dir.clone();
    let final_dir = dir.join(s.dir);
    let version = std::fs::read_to_string(final_dir.join(".kairo-model-version")).ok();
    if crate::offline::present(&final_dir) && version.as_deref().map(str::trim) == Some(s.version) {
        let _ = progress.send(json!({ "phase": "done", "already": true, "modelPath": final_dir }));
        return Ok(final_dir);
    }
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let archive = dir.join(s.archive);
    let _ = progress.send(json!({ "phase": "download", "pct": 0 }));
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let head = |url: &str| {
        let c = client.clone();
        let url = url.to_string();
        async move { c.head(&url).send().await.ok().filter(|r| r.status().is_success()).and_then(|r| r.content_length()) }
    };
    let (url, total) = match head(s.url).await {
        Some(t) => (s.url, t),
        None => (s.fallback, head(s.fallback).await.unwrap_or(0)),
    };
    let on = percent_reporter(progress.clone());
    fetch_verified(&client, url, &archive, total, s.sha256, &on, progress).await?;
    let _ = progress.send(json!({ "phase": "extract" }));
    let top = dir.join(s.top_dir);
    let _ = tokio::fs::remove_dir_all(&top).await;
    let out = tokio::process::Command::new("tar").arg("xjf").arg(&archive).arg("-C").arg(&dir).output().await.map_err(|e| format!("tar extract failed: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar extract failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    if !top.exists() {
        return Err("Extraction produced no model directory".into());
    }
    for p in PRUNE {
        let _ = std::fs::remove_dir_all(top.join(p)).or_else(|_| std::fs::remove_file(top.join(p)));
    }
    let _ = tokio::fs::remove_dir_all(&final_dir).await;
    tokio::fs::rename(&top, &final_dir).await.map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(&archive).await;
    if !crate::offline::present(&final_dir) {
        return Err("Model files missing after extraction — the archive layout may have changed.".into());
    }
    // Done now so the first service doesn't wait for it (loading would do it otherwise).
    let d = final_dir.clone();
    let _ = tokio::task::spawn_blocking(move || crate::offline::map_weights(&d)).await;
    let _ = std::fs::write(final_dir.join(".kairo-model-version"), s.version);
    let _ = progress.send(json!({ "phase": "done", "already": false, "modelPath": final_dir }));
    // Prepared: keep one loaded, so the first Start after setup is instant.
    crate::offline::prepare_at_launch(app.clone());
    Ok(final_dir)
}

/// The Core ML model, file by file into a scratch folder that becomes the model folder once every
/// file checks out; then loaded once, so macOS prepares it for the Neural Engine now (~45 s on an
/// M1) rather than when someone first presses Start.
async fn install_coreml(app: &Shared, s: &CoreMlSpec, progress: &Progress) -> Result<std::path::PathBuf, String> {
    let dir = app.cfg.models_dir.clone();
    let final_dir = dir.join(s.dir);
    let version = std::fs::read_to_string(final_dir.join(".kairo-model-version")).ok();
    if crate::offline::coreml_present(&final_dir) && version.as_deref().map(str::trim) == Some(s.version) {
        let _ = progress.send(json!({ "phase": "done", "already": true, "modelPath": final_dir }));
        return Ok(final_dir);
    }
    let partial = dir.join(format!("{}.partial", s.dir));
    tokio::fs::create_dir_all(&partial).await.map_err(|e| e.to_string())?;
    let _ = progress.send(json!({ "phase": "download", "pct": 0 }));
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let total: u64 = s.files.iter().map(|f| f.1).sum();
    let report = percent_reporter(progress.clone());
    let mut done = 0u64;
    for &(path, size, sha) in s.files {
        let dest = partial.join(path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }
        // A file already here and whole (an earlier, interrupted install) isn't fetched again.
        let whole = tokio::fs::metadata(&dest).await.is_ok_and(|m| m.len() == size) && sha256_file(&dest).await.ok().as_deref() == Some(sha);
        if !whole {
            let base = done;
            let on = |received: u64, _: u64| report(base + received.min(size), total);
            fetch_verified(&client, &format!("{}{path}", s.base), &dest, size, sha, &on, progress).await?;
        }
        done += size;
        report(done, total);
    }
    let _ = tokio::fs::remove_dir_all(&final_dir).await;
    tokio::fs::rename(&partial, &final_dir).await.map_err(|e| e.to_string())?;
    // Shown as the unpacking step: the UI has no other for it.
    let _ = progress.send(json!({ "phase": "extract" }));
    let (a, d) = (app.clone(), final_dir.clone());
    tokio::task::spawn_blocking(move || crate::offline::prepare_coreml(&a, &d)).await.map_err(|e| e.to_string())??;
    let _ = std::fs::write(final_dir.join(".kairo-model-version"), s.version);
    let _ = progress.send(json!({ "phase": "done", "already": false, "modelPath": final_dir }));
    Ok(final_dir)
}

/// POST /api/offline/install?model=en|multi — progress as NDJSON. English on Apple Silicon is
/// the Core ML model (offline::coreml); everywhere else, and other languages, the ONNX one.
pub async fn route(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let multi = q.get("model").is_some_and(|m| m == "multi");
    let (key, spec): (&'static str, &'static Spec) = if multi { ("multi", &MULTI) } else { ("en", &EN) };
    let coreml = !multi && crate::offline::coreml();
    {
        let mut busy = INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if busy.contains(&key) {
            return (StatusCode::CONFLICT, axum::Json(json!({ "error": "install already in progress" }))).into_response();
        }
        busy.push(key);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    // Every progress line is remembered on its way to the caller.
    let (step_tx, mut step_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        while let Some(v) = step_rx.recv().await {
            if v["phase"] == "download" {
                if let Some(pct) = v["pct"].as_i64() {
                    set_progress(key, Some(pct));
                }
            }
            let _ = tx.send(v);
        }
    });
    set_progress(key, Some(0));
    tokio::spawn(async move {
        let result = if coreml { install_coreml(&app, &COREML_EN, &step_tx).await } else { install(&app, spec, &step_tx).await };
        let _ = step_tx.send(match result {
            Ok(_) => json!({ "phase": "complete", "ok": true }),
            Err(e) => json!({ "phase": "complete", "ok": false, "error": e }),
        });
        INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|k| *k != key);
        set_progress(key, None);
    });
    let _ = spec.approx_mb;
    ndjson_stream(rx)
}

/// Progress lines as they come, one JSON object per line.
pub(crate) fn ndjson_stream(rx: tokio::sync::mpsc::UnboundedReceiver<Value>) -> Response {
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (Ok::<_, std::convert::Infallible>(format!("{v}\n")), rx)) });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap()
}
