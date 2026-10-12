//! Downloading the offline speech models: resumable, retried, checked
//! against a known checksum, then unpacked into place — progress reported
//! line by line (NDJSON) as the UI expects.

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
    url: "https://github.com/Kairo-live/Kairo/releases/download/offline-model-v1/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2",
    fallback: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2",
    sha256: "78e2b79fcf7271553a74402a76b771b09ea40117a39566a79f52235b23db6358",
    top_dir: "sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25",
    dir: "sherpa-streaming-en",
    version: "v1",
    archive: "sherpa-en.tar.bz2",
    approx_mb: 465,
};
const MULTI: Spec = Spec {
    url: "https://github.com/Kairo-live/Kairo/releases/download/offline-model-multi-v1/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11.tar.bz2",
    fallback: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11.tar.bz2",
    sha256: "c6bf5e0df765f9d5b43bc9e0536d4b4b3e7d40bdf5ecf13e45f134c51c05ae3a",
    top_dir: "sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11",
    dir: "sherpa-streaming-multi",
    version: "v1",
    archive: "sherpa-multi.tar.bz2",
    approx_mb: 475,
};
const ATTEMPTS: u32 = 6;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PRUNE: [&str; 2] = ["test_wavs", "README.md"];

static INSTALLING: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

pub fn installing(model: &str) -> bool {
    INSTALLING.lock().unwrap().contains(&model)
}

type Progress = tokio::sync::mpsc::UnboundedSender<Value>;

async fn download(client: &reqwest::Client, url: &str, dest: &Path, total: u64, progress: &Progress) -> Result<(), String> {
    let start = tokio::fs::metadata(dest).await.map_or(0, |m| m.len());
    if total > 0 && start >= total {
        let _ = progress.send(json!({ "phase": "download", "pct": 100, "received": total, "total": total }));
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
    let mut last = -1i64;
    loop {
        let chunk = tokio::time::timeout(IDLE_TIMEOUT, resp.chunk()).await.map_err(|_| format!("Download stalled (no activity for {}s)", IDLE_TIMEOUT.as_secs()))?.map_err(|e| e.to_string())?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        if total > 0 {
            let pct = (received * 100 / total) as i64;
            if pct != last {
                last = pct;
                let _ = progress.send(json!({ "phase": "download", "pct": pct, "received": received, "total": total }));
            }
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    let _ = progress.send(json!({ "phase": "download", "pct": 100, "received": received, "total": total }));
    Ok(())
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
    let mut last_err = None;
    for attempt in 1..=ATTEMPTS {
        match download(&client, url, &archive, total, progress).await {
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
    let _ = progress.send(json!({ "phase": "verify" }));
    if sha256_file(&archive).await? != s.sha256 {
        let _ = tokio::fs::remove_file(&archive).await;
        return Err("The downloaded file did not match the expected one (checksum). Nothing was installed — try again.".into());
    }
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
    let _ = std::fs::write(final_dir.join(".kairo-model-version"), s.version);
    let _ = progress.send(json!({ "phase": "done", "already": false, "modelPath": final_dir }));
    Ok(final_dir)
}

/// POST /api/offline/install?model=en|multi — progress as NDJSON.
pub async fn route(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let multi = q.get("model").is_some_and(|m| m == "multi");
    let (key, spec): (&'static str, &'static Spec) = if multi { ("multi", &MULTI) } else { ("en", &EN) };
    {
        let mut busy = INSTALLING.lock().unwrap();
        if busy.contains(&key) {
            return (StatusCode::CONFLICT, axum::Json(json!({ "error": "install already in progress" }))).into_response();
        }
        busy.push(key);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let result = install(&app, spec, &tx).await;
        let _ = tx.send(match result {
            Ok(_) => json!({ "phase": "complete", "ok": true }),
            Err(e) => json!({ "phase": "complete", "ok": false, "error": e }),
        });
        INSTALLING.lock().unwrap().retain(|k| *k != key);
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (Ok::<_, std::convert::Infallible>(format!("{v}\n")), rx)) });
    let _ = spec.approx_mb;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap()
}
