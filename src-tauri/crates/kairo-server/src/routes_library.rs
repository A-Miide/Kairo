//! Routes for songs, triggers, service segments and the media library.

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use base64::Engine as _;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::Shared;

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/api/songs", get(songs).post(add_song))
        .route("/api/songs/{id}", put(update_song).delete(remove_song))
        .route("/api/triggers", get(triggers).post(add_trigger))
        .route("/api/triggers/{id}", put(update_trigger).delete(remove_trigger))
        .route("/api/triggers/{id}/fire", post(fire_trigger))
        .route("/api/triggers/{id}/stop", post(stop_trigger))
        .route("/api/segments", get(segments).post(add_segment))
        .route("/api/segments/reorder", post(reorder_segments))
        .route("/api/segments/{id}", put(update_segment).delete(remove_segment))
        .route("/api/segments/{id}/start", post(start_segment))
        .route("/api/segments/{id}/stop", post(stop_segment))
        .route("/api/segments/{id}/reset", post(reset_segment))
        .route("/api/media/bin", get(bin))
        .route("/api/media/bin/upload", post(bin_upload))
        .route("/api/media/bin/{name}", delete(bin_delete))
        .route("/api/media/bin/file/{name}", get(bin_file))
        .route("/api/media/folders", get(folders).post(add_folder))
        .route("/api/media/folders/{id}", delete(remove_folder))
        .route("/api/media/folders/{id}/items", get(folder_items))
        .route("/api/media/folders/{id}/upload", post(folder_upload))
        .route("/api/media/folders/{id}/file/{name}", get(folder_file))
        .route("/api/media/prepare", post(prepare_video))
        .route("/api/media/resolve", post(resolve_media))
        .route("/api/media/prepared/{file}", get(prepared_file))
        .route("/api/pp-assets/{file}", get(pp_asset))
}

/// A picture or video imported from a ProPresenter file (see propresenter.rs's Assets).
async fn pp_asset(State(app): State<Shared>, Path(file): Path<String>, req: Request) -> Response {
    let ok = file.split_once('.').is_some_and(|(h, e)| h.len() == 32 && h.bytes().all(|c| c.is_ascii_hexdigit()) && !e.is_empty() && e.bytes().all(|c| c.is_ascii_alphanumeric()));
    let p = crate::propresenter::Assets::new(&app.cfg.app_data_dir).dir().join(&file);
    if ok && p.is_file() { send_file(p, req).await } else { StatusCode::NOT_FOUND.into_response() }
}

/// The file on disk behind one of the media addresses the UI uses.
fn media_path(app: &Shared, url: &str) -> Option<std::path::PathBuf> {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let dec = |s: &str| crate::percent_decode(s);
    if let Some(name) = path.strip_prefix("/api/media/bin/file/") {
        return app.media.bin_file(&dec(name));
    }
    let rest = path.strip_prefix("/api/media/folders/")?;
    let (id, name) = rest.split_once("/file/")?;
    app.media.folder_file(id, &dec(name))
}

/// `{ url }` in → `{ path, ffmpeg }`: the file behind a media address the
/// display plays, and this copy's ffmpeg — for the app to play a moving
/// background itself under an NDI / Syphon / DeckLink output (src-tauri
/// video_bg.rs). Media, converted copies and ProPresenter assets only.
async fn resolve_media(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let b = body_or_empty(b);
    let url = b["url"].as_str().unwrap_or("");
    let path = url.split(['?', '#']).next().unwrap_or("");
    let file = if let Some(f) = path.strip_prefix("/api/media/prepared/") {
        let key = f.strip_suffix(".mp4").or_else(|| f.strip_suffix(".webm")).unwrap_or("");
        if key.is_empty() || !key.bytes().all(|c| c.is_ascii_hexdigit()) { None } else { app.video.prepared(key).map(|(p, _)| p) }
    } else if let Some(f) = path.strip_prefix("/api/pp-assets/") {
        let ok = f.split_once('.').is_some_and(|(h, e)| h.len() == 32 && h.bytes().all(|c| c.is_ascii_hexdigit()) && !e.is_empty() && e.bytes().all(|c| c.is_ascii_alphanumeric()));
        let p = crate::propresenter::Assets::new(&app.cfg.app_data_dir).dir().join(f);
        (ok && p.is_file()).then_some(p)
    } else {
        media_path(&app, path)
    };
    match (file, app.video.ffmpeg_path()) {
        (Some(p), Some(ff)) => Json(json!({ "path": p, "ffmpeg": ff })).into_response(),
        (None, _) => not_found("No such file"),
        (_, None) => not_found("This copy of Kairo has no ffmpeg"),
    }
}

