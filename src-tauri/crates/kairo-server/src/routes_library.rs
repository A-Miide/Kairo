//! Routes for songs, triggers, service segments and the media library.

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
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
        .route("/api/audio/tracks", get(audio_tracks))
        .route("/api/audio/upload", post(audio_upload))
        .route("/api/audio/file/{name}", get(audio_file))
        .route("/api/audio/{name}", delete(audio_delete))
        .route("/api/media/prepare", post(prepare_video))
        .route("/api/media/resolve", post(resolve_media))
        .route("/api/media/prepared/{file}", get(prepared_file))
        .route("/api/pp-assets/{file}", get(pp_asset))
}

/// A picture or video imported from a ProPresenter file (see propresenter.rs's Assets).
/// With `?w=480` a picture comes back at most that wide (a copy made once and kept in
/// `small/`): the operator's tiles and previews don't need the 4K original, which the web view
/// decodes to ~33 MB however small it is drawn. The output screens ask without `w`.
async fn pp_asset(State(app): State<Shared>, Path(file): Path<String>, req: Request) -> Response {
    let ok = file.split_once('.').is_some_and(|(h, e)| h.len() == 32 && h.bytes().all(|c| c.is_ascii_hexdigit()) && !e.is_empty() && e.bytes().all(|c| c.is_ascii_alphanumeric()));
    let dir = crate::propresenter::Assets::new(&app.cfg.app_data_dir).dir().to_path_buf();
    let p = dir.join(&file);
    if !(ok && p.is_file()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let width = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("w="))).and_then(|w| w.parse::<u32>().ok());
    let p = match width {
        Some(w) => {
            let source = p.clone();
            tokio::task::spawn_blocking(move || smaller_copy(&dir, &source, w)).await.ok().flatten().unwrap_or(p)
        }
        None => p,
    };
    send_file(p, req).await
}

/// The picture at `source` no wider than `width` (rounded up to a multiple of 160, at most 1920),
/// made once and kept under `dir/small/`. `None` when it is already that small, isn't a PNG or
/// JPEG, or can't be read: the caller then serves the original.
fn smaller_copy(dir: &std::path::Path, source: &std::path::Path, width: u32) -> Option<std::path::PathBuf> {
    let width = width.clamp(160, 1920).div_ceil(160) * 160;
    let ext = source.extension()?.to_str()?.to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => image::ImageFormat::Png,
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        _ => return None,
    };
    let out = dir.join("small").join(format!("{}-{width}.{ext}", source.file_stem()?.to_str()?));
    if out.is_file() {
        return Some(out);
    }
    let img = image::open(source).ok()?;
    if img.width() <= width {
        return None;
    }
    let height = ((img.height() as u64 * width as u64) / img.width() as u64).max(1) as u32;
    let small = img.resize_exact(width, height, image::imageops::FilterType::Triangle);
    std::fs::create_dir_all(out.parent()?).ok()?;
    let tmp = out.with_extension(format!("{ext}.tmp"));
    // A JPEG has no alpha channel; a PNG keeps its own.
    let saved = if format == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(small.to_rgb8()).save_with_format(&tmp, format) } else { small.save_with_format(&tmp, format) };
    if saved.is_err() || std::fs::rename(&tmp, &out).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    Some(out)
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

use crate::util::{body_or_empty, not_found};

