//! Kairo's backend: the HTTP routes and WebSocket the UI talks to (the same
//! contract the Node server served), the speech engines, detection and
//! outputs. Runs inside the Tauri app, or on its own for development
//! (`cargo run -p kairo-server`).

pub mod bible;
pub mod config;
mod deepgram;
pub mod detect;
pub mod hub;
mod clock;
mod library;
mod media;
mod import;
mod installer;
pub mod notes;
mod offline;
mod obs;
mod pdf;
mod pdf_metrics;
mod routes;
mod routes_library;
mod routes_misc;
mod translate;
pub mod settings;
mod vocabulary;
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
    pub listening: tokio::sync::Mutex<Option<Listening>>,
    pub(crate) keep_awake: std::sync::Mutex<Option<KeepAwake>>,
    pub(crate) songs: library::Songs,
    pub(crate) triggers: Arc<library::Triggers>,
    pub(crate) segments: library::Segments,
    pub(crate) media: Arc<media::Media>,
    pub(crate) obs: obs::Obs,
    pub(crate) translator: translate::Translator,
    pub(crate) meaning: OnceLock<Arc<kairo_semantic::MeaningService>>,
    pub(crate) log: detect::Log,
}

/// The speech engine that's listening.
pub enum Listening {
    Deepgram(deepgram::Session),
    Offline(offline::Session),
}

impl Listening {
    fn alive(&self) -> bool {
        match self {
            Listening::Deepgram(s) => s.alive(),
            Listening::Offline(s) => s.alive(),
        }
    }
}

/// Keeps the Mac awake while listening (a service mustn't sleep mid-sermon).
pub(crate) struct KeepAwake(Option<std::process::Child>);

impl KeepAwake {
    pub(crate) fn hold() -> KeepAwake {
        #[cfg(target_os = "macos")]
        {
            let pid = std::process::id().to_string();
            return KeepAwake(std::process::Command::new("caffeinate").args(["-di", "-w", &pid]).spawn().ok());
        }
        #[allow(unreachable_code)]
        KeepAwake(None)
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.kill();
        }
    }
}

pub type Shared = Arc<App>;

impl App {
    pub fn meaning_ready(&self) -> bool {
        self.meaning.get().is_some()
    }

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
    let hub = Arc::new(Hub::new());
    let triggers = Arc::new(library::Triggers::open(&cfg.app_data_dir, hub.clone()));
    let app: Shared = Arc::new(App {
        cfg: cfg.clone(),
        settings,
        songs: library::Songs::open(&cfg.app_data_dir),
        segments: library::Segments::open(&cfg.app_data_dir, triggers.clone()),
        triggers,
        media: media::Media::open(&cfg.app_data_dir, hub.clone()),
        obs: obs::Obs::new(),
        translator: translate::Translator::default(),
        meaning: OnceLock::new(),
        log: Default::default(),
        hub,
        bible: OnceLock::new(),
        detector: OnceLock::new(),
        presentation: Arc::new(RwLock::new(Presentation::default())),
        audio: tokio::sync::Mutex::new(None),
        listening: tokio::sync::Mutex::new(None),
        keep_awake: std::sync::Mutex::new(None),
    });
    app.refresh_presentation().await;
    let obs_app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        routes_misc::connect_obs(&obs_app).await;
        follow_screen_in_obs(obs_app).await;
    });

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
        let detector = Detector::start(bible.clone(), starting.hub.clone(), cfg, starting.presentation.clone(), rt, starting.log.clone());
        let _ = starting.bible.set(bible);
        let _ = starting.detector.set(detector);
        eprintln!("[Server] Detection engine ready in {:.1}s (reference + verbatim + stream + fingerprint).", t.elapsed().as_secs_f32());
        starting.hub.broadcast(json!({ "type": "worker-ready" }));
        load_meaning(&starting);
    });

    let router = routes::router().merge(routes_library::router()).merge(routes_misc::router())
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(axum::extract::DefaultBodyLimit::max(500 * 1024 * 1024))
        .with_state(app.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await?;
    eprintln!("[Server] Listening on http://127.0.0.1:{}{}", cfg.port, if cfg.auth_required() { " (token required)" } else { "" });
    axum::serve(listener, router).await?;
    Ok(())
}