/// Makes a video playable: `{ url }` in, where it stands out. A file the web
/// view plays as it is comes back `native`; with `force` (it failed to play) or
/// for a container it can't read, it's converted once and `ready` carries the
/// address of the converted copy.
async fn prepare_video(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let b = body_or_empty(b);
    let url = b["url"].as_str().unwrap_or("").to_string();
    let force = b["force"].as_bool().unwrap_or(false);
    let Some(path) = media_path(&app, &url) else { return not_found("No such file") };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    if !force && !crate::video::needs_conversion(&name) {
        return Json(json!({ "state": "native" })).into_response();
    }
    let v = app.video.clone();
    let out = tokio::task::spawn_blocking(move || v.start(path)).await.unwrap_or_else(|_| json!({ "state": "failed" }));
    Json(out).into_response()
}

async fn prepared_file(State(app): State<Shared>, Path(file): Path<String>, req: Request) -> Response {
    let key = file.strip_suffix(".mp4").or_else(|| file.strip_suffix(".webm")).unwrap_or("");
    if key.is_empty() || !key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match app.video.prepared(key) {
        Some((p, _)) => send_file(p, req).await,
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn not_found(msg: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": msg }))).into_response()
}

fn body_or_empty(b: Option<Json<Value>>) -> Value {
    b.map(|Json(v)| v).unwrap_or_else(|| json!({}))
}

// ── Songs ───────────────────────────────────────────────────────────────────

async fn songs(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "ok": true, "songs": app.songs.list(), "categories": crate::library::SONG_CATEGORIES }))
}
async fn add_song(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    Json(json!({ "ok": true, "song": app.songs.add(&body_or_empty(b)) }))
}
async fn update_song(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    match app.songs.update(&id, &body_or_empty(b)) {
        Some(song) => Json(json!({ "ok": true, "song": song })).into_response(),
        None => not_found("No such song"),
    }
}
async fn remove_song(State(app): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": app.songs.remove(&id) }))
}

// ── Triggers ────────────────────────────────────────────────────────────────

async fn triggers(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "ok": true, "triggers": app.triggers.list(), "types": crate::library::trigger_types() }))
}
async fn add_trigger(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    Json(json!({ "ok": true, "trigger": app.triggers.add(&body_or_empty(b)) }))
}
async fn update_trigger(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    match app.triggers.update(&id, &body_or_empty(b)) {
        Some(t) => Json(json!({ "ok": true, "trigger": t })).into_response(),
        None => not_found("No such trigger"),
    }
}
async fn remove_trigger(State(app): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": app.triggers.remove(&id) }))
}
async fn fire_trigger(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let r = app.triggers.fire(&id);
    if r.get("error").is_some() { (StatusCode::NOT_FOUND, Json(r)).into_response() } else { Json(r).into_response() }
}
async fn stop_trigger(State(app): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    app.triggers.stop(&id);
    Json(json!({ "ok": true }))
}

// ── Segments ────────────────────────────────────────────────────────────────

