//! Live streaming (Settings → Streaming): what an output shows, sent to RTMP/RTMPS
//! destinations (YouTube, Facebook, Twitch, any server), several at once.
//!
//! One FFmpeg process per streamed output ("pipeline"). Its picture is built from:
//! - Kairo's graphics for that output (scripture, slides, layout frame, overlays): the
//!   output's hidden capture window (display.html?capture=1&stream=1) is snapshotted on
//!   every change (capture.rs); here the last snapshot is written to FFmpeg at the stream's
//!   frame rate over a local TCP socket, with its transparency.
//! - The Program camera: every camera of the Director's inputs is opened by FFmpeg itself,
//!   and the one on air is chosen live with a `streamselect` command, so switching inputs
//!   never drops the stream. It is placed in the layout's video zone (or full frame) by
//!   `crop`/`scale`/`overlay` commands, also live.
//! - Sound: the one audio input chosen in Settings → Audio.
//! It is encoded once (a hardware H.264 encoder where there is one, AAC audio) and sent to
//! every destination with FFmpeg's tee muxer; one destination failing does not stop the
//! others. If FFmpeg stops, it is started again after a short wait.
//!
//! Stream keys are never logged or reported: every message that leaves this module has
//! them replaced. They do reach FFmpeg as part of its output address (FFmpeg has no other
//! way to take them), so they are visible to other processes of the same user on this
//! computer for as long as the stream runs.

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::capture::{blend, Frame};

/// The design canvas zones are given in (display.html's 1920×1080).
const DESIGN_W: f64 = 1920.0;
const DESIGN_H: f64 = 1080.0;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub key: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    pub id: String,
    pub label: String,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConfig {
    /// The output streamed; its capture id is `stream-<output>`.
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    pub destinations: Vec<Destination>,
    #[serde(default)]
    pub cameras: Vec<Camera>,
    /// The camera on air (an id in `cameras`), or none.
    #[serde(default)]
    pub program: Option<String>,
    /// Where the camera sits, in design pixels; none = the whole frame.
    #[serde(default)]
    pub zone: Option<Zone>,
    /// The audio input's name (Settings → Audio), or none for silence.
    #[serde(default)]
    pub audio_input: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DestStatus {
    pub id: String,
    pub name: String,
    /// "connecting" | "live" | "failed"
    pub state: String,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStatus {
    pub output: String,
    /// "starting" | "live" | "retrying" | "stopped" | "failed"
    pub state: String,
    pub uptime_secs: u64,
    pub fps: f32,
    pub kbps: f32,
    pub dropped: u64,
    pub encoder: String,
    pub destinations: Vec<DestStatus>,
    pub error: Option<String>,
    /// Things that work but not as asked (a camera FFmpeg can't find, no audio input…).
    pub warnings: Vec<String>,
}

// ── FFmpeg: which one, its encoders, its devices ─────────────────────────────

/// The H.264 encoder to use with this FFmpeg: hardware first. Kairo's own builds carry only
/// LGPL code; a user's own FFmpeg may also have libx264, used only as the last resort.
pub fn pick_encoder(encoders_listing: &str) -> Option<&'static str> {
    const ORDER: [&str; 6] = ["h264_videotoolbox", "h264_mf", "h264_nvenc", "h264_qsv", "h264_amf", "libx264"];
    ORDER.into_iter().find(|e| encoders_listing.lines().any(|l| l.split_whitespace().nth(1) == Some(*e)))
}

fn encoder_args(enc: &str, cfg: &PipelineConfig) -> Vec<String> {
    let kbps = cfg.video_kbps.clamp(500, 20_000);
    let gop = (cfg.fps.max(1) * 2).to_string();
    let mut a: Vec<String> = vec!["-c:v".into(), enc.into()];
    match enc {
        "h264_videotoolbox" => a.extend(["-realtime", "1", "-allow_sw", "1", "-profile:v", "high"].map(String::from)),
        "libx264" => a.extend(["-preset", "veryfast", "-tune", "zerolatency"].map(String::from)),
        "h264_nvenc" => a.extend(["-preset", "p4", "-tune", "ll"].map(String::from)),
        _ => {}
    }
    a.extend([
        "-b:v".into(), format!("{kbps}k"),
        "-maxrate".into(), format!("{kbps}k"),
        "-bufsize".into(), format!("{}k", kbps * 2),
        "-g".into(), gop.clone(), "-keyint_min".into(), gop,
        "-pix_fmt".into(), "yuv420p".into(),
    ]);
    a
}

/// Device names from `ffmpeg -f avfoundation -list_devices true -i ""` (macOS) or
/// `-f dshow -list_devices true -i dummy` (Windows): (video, audio).
pub fn parse_device_list(stderr: &str) -> (Vec<String>, Vec<String>) {
    let (mut video, mut audio) = (Vec::new(), Vec::new());
    let mut in_audio = false;
    for line in stderr.lines() {
        let l = line.to_lowercase();
        if l.contains("video devices") { in_audio = false; continue; }
        if l.contains("audio devices") { in_audio = true; continue; }
        // avfoundation: "[AVFoundation indev @ 0x…] [0] FaceTime HD Camera"
        if let Some(rest) = line.split("] [").nth(1).and_then(|r| r.split_once("] ")).map(|(_, n)| n.trim().to_string()) {
            if in_audio { audio.push(rest) } else { video.push(rest) }
            continue;
        }
        // dshow: `[dshow @ …] "HD Pro Webcam C920" (video)`
        if let (Some(a), Some(b)) = (line.find('"'), line.rfind('"')) {
            if b > a + 1 && !l.contains("alternative name") {
                let name = line[a + 1..b].to_string();
                if l.contains("(audio)") { audio.push(name) } else if l.contains("(video)") { video.push(name) }
            }
        }
    }
    (video, audio)
}

fn norm(s: &str) -> String {
    let no_paren: String = {
        let mut out = String::new();
        let mut depth = 0;
        for c in s.chars() {
            match c { '(' => depth += 1, ')' => depth = (depth - 1).max(0), _ if depth == 0 => out.push(c), _ => {} }
        }
        out
    };
    no_paren.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The device FFmpeg knows that a browser label (e.g. "FaceTime HD Camera (Built-in)")
/// refers to: exact, then without the bracketed parts, then one containing the other.
pub fn match_device<'a>(label: &str, names: &'a [String]) -> Option<&'a String> {
    let l = norm(label);
    if l.is_empty() { return None; }
    names.iter().find(|n| n.eq_ignore_ascii_case(label))
        .or_else(|| names.iter().find(|n| norm(n) == l))
        .or_else(|| names.iter().find(|n| { let n = norm(n); !n.is_empty() && (n.contains(&l) || l.contains(&n)) }))
}

