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
        .route("/api/stream/check", post(crate::stream_check::check))
        .route("/api/stream/ffmpeg", get(stream_ffmpeg))
        .route("/api/qr", get(crate::qr::matrix))
        .route("/api/offline/status", get(offline_status))
        .route("/api/offline/install", post(crate::installer::route))
        .route("/api/bible-packs/status", get(crate::bible_packs::status))
        .route("/api/bible-packs/catalog", get(crate::bible_packs::catalog))
        .route("/api/bible-packs/install", post(crate::bible_packs::install))
        .route("/api/bible-packs/remove", post(crate::bible_packs::remove))
        .route("/api/meaning-multi/status", get(crate::meaning_multi::status))
        .route("/api/meaning-multi/install", post(crate::meaning_multi::install_route))
        .route("/api/bibles/import", post(crate::bible_import::import).layer(axum::extract::DefaultBodyLimit::max(120 * 1024 * 1024)))
        .route("/api/bibles/mine", get(crate::bible_import::mine))
        .route("/api/api-bible/connect", post(crate::api_bible::connect))
        .route("/api/api-bible/bibles", get(crate::api_bible::bibles))
        .route("/api/api-bible/add", post(crate::api_bible::add))
        .route("/api/api-bible/mine", get(crate::api_bible::mine))
        .route("/api/api-bible/remove", post(crate::api_bible::remove))
        .route("/api/bibles/remove", post(crate::bible_import::remove))
        .route("/api/translate-model/status", get(crate::mt::status_route))
        .route("/api/semantic-model/status", get(semantic_status))
        .route("/api/reranker-model/status", get(reranker_status))
        .route("/api/translate/languages", get(translate_languages))
        .route("/api/service/import", post(service_import))
        .route("/api/export", post(export))
        .route("/api/export/places", get(export_places))
        .route("/api/export/save", post(export_save).layer(axum::extract::DefaultBodyLimit::max(400 * 1024 * 1024)))
        .route("/api/export/reveal", post(export_reveal))
        .route("/api/notes/capability", get(notes_capability))
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

use crate::util::{decode_b64, safe_file_name};

