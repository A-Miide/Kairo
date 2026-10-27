//! Small helpers that were copied between the route files, media, overlays, settings and the
//! importers: poison-tolerant locks, the clock, random ids, percent-encoding, upload bodies and
//! the one way an upload is written to disk (`save_unique`, always off the async runtime).

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};

/// The lock, even if a thread panicked while holding it: the data is plain state a later
/// request can still use, and one panic must not take every request after it down.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Puts this thread, and the threads it starts from now on, below live speech (macOS QoS): the
/// meaning models ran 4 threads each in bursts of 250% CPU while the speech worker needed 150%
/// (live, 2026-10-26). Until the guard drops, so a pooled thread goes back to what it was.
pub fn below_speech() -> BelowSpeech {
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
    BelowSpeech
}

pub struct BelowSpeech;

impl Drop for BelowSpeech {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_DEFAULT, 0);
        }
    }
}

/// Milliseconds since the Unix epoch.
pub fn epoch_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `n` random bytes as lower-case hex.
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).ok();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// "prefix-<ms>-<hex>": unique enough to name a thing made now.
pub fn uid(prefix: &str, random_bytes: usize) -> String {
    format!("{prefix}-{}-{}", epoch_ms(), random_hex(random_bytes))
}

/// A random version-4 UUID, as text.
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("os randomness");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Percent-encodes everything but letters, digits and `-_.~` — and the bytes in `keep`.
pub fn urlencode_keeping(s: &str, keep: &[u8]) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ if keep.contains(&b) => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn urlencode(s: &str) -> String {
    urlencode_keeping(s, b"")
}

// ── Request and response shapes ─────────────────────────────────────────────

/// The JSON body, or `{}` when there was none.
pub fn body_or_empty(b: Option<Json<Value>>) -> Value {
    b.map(|Json(v)| v).unwrap_or_else(|| json!({}))
}

/// `{ "error": msg }` with the status.
pub fn error(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": msg.to_string() }))).into_response()
}
pub fn bad_request(msg: impl std::fmt::Display) -> Response {
    error(StatusCode::BAD_REQUEST, msg)
}
pub fn not_found(msg: impl std::fmt::Display) -> Response {
    error(StatusCode::NOT_FOUND, msg)
}

pub fn decode_b64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// What an upload body carries: `{ filename, dataBase64 }`. The decode of a big file is real
/// work, so callers do this inside `blocking`.
pub fn upload_parts(b: &Value) -> Option<(String, Vec<u8>)> {
    let name = b["filename"].as_str()?.to_string();
    Some((name, decode_b64(b["dataBase64"].as_str()?)?))
}

/// Runs file or CPU work on the blocking pool so it never stalls the async runtime. A panic
/// in `f` comes back as an error, not a crash of the request task.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())
}

// ── Files ───────────────────────────────────────────────────────────────────

/// A file name safe on every system: no separators or control characters, at most 80 characters.
pub fn safe_file_name(t: &str) -> String {
    let s: String = t.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect()
}

/// `name` in `dir`, numbered if it's taken: "x (2).txt".
pub fn numbered_path(dir: &Path, name: &str) -> std::path::PathBuf {
    let p = Path::new(name);
    let (stem, ext) = (p.file_stem().and_then(|s| s.to_str()).unwrap_or("file"), p.extension().and_then(|e| e.to_str()));
    let with = |n: Option<u32>| {
        let base = match n { Some(n) => format!("{stem} ({n})"), None => stem.to_string() };
        dir.join(match ext { Some(e) => format!("{base}.{e}"), None => base })
    };
    let mut c = with(None);
    let mut i = 2;
    while c.exists() {
        c = with(Some(i));
        i += 1;
    }
    c
}

/// Writes `data` to `path`; a half-written file is removed again.
pub fn write_clean(path: &Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, data).inspect_err(|_| {
        let _ = std::fs::remove_file(path);
    })
}

/// Saves an upload into `dir` under a name no other upload can have: "<ms>-<hex>-<name>"
/// (`name` is cut down to its last path part). Returns the name it was saved as. Blocking:
/// call it inside `blocking`.
pub fn save_unique(dir: &Path, name: &str, data: &[u8]) -> std::io::Result<String> {
    let base = Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("file");
    std::fs::create_dir_all(dir)?;
    let saved = format!("{}-{}-{base}", epoch_ms(), random_hex(4));
    write_clean(&dir.join(&saved), data)?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kairo-util-{tag}-{}-{}", std::process::id(), random_hex(3)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_unique_never_overwrites_and_keeps_only_the_file_name() {
        let d = temp("save");
        let a = save_unique(&d, "../../etc/x.png", b"one").unwrap();
        let b = save_unique(&d, "x.png", b"two").unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with("-x.png") && !a.contains('/'));
        assert_eq!(std::fs::read(d.join(&a)).unwrap(), b"one");
        assert_eq!(std::fs::read(d.join(&b)).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn numbered_path_counts_up() {
        let d = temp("num");
        assert_eq!(numbered_path(&d, "a.txt"), d.join("a.txt"));
        std::fs::write(d.join("a.txt"), "").unwrap();
        assert_eq!(numbered_path(&d, "a.txt"), d.join("a (2).txt"));
        std::fs::write(d.join("a (2).txt"), "").unwrap();
        assert_eq!(numbered_path(&d, "a.txt"), d.join("a (3).txt"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn urlencode_matches_what_the_three_copies_did() {
        assert_eq!(urlencode("a b/é.mp4"), "a%20b%2F%C3%A9.mp4");
        assert_eq!(urlencode_keeping("it's (1)!", b"!*'()"), "it's%20(1)!");
    }

    #[test]
    fn upload_parts_needs_both_fields_and_valid_base64() {
        assert_eq!(upload_parts(&json!({ "filename": "a.png", "dataBase64": "aGk=" })), Some(("a.png".into(), b"hi".to_vec())));
        assert!(upload_parts(&json!({ "filename": "a.png" })).is_none());
        assert!(upload_parts(&json!({ "filename": "a.png", "dataBase64": "!!" })).is_none());
    }

    #[test]
    fn uuid_v4_has_the_version_and_variant_bits() {
        let u = uuid_v4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!("89ab".contains(&u[19..20]));
    }

    #[test]
    fn safe_file_name_drops_separators() {
        assert_eq!(safe_file_name("a/b:c  d"), "a b c d");
    }
}
