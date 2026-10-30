//! The HTTP routes, as the Node server served them.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use kairo_bible::VerseId;
use kairo_detect::engine::Event;
use kairo_detect::reference::{Active, Lang, Verses};
use serde_json::{json, Map, Value};

use crate::detect::epoch_ms;
use crate::hub::Underneath;
use crate::bible::Bible;
use crate::{ws, Shared};

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/", get(ws::root))
        .route("/health", get(health))
        .route("/api/settings", get(get_settings).post(post_settings))
        .route("/api/test-transcript", post(test_transcript))
        .route("/api/start-listening", post(start_listening))
        .route("/api/stop-listening", post(stop_listening))
        .route("/api/audio-inputs", get(audio_inputs))
        .route("/api/audio-preview", post(audio_preview))
        .route("/api/capture", post(capture_open).get(capture_status))
        .route("/api/stop-capture", post(capture_stop))
        .route("/api/lookup", post(lookup))
        .route("/api/search", post(search))
        .route("/api/search/lookup", post(search_lookup))
        .route("/api/search/suggest", post(search_suggest))
        .route("/api/slides/scripture", post(slides_scripture))
        .route("/api/range/next", post(range_next))
        .route("/api/verse/step", post(verse_step))
        .route("/api/range/clear", post(range_clear))
        .route("/api/range/jump-to", post(range_jump))
        .route("/api/service/send", post(service_send))
        .route("/api/service/send-media", post(send_media))
        .route("/api/service/send-timer", post(send_timer))
        .route("/api/service/clear-layer", post(clear_layer_route))
        .route("/api/service/media-control", post(media_control))
        .route("/api/service/audio-output", post(audio_output))
        .route("/api/output/back", post(output_back))
        .route("/api/output/leave", post(output_leave))
        .route("/api/look/apply", post(look_apply))
        .route("/api/monitor-message", post(monitor_message))
        .route("/api/debug-log", post(debug_log))
        .route("/api/report/preview", get(crate::report::preview))
        .route("/api/report/send", post(crate::report::send))
        .route("/api/translate", post(crate::translate::route))
        .fallback(get(static_file))
}

/// The UI's page.
pub async fn index(app: &Shared) -> Response {
    serve_file(app, "index.html").await
}

async fn static_file(State(app): State<Shared>, uri: axum::http::Uri) -> Response {
    let path = percent_decode_path(uri.path());
    serve_file(&app, &path).await
}

fn percent_decode_path(p: &str) -> String {
    crate::query_param(&format!("p={}", p.replace('+', "%2B")), "p").unwrap_or_default()
}

async fn serve_file(app: &Shared, rel: &str) -> Response {
    let rel = rel.trim_start_matches('/');
    // Only plain names below the UI folder: no "..", hidden files, drive or
    // root prefixes, and no backslashes (a separator on Windows).
    let plain = std::path::Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if rel.contains('\\') || !plain || rel.split('/').any(|p| p.starts_with('.')) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = app.cfg.ui_dir.join(rel);
    match tokio::fs::read(&path).await {
        // The page and its scripts are read from disk on every request and must never be
        // kept by the web view: an old copy of a script or style left in its cache shows
        // up as "the app didn't change" after the files were updated.
        Ok(bytes) => ([(axum::http::header::CONTENT_TYPE, mime(rel)), (axum::http::header::CACHE_CONTROL, "no-store")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

async fn health(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "ok": true, "workerBasicReady": app.ready().is_some() }))
}

// ── Settings ────────────────────────────────────────────────────────────────

/// The Deepgram key is shown only by its first characters.
fn redacted(key: &str) -> String {
    let k = key.trim();
    if k.is_empty() { String::new() } else { format!("{}…", k.chars().take(8).collect::<String>()) }
}

async fn get_settings(State(app): State<Shared>) -> Json<Value> {
    let mut s = app.settings.snapshot().await;
    let key = s.get("deepgramApiKey").and_then(Value::as_str).unwrap_or("").to_string();
    s.insert("deepgramApiKey".into(), json!(redacted(&key)));
    s.insert("appVersion".into(), json!(env!("CARGO_PKG_VERSION")));
    Json(Value::Object(s))
}

async fn post_settings(State(app): State<Shared>, Json(body): Json<Map<String, Value>>) -> Response {
    // The operator choosing a translation ends a spoken one: nothing to put back later.
    if body.contains_key("translation") {
        crate::spoken::clear();
    }
    let mut incoming = body.clone();
    let current_key = app.settings.str("deepgramApiKey").await.unwrap_or_default();
    // The redacted key coming back unchanged isn't a new key.
    if let Some(k) = incoming.get("deepgramApiKey").and_then(Value::as_str) {
        let trimmed = k.trim().to_string();
        if (!current_key.is_empty() && k == redacted(&current_key)) || trimmed.is_empty() {
            incoming.remove("deepgramApiKey");
        } else {
            incoming.insert("deepgramApiKey".into(), json!(trimmed));
        }
    }
    let animation_changed = incoming.contains_key("outputAnimation") || incoming.contains_key("outputAnimationSpeed");
    let obs_changed = ["obsEnabled", "obsUrl", "obsPassword"].iter().any(|k| body.contains_key(*k));
    let (s, save_error) = app.settings.merge(incoming).await;
    app.refresh_presentation().await;
    if obs_changed {
        let a = app.clone();
        tokio::spawn(async move { crate::routes_misc::connect_obs(&a).await });
    }
    if animation_changed {
        app.hub.broadcast(json!({
            "type": "output-transition", "target": "viewer",
            "animation": s.get("outputAnimation").cloned().unwrap_or_else(|| json!("cut")),
            "speed": s.get("outputAnimationSpeed").cloned().unwrap_or_else(|| json!(1)),
        }));
    }
    // In use either way, but not on disk: say so, so the operator knows it won't survive a restart.
    if let Some(e) = save_error {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("settings couldn't be saved to disk ({e})") }))).into_response();
    }
    Json(json!({ "ok": true })).into_response()
}

