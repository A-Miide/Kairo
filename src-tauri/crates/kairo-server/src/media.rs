//! The media library: the app's own bin of uploaded images and videos, and
//! folders on disk the operator links (watched, so new files appear).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::hub::Hub;

const MAX_FOLDER_ITEMS: usize = 600;
/// How often linked folders are checked for changes (cloud folders sync in bursts).
const WATCH_EVERY: Duration = Duration::from_secs(3);

pub fn kind_for(name: &str) -> Option<&'static str> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => Some("image"),
        "mp4" | "webm" | "mov" | "m4v" | "mkv" | "avi" | "wmv" | "asf" | "flv" | "f4v" | "mpg" | "mpeg" | "ts" | "mts"
        | "m2ts" | "3gp" | "3g2" | "ogv" | "vob" | "mxf" | "divx" => Some("video"),
        _ => None,
    }
}

/// A sound file the audio lane can play (kept apart from pictures and video).
pub fn is_audio(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "aac" | "ogg" | "oga" | "opus" | "flac" | "aif" | "aiff")
}

#[derive(Clone)]
struct Folder {
    id: String,
    name: String,
    dir: PathBuf,
}

pub struct Media {
    bin: PathBuf,
    audio: PathBuf,
    manifest: PathBuf,
    folders: Mutex<Vec<Folder>>,
}

