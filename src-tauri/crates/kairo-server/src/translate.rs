//! Showing a verse or slide in a second language: the Bible in that language
//! when it's a verse; else the language's own translation model, on this
//! machine (`mt.rs`); else Ollama, if one is running.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::Shared;

pub const LANGUAGES: [(&str, &str); 3] = [("fr", "French"), ("es", "Spanish"), ("pt", "Portuguese")];
const CACHE_MAX: usize = 500;
const OLLAMA_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub struct Translator {
    bibles: Mutex<HashMap<String, Option<Value>>>,
    cache: Mutex<HashMap<String, String>>,
    /// The last language's model, kept loaded.
    model: Mutex<Option<crate::mt::Loaded>>,
}

pub struct Failure {
    pub code: &'static str,
    pub message: String,
}

fn lang_name(lang: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|l| l.0 == lang).map(|l| l.1)
}

impl Translator {
    fn verse(&self, app: &Shared, lang: &str, book: &str, chapter: u64, verse: u64) -> Option<String> {
        let mut bibles = self.bibles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bible = bibles.entry(lang.to_string()).or_insert_with(|| {
            let path = app.cfg.db_dir.parent().unwrap_or(&app.cfg.db_dir).join("i18n").join(format!("{lang}.json"));
            std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
        });
        bible.as_ref()?[book][chapter.checked_sub(1)? as usize][verse.checked_sub(1)? as usize].as_str().map(str::to_string)
    }

    fn remember(&self, key: String, t: String) -> String {
        let mut c = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if c.len() > CACHE_MAX {
            c.clear();
        }
        c.insert(key, t.clone());
        t
    }

    /// A removed language's model, out of memory too.
    pub fn forget_model(&self, lang: &str) {
        let mut model = self.model.lock().unwrap_or_else(|p| p.into_inner());
        if model.as_ref().is_some_and(|m| m.lang() == lang) {
            *model = None;
        }
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).retain(|k, _| !k.starts_with(&format!("{lang}|")));
    }

    fn on_this_machine(&self, app: &Shared, lang: &str, text: &str) -> Result<String, String> {
        // A model that failed mid-translation is dropped and loaded afresh.
        let mut model = self.model.lock().unwrap_or_else(|poisoned| {
            let mut m = poisoned.into_inner();
            *m = None;
            m
        });
        if model.as_ref().is_none_or(|m| m.lang() != lang) {
            *model = None; // one model in memory at a time
            *model = Some(crate::mt::Loaded::load(app, lang)?);
        }
        model.as_mut().unwrap().translate(text)
    }

    pub async fn translate(&self, app: &Shared, text: &str, lang: &str, reference: Option<(&str, u64, u64)>) -> Result<Value, Failure> {
        let Some(name) = lang_name(lang) else { return Err(Failure { code: "UNSUPPORTED_LANGUAGE", message: format!("Unsupported language \"{lang}\"") }) };
        if let Some((b, c, v)) = reference {
            if let Some(t) = self.verse(app, lang, b, c, v) {
                return Ok(json!({ "text": t, "source": "bible" }));
            }
        }
        let text = text.trim();
        if text.is_empty() {
            return Ok(json!({ "text": "", "source": "none" }));
        }
        let key = format!("{lang}|{text}");
        if let Some(t) = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            return Ok(json!({ "text": t, "source": "cache" }));
        }
        if crate::mt::installed(app, lang) {
            let (a, l, t) = (app.clone(), lang.to_string(), text.to_string());
            match tokio::task::spawn_blocking(move || a.translator.on_this_machine(&a, &l, &t)).await.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(t) if !t.is_empty() => return Ok(json!({ "text": self.remember(key, t), "source": "local" })),
                Ok(_) => {}
                Err(e) => eprintln!("[Translate] Local model failed, trying Ollama: {e}"),
            }
        }
        let url = app.settings.str("ollamaUrl").await.unwrap_or_else(|| "http://localhost:11434".into()).trim_end_matches('/').to_string();
        let model = app.settings.str("ollamaModel").await.unwrap_or_else(|| "qwen2.5:7b-instruct".into());
        let prompt = format!("You translate live worship-service slide text into {name} for display alongside the English original. Translate faithfully and idiomatically — preserve the exact meaning, tone, and register (no summarizing, no adding or dropping content). Output ONLY the {name} translation, nothing else: no quotes, no notes, no explanation.");
        let body = json!({ "model": model, "stream": false, "options": { "temperature": 0.2 }, "messages": [{ "role": "system", "content": prompt }, { "role": "user", "content": text }] });
        let reply = reqwest::Client::new().post(format!("{url}/api/chat")).json(&body).timeout(OLLAMA_TIMEOUT).send().await;
        if let Ok(r) = reply {
            if let Ok(v) = r.json::<Value>().await {
                let t = v["message"]["content"].as_str().unwrap_or("").trim().to_string();
                if !t.is_empty() {
                    return Ok(json!({ "text": self.remember(key, t), "source": "ollama" }));
                }
            }
        }
        Err(Failure { code: "NO_TRANSLATE_ENGINE", message: "No translation engine available — the local translation model isn't installed and Ollama is unavailable".into() })
    }
}

/// POST /api/translate
pub async fn route(axum::extract::State(app): axum::extract::State<Shared>, axum::Json(b): axum::Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(lang) = b["lang"].as_str() else {
        return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({ "error": "No target language provided" }))).into_response();
    };
    let num = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
    let reference = match (b["book"].as_str(), num(&b["chapter"]), num(&b["verse"])) {
        (Some(bk), Some(c), Some(v)) => Some((bk, c, v)),
        _ => None,
    };
    match app.translator.translate(&app, b["text"].as_str().unwrap_or(""), lang, reference).await {
        Ok(mut v) => {
            v["ok"] = json!(true);
            axum::Json(v).into_response()
        }
        Err(f) => {
            let status = if f.code == "NO_TRANSLATE_ENGINE" { axum::http::StatusCode::BAD_REQUEST } else { axum::http::StatusCode::INTERNAL_SERVER_ERROR };
            (status, axum::Json(json!({ "error": f.message, "code": f.code }))).into_response()
        }
    }
}