// ── Detection input ─────────────────────────────────────────────────────────

async fn test_transcript(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let Some(text) = body["text"].as_str() else { return Json(json!({ "error": "no text" })) };
    let Some((_, d)) = app.ready() else { return Json(json!({ "error": "engine not ready" })) };
    let is_final = body["isFinal"] != false;
    d.speech(&app.hub, text, is_final, 1.0, is_final, None, false);
    Json(json!({ "ok": true }))
}

async fn start_listening(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let mut listening = app.listening.lock().await;
    if listening.as_ref().is_some_and(|l| !l.alive()) {
        *listening = None;
    }
    if listening.is_some() {
        return Json(json!({ "error": "Already listening" }));
    }
    let engine = body["engine"].as_str().unwrap_or("deepgram").to_lowercase();
    let spoken = app.settings.str("sttLanguage").await.unwrap_or_else(|| "en-US".into());
    if let Some((bible, _)) = app.ready() {
        let k = bible.k.clone();
        crate::interpretation::with(|i| i.set_known(std::sync::Arc::new(move |w: &str| k.lexicon.vocab().id(w).is_some())));
    }
    crate::interpretation::with(|i| i.reset(&spoken));
    let started = if engine == "offline" || engine == "browser" {
        crate::offline::start(app.clone()).await.map(crate::Listening::Offline)
    } else {
        if let Some((_, d)) = app.ready() {
            d.configure(|c| c.offline = false);
        }
        crate::deepgram::start(app.clone()).await.map(crate::Listening::Deepgram)
    };
    match started {
        Ok(session) => {
            *listening = Some(session);
            *crate::util::lock(&app.keep_awake) = Some(crate::KeepAwake::hold());
            // The page asked for the input to be captured here (native_audio.rs): open it. If it
            // can't be, the answer says so and the page captures it itself, as before.
            let mut native = Value::Null;
            if let Some(want) = body.get("nativeInput").and_then(|v| v.as_str()) {
                native = open_capture(&app, want).await;
            }
            let mut out = if engine == "offline" || engine == "browser" { json!({ "ok": true, "engine": "offline" }) } else { json!({ "ok": true }) };
            out["native"] = native;
            Json(out)
        }
        Err(e) => Json(e),
    }
}

/// The Settings meter while not listening: `{ input, on }` opens that input here only to report
/// its level (input-level broadcasts); `{ on: false }` closes it. While listening the meter
/// follows the listening capture instead.
async fn audio_preview(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let prev = crate::util::lock(&app.capture_preview).take();
    if let Some(c) = prev { let _ = tokio::task::spawn_blocking(move || c.stop()).await; }
    if body["on"] != json!(true) || crate::util::lock(&app.capture).is_some() {
        return Json(json!({ "ok": true }));
    }
    let want = body["input"].as_str().unwrap_or("").to_string();
    match crate::native_audio::start(app.clone(), &want, false) {
        Ok(c) => { let device = c.device.clone(); *crate::util::lock(&app.capture_preview) = Some(c); Json(json!({ "ok": true, "device": device })) }
        Err(e) => Json(json!({ "error": e })),
    }
}

/// Opens `want` (the system default when empty) as the listening capture, replacing any open
/// one: `{ device }` or `{ error }`.
async fn open_capture(app: &Shared, want: &str) -> Value {
    let old = crate::util::lock(&app.capture).take();
    if let Some(c) = old { let _ = tokio::task::spawn_blocking(move || c.stop()).await; }
    let prev = crate::util::lock(&app.capture_preview).take();
    if let Some(c) = prev { let _ = tokio::task::spawn_blocking(move || c.stop()).await; }
    let (a, w) = (app.clone(), want.to_string());
    let started = tokio::task::spawn_blocking(move || crate::native_audio::start(a, &w, true)).await;
    match started {
        Ok(Ok(c)) => {
            let device = c.device.clone();
            *crate::util::lock(&app.capture) = Some(c);
            write_debug_global("server:native-capture", json!({ "device": device }));
            json!({ "device": device })
        }
        Ok(Err(e)) => {
            write_debug_global("server:native-capture-failed", json!({ "error": e, "wanted": want }));
            json!({ "error": e })
        }
        Err(_) => json!({ "error": "the input couldn't be opened here" }),
    }
}

/// `{ input }`: (re)open the listening capture on that input while listening.
async fn capture_open(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    Json(open_capture(&app, body["input"].as_str().unwrap_or("")).await)
}

async fn capture_stop(State(app): State<Shared>) -> Json<Value> {
    let c = crate::util::lock(&app.capture).take();
    if let Some(c) = c { let _ = tokio::task::spawn_blocking(move || c.stop()).await; }
    Json(json!({ "ok": true }))
}

/// Whether the server is capturing the input (a window opened mid-session asks).
async fn capture_status(State(app): State<Shared>) -> Json<Value> {
    let device = crate::util::lock(&app.capture).as_ref().map(|c| c.device.clone());
    Json(json!({ "native": device }))
}

/// The sound inputs the server can capture itself, by name.
async fn audio_inputs() -> Json<Value> {
    let names = tokio::task::spawn_blocking(crate::native_audio::inputs).await.unwrap_or_default();
    Json(json!({ "inputs": names }))
}