/// An upload: the file is decoded and written on the blocking pool, so a big video never
/// holds up the async runtime (every other request waits behind a blocked worker).
/// `Ok(None)`: the body had no usable file.
async fn upload<T: Send + 'static>(b: Option<Json<Value>>, save: impl FnOnce(String, Vec<u8>) -> T + Send + 'static) -> Result<Option<T>, Response> {
    let b = body_or_empty(b);
    crate::util::blocking(move || crate::util::upload_parts(&b).map(|(name, data)| save(name, data)))
        .await
        .map_err(|e| crate::util::error(StatusCode::INTERNAL_SERVER_ERROR, e))
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


async fn bin(State(app): State<Shared>) -> Json<Value> {
    let m = app.media.clone();
    let items = tokio::task::spawn_blocking(move || m.bin_items()).await.unwrap_or_default();
    Json(json!({ "ok": true, "items": items }))
}
async fn bin_upload(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let m = app.media.clone();
    match upload(b, move |name, data| m.write_to_bin(&name, &data)).await {
        Err(r) => r,
        Ok(None) => crate::util::bad_request("filename and dataBase64 required"),
        Ok(Some(Ok(saved))) => Json(json!({ "ok": true, "name": saved })).into_response(),
        Ok(Some(Err(e))) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
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
async fn audio_tracks(State(app): State<Shared>) -> Json<Value> {
    let m = app.media.clone();
    let items = tokio::task::spawn_blocking(move || m.audio_items()).await.unwrap_or_default();
    Json(json!({ "ok": true, "items": items }))
}
async fn audio_upload(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let m = app.media.clone();
    match upload(b, move |name, data| m.write_to_audio(&name, &data)).await {
        Err(r) => r,
        Ok(None) => crate::util::bad_request("filename and dataBase64 required"),
        Ok(Some(Ok(saved))) => Json(json!({ "ok": true, "name": saved })).into_response(),
        Ok(Some(Err(e))) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn audio_delete(State(app): State<Shared>, Path(name): Path<String>) -> Json<Value> {
    Json(json!({ "ok": app.media.delete_audio_item(&name) }))
}
async fn audio_file(State(app): State<Shared>, Path(name): Path<String>, req: Request) -> Response {
    match app.media.audio_file(&name) {
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
        Err(e) if e.0 == "SAVE_FAILED" => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn remove_folder(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match app.media.remove_folder(&id) {
        Ok(removed) => Json(json!({ "ok": removed })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn folder_items(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let m = app.media.clone();
    match tokio::task::spawn_blocking(move || m.folder_items(&id)).await.ok().flatten() {
        Some((items, truncated)) => Json(json!({ "ok": true, "items": items, "truncated": truncated })).into_response(),
        None => not_found("No such folder"),
    }
}
async fn folder_upload(State(app): State<Shared>, Path(id): Path<String>, b: Option<Json<Value>>) -> Response {
    let m = app.media.clone();
    match upload(b, move |name, data| m.write_into_folder(&id, &name, &data)).await {
        Err(r) => r,
        Ok(None) => crate::util::bad_request("filename and dataBase64 required"),
        Ok(Some(Ok(()))) => Json(json!({ "ok": true })).into_response(),
        Ok(Some(Err(e))) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.1, "code": e.0 }))).into_response(),
    }
}
async fn folder_file(State(app): State<Shared>, Path((id, name)): Path<(String, String)>, req: Request) -> Response {
    match app.media.folder_file(&id, &name) {
        Some(p) => send_file(p, req).await,
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod pp_asset_tests {
    use super::smaller_copy;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kairo-pp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_big_picture_gets_a_smaller_copy_made_once_and_a_small_one_is_left_alone() {
        let dir = temp("small");
        let big = dir.join("0123456789abcdef0123456789abcdef.png");
        image::RgbaImage::from_pixel(1600, 900, image::Rgba([10, 20, 30, 128])).save(&big).unwrap();
        let out = smaller_copy(&dir, &big, 500).expect("a smaller copy");
        let got = image::open(&out).unwrap();
        assert_eq!((got.width(), got.height()), (640, 360), "500 rounds up to 640, aspect kept");
        assert!(got.color().has_alpha(), "a PNG keeps its transparency");
        assert_eq!(smaller_copy(&dir, &big, 500).as_deref(), Some(out.as_path()), "the second ask reuses it");
        assert!(smaller_copy(&dir, &big, 1920).is_none(), "already narrower than the width asked: serve the original");
        let jpg = dir.join("fedcba9876543210fedcba9876543210.jpg");
        image::RgbImage::from_pixel(1000, 500, image::Rgb([200, 100, 50])).save(&jpg).unwrap();
        let j = smaller_copy(&dir, &jpg, 480).expect("a JPEG copy");
        assert_eq!(image::open(&j).unwrap().width(), 480);
        assert!(smaller_copy(&dir, &dir.join("x.gif"), 480).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
