//! OBS, and the status of the downloadable models.

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::obs::Obs;
use crate::Shared;

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/api/obs/test", get(obs_test))
        .route("/api/obs/status", get(obs_status))
        .route("/api/obs/connect", post(obs_connect))
        .route("/api/obs/sources", get(obs_sources))
        .route("/api/offline/status", get(offline_status))
        .route("/api/offline/install", post(crate::installer::route))
        .route("/api/translate-model/status", get(crate::mt::status_route))
        .route("/api/semantic-model/status", get(semantic_status))
        .route("/api/reranker-model/status", get(reranker_status))
        .route("/api/translate/languages", get(translate_languages))
        .route("/api/service/import", post(service_import))
        .route("/api/export", post(export))
        .route("/api/semantic-model/install", post(bundled_install))
        .route("/api/reranker-model/install", post(bundled_install))
        .route("/api/translate-model/install", post(crate::mt::install_route))
        .route("/api/translate-model/remove", post(crate::mt::remove_route))
        .route("/api/theme/import-protheme", post(theme_import))
        .route("/api/vocabulary/learned", get(learned_terms))
        .route("/api/vocabulary/forget", post(forget_term))
        .route("/api/osc/status", get(|| async { Json(crate::osc::status()) }))
        .route("/api/ndi/runtime", get(crate::ndi_runtime::status_route))
        .route("/api/ndi/runtime/install", post(crate::ndi_runtime::install_route))
}

/// Names learned from past services (Settings → Church vocabulary).
async fn learned_terms(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "terms": crate::vocabulary::learned_all(&app) }))
}

async fn forget_term(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    if let Some(t) = b["term"].as_str() {
        crate::vocabulary::forget(&app, t);
    }
    Json(json!({ "ok": true, "terms": crate::vocabulary::learned_all(&app) }))
}

fn unprocessable(e: crate::import::ImportError) -> axum::response::Response {
    use axum::response::IntoResponse;
    (axum::http::StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": e.0, "code": e.1 }))).into_response()
}

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

async fn service_import(State(app): State<Shared>, Json(b): Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    use crate::import::{dedupe_image_layers, from_text, import_slides, ImportError, Imported};
    if let Some(t) = b["text"].as_str().filter(|t| !t.trim().is_empty()) {
        return Json(json!({ "ok": true, "format": "text", "blocks": from_text(t) })).into_response();
    }
    let Some(data) = b["dataBase64"].as_str().and_then(decode_b64) else {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "Nothing to import" }))).into_response();
    };
    let filename = b["filename"].as_str().unwrap_or("").to_string();
    let assets = crate::propresenter::Assets::new(&app.cfg.app_data_dir);
    let result = tokio::task::spawn_blocking(move || import_slides(&filename, &data, &assets)).await.unwrap_or_else(|_| Err(ImportError("import failed".into(), None)));
    match result {
        Ok(Imported::Items { format, mut items, warnings }) => {
            if items.is_empty() {
                return unprocessable(ImportError("No presentations found in that playlist".into(), None));
            }
            let media = dedupe_image_layers(&mut items);
            Json(json!({ "ok": true, "format": format, "items": items, "media": media, "warnings": warnings })).into_response()
        }
        Ok(Imported::Blocks { format, mut blocks, note, warnings }) => {
            if blocks.is_empty() {
                return unprocessable(ImportError("No text found in that file".into(), None));
            }
            let media = dedupe_image_layers(&mut blocks);
            let mut out = json!({ "ok": true, "format": format, "blocks": blocks, "media": media, "warnings": warnings });
            if let Some(n) = note {
                out["note"] = json!(n);
            }
            Json(out).into_response()
        }
        Err(e) => unprocessable(e),
    }
}

async fn theme_import(State(app): State<Shared>, Json(b): Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(data) = b["dataBase64"].as_str().and_then(decode_b64) else {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "Nothing to import" }))).into_response();
    };
    let assets = crate::propresenter::Assets::new(&app.cfg.app_data_dir);
    match tokio::task::spawn_blocking(move || crate::import::from_pro_theme(&data, &assets)).await {
        Ok(Ok((themes, warnings))) => Json(json!({ "ok": true, "themes": themes, "warnings": warnings })).into_response(),
        Ok(Err(e)) => unprocessable(e),
        Err(_) => unprocessable(crate::import::ImportError("import failed".into(), None)),
    }
}