async fn stop_listening(State(app): State<Shared>) -> Json<Value> {
    let capture = crate::util::lock(&app.capture).take();
    if let Some(c) = capture {
        let _ = tokio::task::spawn_blocking(move || c.stop()).await;
    }
    let session = app.listening.lock().await.take();
    match session {
        Some(crate::Listening::Deepgram(s)) => s.stop().await,
        Some(crate::Listening::Offline(s)) => {
            s.stop().await;
            crate::offline::stopped(&app).await;
        }
        None => {}
    }
    crate::util::lock(&app.keep_awake).take();
    Json(json!({ "ok": true }))
}

// ── Lookups and search ──────────────────────────────────────────────────────

fn not_ready() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "engine not ready" }))).into_response()
}

async fn lookup(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let Some((bible, _)) = app.ready() else { return not_ready() };
    let (Some(book), Some(chapter)) = (body["book"].as_str(), body["chapter"].as_u64()) else {
        return crate::util::bad_request("book and chapter required");
    };
    let verse = body["verse"].as_u64();
    match (verse, body["verseEnd"].as_u64()) {
        (Some(v), Some(e)) if e != v => {
            let results: Vec<Value> = bible.range(book, chapter, v, e).into_iter().map(|id| bible.verse_json(id, 1.0, "direct")).collect();
            Json(json!({ "results": results })).into_response()
        }
        (Some(v), _) => Json(json!({ "result": bible.find(book, chapter, v).map(|id| bible.verse_json(id, 1.0, "direct")) })).into_response(),
        _ => crate::util::bad_request("verse required"),
    }
}

/// A typed reference ("john 3 16", "rom 8:28-30"), tried as spoken and then
/// with "chapter" put after each leading word.
/// POST /api/slides/scripture {texts: [..]} → {refs: [[{book, chapter, verse,
/// verseEnd, raw}], ..]}: the scripture references written on each slide of a
/// deck ("– Eph. 2:6-8", "Matt. 21:23-27/ Mak. 11:28-33"), for the slide
/// follower (src/slide_follow.js) to line up with what is read and detected.
async fn slides_scripture(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    static WRITTEN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:\b[1-3]\s?)?\b[A-Za-z]{2,}\.?\s*\d{1,3}\s*:\s*\d{1,3}(?:\s*[-–]\s*\d{1,3})?").expect("reference pattern")
    });
    let Some((bible, _)) = app.ready() else { return Json(json!({ "refs": [] })) };
    let texts = body["texts"].as_array().cloned().unwrap_or_default();
    let refs: Vec<Value> = texts
        .iter()
        .map(|t| {
            let text = t.as_str().unwrap_or("");
            let found: Vec<Value> = WRITTEN
                .find_iter(text)
                .filter_map(|m| {
                    let raw = m.as_str();
                    let c = typed_reference(bible, &raw.replace('–', "-"))?;
                    let (verse, end) = match &c.verses {
                        Verses::One(v) => (*v, *v),
                        Verses::Span(a, b) => (*a, *b),
                        Verses::Spans(s) => (s.first()?.0, s.last()?.1),
                        Verses::Chapter => return None,
                    };
                    Some(json!({ "book": c.book.name(), "chapter": c.chapter, "verse": verse, "verseEnd": end, "raw": raw }))
                })
                .collect();
            Value::Array(found)
        })
        .collect();
    Json(json!({ "refs": refs }))
}

