//! `settings.json`: kept as the UI writes it — every key it knows, untouched.

use std::path::PathBuf;

use serde_json::{Map, Value};
use tokio::sync::RwLock;

pub struct Settings {
    path: PathBuf,
    values: RwLock<Map<String, Value>>,
    /// One save at a time, each writing the newest values: two quick saves can't land in the
    /// wrong order and leave the older one on disk.
    saving: tokio::sync::Mutex<()>,
}

impl Settings {
    pub fn load(path: PathBuf) -> Settings {
        let values = match std::fs::read(&path) {
            Err(_) => Map::new(),
            Ok(b) => match serde_json::from_slice::<Map<String, Value>>(&b) {
                Ok(v) => v,
                // A damaged file is kept beside it instead of being replaced by empty
                // settings on the next save.
                Err(e) => {
                    let keep = path.with_extension(format!("damaged-{}.json", crate::detect::epoch_ms()));
                    let _ = std::fs::copy(&path, &keep);
                    eprintln!("[Settings] {} couldn't be read ({e}); kept as {}", path.display(), keep.display());
                    crate::routes::write_debug_global("server:settings-damaged", serde_json::json!({ "error": e.to_string(), "kept": keep.display().to_string() }));
                    Map::new()
                }
            },
        };
        Settings { path, values: RwLock::new(values), saving: tokio::sync::Mutex::new(()) }
    }

    pub async fn snapshot(&self) -> Map<String, Value> {
        self.values.read().await.clone()
    }

    pub async fn get(&self, key: &str) -> Option<Value> {
        self.values.read().await.get(key).cloned()
    }

    pub async fn str(&self, key: &str) -> Option<String> {
        self.get(key).await.and_then(|v| v.as_str().map(str::to_string))
    }

    pub async fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).await.and_then(|v| v.as_bool())
    }

    /// Merges `incoming` and writes the file: to a temporary file first, then renamed over
    /// the old one, so a crash or full disk mid-save never leaves half a settings file.
    /// The error, when the file couldn't be written (the values are still in use).
    pub async fn merge(&self, incoming: Map<String, Value>) -> (Map<String, Value>, Option<String>) {
        {
            let mut v = self.values.write().await;
            for (k, x) in incoming {
                v.insert(k, x);
            }
        }
        let _one = self.saving.lock().await;
        let snapshot = self.values.read().await.clone();
        let result: Result<(), String> = async {
            if let Some(dir) = self.path.parent() {
                tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
            }
            let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|e| e.to_string())?;
            let tmp = self.path.with_extension("json.saving");
            tokio::fs::write(&tmp, bytes).await.map_err(|e| e.to_string())?;
            tokio::fs::rename(&tmp, &self.path).await.map_err(|e| e.to_string())
        }.await;
        let err = result.err();
        if let Some(e) = &err {
            eprintln!("[Settings] Save failed: {e}");
            crate::routes::write_debug_global("server:settings-save-failed", serde_json::json!({ "error": e }));
        }
        (snapshot, err)
    }
}
