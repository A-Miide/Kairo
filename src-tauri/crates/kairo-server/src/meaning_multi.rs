//! The meaning model for services that aren't in English: multilingual-e5-small, trimmed to the 50,550
//! words the Bibles and sermons use (43 MB, mean pooling), with the KJV's verses already embedded (12 MB).
//! A paraphrase or a quote read from a Bible the detector has no wording of is found by meaning in any of
//! the languages the speech model hears; the English model (bge-small) only reads English.
//!
//! It replaces the English meaning model for a service in another language, not adds to it: loading one
//! drops the other. It comes from a catalogue on GitHub Releases (`meaning-pack/catalog.json`: url, SHA-256,
//! bytes, version) and is fetched with the multilingual speech model, or on its own.
//!
//! Files in the app data folder: `models/meaning-multi/{model.onnx, tokenizer.json, verse_index.kvx}`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::Shared;

const CATALOG: &str = crate::kairo_download!("meaning-pack/catalog.json");

fn catalog_url() -> String {
    std::env::var("KAIRO_MEANING_CATALOG_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| CATALOG.to_string())
}

pub fn dir(app: &Shared) -> PathBuf {
    app.cfg.models_dir.join("meaning-multi")
}

/// Everything the model needs is on this computer.
pub fn present(dir: &Path) -> bool {
    ["model.onnx", "tokenizer.json", "verse_index.kvx"].iter().all(|f| dir.join(f).is_file())
}

/// A spoken-language setting the multilingual model serves: anything but English ("en-US"). "multi" (the
/// multilingual speech model choosing the language itself) counts.
pub fn serves(spoken: &str) -> bool {
    !spoken.trim().is_empty() && !spoken.trim().to_ascii_lowercase().starts_with("en")
}

/// Downloads and unpacks the model unless it is already there at the catalogue's version.
pub async fn install(app: &Shared, progress: &crate::installer::Progress) -> Result<(), String> {
    let final_dir = dir(app);
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(60)).build().map_err(|e| e.to_string())?;
    let catalog: Value = client.get(catalog_url()).send().await.map_err(|_| "Couldn't reach the list of meaning models. Check the internet connection.".to_string())?
        .json().await.map_err(|_| "The list of meaning models couldn't be read.".to_string())?;
    let version = catalog["version"].as_str().unwrap_or("");
    let have = std::fs::read_to_string(final_dir.join(".kairo-model-version")).ok();
    if present(&final_dir) && have.as_deref().map(str::trim) == Some(version) {
        let _ = progress.send(json!({ "phase": "done", "already": true }));
        return Ok(());
    }
    let url = catalog["url"].as_str().filter(|u| u.starts_with("https://") || u.starts_with("http://127.0.0.1")).ok_or("The list has no download address for the meaning model.")?;
    let sha = catalog["sha256"].as_str().filter(|s| s.len() == 64).ok_or("The list has no checksum for the meaning model.")?.to_ascii_lowercase();
    let total = catalog["bytes"].as_u64().unwrap_or(0);
    let parent = app.cfg.models_dir.clone();
    tokio::fs::create_dir_all(&parent).await.map_err(|e| e.to_string())?;
    let archive = parent.join("meaning-multi.tar.gz");
    let _ = progress.send(json!({ "phase": "download", "pct": 0 }));
    let on = crate::installer::percent_reporter(progress.clone());
    crate::installer::fetch_verified(&client, url, &archive, total, &sha, &on, progress).await?;
    let _ = progress.send(json!({ "phase": "extract" }));
    let staging = parent.join("meaning-multi.new");
    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::create_dir_all(&staging).await.map_err(|e| e.to_string())?;
    let out = crate::proc::AsyncCommand::new("tar").arg("xzf").arg(&archive).arg("-C").arg(&staging).output().await.map_err(|e| format!("tar extract failed: {e}"))?;
    let _ = tokio::fs::remove_file(&archive).await;
    if !out.status.success() || !present(&staging) {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err("The meaning model's files were missing after unpacking. Nothing was installed.".into());
    }
    let _ = std::fs::write(staging.join(".kairo-model-version"), version);
    let _ = tokio::fs::remove_dir_all(&final_dir).await;
    tokio::fs::rename(&staging, &final_dir).await.map_err(|e| e.to_string())?;
    let _ = progress.send(json!({ "phase": "done", "already": false }));
    Ok(())
}

/// Loads the model that fits the spoken language now (the English one, or this one when it is installed),
/// unless that one is already loaded. Called when the language setting changes; slow, so off the request.
pub fn follow_language(app: &Shared, spoken: &str) {
    let want_multi = serves(spoken) && present(&dir(app));
    if crate::meaning_is_multi() == Some(want_multi) {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("meaning-swap".into()).spawn(move || crate::load_meaning(&app, want_multi));
}

/// GET /api/meaning-multi/status
pub async fn status(axum::extract::State(app): axum::extract::State<Shared>) -> axum::Json<Value> {
    let spoken = app.settings.str("sttLanguage").await.unwrap_or_default();
    axum::Json(json!({ "present": present(&dir(&app)), "serves": serves(&spoken), "loaded": crate::meaning_is_multi() }))
}

/// POST /api/meaning-multi/install: progress as NDJSON, then the model is loaded if the service is in another language.
pub async fn install_route(axum::extract::State(app): axum::extract::State<Shared>) -> axum::response::Response {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let result = install(&app, &tx).await;
        let _ = tx.send(match &result {
            Ok(()) => json!({ "phase": "complete", "ok": true }),
            Err(e) => json!({ "phase": "complete", "ok": false, "error": e }),
        });
        if result.is_ok() {
            let spoken = app.settings.str("sttLanguage").await.unwrap_or_default();
            follow_language(&app, &spoken);
        }
    });
    crate::installer::ndjson_stream(rx)
}