fn typed_reference(app: &crate::bible::Bible, query: &str) -> Option<kairo_detect::reference::Citation> {
    let q: String = query.trim().to_lowercase().chars().map(|c| if ".,!?;".contains(c) { ' ' } else { c }).collect();
    let parser = app.k.parser(Lang::En);
    if let Some(c) = parser.parse_one(&q, Active::default()) {
        return Some(c);
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.len() <= 8 && !words.contains(&"chapter") {
        for i in 1..words.len() {
            let t = [&words[..i], &["chapter"], &words[i..]].concat().join(" ");
            if let Some(c) = parser.parse_one(&t, Active::default()) {
                return Some(c);
            }
        }
    }
    None
}

fn citation_verses(bible: &crate::bible::Bible, c: &kairo_detect::reference::Citation) -> Vec<VerseId> {
    let book = c.book.name();
    let chapter = c.chapter as u64;
    match &c.verses {
        Verses::Chapter => bible.chapter(book, chapter),
        Verses::One(v) => bible.find(book, chapter, *v as u64).into_iter().collect(),
        Verses::Span(a, b) => bible.range(book, chapter, *a as u64, *b as u64),
        Verses::Spans(s) => s.iter().flat_map(|(a, b)| bible.range(book, chapter, *a as u64, *b as u64)).collect(),
    }
}

async fn search(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let Some((bible, d)) = app.ready() else { return not_ready() };
    let Some(query) = body["query"].as_str().filter(|q| !q.trim().is_empty()) else {
        return crate::util::bad_request("query required");
    };
    if let Some(c) = typed_reference(bible, query) {
        let verses = citation_verses(bible, &c);
        if !verses.is_empty() {
            let json_verses: Vec<Value> = verses.iter().map(|v| bible.verse_json(*v, 1.0, "direct")).collect();
            let method = if matches!(c.verses, Verses::One(_)) { "reference" } else { "range" };
            d.send(Event::OperatorCited(verses));
            let mut out = json!({ "result": json_verses[0], "method": method });
            if method == "range" {
                out["range"] = json!(json_verses);
            }
            return Json(out).into_response();
        }
    }
    let limit = body["limit"].as_u64().unwrap_or(8) as usize;
    let results: Vec<Value> = bible.text_search(query, limit).into_iter().map(|v| bible.verse_json(v, 0.9, "text")).collect();
    Json(json!({ "results": results, "method": "text" })).into_response()
}

async fn search_lookup(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let query = body["query"].as_str().unwrap_or("").trim().to_string();
    let limit = body["limit"].as_u64().unwrap_or(30).clamp(1, 60) as usize;
    let Some((bible, _)) = app.ready().filter(|_| !query.is_empty()) else { return Json(json!({ "results": [], "reference": false })) };
    let mut out: Vec<Value> = Vec::new();
    let mut reference = false;
    if let Some(c) = typed_reference(bible, &query) {
        for v in citation_verses(bible, &c).into_iter().take(limit) {
            let mut j = bible.verse_json(v, 1.0, "direct");
            j["match"] = json!("reference");
            out.push(j);
        }
        reference = !out.is_empty();
    }
    if !reference {
        for v in bible.text_search(&query, limit) {
            let mut j = bible.verse_json(v, 0.9, "text");
            j["match"] = json!("words");
            out.push(j);
        }
    }
    present(&app, bible, &mut out);
    Json(json!({ "results": out, "reference": reference }))
}

async fn search_suggest(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let query = body["query"].as_str().unwrap_or("").trim().to_string();
    let Some((bible, _)) = app.ready().filter(|_| query.split_whitespace().count() >= 3) else { return Json(json!({ "results": [] })) };
    let parser = bible.k.parser(Lang::En);
    let q: String = query.to_lowercase().chars().map(|c| if ".,!?;".contains(c) { ' ' } else { c }).collect();
    let mut out: Vec<Value> = Vec::new();
    if let Some(c) = parser.parse_one(&q, Active::default()) {
        if let Verses::One(v) = c.verses {
            if let Some(id) = bible.find(c.book.name(), c.chapter as u64, v as u64) {
                let mut j = bible.verse_json(id, 1.0, "direct");
                j["match"] = json!("reference");
                out.push(j);
            }
        }
    }
    if out.is_empty() {
        for h in kairo_detect::lexical::verbatim(&bible.k.lexicon, &query, 3, 6) {
            let mut j = bible.verse_json(h.verse, h.score, "verbatim");
            j["match"] = json!("phrase");
            out.push(j);
        }
    }
    present(&app, bible, &mut out);
    Json(json!({ "results": out }))
}

/// The church's translation and language on verses the operator will see.
fn present(app: &Shared, bible: &crate::bible::Bible, verses: &mut [Value]) {
    let p = app.presentation.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    bible.apply_translation(verses, &p.translation);
    bible.apply_language(verses, &p.language);
}

// ── Ranges ──────────────────────────────────────────────────────────────────

async fn range_next(State(app): State<Shared>) -> Json<Value> {
    let Some((_, d)) = app.ready() else { return Json(json!({ "ok": false, "reason": "no range active" })) };
    let active = crate::util::lock(&app.hub.live).range_state.clone();
    if active.is_none() {
        return Json(json!({ "ok": false, "reason": "no range active" }));
    }
    d.send(Event::RangeNext);
    Json(json!({ "ok": true, "verse": active.and_then(|r| r["next"]["reference"].as_str().map(str::to_string)) }))
}

/// Previous / Next under the monitor (owner, 2026-10-30): the verse before what is up, or after it,
/// across a chapter's end; Next inside a cited range moves the range as its own button did.
async fn verse_step(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let Some((bible, d)) = app.ready() else { return Json(json!({ "ok": false, "reason": "still loading" })) };
    let forward = body["dir"].as_i64().unwrap_or(1) >= 0;
    let (now, in_range) = {
        let l = crate::util::lock(&app.hub.live);
        (l.screen_now.clone(), l.range_state.is_some())
    };
    if forward && in_range {
        d.send(Event::RangeNext);
        return Json(json!({ "ok": true }));
    }
    let verses = now.as_ref().and_then(|n| n["verses"].as_array().cloned()).unwrap_or_default();
    let edge = if forward { verses.last() } else { verses.first() };
    let Some((book, chapter, verse)) = edge.and_then(|v| Some((v["book"].as_str()?.to_string(), v["chapter"].as_u64()?, v["verse"].as_u64()?))) else {
        return Json(json!({ "ok": false, "reason": "no verse on screen" }));
    };
    let step = if forward {
        bible.find(&book, chapter, verse + 1).or_else(|| bible.find(&book, chapter + 1, 1))
    } else if verse > 1 {
        bible.find(&book, chapter, verse - 1)
    } else {
        // The last verse of the chapter before.
        let last = chapter.checked_sub(1).filter(|c| *c > 0).map(|c| bible.chapter(&book, c));
        last.and_then(|vs| vs.last().copied())
    };
    let Some(v) = step else { return Json(json!({ "ok": false, "reason": if forward { "last verse of the book" } else { "first verse of the book" } })) };
    // Back inside a cited range: the window sends it as a range card's Send does (the range stays,
    // its position moves); anywhere else it's the operator citing that verse.
    if in_range {
        let mut list = vec![bible.verse_json(v, 1.0, "direct")];
        present(&app, bible, &mut list);
        return Json(json!({ "ok": true, "inRange": true, "send": list.remove(0) }));
    }
    d.send(Event::OperatorCited(vec![v]));
    Json(json!({ "ok": true, "verse": bible.reference(v) }))
}

async fn range_clear(State(app): State<Shared>) -> Json<Value> {
    if let Some((_, d)) = app.ready() {
        d.send(Event::RangeClear);
    }
    Json(json!({ "ok": true }))
}

async fn range_jump(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let Some((bible, d)) = app.ready() else { return Json(json!({ "ok": false, "reason": "no range active" })) };
    let v = body["book"].as_str().and_then(|b| bible.find(b, body["chapter"].as_u64().unwrap_or(0), body["verse"].as_u64().unwrap_or(0)));
    match v {
        Some(v) => {
            d.send(Event::RangeJump(v));
            Json(json!({ "ok": true }))
        }
        None => Json(json!({ "ok": false, "reason": "verse not in active range" })),
    }
}

// ── The output: slides, layers, media, timers ───────────────────────────────

fn clean_next(next: &Value) -> Value {
    if !next.is_object() {
        return Value::Null;
    }
    let s = |k: &str, n: usize| next[k].as_str().unwrap_or("").chars().take(n).collect::<String>();
    json!({ "text": s("text", 2000), "reference": s("reference", 200), "label": s("label", 60), "image": next["image"].as_bool().unwrap_or(false) })
}

/// Is a Live Queue slide really a Bible passage — one the Bible layer should
/// show instead? Only when the slide IS the passage: it opens with a
/// reference ("Romans 8:28 And we know…"), is nothing but a reference
/// ("Joel 2:1-11"), or its whole text is a verse word for word. A statement
/// that cites a verse at the end ("…lost it to abuse – Gen. 3:1-19") is a
/// slide like any other.
fn is_scripture_slide(bible: &crate::bible::Bible, verse: &Value) -> bool {
    let text = format!("{} {}", verse["reference"].as_str().unwrap_or(""), verse["text"].as_str().unwrap_or(""));
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return false;
    }
    let parser = bible.k.parser(Lang::En);
    let cites = |t: &str| parser.parse_all(t, Active::default()).into_iter().any(|c| !matches!(c.verses, Verses::Chapter));
    let opening = words.iter().take(4).copied().collect::<Vec<_>>().join(" ");
    if cites(&opening) {
        return true;
    }
    words.len() >= 6
        && kairo_detect::lexical::verbatim(&bible.k.lexicon, &words.join(" "), 6, 1).first().is_some_and(|h| h.score >= 0.95)
}