/// Can this FFmpeg stream at all: the flv and tee muxers, RTMP, and the filters used here?
pub fn can_stream(ffmpeg: &Path) -> bool {
    let out = |args: &[&str]| Command::new(ffmpeg).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let has = |listing: &str, name: &str| listing.lines().any(|l| l.split_whitespace().any(|w| w == name));
    let muxers = out(&["-hide_banner", "-muxers"]);
    let protocols = out(&["-hide_banner", "-protocols"]);
    let filters = out(&["-hide_banner", "-filters"]);
    has(&muxers, "flv") && has(&muxers, "tee") && has(&protocols, "rtmp")
        && ["streamselect", "overlay", "crop", "scale", "color", "anullsrc"].iter().all(|f| has(&filters, f))
}

/// The FFmpeg to stream with: Kairo's own when it can, else one on this computer that can.
pub fn streaming_ffmpeg(own: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = own.as_ref().filter(|p| can_stream(p)) { return Some(p.clone()); }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].iter().map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(exe)).filter(|p| p.is_file() && Some(p) != own.as_ref()).find(|p| can_stream(p))
}

// ── The FFmpeg command ───────────────────────────────────────────────────────

/// The address FFmpeg sends to: the server address and the stream key, joined.
pub fn destination_url(d: &Destination) -> String {
    let url = d.url.trim().trim_end_matches('/');
    let key = d.key.trim().trim_start_matches('/');
    if key.is_empty() { url.to_string() } else { format!("{url}/{key}") }
}

/// Tee's own special characters, escaped inside one slave address.
fn tee_escape(s: &str) -> String {
    s.chars().flat_map(|c| if "|[]\\".contains(c) { vec!['\\', c] } else { vec![c] }).collect()
}

/// Replaces every stream key in `text`.
pub fn redact(text: &str, dests: &[Destination]) -> String {
    let mut out = text.to_string();
    for d in dests {
        let k = d.key.trim();
        if k.len() >= 3 { out = out.replace(k, "••••"); }
    }
    out
}

/// The camera's crop, size and place on the canvas for a zone (or the whole frame).
/// The camera is first fitted to fill the canvas (W×H); the crop keeps the zone's shape.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Placement { pub crop_w: u32, pub crop_h: u32, pub w: u32, pub h: u32, pub x: i32, pub y: i32 }

pub fn placement(zone: Option<Zone>, width: u32, height: u32) -> Placement {
    let even = |v: f64| ((v.round() as i64).max(2) as u32) & !1;
    let (sx, sy) = (width as f64 / DESIGN_W, height as f64 / DESIGN_H);
    let z = zone.unwrap_or(Zone { x: 0.0, y: 0.0, w: DESIGN_W, h: DESIGN_H });
    let (w, h) = (even(z.w * sx).min(width), even(z.h * sy).min(height));
    let (x, y) = ((z.x * sx).round() as i32, (z.y * sy).round() as i32);
    // The largest piece of the camera picture with the zone's shape.
    let aspect = w as f64 / h as f64;
    let (cw, ch) = if aspect >= width as f64 / height as f64 {
        (width, even(width as f64 / aspect).min(height))
    } else {
        (even(height as f64 * aspect).min(width), height)
    };
    Placement { crop_w: cw, crop_h: ch, w, h, x, y }
}

pub struct Inputs {
    /// FFmpeg arguments for the inputs, in order.
    pub args: Vec<String>,
    /// Index into `cameras` → FFmpeg input number, for the cameras FFmpeg could open.
    pub camera_inputs: Vec<(String, usize)>,
    pub audio_input: Option<usize>,
    pub warnings: Vec<String>,
}

/// Which FFmpeg input device kind this computer uses.
fn device_format() -> Option<&'static str> {
    if cfg!(target_os = "macos") { Some("avfoundation") } else if cfg!(target_os = "windows") { Some("dshow") } else { None }
}

