//! Kairo's backend: the HTTP routes and WebSocket the UI talks to (the same
//! contract the Node server served), the speech engines, detection and
//! outputs. Runs inside the Tauri app, or on its own for development
//! (`cargo run -p kairo-server`).

pub mod bible;
pub mod config;
pub mod detect;
pub mod hub;
mod routes;
pub mod settings;
mod ws;

use std::sync::{Arc, OnceLock, RwLock};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use kairo_bible::Corpus;
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use serde_json::json;

pub use config::Config;

use bible::Bible;
use detect::{Detector, Presentation};
use hub::Hub;
use settings::Settings;

pub struct App {
    pub cfg: Config,
    pub settings: Settings,
    pub hub: Arc<Hub>,
    /// Built in the background at startup; `None` until the engine is ready.
    pub bible: OnceLock<Arc<Bible>>,
    pub detector: OnceLock<Detector>,
    pub presentation: Arc<RwLock<Presentation>>,
    /// Where microphone audio goes while a speech engine is listening.
    pub audio: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<bytes::Bytes>>>,
}

pub type Shared = Arc<App>;

impl App {
    pub fn ready(&self) -> Option<(&Arc<Bible>, &Detector)> {
        Some((self.bible.get()?, self.detector.get()?))
    }

    /// Settings the decisions depend on, re-read after every change.
    pub async fn refresh_presentation(&self) {
        let s = &self.settings;
        let p = Presentation {
            translation: s.str("translation").await.unwrap_or_else(|| "KJV".into()),
            language: s.str("bibleLanguage").await.unwrap_or_else(|| "en".into()),
            auto_send: s.bool("autoSend").await != Some(false),
        };
        let (auto_send, auto_correct) = (p.auto_send, s.bool("autoCorrect").await != Some(false));
        *self.presentation.write().unwrap() = p;
        if let Some(d) = self.detector.get() {
            d.configure(move |c| {
                c.auto_send = auto_send;
                c.auto_correct = auto_correct;
            });
        }
    }
}

/// Serves until the process ends.
pub async fn serve(cfg: Config) -> anyhow::Result<()> {
    let settings = Settings::load(cfg.app_data_dir.join("settings.json"));
    let app: Shared = Arc::new(App {
        cfg: cfg.clone(),
        settings,
        hub: Arc::new(Hub::new()),
        bible: OnceLock::new(),
        detector: OnceLock::new(),
        presentation: Arc::new(RwLock::new(Presentation::default())),
        audio: tokio::sync::Mutex::new(None),
    });
    app.refresh_presentation().await;

    // The engine's knowledge takes a moment: listen first, announce readiness after.
    let starting = app.clone();
    let rt = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let t = std::time::Instant::now();
        let db = &starting.cfg.db_dir;
        let corpus = match Corpus::load(db) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Server] Bible data unavailable: {e}");
                starting.hub.broadcast(json!({ "type": "worker-error", "error": e.to_string() }));
                return;
            }
        };
        let nlt = corpus.load_map_nlt(&db.join("map.json")).ok();
        let niv = corpus.load_flat("NIV", &db.join("translations/NIV.json")).ok();
        let also: Vec<_> = [nlt.clone(), niv].into_iter().flatten().collect();
        let k = Arc::new(Knowledge::build(corpus, Wordings { own: None, also }));
        let nlt = nlt.unwrap_or_else(|| k.corpus.kjv().clone());
        let bible = Arc::new(Bible::new(k, nlt, db.clone()));
        let p = starting.presentation.read().unwrap().clone();
        let cfg = kairo_detect::engine::Config { auto_send: p.auto_send, ..Default::default() };
        let detector = Detector::start(bible.clone(), starting.hub.clone(), cfg, starting.presentation.clone(), rt);
        let _ = starting.bible.set(bible);
        let _ = starting.detector.set(detector);
        eprintln!("[Server] Detection engine ready in {:.1}s (reference + verbatim + stream + fingerprint).", t.elapsed().as_secs_f32());
        starting.hub.broadcast(json!({ "type": "worker-ready" }));
    });

    let router = routes::router()
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(axum::extract::DefaultBodyLimit::max(500 * 1024 * 1024))
        .with_state(app.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    eprintln!("[Server] Listening on http://127.0.0.1:{}{}", cfg.port, if cfg.auth_required() { " (token required)" } else { "" });
    axum::serve(listener, router).await?;
    Ok(())
}

fn is_static_asset(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    path == "/"
        || ["html", "css", "js", "map", "png", "jpg", "jpeg", "gif", "svg", "ico", "woff", "woff2", "ttf", "otf"]
            .iter()
            .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// Every request carries the token (header or `?token=`), except `/health`,
/// the UI's own files, and the WebSocket upgrade (checked in its handler).
async fn auth(State(app): State<Shared>, req: Request, next: Next) -> Response {
    if !app.cfg.auth_required() {
        return next.run(req).await;
    }
    let path = req.uri().path();
    if path == "/health" || is_static_asset(path) {
        return next.run(req).await;
    }
    if token_from(req.headers(), req.uri()).as_deref() == Some(app.cfg.token.as_str()) {
        return next.run(req).await;
    }
    (StatusCode::UNAUTHORIZED, axum::Json(json!({ "error": "unauthorized" }))).into_response()
}

pub(crate) fn token_from(headers: &axum::http::HeaderMap, uri: &axum::http::Uri) -> Option<String> {
    if let Some(b) = headers.get("authorization").and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")) {
        return Some(b.to_string());
    }
    query_param(uri.query().unwrap_or(""), "token")
}

pub(crate) fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(x) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(x);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