/// Puts a Live Queue slide (or a verse) up. Returns where it landed.
pub(crate) async fn send_service_slide(app: &Shared, mut verse: Value, look: Value, auto: bool, follow: bool, next: Value) -> &'static str {
    // A real verse has a book and a chapter; a playlist slide is sent as
    // book "Service", chapter 0 (display.html tells them apart the same way).
    let is_bible = verse["book"].as_str().is_some_and(|b| !b.is_empty() && b != "Service") && verse["chapter"].as_u64().unwrap_or(0) > 0;
    // A verse sent by hand (search, the Bible tab) carries no version label: the translation chosen,
    // shown after its reference on screen ("Ezekiel 12:21 (NLT)").
    if is_bible && verse["translation"].as_str().is_none_or(str::is_empty) && verse.is_object() {
        let p = app.presentation.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        verse["translation"] = json!(p.translation.to_uppercase());
    }
    if follow && !is_bible {
        if let Some((bible, _)) = app.ready() {
            if is_scripture_slide(bible, &verse) {
                eprintln!("[Service] Follower skipped a scripture slide — Bible detection handles it");
                return "scripture-slide";
            }
        }
    }
    if is_bible {
        app.hub.set_bible_on_top(true);
    } else {
        let on_top = {
            let mut l = crate::util::lock(&app.hub.live);
            l.underneath = Some(Underneath { verse: verse.clone(), look: look.clone(), next: next.clone() });
            l.bible_on_top
        };
        if on_top && auto && !follow {
            app.hub.layer_state();
            return "under-bible";
        }
        app.hub.set_bible_on_top(false);
    }
    if let Some((bible, d)) = app.ready() {
        match verse["book"].as_str().and_then(|b| bible.find(b, verse["chapter"].as_u64().unwrap_or(0), verse["verse"].as_u64().unwrap_or(0))) {
            Some(v) => {
                d.send(Event::OperatorSent(v));
                song_live(&app, false);
                crate::util::lock(&app.log).shown.push(crate::notes::Shown { verse: v, at: epoch_ms(), how: crate::notes::How::Cited });
            }
            None if !is_bible => {
                d.send(Event::NonScriptureLive);
                song_live(&app, true);
            }
            None => {}
        }
    }
    app.hub.broadcast(json!({
        "type": "detection", "target": "viewer", "method": "service",
        "verses": [verse], "topScore": 1, "look": if look.is_null() { Value::Null } else { look },
        "next": next, "timestamp": epoch_ms(),
    }));
    "sent"
}

async fn service_send(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let verse = body["verse"].clone();
    if verse.is_null() {
        return crate::util::bad_request("No slide provided");
    }
    // keepBible: a slide the follower moved to while a verse is still being
    // read goes under the verse instead of replacing it (src/service.js).
    if body["keepBible"] == true && verse["book"].as_str().is_none_or(|b| b == "Service") {
        let mut l = crate::util::lock(&app.hub.live);
        if l.bible_on_top {
            l.underneath = Some(Underneath { verse: verse.clone(), look: body["look"].clone(), next: clean_next(&body["next"]) });
            drop(l);
            app.hub.layer_state();
            return Json(json!({ "ok": true, "underBible": true, "skipped": Value::Null })).into_response();
        }
    }
    let landed = send_service_slide(&app, verse, body["look"].clone(), body["auto"] == true, body["follow"] == true, clean_next(&body["next"])).await;
    Json(json!({ "ok": true, "underBible": landed == "under-bible", "skipped": if landed == "scripture-slide" { json!("scripture") } else { Value::Null } })).into_response()
}

async fn send_media(State(app): State<Shared>, Json(body): Json<Value>) -> Response {
    let (Some(src), Some(kind)) = (body["src"].as_str(), body["kind"].as_str()) else {
        return crate::util::bad_request("src and kind required");
    };
    let fit = body["fit"].as_str().unwrap_or("contain");
    app.hub.broadcast(json!({ "type": "media", "target": "viewer", "src": src, "kind": kind, "fit": fit, "timestamp": epoch_ms() }));
    Json(json!({ "ok": true })).into_response()
}

