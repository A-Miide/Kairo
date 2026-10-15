//! Overlays: pictures and videos (a logo, a "Live" bug, a lower-third image) that
//! sit on top of whatever is on screen and stay up while slides, scripture and
//! media change. Any number can be on at once. Each has a place, a size and an
//! opacity the operator sets, and can come on by itself when Kairo opens.
//!
//! The files and their settings are saved; which ones are on right now is not
//! (it starts from each overlay's "show when Kairo opens"). Every change is
//! broadcast so the output screens and the Monitor redraw.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::extract::{Path as UrlPath, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::detect::epoch_ms;
use crate::hub::Hub;
use crate::Shared;

const MAX_OVERLAYS: usize = 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overlay {
    pub id: String,
    pub name: String,
    /// File name inside the overlays folder.
    pub file: String,
    /// "image" or "video".
    pub kind: String,
    /// Centre of the overlay, as a percent of the screen's width and height.
    pub x: f64,
    pub y: f64,
    /// Width as a percent of the screen's width; the height follows the picture.
    pub w: f64,
    pub opacity: f64,
    #[serde(default)]
    pub show_on_launch: bool,
    /// The set (a named arrangement, like "Sunday" or "Youth night") it belongs to.
    #[serde(default = "main_set")]
    pub set: String,
    /// On the screens right now (not saved).
    #[serde(skip)]
    pub live: bool,
}

fn main_set() -> String {
    "Main".into()
}

pub struct Overlays {
    dir: PathBuf,
    manifest: PathBuf,
    items: Mutex<Vec<Overlay>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { lo }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Overlays {
    pub fn open(app_data: &Path) -> Overlays {
        let dir = app_data.join("overlays");
        let manifest = dir.join("overlays.json");
        let mut items: Vec<Overlay> = std::fs::read(&manifest).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        items.retain(|o| dir.join(&o.file).is_file());
        for o in &mut items {
            o.live = o.show_on_launch;
        }
        Overlays { dir, manifest, items: Mutex::new(items) }
    }

    fn save(&self, items: &[Overlay]) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(&self.manifest, serde_json::to_vec_pretty(items).unwrap_or_default());
    }

    pub fn list(&self) -> Vec<Value> {
        lock(&self.items).iter().map(view).collect()
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        let real = self.dir.join(name).canonicalize().ok()?;
        let root = self.dir.canonicalize().ok()?;
        (real.starts_with(&root) && real != root && real.extension().is_some_and(|e| e != "json")).then_some(real)
    }

    /// Adds a picture or video; it starts off, bottom-right, at a quarter of the screen's width.
    pub fn add(&self, filename: &str, data: &[u8]) -> Result<Value, (&'static str, String)> {
        let base = Path::new(filename).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        let kind = crate::media::kind_for(&base).ok_or(("BAD_TYPE", format!("\"{base}\" isn't a picture or video (png, jpg, gif, webp, svg, mp4, webm, mov).")))?;
        if lock(&self.items).len() >= MAX_OVERLAYS {
            return Err(("TOO_MANY", format!("Up to {MAX_OVERLAYS} overlays.")));
        }
        let mut rnd = [0u8; 4];
        getrandom::fill(&mut rnd).ok();
        let id = format!("{}{}", epoch_ms(), rnd.iter().map(|b| format!("{b:02x}")).collect::<String>());
        let file = format!("{id}-{base}");
        std::fs::create_dir_all(&self.dir).map_err(|e| ("WRITE_FAILED", e.to_string()))?;
        std::fs::write(self.dir.join(&file), data).map_err(|e| {
            let _ = std::fs::remove_file(self.dir.join(&file));
            ("WRITE_FAILED", e.to_string())
        })?;
        let name = Path::new(&base).file_stem().and_then(|s| s.to_str()).unwrap_or("Overlay").to_string();
        let o = Overlay { id, name, file, kind: kind.to_string(), x: 88.0, y: 10.0, w: 18.0, opacity: 1.0, show_on_launch: false, set: main_set(), live: false };
        let v = view(&o);
        let mut items = lock(&self.items);
        items.push(o);
        self.save(&items);
        Ok(v)
    }