pub fn build_inputs(cfg: &PipelineConfig, graphics_port: u16, video_names: &[String], audio_names: &[String]) -> Inputs {
    let mut args: Vec<String> = Vec::new();
    let mut warnings = Vec::new();
    // 0: Kairo's graphics, with transparency, from our own socket.
    args.extend([
        "-thread_queue_size", "64", "-use_wallclock_as_timestamps", "1",
        "-f", "rawvideo", "-pix_fmt", "bgra",
    ].map(String::from));
    args.extend(["-s".into(), format!("{}x{}", cfg.width, cfg.height), "-framerate".into(), cfg.fps.to_string()]);
    args.extend(["-i".into(), format!("tcp://127.0.0.1:{graphics_port}?listen=1")]);
    let mut next = 1;
    let mut camera_inputs = Vec::new();
    let test_sources = std::env::var("KAIRO_STREAM_TEST_DEVICES").is_ok();
    for cam in &cfg.cameras {
        if test_sources {
            args.extend(["-f", "lavfi", "-i"].map(String::from));
            args.push(format!("testsrc2=size=1280x720:rate={}", cfg.fps));
        } else {
            let Some(fmt) = device_format() else {
                warnings.push(format!("Cameras can't go into the stream on this computer yet ({}).", cam.label));
                continue;
            };
            let Some(name) = match_device(&cam.label, video_names) else {
                warnings.push(format!("\"{}\" wasn't found for the stream.", cam.label));
                continue;
            };
            args.extend(["-thread_queue_size", "64", "-f", fmt].map(String::from));
            if fmt == "avfoundation" {
                args.extend(["-framerate".into(), cfg.fps.to_string()]);
                args.extend(["-i".into(), name.clone()]);
            } else {
                args.extend(["-i".into(), format!("video={name}")]);
            }
        }
        camera_inputs.push((cam.id.clone(), next));
        next += 1;
    }
    let mut audio_input = None;
    if test_sources {
        args.extend(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"].map(String::from));
        audio_input = Some(next);
    } else if let Some(label) = cfg.audio_input.as_deref().filter(|l| !l.trim().is_empty()) {
        match (device_format(), match_device(label, audio_names)) {
            (Some(fmt), Some(name)) => {
                args.extend(["-thread_queue_size", "1024", "-f", fmt].map(String::from));
                args.extend(["-i".into(), if fmt == "avfoundation" { format!(":{name}") } else { format!("audio={name}") }]);
                audio_input = Some(next);
            }
            (None, _) if cfg!(target_os = "linux") => {
                args.extend(["-thread_queue_size", "1024", "-f", "pulse", "-i", "default"].map(String::from));
                audio_input = Some(next);
            }
            _ => warnings.push(format!("The audio input \"{label}\" wasn't found for the stream; it goes out silent.")),
        }
    } else {
        warnings.push("No audio input is chosen under Settings → Audio; the stream goes out silent.".into());
    }
    if audio_input.is_none() {
        args.extend(["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"].map(String::from));
        audio_input = Some(next);
    }
    Inputs { args, camera_inputs, audio_input, warnings }
}

/// The selection index for `program` among the opened cameras; the last index is "no camera".
pub fn select_index(program: Option<&str>, camera_inputs: &[(String, usize)]) -> usize {
    program.and_then(|p| camera_inputs.iter().position(|(id, _)| id == p)).unwrap_or(camera_inputs.len())
}

pub fn filter_graph(cfg: &PipelineConfig, inputs: &Inputs, selected: usize, place: Placement) -> String {
    let (w, h, fps) = (cfg.width, cfg.height, cfg.fps);
    let mut g = String::new();
    let mut labels = String::new();
    for (i, (_, input)) in inputs.camera_inputs.iter().enumerate() {
        g += &format!("[{input}:v]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,fps={fps},format=yuv420p[c{i}];");
        labels += &format!("[c{i}]");
    }
    g += &format!("color=c=black:s={w}x{h}:r={fps},format=yuv420p[nocam];");
    labels += "[nocam]";
    let n = inputs.camera_inputs.len() + 1;
    g += &format!("{labels}streamselect@sel=inputs={n}:map={selected}[cam];");
    g += &format!("[cam]crop@zc=w={}:h={}[camc];[camc]scale@zs=w={}:h={}[camz];", place.crop_w, place.crop_h, place.w, place.h);
    g += &format!("color=c=black:s={w}x{h}:r={fps},format=yuv420p[base];");
    g += &format!("[base][camz]overlay@zo=x={}:y={}:eof_action=pass[bv];", place.x, place.y);
    g += "[0:v]format=rgba[g];[bv][g]overlay=format=auto:eof_action=repeat,format=yuv420p[v]";
    g
}

pub fn ffmpeg_args(cfg: &PipelineConfig, enc: &str, inputs: &Inputs, selected: usize, place: Placement) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "warning", "-stats", "-stats_period", "1", "-y"].map(String::from).to_vec();
    a.extend(inputs.args.iter().cloned());
    a.extend(["-filter_complex".into(), filter_graph(cfg, inputs, selected, place)]);
    a.extend(["-map".into(), "[v]".into(), "-map".into(), format!("{}:a", inputs.audio_input.unwrap_or(1))]);
    a.extend(encoder_args(enc, cfg));
    a.extend([
        "-c:a".into(), "aac".into(), "-b:a".into(), format!("{}k", cfg.audio_kbps.clamp(64, 320)),
        "-ar".into(), "48000".into(), "-ac".into(), "2".into(),
        "-flags".into(), "+global_header".into(),
    ]);
    let slaves: Vec<String> = cfg.destinations.iter().map(|d| format!("[f=flv:onfail=ignore]{}", tee_escape(&destination_url(d)))).collect();
    a.extend(["-f".into(), "tee".into(), slaves.join("|")]);
    a
}

