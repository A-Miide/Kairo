//! Playing any common video. The app's web view decodes MP4 (H.264/AAC) and
//! little else, so a file in another container or codec is converted once, with
//! the ffmpeg that ships inside the app, to a standard MP4 kept in a cache
//! folder. The original is never touched, and the converted copy is reused
//! until the original changes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Containers a web view doesn't play, converted without trying first.
const CONVERT_ALWAYS: &[&str] = &[
    "mkv", "avi", "wmv", "asf", "flv", "f4v", "mpg", "mpeg", "ts", "mts", "m2ts", "3gp", "3g2", "ogv", "vob", "mxf",
    "divx", "rm", "rmvb",
];

/// Encoders that make web-playable H.264, best first.
const H264_ENCODERS: &[&str] = &["libx264", "h264_videotoolbox", "h264_mf", "libopenh264", "h264_nvenc", "h264_qsv", "h264_amf"];

pub fn needs_conversion(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CONVERT_ALWAYS.contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Clone)]
enum Job {
    Running(u8),
    Failed(String),
}

pub struct Video {
    cache: PathBuf,
    ffmpeg: Option<PathBuf>,
    encoder: OnceLock<Option<&'static str>>,
    jobs: Mutex<HashMap<String, Job>>,
    /// One conversion at a time: a folder of files all asking at once would otherwise start them all.
    gate: Mutex<()>,
}

impl Video {
    pub fn open(app_data_dir: &Path) -> Arc<Self> {
        let cache = app_data_dir.join("media-cache");
        let _ = std::fs::create_dir_all(&cache);
        Arc::new(Self { cache, ffmpeg: find_ffmpeg(), encoder: OnceLock::new(), jobs: Mutex::new(HashMap::new()), gate: Mutex::new(()) })
    }

    /// The cache key for a file: changes when the file does.
    pub fn key(path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
        let mut h = Sha256::new();
        h.update(path.to_string_lossy().as_bytes());
        h.update(meta.len().to_le_bytes());
        h.update(mtime.to_le_bytes());
        Some(h.finalize().iter().take(12).map(|b| format!("{b:02x}")).collect())
    }

    pub fn prepared_path(&self, key: &str) -> PathBuf {
        self.cache.join(format!("{key}.mp4"))
    }

    pub fn prepared_url(key: &str) -> String {
        format!("/api/media/prepared/{key}.mp4")
    }

    /// Where a conversion stands: ready (with its address), converting, failed,
    /// or not possible here (no ffmpeg).
    pub fn status(&self, src: &Path) -> Value {
        let Some(key) = Self::key(src) else { return json!({ "state": "failed", "error": "The file can't be read." }) };
        if self.prepared_path(&key).is_file() {
            return json!({ "state": "ready", "url": Self::prepared_url(&key) });
        }
        match self.jobs.lock().unwrap().get(&key) {
            Some(Job::Running(p)) => json!({ "state": "converting", "progress": p }),
            Some(Job::Failed(e)) => json!({ "state": "failed", "error": e }),
            None if self.ffmpeg.is_none() => json!({ "state": "unavailable" }),
            None => json!({ "state": "idle" }),
        }
    }

    /// Starts converting (if it isn't already, and isn't done) and returns the status.
    pub fn start(self: &Arc<Self>, src: PathBuf) -> Value {
        let st = self.status(&src);
        if st["state"] != "idle" {
            return st;
        }
        let Some(key) = Self::key(&src) else { return st };
        self.jobs.lock().unwrap().insert(key.clone(), Job::Running(0));
        let me = self.clone();
        std::thread::spawn(move || {
            let result = me.convert(&src, &key);
            let mut jobs = me.jobs.lock().unwrap();
            match result {
                Ok(()) => { jobs.remove(&key); }
                Err(e) => { jobs.insert(key, Job::Failed(e)); }
            }
        });
        json!({ "state": "converting", "progress": 0 })
    }