/// What goes on screen is written into OBS's text source, and cleared with it.
async fn follow_screen_in_obs(app: Shared) {
    let mut rx = app.hub.subscribe();
    loop {
        let m = match rx.recv().await {
            Ok(m) => m,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        if !app.obs.connected().await {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&m) else { continue };
        let source = app.settings.str("obsTextSource").await.unwrap_or_else(|| "Scripture".into());
        match v["type"].as_str() {
            Some("detection") if v["target"] == "viewer" => {
                let auto = app.presentation.read().unwrap().auto_send;
                if v["method"] != "service" && !auto {
                    continue;
                }
                let verse = &v["verses"][0];
                let t = app.settings.str("translation").await.unwrap_or_else(|| "KJV".into());
                let text = format!("{}\n{} ({t})", verse["text"].as_str().unwrap_or(""), verse["reference"].as_str().unwrap_or(""));
                app.obs.set_text(&source, &text).await;
            }
            Some("clear-layer") if matches!(v["layer"].as_str(), Some("slide") | Some("all")) => app.obs.set_text(&source, "").await,
            _ => {}
        }
    }
}

/// The meaning models (embedding model, verse index, cross-encoder): the
/// paraphrase and meaning paths turn on once they're loaded.
fn load_meaning(app: &Shared) {
    use kairo_semantic::{Embedder, EmbedderConfig, MeaningService, Pooling, Reranker, RerankerConfig, VerseIndex};
    let t = std::time::Instant::now();
    let db = &app.cfg.db_dir;
    let gemma = db.join("model/onnx-community/embeddinggemma-300m-ONNX");
    let cfg = EmbedderConfig {
        name: "gemma-legacy".into(),
        model: gemma.join("onnx/model_q4.onnx"),
        tokenizer: gemma.join("tokenizer.json"),
        pooling: Pooling::Mean,
        query_prefix: String::new(),
        document_prefix: String::new(),
        truncate_dims: 0,
        max_tokens: 256,
        threads: 0,
    };
    let index_name = format!("verse_index.{}.kvx", cfg.name);
    let embedder = match Embedder::load(cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[Server] Meaning model unavailable ({e}) — meaning-based detection off.");
            return;
        }
    };
    let built = app.cfg.app_data_dir.join("models").join(&index_name);
    let index = match VerseIndex::load(&db.join(&index_name)).or_else(|_| VerseIndex::load(&built)) {
        Ok(i) => i,
        Err(_) => {
            let Some(bible) = app.bible.get() else { return };
            eprintln!("[Server] Building the verse index (first run, a few minutes)…");
            let cache = app.cfg.app_data_dir.join("models");
            let b = kairo_semantic::Builder { embedder: &embedder, corpus: &bible.k.corpus, cache_dir: cache };
            match b.build(&[bible.k.corpus.kjv()], |_, _, _| {}) {
                Ok(i) => {
                    let _ = std::fs::create_dir_all(built.parent().unwrap_or(db));
                    let _ = i.save(&built);
                    i
                }
                Err(e) => {
                    eprintln!("[Server] Verse index build failed ({e}) — meaning-based detection off.");
                    return;
                }
            }
        }
    };
    let rr_dir = db.join("reranker_model/Xenova/ms-marco-MiniLM-L-12-v2");
    let reranker = Reranker::load(RerankerConfig { name: "minilm-l12".into(), model: rr_dir.join("onnx/model_int8.onnx"), tokenizer: rr_dir.join("tokenizer.json"), max_tokens: 256, threads: 0 }).ok();
    let Some((bible, detector)) = app.ready() else { return };
    let mut modern = vec![bible.nlt.clone()];
    if let Ok(niv) = bible.k.corpus.load_flat("NIV", &db.join("translations/NIV.json")) {
        modern.push(niv);
    }
    let m = Arc::new(MeaningService::new(embedder, index, reranker, bible.k.corpus.kjv().clone(), modern));
    detector.meaning_ready(m.clone());
    let _ = app.meaning.set(m);
    eprintln!("[Server] Meaning-based detection ready in {:.1}s.", t.elapsed().as_secs_f32());
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
