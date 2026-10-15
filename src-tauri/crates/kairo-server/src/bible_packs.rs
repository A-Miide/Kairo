//! Bibles that aren't inside the app: a catalogue on GitHub Releases lists them, each
//! with its size and SHA-256, and one is downloaded on request into the app data
//! folder (`packs/<lang>.json`), where the loader looks before the bundled ones.
//!
//! Open-licence texts only (public domain, CC BY-SA): the catalogue carries each
//! pack's translation name and licence so the app can show its credit.

use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::Shared;

/// Where the catalogue lives unless `KAIRO_BIBLE_CATALOG_URL` says otherwise.
const DEFAULT_CATALOG: &str = "https://github.com/miide-Adeyemi/Kairo/releases/download/bible-packs/catalog.json";

static INSTALLING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn catalog_url() -> String {
    std::env::var("KAIRO_BIBLE_CATALOG_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| DEFAULT_CATALOG.to_string())
}

/// A language code that is safe to use as a file name.
fn clean_lang(lang: &str) -> Option<String> {
    let l = lang.trim().to_ascii_lowercase();
    (!l.is_empty() && l.len() <= 8 && l.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')).then_some(l)
}

fn is_installing(lang: &str) -> bool {
    INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|l| l == lang)
}

async fn fetch_catalog() -> Result<Value, String> {
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
    let url = catalog_url();
    let resp = client.get(&url).send().await.map_err(|e| {
        eprintln!("[BiblePacks] Couldn't fetch {url}: {e:?}");
        "Couldn't reach the list of Bibles. Check the internet connection.".to_string()
    })?;
    if !resp.status().is_success() {
        return Err(format!("The list of Bibles isn't available yet (HTTP {}).", resp.status().as_u16()));
    }
    resp.json::<Value>().await.map_err(|_| "The list of Bibles couldn't be read.".to_string())
}

/// What is on this computer for a language, without touching the network.
pub async fn status(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    let Some(lang) = q.get("lang").and_then(|l| clean_lang(l)) else { return Json(json!({ "error": "lang required" })) };
    let Some(bible) = app.bible.get() else { return Json(json!({ "lang": lang, "ready": false })) };
    let bundled = bible.bundled_pack(&lang);
    let downloaded = bible.user_packs_dir().join(format!("{lang}.json")).is_file();
    Json(json!({
        "lang": lang, "ready": true, "bundled": bundled, "downloaded": downloaded, "present": bundled || downloaded,
        "installing": is_installing(&lang),
    }))
}

/// The list of Bibles that can be downloaded.
pub async fn catalog() -> Json<Value> {
    match fetch_catalog().await {
        Ok(c) => Json(json!({ "ok": true, "packs": c["packs"] })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// POST /api/bible-packs/install?lang=xx: progress as NDJSON (download, verify, complete).
pub async fn install(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let Some(lang) = q.get("lang").and_then(|l| clean_lang(l)) else {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "lang required" }))).into_response();
    };
    {
        let mut busy = INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if busy.contains(&lang) {
            return (axum::http::StatusCode::CONFLICT, Json(json!({ "error": "install already in progress" }))).into_response();
        }
        busy.push(lang.clone());
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let result = run_install(&app, &lang, &tx).await;
        let _ = tx.send(match result {
            Ok(()) => json!({ "phase": "complete", "ok": true }),
            Err(e) => json!({ "phase": "complete", "ok": false, "error": e }),
        });
        INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|l| *l != lang);
    });
    crate::installer::ndjson_stream(rx)
}

/// Plain HTTP only from this computer itself (tests with a local list).
fn is_local_test_url(u: &str) -> bool {
    u.strip_prefix("http://127.0.0.1").is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
}

async fn run_install(app: &Shared, lang: &str, progress: &crate::installer::Progress) -> Result<(), String> {
    let bible = app.bible.get().ok_or("Kairo is still starting. Try again in a moment.")?;
    let already = || { let _ = progress.send(json!({ "phase": "done", "already": true })); };
    if bible.user_packs_dir().join(format!("{lang}.json")).is_file() {
        already();
        return Ok(());
    }
    let _ = progress.send(json!({ "phase": "download", "pct": 0 }));
    // A Bible that comes with the app is replaced by a download only when the list offers one
    // (a fuller one, e.g. a whole Bible where the app has the New Testament only).
    let bundled = bible.bundled_pack(lang);
    let catalog = match fetch_catalog().await {
        Ok(c) => c,
        Err(_) if bundled => { already(); return Ok(()); }
        Err(e) => return Err(e),
    };
    let Some(entry) = catalog["packs"].as_array().and_then(|p| p.iter().find(|e| e["lang"].as_str() == Some(lang))) else {
        if bundled { already(); return Ok(()); }
        return Err("That Bible isn't in the list.".into());
    };
    let url = entry["url"].as_str().filter(|u| u.starts_with("https://") || is_local_test_url(u)).ok_or("The list has no download address for that Bible.")?;
    let sha = entry["sha256"].as_str().filter(|s| s.len() == 64).ok_or("The list has no checksum for that Bible.")?.to_ascii_lowercase();
    let total = entry["bytes"].as_u64().unwrap_or(0);
    let dir = bible.user_packs_dir().to_path_buf();
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let part = dir.join(format!("{lang}.json.part"));
    let _ = tokio::fs::remove_file(&part).await;
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let on = crate::installer::percent_reporter(progress.clone());
    crate::installer::fetch_verified(&client, url, &part, total, &sha, &on, progress).await?;
    let _ = progress.send(json!({ "phase": "verify" }));
    // It must be a Bible pack we can read, not just a file with the right checksum.
    let bytes = tokio::fs::read(&part).await.map_err(|e| e.to_string())?;
    let ok = serde_json::from_slice::<Value>(&bytes).ok().is_some_and(|v| v["verses"].as_array().is_some_and(|a| !a.is_empty()));
    if !ok {
        let _ = tokio::fs::remove_file(&part).await;
        return Err("The downloaded file isn't a Bible pack. Nothing was installed.".into());
    }
    tokio::fs::rename(&part, dir.join(format!("{lang}.json"))).await.map_err(|e| e.to_string())?;
    bible.forget_language(lang);
    Ok(())
}

/// POST /api/bible-packs/remove {lang}: deletes a downloaded pack (never one inside the app).
pub async fn remove(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    let b = b.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    let Some(lang) = b["lang"].as_str().and_then(clean_lang) else { return Json(json!({ "ok": false })) };
    let Some(bible) = app.bible.get() else { return Json(json!({ "ok": false })) };
    let path = bible.user_packs_dir().join(format!("{lang}.json"));
    let ok = path.is_file() && std::fs::remove_file(&path).is_ok();
    if ok {
        bible.forget_language(&lang);
    }
    Json(json!({ "ok": ok }))
}

#[cfg(test)]
mod tests {
    use super::clean_lang;

    #[test]
    fn language_codes_are_plain_file_names() {
        assert_eq!(clean_lang("PT"), Some("pt".into()));
        assert_eq!(clean_lang("pt-br"), Some("pt-br".into()));
        assert_eq!(clean_lang("../settings"), None);
        assert_eq!(clean_lang(""), None);
        assert_eq!(clean_lang("a/b"), None);
    }
}