    /// Applies the fields present in `b`; false when there is no such overlay.
    pub fn update(&self, id: &str, b: &Value) -> bool {
        let mut items = lock(&self.items);
        let Some(o) = items.iter_mut().find(|o| o.id == id) else { return false };
        if let Some(n) = b["name"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
            o.name = n.chars().take(60).collect();
        }
        if let Some(v) = b["x"].as_f64() { o.x = clamp(v, 0.0, 100.0); }
        if let Some(v) = b["y"].as_f64() { o.y = clamp(v, 0.0, 100.0); }
        if let Some(v) = b["w"].as_f64() { o.w = clamp(v, 2.0, 100.0); }
        if let Some(v) = b["opacity"].as_f64() { o.opacity = clamp(v, 0.0, 1.0); }
        if let Some(v) = b["showOnLaunch"].as_bool() { o.show_on_launch = v; }
        if let Some(v) = b["live"].as_bool() { o.live = v; }
        if let Some(n) = b["set"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
            o.set = n.chars().take(40).collect();
        }
        self.save(&items);
        true
    }

    /// Puts every overlay in a set on or off the screens together.
    pub fn set_live(&self, set: &str, on: bool) {
        for o in lock(&self.items).iter_mut().filter(|o| o.set == set) {
            o.live = on;
        }
    }

    /// Takes every overlay off the screens.
    pub fn clear_live(&self) {
        for o in lock(&self.items).iter_mut() {
            o.live = false;
        }
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut items = lock(&self.items);
        let Some(pos) = items.iter().position(|o| o.id == id) else { return false };
        let o = items.remove(pos);
        let _ = std::fs::remove_file(self.dir.join(&o.file));
        self.save(&items);
        true
    }
}

fn view(o: &Overlay) -> Value {
    json!({
        "id": o.id, "name": o.name, "kind": o.kind, "url": format!("/api/overlays/file/{}", urlencode(&o.file)),
        "x": o.x, "y": o.y, "w": o.w, "opacity": o.opacity, "showOnLaunch": o.show_on_launch, "set": o.set, "live": o.live,
    })
}

fn changed(app: &Shared) {
    notify(&app.hub, &app.overlays);
}

fn notify(hub: &Hub, o: &Overlays) {
    hub.broadcast(json!({ "type": "overlays", "items": o.list() }));
}

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/api/overlays", get(list))
        .route("/api/overlays/upload", post(upload))
        .route("/api/overlays/clear", post(clear))
        .route("/api/overlays/set-live", post(set_live))
        .route("/api/overlays/file/{name}", get(file))
        .route("/api/overlays/{id}", post(update).delete(remove_one))
}

async fn list(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "ok": true, "items": app.overlays.list() }))
}

async fn upload(State(app): State<Shared>, b: Option<Json<Value>>) -> Response {
    let b = b.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    let parsed = b["filename"].as_str().and_then(|n| base64::engine::general_purpose::STANDARD.decode(b["dataBase64"].as_str()?).ok().map(|d| (n.to_string(), d)));
    let Some((name, data)) = parsed else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "filename and dataBase64 required" }))).into_response();
    };
    match app.overlays.add(&name, &data) {
        Ok(item) => {
            changed(&app);
            Json(json!({ "ok": true, "item": item })).into_response()
        }
        Err((code, error)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": error, "code": code }))).into_response(),
    }
}

async fn update(State(app): State<Shared>, UrlPath(id): UrlPath<String>, b: Option<Json<Value>>) -> Response {
    let b = b.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    if !app.overlays.update(&id, &b) {
        return StatusCode::NOT_FOUND.into_response();
    }
    changed(&app);
    Json(json!({ "ok": true })).into_response()
}

async fn remove_one(State(app): State<Shared>, UrlPath(id): UrlPath<String>) -> Json<Value> {
    let ok = app.overlays.remove(&id);
    if ok {
        changed(&app);
    }
    Json(json!({ "ok": ok }))
}

async fn clear(State(app): State<Shared>) -> Json<Value> {
    app.overlays.clear_live();
    changed(&app);
    Json(json!({ "ok": true }))
}

