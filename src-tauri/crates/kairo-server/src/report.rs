//! "Report a problem" (owner, 2026-10-18): what the Report window shows and what it sends.
//!
//! Nothing here runs by itself. The window asks for a preview (`GET /api/report/preview`), the person
//! reads it, and only their press of Send makes `POST /api/report/send` post one form to the project's
//! own feedback form. What can leave is exactly what the preview shows: the person's words, the version,
//! the kind of computer, the speech engine and a short list of recent events. Never audio, transcripts,
//! verses, slide text, names, file paths or keys.
//!
//! A panic (an internal error) also leaves a small crash file in the app's data folder; the window offers
//! to add it. The file only ever leaves with a Send.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::Shared;

/// The feedback form reports are posted to: `https://docs.google.com/forms/d/e/<id>/formResponse`.
/// Empty until the form exists; then Send says reporting isn't set up instead of failing silently.
const FORM_URL: &str = "";
/// The form's question ids (`entry.<n>`), in this order: what happened, what they were doing, version,
/// computer, speech engine, details (events and crash).
const FORM_ENTRIES: [&str; 6] = ["", "", "", "", "", ""];

/// At most this many reports from one computer an hour: a form address pulled out of the app can't
/// be used to flood the inbox from here.
const MAX_PER_HOUR: usize = 3;
static SENT: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "macOS 15.1 (arm64)", "Windows 11 (x86_64)", "Ubuntu 24.04 (x86_64)".
fn computer() -> String {
    let arch = std::env::consts::ARCH;
    #[cfg(target_os = "macos")]
    let os = {
        let v = std::process::Command::new("sw_vers").arg("-productVersion").output().ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        format!("macOS {v}").trim().to_string()
    };
    #[cfg(target_os = "windows")]
    let os = {
        let v = std::process::Command::new("cmd").args(["/C", "ver"]).output().ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        if v.is_empty() { "Windows".to_string() } else { v }
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let os = std::fs::read_to_string("/etc/os-release").ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string())))
        .unwrap_or_else(|| "Linux".to_string());
    format!("{os} ({arch})")
}

/// Text with the home folder and the user's name taken out.
fn scrub(s: &str) -> String {
    let mut out = s.to_string();
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(var) {
            if home.len() > 3 { out = out.replace(&home, "~"); }
        }
    }
    out
}

// ── The recent events, without anything private ──────────────────────────────

/// Keys whose values are never sent: content and anything that names a person, a file or a secret.
const BLOCKED_KEYS: [&str; 22] = [
    "text", "transcript", "words", "title", "name", "path", "file", "url", "key", "token", "email", "verse",
    "verses", "reference", "itemId", "source", "label", "message", "note", "body", "heard", "address",
];

fn short(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::String(s) if s.chars().count() <= 80 => Some(scrub(s)),
        Value::String(s) => Some(format!("{}…", scrub(&s.chars().take(80).collect::<String>()))),
        _ => None,
    }
}

/// The last events of debug.log as short lines: "10:42:05 client:director on=true".
/// Events about detection, transcripts and verses are left out entirely.
fn recent_events(log: &Path) -> String {
    let Ok(raw) = std::fs::read_to_string(log) else { return String::new() };
    let lines: Vec<&str> = raw.lines().rev().take(400).collect();
    let mut out: Vec<String> = Vec::new();
    for l in lines.into_iter().rev() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        let event = v["event"].as_str().unwrap_or("");
        let lower = event.to_lowercase();
        if event.is_empty() || ["detect", "transcript", "verse", "scripture", "send-slide", "paraphrase"].iter().any(|w| lower.contains(w)) { continue; }
        let secs = v["t"].as_u64().unwrap_or(0) / 1000 % 86_400;
        let mut bits: Vec<String> = Vec::new();
        if let Some(obj) = v["data"].as_object() {
            for (k, val) in obj {
                // "name" is blocked (it can be a person's or an item's) unless it is an error's name.
                let error_name = k.eq_ignore_ascii_case("name") && val.as_str().is_some_and(|n| n.ends_with("Error") || n.ends_with("Exception"));
                if !error_name && BLOCKED_KEYS.iter().any(|b| b.eq_ignore_ascii_case(k)) { continue; }
                if let Some(s) = short(val) { bits.push(format!("{k}={s}")); }
            }
        }
        out.push(format!("{:02}:{:02}:{:02} {event} {}", secs / 3600, secs / 60 % 60, secs % 60, bits.join(" ")).trim_end().to_string());
    }
    let keep = out.len().saturating_sub(80);
    let text = out[keep..].join("\n");
    // The end is what matters: if it is too long, the oldest part goes.
    if text.len() > 6000 { text[text.len() - 6000..].to_string() } else { text }
}

// ── Crash files ──────────────────────────────────────────────────────────────

fn crash_dir(data: &Path) -> PathBuf { data.join("crashes") }

/// Writes a small crash file for every panic (an internal error), then carries on as before.
/// The file holds the version, the computer, where it happened and a short trace: no content.
pub(crate) fn install_crash_hook(data: PathBuf) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = crash_dir(&data);
        let _ = std::fs::create_dir_all(&dir);
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let msg: String = msg.chars().take(200).collect();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let trace: String = trace.lines().filter(|l| l.contains("kairo") || l.trim_start().starts_with(char::is_numeric)).take(40).collect::<Vec<_>>().join("\n");
        let text = scrub(&format!("Kairo {}\n{}\nthread: {thread}\nat: {at}\nerror: {msg}\n{trace}\n", version(), computer()));
        let stamp = crate::util::epoch_ms();
        let _ = std::fs::write(dir.join(format!("crash-{stamp}.txt")), text);
        // Only the newest few are kept.
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "txt")).collect();
            files.sort();
            while files.len() > 5 { let _ = std::fs::remove_file(files.remove(0)); }
        }
        prev(info);
    }));
}