pub async fn connect_obs(app: &Shared) {
    let s = &app.settings;
    app.obs.connect(s.bool("obsEnabled").await == Some(true), s.str("obsUrl").await, s.str("obsPassword").await).await;
}

async fn obs_test(State(app): State<Shared>) -> Json<Value> {
    Json(Obs::test(app.settings.str("obsUrl").await, app.settings.str("obsPassword").await).await)
}
async fn obs_status(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "connected": app.obs.connected().await, "enabled": app.settings.bool("obsEnabled").await == Some(true) }))
}
async fn obs_connect(State(app): State<Shared>) -> Json<Value> {
    connect_obs(&app).await;
    Json(json!({ "ok": app.obs.connected().await }))
}
async fn obs_sources(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "sources": app.obs.text_sources().await }))
}

// ── Models ──────────────────────────────────────────────────────────────────

const OFFLINE_REQUIRED: [&str; 4] = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];
const OFFLINE_VERSION: &str = "v1";

fn offline_spec(model: &str) -> (&'static str, &'static str, u32) {
    if model == "multi" { ("multi", "sherpa-streaming-multi", 475) } else { ("en", "sherpa-streaming-en", 465) }
}

pub fn offline_present(app: &Shared, model: &str) -> bool {
    let dir = app.cfg.models_dir.join(offline_spec(model).1);
    OFFLINE_REQUIRED.iter().all(|f| dir.join(f).exists()) && std::fs::metadata(dir.join("encoder.int8.onnx")).is_ok_and(|m| m.len() > 5_000_000)
}

async fn offline_status(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    let (model, dir, mb) = offline_spec(q.get("model").map(String::as_str).unwrap_or("en"));
    let path = app.cfg.models_dir.join(dir);
    let installed = offline_present(&app, model);
    let version = std::fs::read_to_string(path.join(".kairo-model-version")).ok().map(|v| v.trim().to_string());
    let lang = app.settings.str("sttLanguage").await.unwrap_or_else(|| "en-US".into());
    let needed = if lang.to_lowercase().starts_with("en") { "en" } else { "multi" };
    Json(json!({
        "model": model, "installed": installed, "needsUpdate": installed && version.as_deref() != Some(OFFLINE_VERSION),
        "modelPath": path, "installing": crate::installer::installing(model), "progress": crate::installer::progress(model), "approxMB": mb, "needed": needed,
    }))
}

/// The meaning models ship with the app.
async fn semantic_status(State(app): State<Shared>) -> Json<Value> {
    let ready = app.meaning_ready();
    Json(json!({ "modelInstalled": true, "embeddingsBuilt": true, "installed": true, "installing": !ready }))
}
async fn reranker_status() -> Json<Value> {
    Json(json!({ "installed": true, "installing": false }))
}

async fn translate_languages() -> Json<Value> {
    Json(json!({ "languages": crate::translate::LANGUAGES.iter().map(|(code, name)| json!({ "code": code, "name": name })).collect::<Vec<_>>() }))
}

/// A file name safe on every system: no separators or control characters, at most 80 characters.
fn safe_file_name(t: &str) -> String {
    let s: String = t.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect()
}

/// `name` in `dir`, numbered if it's taken: "x (2).txt".
fn unique_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(name);
    let (stem, ext) = (p.file_stem().and_then(|s| s.to_str()).unwrap_or("file"), p.extension().and_then(|e| e.to_str()));
    let with = |n: Option<u32>| {
        let base = match n { Some(n) => format!("{stem} ({n})"), None => stem.to_string() };
        dir.join(match ext { Some(e) => format!("{base}.{e}"), None => base })
    };
    let mut c = with(None);
    let mut i = 2;
    while c.exists() {
        c = with(Some(i));
        i += 1;
    }
    c
}