/// FFmpeg commands that move the camera from one selection and place to another, while it
/// runs: only what changed (each command costs FFmpeg a keyboard poll, about 100 ms).
pub fn live_commands(from: Option<(usize, Placement)>, selected: usize, place: Placement) -> Vec<String> {
    let mut c = Vec::new();
    let (fs, fp) = match from { Some((s, p)) => (Some(s), Some(p)), None => (None, None) };
    if fs != Some(selected) { c.push(format!("streamselect@sel -1 map {selected}")); }
    let changed = |f: fn(&Placement) -> i64| fp.map_or(true, |p| f(&p) != f(&place));
    if changed(|p| p.crop_w as i64) { c.push(format!("crop@zc -1 w {}", place.crop_w)); }
    if changed(|p| p.crop_h as i64) { c.push(format!("crop@zc -1 h {}", place.crop_h)); }
    if changed(|p| p.w as i64) { c.push(format!("scale@zs -1 w {}", place.w)); }
    if changed(|p| p.h as i64) { c.push(format!("scale@zs -1 h {}", place.h)); }
    if changed(|p| p.x as i64) { c.push(format!("overlay@zo -1 x {}", place.x)); }
    if changed(|p| p.y as i64) { c.push(format!("overlay@zo -1 y {}", place.y)); }
    c
}

/// One `-stats` line: "frame=  120 fps= 30 q=… size=… time=… bitrate=2998.4kbits/s dup=0 drop=3 speed=1x".
pub fn parse_stats(line: &str) -> Option<(f32, f32, u64)> {
    if !line.contains("frame=") { return None; }
    let field = |k: &str| -> Option<&str> {
        let i = line.find(k)? + k.len();
        line[i..].split_whitespace().next()
    };
    let fps = field("fps=").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let kbps = field("bitrate=").and_then(|v| v.trim_end_matches("kbits/s").parse().ok()).unwrap_or(0.0);
    let drop = field("drop=").and_then(|v| v.parse().ok()).unwrap_or(0);
    Some((fps, kbps, drop))
}

// ── A running pipeline ───────────────────────────────────────────────────────

struct Shared {
    cfg: Mutex<PipelineConfig>,
    status: Mutex<PipelineStatus>,
    stop: AtomicBool,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    /// The frame to send: (current, previous and when the fade to current began, fade ms).
    frame: Mutex<FrameSlot>,
    camera_inputs: Mutex<Vec<(String, usize)>>,
    /// What FFmpeg was last told (selection, placement): the starting command line, then commands.
    sent: Mutex<Option<(usize, Placement)>>,
    /// One command sender at a time, in order.
    commands: Mutex<()>,
}

#[derive(Default)]
struct FrameSlot {
    current: Option<Arc<Frame>>,
    previous: Option<Arc<Frame>>,
    fade_from: Option<Instant>,
    fade_ms: u32,
    /// `current` unpremultiplied, made once.
    ready: Option<Arc<Vec<u8>>>,
}

pub struct Pipeline {
    shared: Arc<Shared>,
}

pub type OnStatus = Arc<dyn Fn(PipelineStatus) + Send + Sync>;

impl Pipeline {
    pub fn start(ffmpeg: PathBuf, cfg: PipelineConfig, on_status: OnStatus) -> Pipeline {
        let status = PipelineStatus {
            output: cfg.output.clone(),
            state: "starting".into(),
            destinations: cfg.destinations.iter().map(|d| DestStatus { id: d.id.clone(), name: d.name.clone(), state: "connecting".into(), error: None }).collect(),
            ..Default::default()
        };
        let shared = Arc::new(Shared {
            cfg: Mutex::new(cfg),
            status: Mutex::new(status),
            stop: AtomicBool::new(false),
            stdin: Mutex::new(None),
            child: Mutex::new(None),
            frame: Mutex::new(FrameSlot::default()),
            camera_inputs: Mutex::new(Vec::new()),
            sent: Mutex::new(None),
            commands: Mutex::new(()),
        });
        let s2 = shared.clone();
        thread::spawn(move || supervise(ffmpeg, s2, on_status));
        Pipeline { shared }
    }

    pub fn output(&self) -> String {
        self.shared.cfg.lock().map(|c| c.output.clone()).unwrap_or_default()
    }

    pub fn status(&self) -> PipelineStatus {
        self.shared.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// A new snapshot of the output's graphics, faded in over `fade_ms`.
    pub fn push_frame(&self, frame: Arc<Frame>, fade_ms: u32) {
        if let Ok(mut slot) = self.shared.frame.lock() {
            slot.previous = if fade_ms > 0 { slot.current.take() } else { None };
            slot.fade_from = (fade_ms > 0 && slot.previous.is_some()).then(Instant::now);
            slot.fade_ms = fade_ms;
            slot.current = Some(frame);
            slot.ready = None;
        }
    }

    /// The camera on air and its place changed (Director switched inputs or layouts).
    pub fn set_program(&self, program: Option<String>, zone: Option<Zone>) {
        let (selected, place) = {
            let Ok(mut cfg) = self.shared.cfg.lock() else { return };
            cfg.program = program;
            cfg.zone = zone;
            let cams = self.shared.camera_inputs.lock().map(|c| c.clone()).unwrap_or_default();
            (select_index(cfg.program.as_deref(), &cams), placement(cfg.zone, cfg.width, cfg.height))
        };
        // 'c' then the command line: FFmpeg's interactive filter command. FFmpeg polls its
        // keyboard about every 100 ms and the last command of a burst waits for the next key,
        // so the commands go one at a time and a harmless one follows them.
        let shared = self.shared.clone();
        thread::spawn(move || {
            let Ok(_turn) = shared.commands.lock() else { return };
            let from = shared.sent.lock().ok().and_then(|s| *s);
            let mut cmds = live_commands(from, selected, place);
            if cmds.is_empty() { return; }
            cmds.push(format!("streamselect@sel -1 map {selected}"));
            for c in cmds {
                {
                    let Ok(mut stdin) = shared.stdin.lock() else { return };
                    let Some(w) = stdin.as_mut() else { return };
                    if w.write_all(format!("c{c}\n").as_bytes()).and_then(|_| w.flush()).is_err() { return; }
                }
                thread::sleep(Duration::from_millis(110));
            }
            if let Ok(mut s) = shared.sent.lock() { *s = Some((selected, place)); }
        });
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(mut stdin) = self.shared.stdin.lock() {
            if let Some(w) = stdin.as_mut() { let _ = w.write_all(b"q"); let _ = w.flush(); }
            *stdin = None;
        }
        // Let FFmpeg finish the streams cleanly, then make sure it is gone.
        let shared = self.shared.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(3));
            if let Ok(mut c) = shared.child.lock() { if let Some(ch) = c.as_mut() { let _ = ch.kill(); } }
        });
    }
}