async fn service_import(State(app): State<Shared>, Json(b): Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    use crate::import::{dedupe_image_layers, from_text, import_files, ImportError, Imported};
    if let Some(t) = b["text"].as_str().filter(|t| !t.trim().is_empty()) {
        return Json(json!({ "ok": true, "format": "text", "blocks": from_text(t) })).into_response();
    }
    let Some(data) = b["dataBase64"].as_str().and_then(decode_b64) else {
        return crate::util::bad_request("Nothing to import");
    };
    let filename = b["filename"].as_str().unwrap_or("").to_string();
    // Files picked with it (an EasyWorship library is two): `companions: [{ filename, dataBase64 }]`.
    let companions: Vec<(String, Vec<u8>)> = b["companions"].as_array().into_iter().flatten()
        .filter_map(|c| Some((c["filename"].as_str()?.to_string(), c["dataBase64"].as_str().and_then(decode_b64)?)))
        .collect();
    // The request's base64 copy (up to 500 MB) isn't needed once decoded: freed before the
    // import unzips everything in it.
    drop(b);
    let assets = crate::propresenter::Assets::new(&app.cfg.app_data_dir);
    let result = tokio::task::spawn_blocking(move || import_files(&filename, &data, &companions, &assets)).await.unwrap_or_else(|_| Err(ImportError("import failed".into(), None)));
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
        return crate::util::bad_request("Nothing to import");
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

const OFFLINE_VERSION: &str = "v1";

/// The model's key and its download size here (English on Apple Silicon is the Core ML one).
fn offline_spec(model: &str) -> (&'static str, u32) {
    if model == "multi" { ("multi", 475) } else if crate::offline::uses_coreml("en") { ("en", 626) } else { ("en", 465) }
}

pub fn offline_present(app: &Shared, model: &str) -> bool {
    let model = offline_spec(model).0;
    crate::offline::present_for(model, &crate::offline::model_dir(app, model))
}

async fn offline_status(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    let (model, mb) = offline_spec(q.get("model").map(String::as_str).unwrap_or("en"));
    let path = crate::offline::model_dir(&app, model);
    let installed = offline_present(&app, model);
    let version = std::fs::read_to_string(path.join(".kairo-model-version")).ok().map(|v| v.trim().to_string());
    let lang = app.settings.str("sttLanguage").await.unwrap_or_else(|| "en-US".into());
    let needed = if lang.to_lowercase().starts_with("en") { "en" } else { "multi" };
    Json(json!({
        "model": model, "installed": installed, "needsUpdate": installed && version.as_deref() != Some(OFFLINE_VERSION),
        "modelPath": path, "installing": crate::installer::installing(model), "progress": crate::installer::progress(model), "approxMB": mb, "needed": needed,
        "engine": if model == "en" { crate::offline::engine_state() } else { "" },
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

/// The FFmpeg this copy of Kairo uses, for the app's streaming engine.
async fn stream_ffmpeg(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "path": app.video.ffmpeg_path().map(|p| p.to_string_lossy().to_string()) }))
}

async fn translate_languages() -> Json<Value> {
    Json(json!({ "languages": crate::translate::LANGUAGES.iter().map(|(code, name)| json!({ "code": code, "name": name })).collect::<Vec<_>>() }))
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
        let log = crate::util::lock(&app.log);
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
    let meaning = app.meaning.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let date = date.to_string();
    // The outline first: it's the notes on every machine, and the parts the writer is given.
    let name = session.name.clone();
    let (mut notes, segments, session) = {
        let bible = bible.clone();
        tokio::task::spawn_blocking(move || {
            let (notes, segments) = crate::notes::outline(&bible.k(), &session, meaning.as_deref().map(|m| m as &dyn crate::notes::Meaning));
            (notes, segments, session)
        })
        .await
        .map_err(|e| e.to_string())?
    };
    let _ = session;
    if !segments.is_empty() {
        match crate::notes_writer::available(app).await {
            Ok(()) => match crate::notes_writer::write(app, &segments).await {
                Ok(written) => crate::notes::apply_written(&mut notes, &name, &written),
                Err(e) => eprintln!("[Notes] the writer failed, outline instead: {e}"),
            },
            Err(why) => eprintln!("[Notes] outline only: {why}"),
        }
    }
    tokio::task::spawn_blocking(move || {
        let when = notes
            .started_at
            .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64))
            .map(|t| t.with_timezone(&chrono::Local).format("%A, %B %-d, %Y at %-I:%M %p").to_string())
            .unwrap_or_default();
        let pdf = crate::notes::render_pdf(&bible.k(), &notes, &when);
        let title = safe_file_name(&notes.title);
        (format!("{} — {date}.pdf", if title.is_empty() { "Sermon Notes".to_string() } else { title }), pdf)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Whether this machine writes the sermon notes (Apple Intelligence) or outlines them.
async fn notes_capability(State(app): State<Shared>) -> Json<Value> {
    match crate::notes_writer::available(&app).await {
        Ok(()) => Json(json!({ "written": true })),
        Err(why) => Json(json!({ "written": false, "reason": why })),
    }
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
        _ => return crate::util::bad_request("Unknown export"),
    };
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else {
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "No home folder" }))).into_response();
    };
    let dir = std::path::PathBuf::from(home).join("Downloads");
    let name = safe_file_name(&filename);
    let written = crate::util::blocking(move || {
        let _ = std::fs::create_dir_all(&dir);
        let file = crate::util::numbered_path(&dir, &name);
        std::fs::write(&file, data).map(|()| file)
    })
    .await;
    let file = match written {
        Ok(Ok(file)) => file,
        Ok(Err(e)) => {
            eprintln!("[Export] failed: {e}");
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response();
        }
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response(),
    };
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&file).spawn();
    }
    Json(json!({ "ok": true, "path": file, "file": file.file_name().and_then(|n| n.to_str()) })).into_response()
}

// ── Slide exports (PowerPoint, ProPresenter): saved where the operator chose, under the name they gave ──

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from)
}

/// Where exports go unless the operator picks another folder (Documents/Kairo Exports, made if missing),
/// and where ProPresenter keeps its media on this computer (a .probundle's pictures point there).
async fn export_places() -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(home) = home_dir() else {
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "No home folder" }))).into_response();
    };
    let exports = home.join("Documents").join("Kairo Exports");
    let made = exports.clone();
    let _ = crate::util::blocking(move || std::fs::create_dir_all(&made)).await;
    let media = home.join("Documents").join("ProPresenter").join("Media").join("Assets");
    Json(json!({ "exports": exports, "proMedia": media })).into_response()
}

/// Kinds of file the export writes; nothing else is written here.
const EXPORT_KINDS: [&str; 4] = ["pptx", "pro", "probundle", "kairo"];