/// The notes PDF: the transcript the UI captured, and the service's own
/// record of what went on screen and how (else the UI's list of verses).
async fn sermon_notes(app: &Shared, b: &Value, date: &str) -> Result<(String, Vec<u8>), String> {
    let Some((bible, _)) = app.ready() else { return Err("The detection engine is still loading".into()) };
    let bible = bible.clone();
    let mut session = crate::notes::Session {
        name: b["name"].as_str().unwrap_or("").to_string(),
        lines: b["transcript"].as_array().into_iter().flatten().filter_map(|t| Some(crate::notes::Line { text: t["text"].as_str()?.to_string(), at: t["at"].as_u64().unwrap_or(0) })).collect(),
        ..Default::default()
    };
    {
        let log = app.log.lock().unwrap();
        session.shown = log.shown.clone();
        session.ranges = log.ranges.clone();
    }
    if session.shown.is_empty() {
        for v in b["verses"].as_array().into_iter().flatten() {
            let Some((book, cv)) = v["ref"].as_str().and_then(|r| r.rsplit_once(' ')) else { continue };
            let Some((c, n)) = cv.split_once(':') else { continue };
            if let Some(id) = bible.find(book, c.parse().unwrap_or(0), n.parse().unwrap_or(0)) {
                session.shown.push(crate::notes::Shown { verse: id, at: v["at"].as_u64().unwrap_or(0), how: crate::notes::How::Cited });
            }
        }
    }
    if session.lines.is_empty() {
        return Err("Nothing captured yet — start listening first".into());
    }
    let meaning = app.meaning.get().cloned();
    let date = date.to_string();
    tokio::task::spawn_blocking(move || {
        let notes = crate::notes::build(&bible.k, &session, meaning.as_deref().map(|m| m as &dyn crate::notes::Meaning));
        let when = notes
            .started_at
            .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64))
            .map(|t| t.with_timezone(&chrono::Local).format("%A, %B %-d, %Y at %-I:%M %p").to_string())
            .unwrap_or_default();
        let pdf = crate::notes::render_pdf(&bible.k, &notes, &when);
        let title = safe_file_name(&notes.title);
        (format!("{} — {date}.pdf", if title.is_empty() { "Sermon Notes".to_string() } else { title }), pdf)
    })
    .await
    .map_err(|e| e.to_string())
}

async fn export(axum::extract::State(app): axum::extract::State<Shared>, Json(b): Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let list = |k: &str| b[k].as_array().cloned().unwrap_or_default();
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    let (filename, data): (String, Vec<u8>) = match b["kind"].as_str() {
        Some("transcript") => (format!("KAIRO transcript — {date}.txt"), (list("transcript").iter().map(|p| format!("[{}] {}", s(p, "time"), s(p, "text"))).collect::<Vec<_>>().join("\n") + "\n").into_bytes()),
        Some("verses") => (format!("KAIRO verses — {date}.txt"), list("verses").iter().map(|v| format!("{}  {}\n{}\n", s(v, "time"), s(v, "ref"), s(v, "text"))).collect::<Vec<_>>().join("\n").into_bytes()),
        Some("notes") => match sermon_notes(&app, &b, &date).await {
            Ok(x) => x,
            Err(e) => return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e }))).into_response(),
        },
        _ => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "Unknown export" }))).into_response(),
    };
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else {
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "No home folder" }))).into_response();
    };
    let dir = std::path::PathBuf::from(home).join("Downloads");
    let _ = std::fs::create_dir_all(&dir);
    let file = unique_path(&dir, &safe_file_name(&filename));
    if let Err(e) = std::fs::write(&file, data) {
        eprintln!("[Export] failed: {e}");
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&file).spawn();
    }
    Json(json!({ "ok": true, "path": file, "file": file.file_name().and_then(|n| n.to_str()) })).into_response()
}

/// The meaning models ship with the app: nothing to download.
async fn bundled_install() -> axum::response::Response {
    ndjson(vec![json!({ "phase": "done", "already": true }), json!({ "phase": "complete", "ok": true })])
}

fn ndjson(lines: Vec<Value>) -> axum::response::Response {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    axum::response::Response::builder().header("content-type", "application/x-ndjson").body(axum::body::Body::from(body)).unwrap()
}