fn set_status(shared: &Shared, on_status: &OnStatus, f: impl FnOnce(&mut PipelineStatus)) {
    let snapshot = {
        let Ok(mut s) = shared.status.lock() else { return };
        f(&mut s);
        s.clone()
    };
    on_status(snapshot);
}

fn list_devices(ffmpeg: &Path) -> (Vec<String>, Vec<String>) {
    let args: &[&str] = match device_format() {
        Some("avfoundation") => &["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""],
        Some("dshow") => &["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"],
        _ => return (Vec::new(), Vec::new()),
    };
    let out = Command::new(ffmpeg).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output();
    out.map(|o| parse_device_list(&String::from_utf8_lossy(&o.stderr))).unwrap_or_default()
}

fn supervise(ffmpeg: PathBuf, shared: Arc<Shared>, on_status: OnStatus) {
    let encoders = Command::new(&ffmpeg).args(["-hide_banner", "-encoders"]).stdin(Stdio::null()).stderr(Stdio::null()).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let Some(enc) = pick_encoder(&encoders) else {
        set_status(&shared, &on_status, |s| { s.state = "failed".into(); s.error = Some("This copy of FFmpeg has no H.264 encoder Kairo can use.".into()); });
        return;
    };
    let (video_names, audio_names) = list_devices(&ffmpeg);
    let mut backoff = 2u64;
    while !shared.stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        let cfg = match shared.cfg.lock() { Ok(c) => c.clone(), Err(_) => return };
        let port = match TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) { Ok(a) => a.port(), Err(e) => {
            set_status(&shared, &on_status, |s| { s.state = "failed".into(); s.error = Some(format!("No local port for the picture: {e}")); });
            return;
        } };
        let inputs = build_inputs(&cfg, port, &video_names, &audio_names);
        let selected = select_index(cfg.program.as_deref(), &inputs.camera_inputs);
        if let Ok(mut c) = shared.camera_inputs.lock() { *c = inputs.camera_inputs.clone(); }
        let place0 = placement(cfg.zone, cfg.width, cfg.height);
        if let Ok(mut s) = shared.sent.lock() { *s = Some((selected, place0)); }
        let args = ffmpeg_args(&cfg, enc, &inputs, selected, place0);
        let child = Command::new(&ffmpeg).args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                set_status(&shared, &on_status, |s| { s.state = "failed".into(); s.error = Some(format!("FFmpeg didn't start: {e}")); });
                return;
            }
        };
        let warnings = inputs.warnings.clone();
        set_status(&shared, &on_status, |s| {
            s.state = "starting".into(); s.encoder = enc.into(); s.error = None; s.warnings = warnings;
            for d in &mut s.destinations { d.state = "connecting".into(); d.error = None; }
        });
        if let Ok(mut s) = shared.stdin.lock() { *s = child.stdin.take(); }
        let stderr = child.stderr.take();
        if let Ok(mut c) = shared.child.lock() { *c = Some(child); }

        // The picture: our socket → FFmpeg's graphics input.
        let s_writer = shared.clone();
        let writer = thread::spawn(move || write_graphics(port, &s_writer));
        // FFmpeg's messages: progress, and each destination's trouble.
        if let Some(err) = stderr {
            read_messages(err, &shared, &on_status, &cfg.destinations, started);
        }
        let code = shared.child.lock().ok().and_then(|mut c| c.as_mut().and_then(|ch| ch.wait().ok()));
        if let Ok(mut c) = shared.child.lock() { *c = None; }
        if let Ok(mut s) = shared.stdin.lock() { *s = None; }
        let _ = writer.join();
        if shared.stop.load(Ordering::SeqCst) { break; }
        // It stopped on its own: try again shortly (sooner if it had run for a while).
        if started.elapsed() > Duration::from_secs(60) { backoff = 2; }
        let why = code.map(|c| format!("FFmpeg stopped ({c})")).unwrap_or_else(|| "FFmpeg stopped".into());
        set_status(&shared, &on_status, |s| { s.state = "retrying".into(); if s.error.is_none() { s.error = Some(why); } });
        for _ in 0..backoff * 10 {
            if shared.stop.load(Ordering::SeqCst) { break; }
            thread::sleep(Duration::from_millis(100));
        }
        backoff = (backoff * 2).min(30);
    }
    set_status(&shared, &on_status, |s| { s.state = "stopped".into(); s.fps = 0.0; s.kbps = 0.0; });
}