async fn send_timer(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    if b["clear"] == true {
        app.hub.broadcast(json!({ "type": "timer-slide", "target": "viewer", "clear": true }));
    } else {
        let or_null = |k: &str| if b[k].is_null() { Value::Null } else { b[k].clone() };
        app.hub.broadcast(json!({
            "type": "timer-slide", "target": "viewer",
            "look": or_null("look"), "style": if b["style"].is_object() { b["style"].clone() } else { json!({}) },
            "label": or_null("label"), "timerText": b["timerText"].as_str().unwrap_or("0:00"),
            "scenes": or_null("scenes"), "pace": or_null("pace"), "resend": b["resend"] == true, "timestamp": epoch_ms(),
        }));
    }
    Json(json!({ "ok": true }))
}

/// Clears a layer: `bible` brings back the slide under it; `slide` under a
/// Bible only forgets the slide.
pub(crate) async fn clear_layer(app: &Shared, layer: &str) -> Value {
    if layer == "timer" || layer == "all" {
        app.segments.stop_live();
        for id in app.triggers.active_clocks() {
            app.triggers.stop(&id);
        }
    }
    if layer == "bible" {
        app.hub.set_bible_on_top(false);
        let under = crate::util::lock(&app.hub.live).underneath.clone();
        if let Some(u) = under {
            Box::pin(send_service_slide(app, u.verse, u.look, false, false, u.next)).await;
            return json!({ "restored": true });
        }
        app.hub.broadcast(json!({ "type": "clear-layer", "target": "viewer", "layer": "slide" }));
        screen_cleared(app);
        return json!({ "restored": false });
    }
    if layer == "slide" && crate::util::lock(&app.hub.live).bible_on_top {
        crate::util::lock(&app.hub.live).underneath = None;
        app.hub.layer_state();
        return json!({ "keptBible": true });
    }
    app.hub.broadcast(json!({ "type": "clear-layer", "target": "viewer", "layer": layer }));
    if layer == "slide" || layer == "all" {
        crate::util::lock(&app.hub.live).underneath = None;
        app.hub.set_bible_on_top(false);
        app.hub.layer_state();
        screen_cleared(app);
    }
    json!({})
}

fn screen_cleared(app: &Shared) {
    if let Some((_, d)) = app.ready() {
        d.send(Event::ScreenCleared);
    }
}

async fn clear_layer_route(State(app): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let layer = body["layer"].as_str().unwrap_or("all").to_string();
    let mut out = clear_layer(&app, &layer).await;
    out["ok"] = json!(true);
    Json(out)
}

/// Back pressed soon after Kairo put a verse up by itself: most likely a wrong send. Logged
/// (debug.log "server:likely-wrong-send", and server.log) with what was taken down and how it was
/// found, so real services show which detections go wrong.
const LIKELY_WRONG_WITHIN_MS: u64 = 20_000;

fn note_likely_wrong_send(app: &Shared, undone: Option<&Value>, restored: Option<&Value>) {
    let Some(m) = undone else { return };
    if m["type"] != "detection" { return; }
    let method = m["method"].as_str().unwrap_or("");
    // Sent by the operator (search, a click), not by detection.
    if method.is_empty() || method == "service" { return; }
    let at = m["timestamp"].as_u64().unwrap_or(0);
    let on_screen = epoch_ms().saturating_sub(at);
    if at == 0 || on_screen > LIKELY_WRONG_WITHIN_MS { return; }
    let reference = m["verses"][0]["reference"].as_str().unwrap_or("").to_string();
    let back_to = restored.and_then(|r| r["verses"][0]["reference"].as_str()).unwrap_or("").to_string();
    eprintln!("[Detect] Back {:.1}s after {reference} went up by itself ({method}): likely a wrong send", on_screen as f64 / 1000.0);
    write_debug_line(app, "server:likely-wrong-send", json!({
        "reference": reference, "method": method, "score": m["topScore"], "secondsOnScreen": (on_screen as f64 / 100.0).round() / 10.0,
        "correctedFrom": m["correctedFrom"], "backTo": back_to,
    }));
}

/// Back: undoes the last change to what's on screen.
/// An output covering the operator's own screen, left with its × (display.html): the operator
/// window takes it off that screen, as choosing None for it in Settings → Outputs does.
async fn output_leave(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    let Some(output) = b["output"].as_str().filter(|o| !o.is_empty()) else { return Json(json!({ "ok": false })) };
    app.hub.broadcast(json!({ "type": "output-leave", "output": output }));
    Json(json!({ "ok": true }))
}

async fn output_back(State(app): State<Shared>) -> Json<Value> {
    let (prev, undone) = {
        let mut l = crate::util::lock(&app.hub.live);
        let Some(prev) = l.history.pop_back() else { return Json(json!({ "ok": false, "reason": "Nothing earlier to go back to" })) };
        l.restoring = true;
        l.underneath = prev.underneath.clone();
        (prev, l.screen_now.clone())
    };
    match &prev.slide {
        Some(slide) => {
            let mut m = slide.clone();
            m["timestamp"] = json!(epoch_ms());
            m["restoredByBack"] = json!(true);
            app.hub.broadcast(m);
        }
        None => app.hub.broadcast(json!({ "type": "clear-layer", "target": "viewer", "layer": "slide", "restoredByBack": true })),
    }
    {
        let mut l = crate::util::lock(&app.hub.live);
        l.screen_now = prev.slide.clone();
        l.bible_on_top = prev.bible_on_top;
        l.restoring = false;
    }
    app.hub.layer_state();
    note_likely_wrong_send(&app, undone.as_ref(), prev.slide.as_ref());
    let restored_ref = prev.slide.as_ref().and_then(|s| s["verses"][0]["reference"].as_str().map(str::to_string));
    if let Some((bible, d)) = app.ready() {
        let id = |m: Option<&Value>| {
            let v = &m?["verses"][0];
            bible.find(v["book"].as_str()?, v["chapter"].as_u64()?, v["verse"].as_u64()?)
        };
        d.send(Event::Restored { up: id(prev.slide.as_ref()), taken_down: id(undone.as_ref()) });
    }
    Json(json!({ "ok": true, "restored": restored_ref }))
}