pub struct MediaError(pub &'static str, pub String);

fn mtime_ms(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// `name` inside `root`, refusing anything that escapes it (`..`, links out).
fn inside(root: &Path, name: &str) -> Option<PathBuf> {
    let full = root.join(name);
    let real = full.canonicalize().ok()?;
    let real_root = root.canonicalize().ok()?;
    (real.starts_with(&real_root) && real != real_root).then_some(real)
}

fn items_in(dir: &Path, url: impl Fn(&str) -> String) -> (Vec<Value>, bool) {
    let mut media: Vec<(String, &'static str)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter_map(|n| kind_for(&n).map(|k| (n, k))).collect())
        .unwrap_or_default();
    let truncated = media.len() > MAX_FOLDER_ITEMS;
    media.truncate(MAX_FOLDER_ITEMS);
    let mut items: Vec<(u64, Value)> = media
        .into_iter()
        .map(|(name, kind)| {
            let m = mtime_ms(&dir.join(&name));
            (m, json!({ "name": name, "kind": kind, "mtimeMs": m, "url": url(&name) }))
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    (items.into_iter().map(|(_, v)| v).collect(), truncated)
}

fn urlencode(s: &str) -> String {
    crate::util::urlencode_keeping(s, b"!*'()")
}

impl Media {
    pub fn open(app_data: &Path, hub: Arc<Hub>) -> Arc<Media> {
        let bin = app_data.join("media").join("bin");
        let manifest = app_data.join("media").join("folders.json");
        let audio = app_data.join("media").join("audio");
        let _ = std::fs::create_dir_all(&bin);
        let folders: Vec<Folder> = std::fs::read(&manifest)
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| Some(Folder { id: f["id"].as_str()?.into(), name: f["name"].as_str().unwrap_or("").into(), dir: PathBuf::from(f["dirPath"].as_str()?) }))
            .collect();
        let me = Arc::new(Media { bin, audio, manifest, folders: Mutex::new(folders) });
        let watcher = me.clone();
        tokio::spawn(async move { watcher.watch(hub).await });
        me
    }

    /// Writes the linked-folder list. A failure is logged and returned: the change works for now
    /// but would be gone at the next start, so the route says so instead of answering ok.
    fn save(&self) -> Result<(), MediaError> {
        let list: Vec<Value> = crate::util::lock(&self.folders).iter().map(|f| json!({ "id": f.id, "name": f.name, "dirPath": f.dir })).collect();
        if let Some(d) = self.manifest.parent() {
            let _ = std::fs::create_dir_all(d);   // the write below reports a folder that couldn't be made
        }
        std::fs::write(&self.manifest, serde_json::to_vec_pretty(&list).unwrap_or_default()).map_err(|e| {
            crate::routes::write_debug_global("server:media-manifest-write-failed", json!({ "path": self.manifest.display().to_string(), "error": e.to_string() }));
            eprintln!("[Media] could not save the folder list {}: {e}", self.manifest.display());
            MediaError("SAVE_FAILED", format!("The media folder list couldn't be saved ({e}). The change is gone after a restart."))
        })
    }

    /// Tells the UI when a linked folder's contents change.
    async fn watch(self: Arc<Self>, hub: Arc<Hub>) {
        let mut seen: std::collections::HashMap<String, Vec<(String, u64)>> = std::collections::HashMap::new();
        let mut tick = tokio::time::interval(WATCH_EVERY);
        loop {
            tick.tick().await;
            let folders = crate::util::lock(&self.folders).clone();
            for f in folders {
                let dir = f.dir.clone();
                let sig = tokio::task::spawn_blocking(move || {
                    let mut s: Vec<(String, u64)> = std::fs::read_dir(&dir)
                        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| kind_for(n).is_some()).map(|n| (n.clone(), mtime_ms(&dir.join(&n)))).collect())
                        .unwrap_or_default();
                    s.sort();
                    s
                })
                .await
                .unwrap_or_default();
                match seen.insert(f.id.clone(), sig.clone()) {
                    Some(prev) if prev != sig => hub.broadcast(json!({ "type": "media-folder-changed", "folderId": f.id })),
                    _ => {}
                }
            }
        }
    }

    pub fn bin_items(&self) -> Vec<Value> {
        items_in(&self.bin, |n| format!("/api/media/bin/file/{}", urlencode(n))).0
    }

    pub fn bin_file(&self, name: &str) -> Option<PathBuf> {
        inside(&self.bin, name)
    }

    pub fn write_to_bin(&self, filename: &str, data: &[u8]) -> Result<String, MediaError> {
        let base = Path::new(filename).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        if kind_for(&base).is_none() {
            return Err(MediaError("BAD_TYPE", format!("Unsupported file type \"{}\"", Path::new(&base).extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default())));
        }
        crate::util::save_unique(&self.bin, &base, data).map_err(|e| MediaError("WRITE_FAILED", e.to_string()))
    }

    pub fn delete_bin_item(&self, name: &str) -> bool {
        self.bin_file(name).is_some_and(|p| std::fs::remove_file(p).is_ok())
    }

    /// The audio lane's library, oldest first so a playlist keeps the order tracks were added.
    pub fn audio_items(&self) -> Vec<Value> {
        let mut v: Vec<(u64, Value)> = std::fs::read_dir(&self.audio)
            .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| is_audio(n)).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|n| {
                let m = mtime_ms(&self.audio.join(&n));
                // Saved as "<ms>-<hex>-<original>": show the original name.
                let label = n.splitn(3, '-').nth(2).unwrap_or(&n).to_string();
                (m, json!({ "name": n, "label": label, "mtimeMs": m, "url": format!("/api/audio/file/{}", urlencode(&n)) }))
            })
            .collect();
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|x| x.1).collect()
    }

    pub fn audio_file(&self, name: &str) -> Option<PathBuf> {
        let _ = std::fs::create_dir_all(&self.audio);
        inside(&self.audio, name).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(is_audio))
    }

    pub fn write_to_audio(&self, filename: &str, data: &[u8]) -> Result<String, MediaError> {
        let base = Path::new(filename).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        if !is_audio(&base) {
            return Err(MediaError("BAD_TYPE", format!("\"{base}\" isn't a sound file (mp3, wav, m4a, aac, ogg, flac or aiff).")));
        }
        crate::util::save_unique(&self.audio, &base, data).map_err(|e| MediaError("WRITE_FAILED", e.to_string()))
    }

    pub fn delete_audio_item(&self, name: &str) -> bool {
        self.audio_file(name).is_some_and(|p| std::fs::remove_file(p).is_ok())
    }

    pub fn folders(&self) -> Vec<Value> {
        crate::util::lock(&self.folders).iter().map(|f| json!({ "id": f.id, "name": f.name, "path": f.dir })).collect()
    }

    fn register(&self, name: Option<&str>, dir: PathBuf) -> Result<Value, MediaError> {
        let mut list = crate::util::lock(&self.folders);
        if let Some(f) = list.iter().find(|f| f.dir == dir) {
            return Ok(json!({ "id": f.id, "name": f.name, "dirPath": f.dir }));
        }
        let id = crate::util::uuid_v4();
        let name = name.filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| dir.file_name().and_then(|n| n.to_str()).unwrap_or("Folder").to_string());
        let f = Folder { id, name, dir };
        let out = json!({ "id": f.id, "name": f.name, "dirPath": f.dir });
        list.push(f);
        drop(list);
        self.save()?;
        Ok(out)
    }

    /// Links an existing folder.
    pub fn add_folder(&self, name: Option<&str>, dir: &str) -> Result<Value, MediaError> {
        let dir = PathBuf::from(dir);
        let dir = dir.canonicalize().map_err(|_| MediaError("BAD_FOLDER", "Not a valid folder path".into()))?;
        if !dir.is_dir() {
            return Err(MediaError("BAD_FOLDER", "Not a valid folder path".into()));
        }
        self.register(name, dir)
    }

    /// Creates a folder under ~/Documents/Kairo Media and links it.
    pub fn create_folder(&self, name: &str) -> Result<Value, MediaError> {
        let safe: String = name.chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect::<String>().trim().to_string();
        let safe = if safe.is_empty() || safe == "." || safe == ".." { "New folder".to_string() } else { safe };
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).ok_or(MediaError("BAD_FOLDER", "No home folder".into()))?;
        let dir = home.join("Documents").join("Kairo Media").join(&safe);
        std::fs::create_dir_all(&dir).map_err(|e| MediaError("BAD_FOLDER", e.to_string()))?;
        self.register(Some(name).filter(|n| !n.is_empty()), dir)
    }

    pub fn remove_folder(&self, id: &str) -> Result<bool, MediaError> {
        let mut list = crate::util::lock(&self.folders);
        let before = list.len();
        list.retain(|f| f.id != id);
        let removed = list.len() != before;
        drop(list);
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn folder_items(&self, id: &str) -> Option<(Vec<Value>, bool)> {
        let f = crate::util::lock(&self.folders).iter().find(|f| f.id == id).cloned()?;
        Some(items_in(&f.dir, |n| format!("/api/media/folders/{}/file/{}", f.id, urlencode(n))))
    }

    pub fn folder_file(&self, id: &str, name: &str) -> Option<PathBuf> {
        let f = crate::util::lock(&self.folders).iter().find(|f| f.id == id).cloned()?;
        inside(&f.dir, name)
    }

    pub fn write_into_folder(&self, id: &str, filename: &str, data: &[u8]) -> Result<(), MediaError> {
        let f = crate::util::lock(&self.folders).iter().find(|f| f.id == id).cloned().ok_or(MediaError("NOT_FOUND", "No such folder".into()))?;
        let safe = Path::new(filename).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        if kind_for(&safe).is_none() {
            return Err(MediaError("BAD_TYPE", format!("Unsupported file type \"{safe}\"")));
        }
        let dest = f.dir.join(&safe);
        crate::util::write_clean(&dest, data).map_err(|e| MediaError("WRITE_FAILED", e.to_string()))
    }
}