/// The file in `dir` named `name`, if both are acceptable: an existing folder given as a full path, and a
/// plain file name (no folders in it) of a kind the export writes.
fn export_target(dir: &str, name: &str) -> Result<std::path::PathBuf, &'static str> {
    let dir = std::path::Path::new(dir);
    if !dir.is_absolute() || !dir.is_dir() {
        return Err("That folder can't be found.");
    }
    let clean = crate::util::safe_file_name(name);
    let clean = clean.trim().trim_start_matches('.').to_string();
    let ext = std::path::Path::new(&clean).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if clean.is_empty() || clean != name.trim() || !EXPORT_KINDS.contains(&ext.as_str()) {
        return Err("That file name can't be used.");
    }
    Ok(dir.join(clean))
}

#[derive(serde::Deserialize)]
struct SaveQuery {
    dir: String,
    name: String,
    #[serde(default, deserialize_with = "flag")]
    replace: bool,
}

/// A yes/no in a query string: `1` or `true` is yes (a plain bool would refuse `replace=1` and the replace never happened).
fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v = <String as serde::Deserialize>::deserialize(d)?;
    Ok(matches!(v.as_str(), "1" | "true"))
}

/// Writes the export's bytes to `dir`/`name`. A file already there is replaced only when asked (`replace`);
/// otherwise the answer says it exists, and the app asks the operator.
async fn export_save(Query(q): Query<SaveQuery>, body: axum::body::Bytes) -> axum::response::Response {
    use axum::response::IntoResponse;
    let path = match export_target(&q.dir, &q.name) {
        Ok(p) => p,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    };
    if path.exists() && !q.replace {
        return (axum::http::StatusCode::CONFLICT, Json(json!({ "exists": true }))).into_response();
    }
    let p = path.clone();
    match crate::util::blocking(move || {
        // Written beside, then moved into place: a half-written file never replaces a good one.
        let tmp = p.with_extension("kairo-part");
        std::fs::write(&tmp, &body).and_then(|()| std::fs::rename(&tmp, &p))
    })
    .await
    {
        Ok(Ok(())) => Json(json!({ "ok": true, "path": path })).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response(),
    }
}

/// Shows an exported file in Finder / Explorer (selected), or its folder elsewhere. Only a file of a kind
/// the export writes.
async fn export_reveal(Json(b): Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let path = std::path::PathBuf::from(b["path"].as_str().unwrap_or(""));
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if !path.is_absolute() || !path.is_file() || !EXPORT_KINDS.contains(&ext.as_str()) {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": "That file can't be found." }))).into_response();
    }
    #[cfg(target_os = "macos")]
    let r = crate::proc::Command::new("open").arg("-R").arg(&path).spawn();
    #[cfg(windows)]
    let r = crate::proc::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    let r = crate::proc::Command::new("xdg-open").arg(path.parent().unwrap_or(&path)).spawn();
    match r {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[cfg(test)]
mod export_tests {
    use super::{export_target, SaveQuery};

    #[test]
    fn the_replace_flag_reads_one_and_true() {
        let q = |s: &str| serde_urlencoded::from_str::<SaveQuery>(s).unwrap().replace;
        assert!(q("dir=/x&name=a.pro&replace=1"));
        assert!(q("dir=/x&name=a.pro&replace=true"));
        assert!(!q("dir=/x&name=a.pro"));
        assert!(!q("dir=/x&name=a.pro&replace=0"));
    }

    #[test]
    fn only_a_plain_name_of_an_export_kind_in_a_real_folder() {
        let dir = std::env::temp_dir();
        let d = dir.to_str().unwrap();
        assert!(export_target(d, "Sunday deck.pptx").is_ok());
        assert!(export_target(d, "Sunday deck.probundle").is_ok());
        assert!(export_target(d, "Sunday deck.kairo").is_ok());
        assert!(export_target(d, "../escape.pptx").is_err(), "no folders in the name");
        assert!(export_target(d, "notes.txt").is_err(), "only slide files");
        assert!(export_target(d, ".pptx").is_err());
        assert!(export_target("relative/dir", "a.pro").is_err(), "a full path only");
        assert!(export_target("/no/such/folder/here", "a.pro").is_err());
    }
}

/// The meaning models ship with the app: nothing to download.
async fn bundled_install() -> axum::response::Response {
    ndjson(vec![json!({ "phase": "done", "already": true }), json!({ "phase": "complete", "ok": true })])
}

fn ndjson(lines: Vec<Value>) -> axum::response::Response {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    axum::response::Response::builder().header("content-type", "application/x-ndjson").body(axum::body::Body::from(body)).unwrap()
}