async fn media_control(State(app): State<Shared>, Json(b): Json<Value>) -> Response {
    let Some(action) = b["action"].as_str() else { return crate::util::bad_request("action required") };
    app.hub.broadcast(json!({ "type": "media-control", "target": "viewer", "action": action, "value": b["value"] }));
    Json(json!({ "ok": true })).into_response()
}

async fn audio_output(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    app.hub.broadcast(json!({ "type": "audio-output", "target": "viewer", "deviceId": if b["deviceId"].is_string() { b["deviceId"].clone() } else { Value::Null } }));
    Json(json!({ "ok": true }))
}

async fn look_apply(State(app): State<Shared>, Json(b): Json<Value>) -> Response {
    let (look, themes, layers, keying, modes) = (&b["look"], &b["themes"], &b["layers"], &b["keying"], &b["modes"]);
    let frames = &b["frames"];
    let layers_out = if layers.is_object() { layers.clone() } else { Value::Null };
    let count = |v: &Value| v.as_object().map_or(0, |o| o.len());
    if modes.is_object() && look.is_null() && themes.is_null() {
        app.hub.broadcast(json!({ "type": "look-update", "modes": modes }));
        return Json(json!({ "ok": true, "outputs": count(modes) })).into_response();
    }
    if themes.is_object() {
        crate::util::lock(&app.hub.live).output_themes = themes.clone();
        app.hub.broadcast(json!({ "type": "look-update", "themes": themes, "look": look, "layers": layers_out }));
        return Json(json!({ "ok": true, "outputs": count(themes) })).into_response();
    }
    if layers.is_object() && look.is_null() {
        app.hub.broadcast(json!({ "type": "look-update", "layers": layers }));
        return Json(json!({ "ok": true, "outputs": count(layers) })).into_response();
    }
    // A Monitor output's layout (a Monitor layout from Theme Studio, or none for the built-in).
    let monitors = &b["monitors"];
    if monitors.is_object() && look.is_null() && themes.is_null() {
        app.hub.broadcast(json!({ "type": "look-update", "monitors": monitors }));
        return Json(json!({ "ok": true, "outputs": count(monitors) })).into_response();
    }
    // A layout's frame (Director mode): the screen around the slide or verse, per output.
    if frames.is_object() && look.is_null() && themes.is_null() {
        app.hub.broadcast(json!({ "type": "look-update", "frames": frames }));
        return Json(json!({ "ok": true, "outputs": count(frames) })).into_response();
    }
    if keying.is_object() && look.is_null() {
        app.hub.broadcast(json!({ "type": "look-update", "keying": keying }));
        return Json(json!({ "ok": true, "outputs": count(keying) })).into_response();
    }
    if !look.is_object() {
        return crate::util::bad_request("No look provided");
    }
    app.hub.broadcast(json!({ "type": "look-update", "look": look, "layers": layers_out }));
    Json(json!({ "ok": true })).into_response()
}

/// A message from the booth: to the Monitor outputs (the stage), the audience screens, or both.
async fn monitor_message(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    let text: String = b["text"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    let to = match b["to"].as_str() { Some("screens") => "screens", Some("all") => "all", _ => "monitors" };
    app.hub.broadcast(json!({ "type": "monitor-message", "text": text, "to": to }));
    Json(json!({ "ok": true, "text": text, "to": to }))
}

/// Where debug.log is, so code without the app state (the detection thread)
/// can log too. Set once at start.
static DEBUG_LOG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
/// The one thread that writes debug.log, fed in order: lines land in the order they were
/// logged, and logging costs a queue push, not a thread per line (which reordered lines and
/// piled threads up under load). Any thread can log, async or not.
static DEBUG_LOG_TX: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<(std::path::PathBuf, String)>>> = std::sync::OnceLock::new();
fn debug_log_sender() -> &'static std::sync::Mutex<std::sync::mpsc::Sender<(std::path::PathBuf, String)>> {
    DEBUG_LOG_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(std::path::PathBuf, String)>();
        let _ = std::thread::Builder::new().name("debug-log".into()).spawn(move || {
            use std::io::Write;
            for (path, line) in rx {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                    let _ = f.write_all(line.as_bytes());
                }
                // It keeps itself small: past 8 MB it becomes debug.log.1 (the one before goes) and a
                // new one starts, so a whole service is always in the two files. Halving it in place
                // lost the start of a long run, and rewrote 2 MB on every line once full.
                if std::fs::metadata(&path).is_ok_and(|m| m.len() > DEBUG_LOG_MAX_BYTES) {
                    let _ = std::fs::rename(&path, path.with_extension("log.1"));
                }
            }
        });
        std::sync::Mutex::new(tx)
    })
}

/// debug.log rolls over to debug.log.1 past this size.
const DEBUG_LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn set_debug_log_path(path: std::path::PathBuf) {
    let _ = DEBUG_LOG_PATH.set(path);
}

