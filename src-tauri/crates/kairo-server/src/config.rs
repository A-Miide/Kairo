//! Where things are and how the server is reached — set by the Tauri app
//! (environment variables, as for the Node server), with dev defaults.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    /// Shared secret every request must carry; empty disables auth (dev).
    pub token: String,
    /// The Bible data: `kjv.json`, `map.json`, `translations/`, `packs/`, models.
    pub db_dir: PathBuf,
    /// Writable: `settings.json`, media, songs, installed models.
    pub app_data_dir: PathBuf,
    /// The UI files served at `/`.
    pub ui_dir: PathBuf,
    /// Downloaded models (offline speech, translation).
    pub models_dir: PathBuf,
}

impl Config {
    /// From `KAIRO_SERVER_PORT`, `KAIRO_AUTH_TOKEN`, `KAIRO_DB_DIR`,
    /// `KAIRO_APP_DATA_DIR`, `KAIRO_UI_DIR`; else paths relative to `project_root`.
    pub fn from_env(project_root: PathBuf) -> Config {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Config {
            port: env("KAIRO_SERVER_PORT").and_then(|p| p.parse().ok()).unwrap_or(7777),
            token: env("KAIRO_AUTH_TOKEN").unwrap_or_default(),
            db_dir: env("KAIRO_DB_DIR").map(PathBuf::from).unwrap_or_else(|| project_root.join("databases/bibles")),
            app_data_dir: env("KAIRO_APP_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| project_root.join("databases")),
            ui_dir: env("KAIRO_UI_DIR").map(PathBuf::from).unwrap_or_else(|| project_root.join("src")),
            models_dir: env("KAIRO_APP_DATA_DIR").map(|d| PathBuf::from(d).join("models")).unwrap_or_else(|| project_root.join("models")),
        }
    }

    pub fn auth_required(&self) -> bool {
        !self.token.is_empty()
    }
}
