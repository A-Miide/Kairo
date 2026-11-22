//! The playlists, kept in a file under the app's data folder (`playlists.json`).
//!
//! They lived only in the page's own storage, which WebView2 caps at a few megabytes: a 151-slide
//! ProPresenter deck with its pictures filled it, and every change after that was lost on restart
//! (church PC, 22 Nov 2026). The page still keeps a copy there when it fits; this file is the one that
//! always holds. The page sends each picture once (service.js packPlaylists), so a background used on
//! every slide is stored once.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::Shared;

/// One write at a time: two saves landing together must not interleave.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn router() -> Router<Shared> {
    Router::new().route(
        "/api/playlists",
        get(read).put(write).layer(axum::extract::DefaultBodyLimit::max(400 * 1024 * 1024)),
    )
}

fn path(app: &Shared) -> std::path::PathBuf {
    app.cfg.app_data_dir.join("playlists.json")
}

/// The saved playlists as they were sent, or `{"none": true}` before the first save.
async fn read(State(app): State<Shared>) -> Response {
    let p = path(&app);
    match crate::util::blocking(move || std::fs::read(&p)).await {
        Ok(Ok(bytes)) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        _ => Json(json!({ "none": true })).into_response(),
    }
}

/// Replaces the saved playlists. Only a JSON object with a `playlists` array is taken; the last good
/// file is kept beside it (`playlists.json.bak`).
async fn write(State(app): State<Shared>, body: Bytes) -> Response {
    if let Err(e) = check(&body) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
    }
    let p = path(&app);
    match crate::util::blocking(move || save(&p, &body)).await {
        Ok(Ok(())) => Json(json!({ "ok": true })).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response(),
    }
}

fn check(body: &[u8]) -> Result<(), &'static str> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "not JSON")?;
    if v["playlists"].is_array() { Ok(()) } else { Err("no playlists") }
}

/// Written beside, then moved into place: a half-written file never replaces a good one.
fn save(p: &std::path::Path, body: &[u8]) -> std::io::Result<()> {
    let _one = WRITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    if p.is_file() {
        let _ = std::fs::copy(p, p.with_extension("json.bak"));
    }
    std::fs::rename(&tmp, p)
}

#[cfg(test)]
mod tests {
    use super::{check, save};

    #[test]
    fn only_playlists_are_taken() {
        assert!(check(br#"{"playlists":[],"activePlaylistId":null}"#).is_ok());
        assert!(check(br#"{"items":[]}"#).is_err());
        assert!(check(b"not json").is_err());
    }

    #[test]
    fn a_save_keeps_the_last_good_file() {
        let dir = std::env::temp_dir().join(format!("kairo-playlists-test-{}", std::process::id()));
        let p = dir.join("playlists.json");
        save(&p, br#"{"playlists":[1]}"#).unwrap();
        save(&p, br#"{"playlists":[2]}"#).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), r#"{"playlists":[2]}"#);
        assert_eq!(std::fs::read_to_string(p.with_extension("json.bak")).unwrap(), r#"{"playlists":[1]}"#);
        assert!(!p.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
