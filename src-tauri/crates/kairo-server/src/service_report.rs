//! A one-page summary of each service, written when listening stops: how many verses went up and
//! how, what the operator corrected, and any trouble with the sound or the speech engine. So the
//! owner can see how a Sunday went without reading debug.log (owner, 23 Nov 2026).
//!
//! Read from debug.log (and debug.log.1, which a long service rolls into) from the last
//! "server:listening" on. Written to `service-reports/<date> <time>.txt` in the app's data folder,
//! the newest also served at `GET /api/service-report`. Nothing leaves the computer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The folder the reports are kept in, in the app's data folder.
pub const DIR: &str = "service-reports";

#[derive(Default, Debug, PartialEq)]
pub struct Summary {
    pub started_ms: u64,
    pub ended_ms: u64,
    /// Verses Kairo put on screen, by how it found them ("direct", "stream"…).
    pub sent_by: BTreeMap<String, usize>,
    /// Distinct verses on screen.
    pub verses: Vec<String>,
    pub offered: usize,
    pub operator_sent: usize,
    pub taken_down: usize,
    pub slide_moves: usize,
    pub speech_skipped_ms: u64,
    pub input_lost: usize,
    pub engine_stuck: usize,
    pub input_warnings: usize,
}

/// The events of the latest listening session, oldest first, from the log files given oldest first.
fn session_events(files: &[PathBuf]) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        events.extend(text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|e| e.is_object()));
    }
    let start = events.iter().rposition(|e| e["event"] == "server:listening").unwrap_or(events.len());
    events.split_off(start)
}

/// The summary of the latest listening session in these log files (oldest first).
pub fn summarize(files: &[PathBuf]) -> Option<Summary> {
    let events = session_events(files);
    let first = events.first()?;
    let mut s = Summary { started_ms: first["t"].as_u64().unwrap_or(0), ..Summary::default() };
    for e in &events {
        let d = &e["data"];
        s.ended_ms = s.ended_ms.max(e["t"].as_u64().unwrap_or(0));
        match e["event"].as_str().unwrap_or("") {
            "server:detect" if d["target"] == "screen" => {
                *s.sent_by.entry(d["method"].as_str().unwrap_or("?").to_string()).or_default() += 1;
                for v in d["verses"].as_array().into_iter().flatten().take(1).filter_map(Value::as_str) {
                    if !s.verses.iter().any(|x| x == v) {
                        s.verses.push(v.to_string());
                    }
                }
            }
            "server:detect" if d["target"] == "suggestions" => s.offered += 1,
            "server:engine" if d["event"].get("OperatorSent").is_some() => s.operator_sent += 1,
            "server:likely-wrong-send" => s.taken_down += 1,
            "client:slide-follow-advance" => s.slide_moves += 1,
            "server:speech-behind" => s.speech_skipped_ms += d["skippedMs"].as_u64().unwrap_or(0),
            "server:native-capture-lost" => s.input_lost += 1,
            "server:speech-engine-stuck" => s.engine_stuck += 1,
            "client:input-health" => s.input_warnings += 1,
            _ => {}
        }
    }
    Some(s)
}

/// The summary as the page a person reads.
pub fn text(s: &Summary) -> String {
    let minutes = s.ended_ms.saturating_sub(s.started_ms) / 60_000;
    let sent: usize = s.sent_by.values().sum();
    let by = s.sent_by.iter().map(|(m, n)| format!("{m} {n}")).collect::<Vec<_>>().join(", ");
    let mut out = format!("Kairo service summary\n\nListened for {minutes} min.\n\n");
    out += &format!("Scripture\n  {sent} sends to the screen, {} different verses ({by}).\n", s.verses.len());
    out += &format!("  Sent by the operator: {}. Taken down as wrong: {}. Offered in Possible Matches: {}.\n", s.operator_sent, s.taken_down, s.offered);
    if s.slide_moves > 0 {
        out += &format!("\nSlides\n  Moved on by slide follow: {}.\n", s.slide_moves);
    }
    out += "\nSound and speech engine\n";
    let mut trouble = Vec::new();
    if s.input_lost > 0 {
        trouble.push(format!("  The sound input was lost {} time(s).", s.input_lost));
    }
    if s.speech_skipped_ms > 0 {
        trouble.push(format!("  The speech engine fell behind and skipped {} s of speech.", s.speech_skipped_ms / 1000));
    }
    if s.engine_stuck > 0 {
        trouble.push(format!("  The speech engine stopped answering and was restarted {} time(s).", s.engine_stuck));
    }
    if s.input_warnings > 0 {
        trouble.push(format!("  The input warned of silence or clipping {} time(s).", s.input_warnings));
    }
    if trouble.is_empty() {
        trouble.push("  No trouble.".into());
    }
    out += &(trouble.join("\n") + "\n");
    if !s.verses.is_empty() {
        out += &format!("\nVerses on screen\n  {}\n", s.verses.join(", "));
    }
    out
}

/// Writes the latest session's summary into the app's data folder; returns its path.
pub fn write(app_data: &Path) -> Option<PathBuf> {
    let files = [app_data.join("debug.log.1"), app_data.join("debug.log")];
    let s = summarize(&files)?;
    let at = chrono_name(s.started_ms);
    let dir = app_data.join(DIR);
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{at}.txt"));
    std::fs::write(&path, text(&s)).ok()?;
    Some(path)
}

/// "2026-11-23 0915" in local time, for the file name.
fn chrono_name(ms: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms as i64).single().map_or_else(|| "service".into(), |t| t.format("%Y-%m-%d %H%M").to_string())
}

/// The newest report's text, if there is one.
pub fn latest(app_data: &Path) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(app_data.join(DIR)).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect();
    files.sort();
    std::fs::read_to_string(files.last()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_summed_up_from_its_own_events() {
        let dir = std::env::temp_dir().join(format!("kairo-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("debug.log");
        let lines = [
            r#"{"event":"server:detect","t":1000,"data":{"target":"screen","method":"direct","verses":["John 3:16"]}}"#,
            r#"{"event":"server:listening","t":2000,"data":{}}"#,
            r#"{"event":"server:detect","t":3000,"data":{"target":"screen","method":"direct","verses":["Romans 8:28"]}}"#,
            r#"{"event":"server:detect","t":4000,"data":{"target":"screen","method":"stream","verses":["Romans 8:28"]}}"#,
            r#"{"event":"server:detect","t":5000,"data":{"target":"suggestions","method":"verbatim","verses":["Psalms 23:1"]}}"#,
            r#"{"event":"server:speech-behind","t":6000,"data":{"skippedMs":4000}}"#,
            r#"{"event":"server:engine","t":7000,"data":{"event":{"OperatorSent":123}}}"#,
            r#"{"event":"client:slide-follow-advance","t":125000,"data":{}}"#,
        ];
        std::fs::write(&log, lines.join("\n")).unwrap();
        let s = summarize(&[log]).unwrap();
        assert_eq!(s.verses, vec!["Romans 8:28"], "only the latest session");
        assert_eq!(s.sent_by.values().sum::<usize>(), 2);
        assert_eq!((s.offered, s.operator_sent, s.slide_moves, s.speech_skipped_ms), (1, 1, 1, 4000));
        let t = text(&s);
        assert!(t.contains("Listened for 2 min") && t.contains("skipped 4 s"), "{t}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