fn read_messages(err: impl Read, shared: &Shared, on_status: &OnStatus, dests: &[Destination], started: Instant) {
    let mut reader = BufReader::new(err);
    let mut buf = Vec::new();
    let mut last_emit = Instant::now() - Duration::from_secs(5);
    // FFmpeg ends progress lines with '\r' and messages with '\n'.
    loop {
        buf.clear();
        let mut byte = [0u8; 1];
        let mut got = false;
        while let Ok(1) = reader.read(&mut byte) {
            got = true;
            if byte[0] == b'\n' || byte[0] == b'\r' { break; }
            buf.push(byte[0]);
        }
        if !got { break; }
        let line = redact(&String::from_utf8_lossy(&buf), dests);
        if line.trim().is_empty() { continue; }
        if let Some((fps, kbps, drop)) = parse_stats(&line) {
            let emit = last_emit.elapsed() >= Duration::from_secs(1);
            let up = started.elapsed().as_secs();
            let mut snapshot = None;
            if let Ok(mut s) = shared.status.lock() {
                s.fps = fps; s.kbps = kbps; s.dropped = drop; s.uptime_secs = up;
                if s.state == "starting" || s.state == "retrying" { s.state = "live".into(); }
                for d in &mut s.destinations { if d.state == "connecting" { d.state = "live".into(); } }
                if emit { snapshot = Some(s.clone()); }
            }
            if let Some(snap) = snapshot { last_emit = Instant::now(); on_status(snap); }
            continue;
        }
        if line.starts_with("Enter command") || line.starts_with("Command reply") { continue; }
        eprintln!("[Stream] {line}");
        // A destination's own failure names its server.
        let lower = line.to_lowercase();
        let failed_dest = dests.iter().find(|d| {
            let host = d.url.split("://").nth(1).and_then(|r| r.split(['/', ':']).next()).unwrap_or("");
            !host.is_empty() && lower.contains(&host.to_lowercase())
        });
        let is_error = lower.contains("error") || lower.contains("failed") || lower.contains("refused") || lower.contains("timed out");
        if !is_error { continue; }
        set_status(shared, on_status, |s| {
            match failed_dest {
                Some(d) => if let Some(ds) = s.destinations.iter_mut().find(|x| x.id == d.id) { ds.state = "failed".into(); ds.error = Some(line.clone()); },
                None => s.error = Some(line.clone()),
            }
        });
    }
}

/// Writes the newest graphics frame to FFmpeg at the stream's rate, until FFmpeg goes.
fn write_graphics(port: u16, shared: &Shared) {
    let (w, h, fps) = match shared.cfg.lock() { Ok(c) => (c.width, c.height, c.fps.max(1)), Err(_) => return };
    let size = (w * h * 4) as usize;
    // FFmpeg opens the port as it starts.
    let mut sock = None;
    for _ in 0..100 {
        if shared.stop.load(Ordering::SeqCst) { return; }
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) { sock = Some(s); break; }
        thread::sleep(Duration::from_millis(100));
    }
    let Some(mut sock) = sock else { return };
    let _ = sock.set_nodelay(true);
    let clear = vec![0u8; size];
    let period = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next = Instant::now();
    loop {
        let bytes = next_frame(shared, w, h);
        let out: &[u8] = match bytes.as_deref() { Some(b) if b.len() == size => b, _ => &clear };
        if sock.write_all(out).is_err() { return; }
        next += period;
        let now = Instant::now();
        if next > now { thread::sleep(next - now); } else { next = now; }
    }
}

/// The frame to send now: the newest snapshot (fading in from the one before), with plain
/// (not premultiplied) transparency, which FFmpeg's overlay expects.
fn next_frame(shared: &Shared, w: u32, h: u32) -> Option<Arc<Vec<u8>>> {
    let mut slot = shared.frame.lock().ok()?;
    let cur = slot.current.clone()?;
    if cur.width != w || cur.height != h { return None; }
    if let (Some(prev), Some(from)) = (slot.previous.clone(), slot.fade_from) {
        let t = from.elapsed().as_secs_f32() * 1000.0 / slot.fade_ms.max(1) as f32;
        if t < 1.0 && prev.width == w && prev.height == h {
            let mut mixed = vec![0u8; cur.bgra.len()];
            blend(&mut mixed, &prev.bgra, &cur.bgra, t);
            let f = Frame { width: w, height: h, bgra: mixed };
            return Some(Arc::new(f.unpremultiplied()));
        }
        slot.previous = None;
        slot.fade_from = None;
    }
    if slot.ready.is_none() { slot.ready = Some(Arc::new(cur.unpremultiplied())); }
    slot.ready.clone()
}

// ── All the pipelines ────────────────────────────────────────────────────────

#[derive(Default)]
pub struct StreamState {
    pipelines: Mutex<Vec<Pipeline>>,
}

impl StreamState {
    pub fn start(&self, ffmpeg: PathBuf, configs: Vec<PipelineConfig>, on_status: OnStatus) -> Result<(), String> {
        let mut p = self.pipelines.lock().map_err(|e| e.to_string())?;
        for old in p.drain(..) { old.stop(); }
        for cfg in configs {
            if cfg.destinations.is_empty() { continue; }
            p.push(Pipeline::start(ffmpeg.clone(), cfg, on_status.clone()));
        }
        if p.is_empty() { return Err("No destination is switched on.".into()); }
        Ok(())
    }

    pub fn stop(&self) {
        if let Ok(mut p) = self.pipelines.lock() { for old in p.drain(..) { old.stop(); } }
    }

    /// Kairo is quitting: end every FFmpeg now, so no stream outlives the app.
    pub fn kill_now(&self) {
        if let Ok(mut p) = self.pipelines.lock() {
            for x in p.drain(..) {
                x.shared.stop.store(true, Ordering::SeqCst);
                if let Ok(mut c) = x.shared.child.lock() { if let Some(ch) = c.as_mut() { let _ = ch.kill(); let _ = ch.wait(); } }
            }
        }
    }

