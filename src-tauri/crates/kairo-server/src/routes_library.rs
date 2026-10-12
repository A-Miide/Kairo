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