async fn set_live(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    let b = b.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    if let Some(set) = b["set"].as_str() {
        app.overlays.set_live(set, b["on"].as_bool().unwrap_or(false));
        changed(&app);
    }
    Json(json!({ "ok": true }))
}

async fn file(State(app): State<Shared>, UrlPath(name): UrlPath<String>, req: Request) -> Response {
    let Some(p) = app.overlays.file(&name) else { return StatusCode::NOT_FOUND.into_response() };
    match tower_http::services::ServeFile::new(p).oneshot(req).await {
        Ok(r) => r.map(axum::body::Body::new).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Overlays, PathBuf) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("kairo-overlays-{}-{}-{n}", std::process::id(), epoch_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        (Overlays::open(&dir), dir)
    }

    #[test]
    fn several_overlays_can_be_added_and_kept_between_runs() {
        let (o, dir) = store();
        let a = o.add("logo.png", b"x").ok().unwrap();
        let b = o.add("bug.mp4", b"y").ok().unwrap();
        assert_ne!(a["id"], b["id"]);
        assert_eq!(a["kind"], "image");
        assert_eq!(b["kind"], "video");
        let again = Overlays::open(&dir);
        assert_eq!(again.list().len(), 2);
    }

    #[test]
    fn only_pictures_and_videos_are_accepted() {
        let (o, _) = store();
        assert!(o.add("song.mp3", b"x").is_err());
        assert!(o.add("notes.txt", b"x").is_err());
        assert!(o.add("photo.JPG", b"x").is_ok());
    }

    #[test]
    fn changes_are_clamped_and_launch_flag_decides_what_starts_on() {
        let (o, dir) = store();
        let id = o.add("a.png", b"x").ok().unwrap()["id"].as_str().unwrap().to_string();
        assert!(o.update(&id, &json!({ "x": 500, "w": 0.1, "opacity": 3, "showOnLaunch": true, "live": true })));
        let v = &o.list()[0];
        assert_eq!(v["x"], 100.0);
        assert_eq!(v["w"], 2.0);
        assert_eq!(v["opacity"], 1.0);
        assert_eq!(v["live"], true);
        let again = Overlays::open(&dir);
        assert_eq!(again.list()[0]["live"], true, "on at launch because showOnLaunch is set");
        assert!(again.update(&id, &json!({ "showOnLaunch": false })));
        assert_eq!(Overlays::open(&dir).list()[0]["live"], false);
    }

    #[test]
    fn clear_turns_everything_off_and_remove_deletes_the_file() {
        let (o, dir) = store();
        let id = o.add("a.png", b"x").ok().unwrap()["id"].as_str().unwrap().to_string();
        o.update(&id, &json!({ "live": true }));
        o.clear_live();
        assert_eq!(o.list()[0]["live"], false);
        assert!(o.remove(&id));
        assert!(std::fs::read_dir(&dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with("a.png")));
        assert!(o.list().is_empty());
    }

    #[test]
    fn overlays_belong_to_sets_that_switch_on_together() {
        let (o, _) = store();
        let a = o.add("a.png", b"x").ok().unwrap()["id"].as_str().unwrap().to_string();
        let b = o.add("b.png", b"x").ok().unwrap()["id"].as_str().unwrap().to_string();
        let c = o.add("c.png", b"x").ok().unwrap()["id"].as_str().unwrap().to_string();
        assert_eq!(o.list()[0]["set"], "Main");
        o.update(&a, &json!({ "set": "Youth night" }));
        o.update(&b, &json!({ "set": "Youth night" }));
        o.set_live("Youth night", true);
        let live: Vec<bool> = o.list().iter().map(|v| v["live"].as_bool().unwrap()).collect();
        assert_eq!(live, vec![true, true, false]);
        let _ = c;
        o.set_live("Youth night", false);
        assert!(o.list().iter().all(|v| v["live"] == false));
    }

    #[test]
    fn a_path_outside_the_folder_is_refused() {
        let (o, _) = store();
        assert!(o.file("../overlays.json").is_none());
        assert!(o.file("overlays.json").is_none());
    }
}