    pub fn status(&self) -> Vec<PipelineStatus> {
        self.pipelines.lock().map(|p| p.iter().map(|x| x.status()).collect()).unwrap_or_default()
    }

    /// A snapshot from a capture window: `stream-<output>` goes to that output's pipeline.
    pub fn push_frame(&self, capture_id: &str, frame: Arc<Frame>, fade_ms: u32) {
        let Some(output) = capture_id.strip_prefix("stream-") else { return };
        if let Ok(p) = self.pipelines.lock() {
            for x in p.iter().filter(|x| x.output() == output) { x.push_frame(frame.clone(), fade_ms); }
        }
    }

    pub fn set_program(&self, output: Option<&str>, program: Option<String>, zone: Option<Zone>) {
        if let Ok(p) = self.pipelines.lock() {
            for x in p.iter().filter(|x| output.is_none_or(|o| x.output() == o)) { x.set_program(program.clone(), zone); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PipelineConfig {
        PipelineConfig {
            output: "display-1".into(), width: 1280, height: 720, fps: 30, video_kbps: 4500, audio_kbps: 160,
            destinations: vec![
                Destination { id: "a".into(), name: "YouTube".into(), url: "rtmp://a.rtmp.youtube.com/live2/".into(), key: "abcd-efgh-1234".into() },
                Destination { id: "b".into(), name: "Other".into(), url: "rtmps://x.example.com:443/app".into(), key: "".into() },
            ],
            cameras: vec![Camera { id: "cam1".into(), label: "FaceTime HD Camera (Built-in)".into() }, Camera { id: "cam2".into(), label: "Missing".into() }],
            program: Some("cam1".into()), zone: None, audio_input: Some("MacBook Pro Microphone (Built-in)".into()),
        }
    }

    #[test]
    fn picks_hardware_encoders_first() {
        let listing = " V....D libx264  libx264 H.264\n V....D h264_videotoolbox VideoToolbox\n";
        assert_eq!(pick_encoder(listing), Some("h264_videotoolbox"));
        assert_eq!(pick_encoder(" V....D libx264  x\n"), Some("libx264"));
        assert_eq!(pick_encoder(" V....D mpeg4 x\n"), None);
    }

    #[test]
    fn urls_and_keys() {
        let c = cfg();
        assert_eq!(destination_url(&c.destinations[0]), "rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234");
        assert_eq!(destination_url(&c.destinations[1]), "rtmps://x.example.com:443/app");
        assert_eq!(redact("failed rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234 x", &c.destinations), "failed rtmp://a.rtmp.youtube.com/live2/•••• x");
        assert_eq!(tee_escape("rtmp://h/a|b[c]"), "rtmp://h/a\\|b\\[c\\]");
    }

    #[test]
    fn matches_browser_labels_to_devices() {
        let names = vec!["FaceTime HD Camera".to_string(), "Capture screen 0".to_string()];
        assert_eq!(match_device("FaceTime HD Camera (Built-in)", &names).map(String::as_str), Some("FaceTime HD Camera"));
        assert_eq!(match_device("Nope", &names), None);
        let audio = vec!["BlackHole 2ch".to_string(), "MacBook Pro Microphone".to_string()];
        assert_eq!(match_device("MacBook Pro Microphone (Built-in)", &audio).map(String::as_str), Some("MacBook Pro Microphone"));
    }

    #[test]
    fn parses_device_lists() {
        let mac = "[AVFoundation indev @ 0x1] AVFoundation video devices:\n[AVFoundation indev @ 0x1] [0] FaceTime HD Camera\n[AVFoundation indev @ 0x1] [1] Capture screen 0\n[AVFoundation indev @ 0x1] AVFoundation audio devices:\n[AVFoundation indev @ 0x1] [0] BlackHole 2ch\n[AVFoundation indev @ 0x1] [1] MacBook Pro Microphone\n";
        let (v, a) = parse_device_list(mac);
        assert_eq!(v, vec!["FaceTime HD Camera", "Capture screen 0"]);
        assert_eq!(a, vec!["BlackHole 2ch", "MacBook Pro Microphone"]);
        let win = "[dshow @ 0] \"HD Pro Webcam C920\" (video)\n[dshow @ 0]   Alternative name \"@device_pnp_x\"\n[dshow @ 0] \"Microphone (C920)\" (audio)\n";
        let (v, a) = parse_device_list(win);
        assert_eq!(v, vec!["HD Pro Webcam C920"]);
        assert_eq!(a, vec!["Microphone (C920)"]);
    }

    #[test]
    fn places_the_camera_in_a_zone() {
        // Full frame.
        assert_eq!(placement(None, 1280, 720), Placement { crop_w: 1280, crop_h: 720, w: 1280, h: 720, x: 0, y: 0 });
        // A portrait zone on the left of a 1080p canvas: the crop keeps the zone's shape.
        let p = placement(Some(Zone { x: 60.0, y: 120.0, w: 640.0, h: 840.0 }), 1920, 1080);
        assert_eq!((p.w, p.h, p.x, p.y), (640, 840, 60, 120));
        assert_eq!(p.crop_h, 1080);
        assert!((p.crop_w as f64 / p.crop_h as f64 - 640.0 / 840.0).abs() < 0.01);
        assert_eq!(p.crop_w % 2, 0);
    }

    #[test]
    fn builds_the_command() {
        let c = cfg();
        let video = vec!["FaceTime HD Camera".to_string()];
        let audio = vec!["MacBook Pro Microphone".to_string()];
        let inputs = build_inputs(&c, 5555, &video, &audio);
        if cfg!(target_os = "macos") {
            assert_eq!(inputs.camera_inputs, vec![("cam1".to_string(), 1)]);
            assert_eq!(inputs.audio_input, Some(2));
            assert!(inputs.warnings.iter().any(|w| w.contains("Missing")));
        }
        assert_eq!(select_index(Some("cam1"), &inputs.camera_inputs), if cfg!(target_os = "macos") { 0 } else { inputs.camera_inputs.len() });
        assert_eq!(select_index(None, &inputs.camera_inputs), inputs.camera_inputs.len());
        let args = ffmpeg_args(&c, "h264_videotoolbox", &inputs, 0, placement(None, 1280, 720)).join(" ");
        assert!(args.contains("tcp://127.0.0.1:5555?listen=1"));
        assert!(args.contains("[f=flv:onfail=ignore]rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234|[f=flv:onfail=ignore]rtmps://x.example.com:443/app"));
        assert!(args.contains("streamselect@sel"));
        assert!(args.contains("-b:v 4500k"));
    }

    #[test]
    fn reads_progress() {
        let l = "frame=  120 fps= 30 q=-0.0 size=    1024KiB time=00:00:04.00 bitrate=2998.4kbits/s dup=0 drop=3 speed=   1x";
        assert_eq!(parse_stats(l), Some((30.0, 2998.4, 3)));
        assert_eq!(parse_stats("Error opening output"), None);
    }

    #[test]
    fn live_commands_move_the_camera() {
        let full = placement(None, 1920, 1080);
        let zone = placement(Some(Zone { x: 100.0, y: 50.0, w: 960.0, h: 540.0 }), 1920, 1080);
        let cmds = live_commands(Some((0, full)), 1, zone);
        assert_eq!(cmds[0], "streamselect@sel -1 map 1");
        assert!(cmds.contains(&"overlay@zo -1 x 100".to_string()));
        assert!(cmds.contains(&"scale@zs -1 w 960".to_string()));
        // Same shape: the crop stays as it is.
        assert!(!cmds.iter().any(|c| c.starts_with("crop@zc")));
        // Only the camera switched.
        assert_eq!(live_commands(Some((1, zone)), 0, zone), vec!["streamselect@sel -1 map 0".to_string()]);
        assert!(live_commands(Some((0, zone)), 0, zone).is_empty());
    }

    /// End to end against two local RTMP listeners, with FFmpeg's test sources standing in for
    /// the camera and microphone: `KAIRO_STREAM_E2E=1 cargo test -p kairo --lib stream_e2e -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn stream_e2e() {
        if std::env::var("KAIRO_STREAM_E2E").is_err() { return; }
        std::env::set_var("KAIRO_STREAM_TEST_DEVICES", "1");
        let ffmpeg = PathBuf::from(std::env::var("KAIRO_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
        let dir = std::env::temp_dir().join(format!("kairo-stream-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let listen = |port: u16, name: &str| Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-listen", "1", "-i", &format!("rtmp://127.0.0.1:{port}/live/key{port}"), "-c", "copy"])
            .arg(dir.join(name)).stdin(Stdio::null()).spawn().unwrap();
        let mut l1 = listen(19351, "a.flv");
        let mut l2 = listen(19352, "b.flv");
        thread::sleep(Duration::from_millis(800));
        let mut c = cfg();
        c.width = 640; c.height = 360;
        c.destinations = vec![
            Destination { id: "a".into(), name: "A".into(), url: "rtmp://127.0.0.1:19351/live".into(), key: "key19351".into() },
            Destination { id: "b".into(), name: "B".into(), url: "rtmp://127.0.0.1:19352/live/".into(), key: "key19352".into() },
        ];
        c.program = None;
        let statuses = Arc::new(Mutex::new(Vec::<PipelineStatus>::new()));
        let st2 = statuses.clone();
        let p = Pipeline::start(ffmpeg.clone(), c, Arc::new(move |s| st2.lock().unwrap().push(s)));
        // A half-transparent red box over the middle, as a capture would give.
        let mut bgra = vec![0u8; 640 * 360 * 4];
        for y in 120..240 { for x in 220..420 { let i = (y * 640 + x) * 4; bgra[i..i + 4].copy_from_slice(&[0, 0, 128, 128]); } }
        p.push_frame(Arc::new(Frame { width: 640, height: 360, bgra }), 0);
        thread::sleep(Duration::from_secs(3));
        p.set_program(Some("cam1".into()), Some(Zone { x: 60.0, y: 120.0, w: 640.0, h: 840.0 }));
        thread::sleep(Duration::from_secs(4));
        let live = p.status();
        p.stop();
        thread::sleep(Duration::from_secs(4));
        let _ = l1.wait(); let _ = l2.wait();
        println!("status while live: {live:?}");
        assert_eq!(live.state, "live", "{live:?}");
        assert!(live.destinations.iter().all(|d| d.state == "live"), "{live:?}");
        for f in ["a.flv", "b.flv"] {
            let probe = Command::new(&ffmpeg).args(["-hide_banner", "-i"]).arg(dir.join(f)).output().unwrap();
            let info = String::from_utf8_lossy(&probe.stderr).to_string();
            println!("{f}: {}", info.lines().filter(|l| l.contains("Stream #") || l.contains("Duration")).collect::<Vec<_>>().join(" | "));
            assert!(info.contains("Video: h264"), "{info}");
            assert!(info.contains("Audio: aac"), "{info}");
        }
        // A frame from the middle of the stream, for a look.
        let _ = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "5", "-i"]).arg(dir.join("a.flv"))
            .args(["-frames:v", "1"]).arg(dir.join("a.png")).status();
        println!("frames in {}", dir.display());
    }

}