/// One line in debug.log (app data). A debugging aid, not an audit log: it
/// keeps itself small.
pub(crate) fn write_debug_line(app: &Shared, event: &str, data: Value) {
    write_debug_to(app.cfg.app_data_dir.join("debug.log"), event, data);
}

/// The same, from code that has no app state (needs `set_debug_log_path` first).
pub(crate) fn write_debug_global(event: &str, data: Value) {
    if let Some(path) = DEBUG_LOG_PATH.get() {
        write_debug_to(path.clone(), event, data);
    }
}

fn write_debug_to(path: std::path::PathBuf, event: &str, data: Value) {
    let line = format!("{}\n", json!({ "t": epoch_ms(), "event": event, "data": data }));
    let tx = crate::util::lock(&debug_log_sender()).clone();
    let _ = tx.send((path, line));
}

async fn debug_log(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    if let Some(event) = b["event"].as_str() {
        write_debug_line(&app, &format!("client:{event}"), b["data"].clone());
    }
    Json(json!({ "ok": true }))
}

/// A song (or other non-scripture) slide going up or coming down: an interpreter is not expected while it plays.
fn song_live(app: &Shared, live: bool) {
    if let Some(change) = crate::interpretation::with(|i| i.set_song_live(live)) {
        crate::detect::announce_interpretation(&app.hub, change);
    }
}

/// A verse on screen this recently is the one the spoken translation was asked for.
const SPOKEN_FOR_SCREEN_MS: u64 = 15_000;

/// The spoken translation's passage is over: the church's translation again, the windows' selects
/// with it, and what is on screen shown in it (an operator's send may have gone up in the spoken one).
/// The verse going up belongs elsewhere than the translation asked for, or back in its passage (spoken.rs).
pub(crate) async fn restore_translation(app: &Shared, code: String) {
    write_debug_global("server:translation-restored", json!({ "code": code }));
    set_translation(app, &code, &format!("{code} for this passage")).await;
}

/// The preacher asked for another translation ("switch to the NIV", "can I have it in NLT"):
/// it becomes the translation for this passage (spoken.rs), and the verse on screen is shown again
/// in it. One this install doesn't have (the Message) is only logged.
/// Bundled, or a Bible the church added (a file, or API.Bible) under that code: an NIV of its own.
fn translation_available(app: &Shared, bible: &Bible, code: &str) -> bool {
    code == "KJV" || bible.translation_path(code).is_file() || crate::api_bible::connected(&app.cfg.app_data_dir, code).is_some()
}

/// A saved translation whose Bible is gone (NIV, NLT, ESV, NASB and NKJV stopped shipping, 2026-10; a Bible
/// removed in Settings) goes back to the KJV, so a verse is never labelled with a version it isn't in.
pub(crate) async fn drop_missing_translation(app: &Shared) {
    let Some((bible, _)) = app.ready() else { return };
    let Some(code) = app.settings.str("translation").await.map(|c| c.to_uppercase()) else { return };
    if code.is_empty() || translation_available(app, bible, &code) {
        return;
    }
    write_debug_global("server:translation-missing", json!({ "code": code }));
    let mut patch = Map::new();
    patch.insert("translation".into(), json!("KJV"));
    let _ = app.settings.merge(patch).await;
    app.refresh_presentation().await;
}

pub(crate) async fn spoken_translation(app: &Shared, code: &'static str, heard: String) {
    let Some((bible, _)) = app.ready() else { return };
    let available = translation_available(app, bible, code);
    let current = app.settings.str("translation").await.unwrap_or_else(|| "KJV".into());
    write_debug_global("server:translation-spoken", json!({ "code": code, "available": available, "from": current }));
    if !available || current.eq_ignore_ascii_case(code) {
        return;
    }
    // For this passage only: the verse on screen if it went up just now (the one cited with the
    // request), else the next one sent.
    let now = crate::util::lock(&app.hub.live).screen_now.clone();
    let passage = now.as_ref().filter(|n| crate::util::epoch_ms().saturating_sub(n["timestamp"].as_u64().unwrap_or(0)) < SPOKEN_FOR_SCREEN_MS).and_then(|n| {
        let v = n["verses"].get(0)?;
        Some((v["book"].as_str()?.to_string(), v["chapter"].as_u64()?))
    });
    crate::spoken::begin(current, code, passage);
    set_translation(app, code, &heard).await;
}

/// The translation changed by voice: the setting, the windows' selects ("translation-changed",
/// app.js), and the verse on screen repainted in it (no new send).
async fn set_translation(app: &Shared, code: &str, heard: &str) {
    let Some((bible, _)) = app.ready() else { return };
    let mut patch = Map::new();
    patch.insert("translation".into(), json!(code));
    let _ = app.settings.merge(patch).await;
    app.refresh_presentation().await;
    app.hub.broadcast(json!({ "type": "translation-changed", "translation": code, "heard": heard }));
    let now = crate::util::lock(&app.hub.live).screen_now.clone();
    let Some(now) = now else { return };
    let ids: Vec<_> = now["verses"].as_array().into_iter().flatten()
        .filter_map(|v| bible.find(v["book"].as_str()?, v["chapter"].as_u64()?, v["verse"].as_u64()?))
        .collect();
    if ids.is_empty() {
        return;
    }
    let p = app.presentation.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let mut list: Vec<Value> = ids.iter().map(|v| bible.verse_json(*v, 1.0, now["method"].as_str().unwrap_or("direct"))).collect();
    bible.apply_translation(&mut list, &p.translation);
    bible.apply_language(&mut list, &p.language);
    let mut msg = now.clone();
    msg["verses"] = json!(list);
    msg["retranslated"] = json!(true);
    msg["timestamp"] = json!(crate::util::epoch_ms());
    app.hub.broadcast(msg);
}
