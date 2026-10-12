//! `settings.json`: kept as the UI writes it — every key it knows, untouched.

use std::path::PathBuf;

use serde_json::{Map, Value};
use tokio::sync::RwLock;

pub struct Settings {
    path: PathBuf,
    values: RwLock<Map<String, Value>>,
}

impl Settings {
    pub fn load(path: PathBuf) -> Settings {
        let values = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Map<String, Value>>(&b).ok()).unwrap_or_default();
        Settings { path, values: RwLock::new(values) }
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

    /// Merges `incoming` and writes the file.
    pub async fn merge(&self, incoming: Map<String, Value>) -> Map<String, Value> {
        let mut v = self.values.write().await;
        for (k, x) in incoming {
            v.insert(k, x);
        }
        let snapshot = v.clone();
        drop(v);
        if let Some(dir) = self.path.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        match serde_json::to_vec_pretty(&snapshot) {
            Ok(bytes) => {
                if let Err(e) = tokio::fs::write(&self.path, bytes).await {
                    eprintln!("[Settings] Save failed: {e}");
                }
            }
            Err(e) => eprintln!("[Settings] Save failed: {e}"),
        }
        snapshot
    }
}