    fn encoder(&self) -> Option<&'static str> {
        *self.encoder.get_or_init(|| {
            let ff = self.ffmpeg.as_ref()?;
            let out = Command::new(ff).args(["-hide_banner", "-encoders"]).output().ok()?;
            let listing = String::from_utf8_lossy(&out.stdout).to_string();
            H264_ENCODERS.iter().copied().find(|name| listing.lines().any(|l| l.split_whitespace().nth(1) == Some(name)))
        })
    }

    fn set_progress(&self, key: &str, pct: u8) {
        if let Some(Job::Running(p)) = self.jobs.lock().unwrap().get_mut(key) {
            *p = pct.min(99);
        }
    }

    fn convert(&self, src: &Path, key: &str) -> Result<(), String> {
        let _turn = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let ff = self.ffmpeg.as_ref().ok_or("This copy of Kairo can't convert video.")?;
        let enc = self.encoder().ok_or("No video encoder is available to convert this file.")?;
        let out = self.prepared_path(key);
        let partial = self.cache.join(format!("{key}.partial.mp4"));
        let _ = std::fs::remove_file(&partial);

        // Longest side capped at 1920 px: plenty for a screen, and quick to convert.
        let scale = "scale='if(gt(iw,ih),min(1920,iw),-2)':'if(gt(iw,ih),-2,min(1920,ih))',format=yuv420p";
        let mut cmd = Command::new(ff);
        cmd.args(["-hide_banner", "-nostdin", "-y", "-i"]).arg(src);
        cmd.args(["-map", "0:v:0", "-map", "0:a:0?", "-sn", "-dn", "-vf", scale, "-c:v", enc]);
        match enc {
            "libx264" => { cmd.args(["-preset", "veryfast", "-crf", "20"]); }
            "libopenh264" => { cmd.args(["-b:v", "8M"]); }
            _ => { cmd.args(["-b:v", "8M"]); }
        }
        cmd.args(["-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart", "-progress", "pipe:1", "-nostats"]);
        cmd.arg(&partial).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start the converter: {e}"))?;

        // Duration comes on stderr; progress on stdout. Read stderr on its own thread.
        let duration_us = Arc::new(Mutex::new(0u64));
        let tail = Arc::new(Mutex::new(String::new()));
        let stderr = child.stderr.take();
        let (d2, t2) = (duration_us.clone(), tail.clone());
        let reader = std::thread::spawn(move || {
            if let Some(e) = stderr {
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    if let Some(i) = line.find("Duration: ") {
                        if let Some(us) = parse_clock(&line[i + 10..]) {
                            let mut d = d2.lock().unwrap();
                            if *d == 0 { *d = us; }
                        }
                    }
                    let mut t = t2.lock().unwrap();
                    t.push_str(&line);
                    t.push('\n');
                    if t.len() > 4000 { let cut = t.len() - 3000; *t = t[cut..].to_string(); }
                }
            }
        });
        if let Some(o) = child.stdout.take() {
            for line in BufReader::new(o).lines().map_while(Result::ok) {
                if let Some(v) = line.strip_prefix("out_time_us=") {
                    let total = *duration_us.lock().unwrap();
                    if let (Ok(done), true) = (v.trim().parse::<u64>(), total > 0) {
                        self.set_progress(key, ((done * 100) / total) as u8);
                    }
                }
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let _ = reader.join();
        if !status.success() || !partial.is_file() {
            let _ = std::fs::remove_file(&partial);
            let t = tail.lock().unwrap();
            let last = t.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("conversion failed");
            return Err(format!("This video couldn't be converted ({last})."));
        }
        std::fs::rename(&partial, &out).map_err(|e| e.to_string())
    }
}

/// "00:01:23.45" → microseconds.
fn parse_clock(s: &str) -> Option<u64> {
    let t = s.split(',').next()?.trim();
    let mut parts = t.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    Some(((h * 3600.0 + m * 60.0 + sec) * 1_000_000.0) as u64)
}

/// The ffmpeg that ships inside the app, else one on the machine.
fn find_ffmpeg() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = std::env::var_os("KAIRO_FFMPEG").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    let mut candidates = vec![dir.join(exe), dir.join("bin").join(exe), dir.join("resources").join("bin").join(exe)];
    // macOS: Kairo.app/Contents/MacOS/Kairo → Contents/Resources/bin/ffmpeg
    if let Some(contents) = dir.parent() {
        candidates.push(contents.join("Resources").join("bin").join(exe));
        candidates.push(contents.join("Resources").join("resources").join("bin").join(exe));
    }
    // Running from the source tree.
    if let Some(root) = dir.ancestors().find(|p| p.join("src-tauri").is_dir()) {
        candidates.push(root.join("src-tauri").join("bin").join(exe));
    }
    if let Some(p) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    // A copy on the machine is better than none.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].iter().map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}