/// The newest crash file that has not been sent: (path, text).
fn latest_crash(data: &Path) -> Option<(PathBuf, String)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(crash_dir(data)).ok()?.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt")).collect();
    files.sort();
    let p = files.pop()?;
    let text = std::fs::read_to_string(&p).ok()?;
    Some((p, text))
}

// ── Routes ───────────────────────────────────────────────────────────────────

async fn engine(app: &Shared) -> String {
    app.settings.get("speechEngine").await.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "deepgram".into())
}

/// What the Report window shows before anything is sent.
pub(crate) async fn preview(State(app): State<Shared>) -> Json<Value> {
    let data = app.cfg.app_data_dir.clone();
    let crash = latest_crash(&data).map(|(_, t)| t);
    Json(json!({
        "version": version(),
        "computer": computer(),
        "engine": engine(&app).await,
        "events": recent_events(&data.join("debug.log")),
        "crash": crash,
        "ready": !FORM_URL.is_empty(),
    }))
}

fn clip(v: &Value, max: usize) -> String {
    v.as_str().unwrap_or("").chars().take(max).collect()
}

/// Posts the report: the person's words plus exactly what the preview listed.
pub(crate) async fn send(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    if FORM_URL.is_empty() || FORM_ENTRIES.iter().any(|e| e.is_empty()) {
        return Json(json!({ "error": "Reporting isn't set up in this copy of Kairo." }));
    }
    {
        let mut sent = crate::util::lock(&SENT);
        sent.retain(|t| t.elapsed() < Duration::from_secs(3600));
        if sent.len() >= MAX_PER_HOUR {
            return Json(json!({ "error": "A few reports have just been sent from this computer. Please try again in a while." }));
        }
    }
    let what = clip(&b["what"], 3000);
    if what.trim().is_empty() {
        return Json(json!({ "error": "Please say what happened." }));
    }
    let data = app.cfg.app_data_dir.clone();
    let mut details = String::new();
    let crash = if b["includeCrash"].as_bool().unwrap_or(false) { latest_crash(&data) } else { None };
    if let Some((_, t)) = &crash { details.push_str(t); details.push_str("\n\n"); }
    if b["includeEvents"].as_bool().unwrap_or(false) { details.push_str(&recent_events(&data.join("debug.log"))); }
    let details: String = details.chars().take(12_000).collect();
    let params: Vec<(&str, String)> = vec![
        (FORM_ENTRIES[0], what),
        (FORM_ENTRIES[1], clip(&b["doing"], 3000)),
        (FORM_ENTRIES[2], version().to_string()),
        (FORM_ENTRIES[3], computer()),
        (FORM_ENTRIES[4], engine(&app).await),
        (FORM_ENTRIES[5], details),
    ];
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(c) => c,
        Err(e) => return Json(json!({ "error": format!("Could not send: {e}") })),
    };
    let form: Vec<(String, String)> = params.into_iter().map(|(k, v)| (format!("entry.{k}"), v)).collect();
    match client.post(FORM_URL).form(&form).send().await {
        Ok(r) if r.status().is_success() || r.status().is_redirection() => {
            crate::util::lock(&SENT).push(Instant::now());
            // A crash that was sent is not offered again.
            if let Some((p, _)) = crash { let _ = std::fs::rename(&p, p.with_extension("sent")); }
            Json(json!({ "ok": true }))
        }
        Ok(r) => Json(json!({ "error": format!("The report could not be delivered ({}).", r.status().as_u16()) })),
        Err(_) => Json(json!({ "error": "No connection, so the report was not sent. You can copy it instead." })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_leave_out_content_and_secrets() {
        let dir = std::env::temp_dir().join(format!("kairo-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("debug.log");
        std::fs::write(&log, [
            r#"{"t":1792333383282,"event":"client:director","data":{"on":true}}"#,
            r#"{"t":1792333383300,"event":"server:detect","data":{"verses":["John 3:16"],"score":0.9}}"#,
            r#"{"t":1792333383400,"event":"client:video-problem","data":{"what":"camera","name":"OverconstrainedError","title":"Sunday sermon","path":"/Users/pastor/x"}}"#,
            r#"{"t":1792333383500,"event":"client:transcript","data":{"text":"for God so loved"}}"#,
        ].join("\n")).unwrap();
        let s = recent_events(&log);
        assert!(s.contains("client:director on=true"), "{s}");
        assert!(s.contains("OverconstrainedError") && s.contains("what=camera"), "{s}");
        assert!(!s.contains("John 3:16") && !s.contains("Sunday sermon") && !s.contains("pastor") && !s.contains("loved"), "{s}");
        assert!(!s.contains("detect") && !s.contains("transcript"), "{s}");
    }

    #[test]
    fn a_crash_file_is_found_and_marked_sent() {
        let dir = std::env::temp_dir().join(format!("kairo-crash-{}", std::process::id()));
        std::fs::create_dir_all(crash_dir(&dir)).unwrap();
        std::fs::write(crash_dir(&dir).join("crash-1.txt"), "old").unwrap();
        std::fs::write(crash_dir(&dir).join("crash-2.txt"), "new").unwrap();
        let (p, t) = latest_crash(&dir).unwrap();
        assert_eq!(t, "new");
        std::fs::rename(&p, p.with_extension("sent")).unwrap();
        assert_eq!(latest_crash(&dir).unwrap().1, "old");
    }
}