async fn segments(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "ok": true, "segments": app.segments.list() }))
}
async fn add_segment(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    let b = body_or_empty(b);
    let name = b["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("Segment").to_string();
    let opts = json!({ "themeId": b["themeId"], "slideStyles": b["slideStyles"], "scenes": b["scenes"], "scenePace": b["scenePace"] });
    Json(json!({ "ok": true, "segment": app.segments.add(&name, &opts) }))
}
async fn update_segment(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    match app.segments.update(&id, &body_or_empty(b)) {
        Some(s) => Json(json!({ "ok": true, "segment": s })).into_response(),
        None => not_found("No such segment"),
    }
}
async fn remove_segment(State(app): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": app.segments.remove(&id) }))
}
async fn reorder_segments(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    let ids = body_or_empty(b)["orderedIds"].as_array().cloned().unwrap_or_default();
    Json(json!({ "ok": true, "segments": app.segments.reorder(&ids) }))
}
async fn start_segment(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    let b = body_or_empty(b);
    let r = app.segments.start(&id, b.get("endAtTime"));
    if r.get("error").is_some() { (StatusCode::BAD_REQUEST, Json(r)).into_response() } else { Json(r).into_response() }
}
async fn stop_segment(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    let r = app.segments.stop(&id, body_or_empty(b)["markDone"] == true);
    if r.get("error").is_some() { (StatusCode::NOT_FOUND, Json(r)).into_response() } else { Json(r).into_response() }
}
async fn reset_segment(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let r = app.segments.reset(&id);
    if r.get("error").is_some() { (StatusCode::NOT_FOUND, Json(r)).into_response() } else { Json(r).into_response() }
}

// ── Media ───────────────────────────────────────────────────────────────────

/// Serves a file with byte ranges (videos seek).
async fn send_file(path: std::path::PathBuf, req: Request) -> Response {
    match tower_http::services::ServeFile::new(path).oneshot(req).await {
        Ok(r) => r.map(Body::new).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn upload_parts(b: &Value) -> Option<(String, Vec<u8>)> {
    let name = b["filename"].as_str()?.to_string();
    let data = base64::engine::general_purpose::STANDARD.decode(b["dataBase64"].as_str()?).ok()?;
    Some((name, data))
}

async fn bin(State(app): State<Shared>) -> Json<Value> {
    let m = app.media.clone();
    let items = tokio::task::spawn_blocking(move || m.bin_items()).await.unwrap_or_default();
    Json(json!({ "ok": true, "items": items }))
}
async fn bin_upload(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let Some((name, data)) = upload_parts(&body_or_empty(b)) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "filename and dataBase64 required" }))).into_response();
    };
    match app.media.write_to_bin(&name, &data) {
        Ok(saved) => Json(json!({ "ok": true, "name": saved })).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn bin_delete(State(app): State<Shared>, Path(name): Path<String>) -> Json<Value> {
    Json(json!({ "ok": app.media.delete_bin_item(&name) }))
}
async fn bin_file(State(app): State<Shared>, Path(name): Path<String>, req: Request) -> Response {
    match app.media.bin_file(&name) {
        Some(p) => send_file(p, req).await,
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn folders(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "ok": true, "folders": app.media.folders() }))
}
async fn add_folder(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let b = body_or_empty(b);
    let name = b["name"].as_str().map(str::trim).filter(|n| !n.is_empty());
    let r = match b["dirPath"].as_str().filter(|d| !d.is_empty()) {
        Some(dir) => app.media.add_folder(name, dir),
        None => app.media.create_folder(name.unwrap_or("")),
    };
    match r {
        Ok(folder) => Json(json!({ "ok": true, "folder": folder })).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn remove_folder(State(app): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": app.media.remove_folder(&id) }))
}
async fn folder_items(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let m = app.media.clone();
    match tokio::task::spawn_blocking(move || m.folder_items(&id)).await.ok().flatten() {
        Some((items, truncated)) => Json(json!({ "ok": true, "items": items, "truncated": truncated })).into_response(),
        None => not_found("No such folder"),
    }
}
async fn folder_upload(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    let Some((name, data)) = upload_parts(&body_or_empty(b)) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "filename and dataBase64 required" }))).into_response();
    };
    match app.media.write_into_folder(&id, &name, &data) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn folder_file(State(app): State<Shared>, Path((id, name)): Path<(String, String)>, req: Request) -> Response {
    match app.media.folder_file(&id, &name) {
        Some(p) => send_file(p, req).await,
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
