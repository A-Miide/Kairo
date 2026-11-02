//! The sermon-notes writer: the Mac's own on-device model (Apple Intelligence), run as a program of
//! its own (src-tauri/notes-writer, built by scripts/build-notes-writer.sh) so that nothing is
//! downloaded and the model's memory is the system's, not Kairo's. Kairo cuts the sermon into parts
//! and finds each part's scriptures (notes::segments); the writer only puts each part into words.
//! Everything else (Windows, Linux, Intel Macs, Macs without Apple Intelligence) gets the outline.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

use crate::notes::{Segment, Written};
use crate::Shared;

const EXE: &str = "kairo-notes-writer";

/// The writer's program: inside the app beside the UI (`bin`, as ffmpeg), or in the source tree
/// when running from it (scripts/build-notes-writer.sh).
fn program(app: &Shared) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KAIRO_NOTES_WRITER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let mut candidates = Vec::new();
    if let Some(res) = app.cfg.ui_dir.parent() {
        candidates.push(res.join("bin").join(EXE));
        candidates.push(res.join("src-tauri").join("bin").join(EXE));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|me| me.parent().map(PathBuf::from)) {
        candidates.push(dir.join(EXE));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// What the program says about itself, as the reason a person can act on.
fn friendly(reason: &str) -> String {
    let r = reason.to_lowercase();
    if r.contains("notenabled") || r.contains("not enabled") {
        "Apple Intelligence is turned off: turn it on in System Settings to have the notes written.".into()
    } else if r.contains("notready") || r.contains("not ready") {
        "Apple Intelligence is still getting ready on this Mac.".into()
    } else if r.contains("macos 26") {
        "Written notes need macOS 26 or later.".into()
    } else {
        "This Mac can't write notes (Apple Intelligence isn't available on it).".into()
    }
}

/// This machine can have its notes written now: `Ok`, or why not.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub async fn available(app: &Shared) -> Result<(), String> {
    let Some(exe) = program(app) else { return Err("The notes writer isn't part of this build.".into()) };
    let mut cmd = crate::proc::AsyncCommand::new(exe);
    cmd.arg("--check").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(15), cmd.output()).await.map_err(|_| "The notes writer didn't answer.".to_string())?.map_err(|e| format!("The notes writer couldn't start: {e}"))?;
    let v: Value = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).last().unwrap_or(Value::Null);
    match v["available"].as_bool() {
        Some(true) => Ok(()),
        _ => Err(friendly(v["reason"].as_str().unwrap_or(""))),
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub async fn available(_app: &Shared) -> Result<(), String> {
    Err("Written notes need a Mac with an Apple chip and Apple Intelligence. You get the outline: each part of the sermon with its scriptures and their words.".into())
}

/// The program's input for these parts.
pub fn input(segments: &[Segment]) -> Value {
    json!({ "sections": segments.iter().enumerate().map(|(i, g)| json!({ "id": format!("s{}", i + 1), "refs": g.refs.iter().take(8).collect::<Vec<_>>(), "text": g.text })).collect::<Vec<_>>() })
}

/// What the program printed: its last line that holds the notes.
pub fn parse(stdout: &str) -> Result<Written, String> {
    let done = stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["done"].as_bool() == Some(true) || v.get("sections").is_some()).last();
    let Some(v) = done else {
        let why = stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| v["error"].as_str().map(str::to_string)).last();
        return Err(why.unwrap_or_else(|| "The notes writer gave no notes.".into()));
    };
    let text = |x: &Value| x.as_str().unwrap_or("").trim().to_string();
    Ok(Written {
        title: text(&v["title"]),
        summary: text(&v["summary"]),
        sections: v["sections"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| (text(&s["id"]), text(&s["heading"]), s["bullets"].as_array().into_iter().flatten().map(&text).filter(|b| !b.is_empty()).collect()))
            .collect(),
    })
}

/// Has the writer put these parts into words. A long sermon takes a few minutes.
pub async fn write(app: &Shared, segments: &[Segment]) -> Result<Written, String> {
    use tokio::io::AsyncWriteExt;
    let Some(exe) = program(app) else { return Err("The notes writer isn't part of this build.".into()) };
    let mut cmd = crate::proc::AsyncCommand::new(exe);
    cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("The notes writer couldn't start: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("The notes writer has no input.")?;
    let body = input(segments).to_string();
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(body.as_bytes()).await;
        // Closing the input tells the program it has the whole sermon.
    });
    let out = tokio::time::timeout(Duration::from_secs(30 * 60), child.wait_with_output()).await.map_err(|_| "The notes writer took too long.".to_string())?.map_err(|e| e.to_string())?;
    let _ = feed.await;
    parse(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_line_with_notes_and_ignores_progress() {
        let out = "{\"progress\":1,\"of\":3}\n{\"progress\":2,\"of\":3}\n{\"done\":true,\"title\":\" Strength \",\"summary\":\"About strength.\",\"sections\":[{\"id\":\"s1\",\"heading\":\"Be strong\",\"bullets\":[\"One.\",\"\",\"Two.\"]},{\"id\":\"s2\",\"heading\":\"\",\"bullets\":[]}]}\n";
        let w = parse(out).unwrap();
        assert_eq!((w.title.as_str(), w.summary.as_str()), ("Strength", "About strength."));
        assert_eq!(w.sections[0], ("s1".to_string(), "Be strong".to_string(), vec!["One.".to_string(), "Two.".to_string()]));
        assert!(w.sections[1].2.is_empty());
    }

    #[test]
    fn says_why_when_there_are_no_notes() {
        assert_eq!(parse("{\"error\":\"unreadable input\"}\n").unwrap_err(), "unreadable input");
        assert!(parse("").is_err());
        assert!(friendly("appleIntelligenceNotEnabled").contains("turned off"));
    }

    #[test]
    fn the_input_carries_each_part_and_its_scriptures() {
        let g = Segment { at: 0, end: 10, text: "hello there".into(), refs: vec!["Isaiah 58:6".into()], point: None };
        let v = input(&[g.clone(), g]);
        assert_eq!(v["sections"][1]["id"], "s2");
        assert_eq!(v["sections"][0]["refs"][0], "Isaiah 58:6");
        assert_eq!(v["sections"][0]["text"], "hello there");
    }
}
