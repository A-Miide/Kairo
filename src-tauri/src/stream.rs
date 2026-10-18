//! Live streaming (Settings → Streaming): what an output shows, sent to RTMP/RTMPS
//! destinations (YouTube, Facebook, Twitch, any server), several at once.
//!
//! One FFmpeg process per streamed output ("pipeline"). Its picture is built from:
//! - Kairo's graphics for that output (scripture, slides, layout frame, overlays): the
//!   output's hidden capture window (display.html?capture=1&stream=1) is snapshotted on
//!   every change (capture.rs); here the last snapshot is written to FFmpeg at the stream's
//!   frame rate over a local TCP socket, with its transparency.
//! - The Program camera: the Director's cameras are opened by FFmpeg itself, and the one on
//!   air is chosen live with a `streamselect` command, so switching inputs never drops the
//!   stream. It is placed in the layout's video zone (or full frame) by `crop`/`scale`/
//!   `overlay` commands, also live.
//! - Sound: the one audio input chosen in Settings → Audio.
//! It is encoded once (a hardware H.264 encoder where there is one, AAC audio) and sent to
//! every destination with FFmpeg's tee muxer, each destination behind its own fifo that
//! reconnects by itself, so one destination dropping or stalling never touches the others.
//! If FFmpeg itself stops or stalls, it is started again after a short wait; a camera that
//! won't open is left out on the next start instead of stopping the stream.
//!
//! Stream keys are never logged or reported: every message that leaves this module has the
//! destinations' addresses and keys replaced. They do reach FFmpeg as part of its output
//! address (FFmpeg has no other way to take them), so they are visible to other processes of
//! the same user on this computer for as long as the stream runs.

use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::capture::{blend, Frame};

/// The design canvas zones are given in (display.html's 1920×1080).
const DESIGN_W: f64 = 1920.0;
const DESIGN_H: f64 = 1080.0;
/// No progress from FFmpeg for this long while live: it is stuck, start it again.
const STALL: Duration = Duration::from_secs(12);

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
    /// Which of several devices with this name the input is bound to (0 = the first the
    /// computer lists, 1 = the second…), worked out by the page from the input's own device id.
    /// Two cameras of one model have the same name; this is what tells them apart.
    #[serde(default)]
    pub slot: Option<usize>,
}

/// One channel of the sound mixer (Settings → Streaming → Sound): an audio input, its level
/// and whether it is muted. Levels and mutes change live, without restarting FFmpeg.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioIn {
    pub id: String,
    /// The input's name, as FFmpeg lists it.
    pub label: String,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub muted: bool,
}

/// A channel's level as FFmpeg's `volume` filter takes it: a factor, 0 when muted.
pub fn mix_volume(c: &AudioIn) -> f32 {
    if c.muted { 0.0 } else { 10f32.powf(c.gain_db.clamp(-60.0, 12.0) / 20.0) }
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
    /// Every video zone the layouts use: the camera is prepared for each of them, so a layout
    /// change only switches between ready pictures (resizing while running isn't reliable).
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// Recording (the top bar's Record): the file written instead of streaming. A restart
    /// after trouble writes the next part beside it ("… part 2.mp4"), never over it.
    #[serde(default)]
    pub record_to: Option<String>,
    /// The sound mixer's channels; when there are any they replace `audio_input`.
    #[serde(default)]
    pub audio_mix: Vec<AudioIn>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DestStatus {
    pub id: String,
    pub name: String,
    /// "connecting" | "live" | "reconnecting"
    pub state: String,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStatus {
    pub output: String,
    /// Which start this status belongs to: a later Go live has a higher number.
    pub run: u64,
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
    /// A recording's files so far (more than one when it had to restart).
    pub files: Vec<String>,
}

// ── FFmpeg: which one, its encoders, its devices ─────────────────────────────

/// H.264 encoders in the order tried: hardware first. Kairo's own builds carry only LGPL
/// code; a user's own FFmpeg may also have libx264, used only as the last resort.
const ENCODERS: [&str; 6] = ["h264_videotoolbox", "h264_nvenc", "h264_qsv", "h264_amf", "h264_mf", "libx264"];

/// The encoders this FFmpeg lists, in the order to try them.
pub fn encoder_candidates(encoders_listing: &str) -> Vec<&'static str> {
    ENCODERS.into_iter().filter(|e| encoders_listing.lines().any(|l| l.split_whitespace().nth(1) == Some(*e))).collect()
}

/// The first encoder that actually works on this computer (a listed hardware encoder may
/// have no hardware behind it): one tiny frame each.
fn working_encoder(ffmpeg: &Path, listing: &str) -> Option<&'static str> {
    encoder_candidates(listing).into_iter().find(|enc| {
        let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=320x240:r=30", "-frames:v", "2", "-c:v"].map(String::from).to_vec();
        args.push(enc.to_string());
        if *enc == "h264_mf" { args.extend(["-hw_encoding", "1"].map(String::from)); }
        args.extend(["-pix_fmt", "yuv420p", "-f", "null", "-"].map(String::from));
        Command::new(ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
            // Media Foundation without a hardware encoder: its software one, still LGPL.
            || (*enc == "h264_mf" && Command::new(ffmpeg).args(args.iter().filter(|a| *a != "-hw_encoding" && *a != "1"))
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()))
    })
}

fn encoder_args(enc: &str, cfg: &PipelineConfig) -> Vec<String> {
    let kbps = cfg.video_kbps.clamp(500, 20_000);
    let gop = (cfg.fps.max(1) * 2).to_string();
    let mut a: Vec<String> = vec!["-c:v".into(), enc.into()];
    match enc {
        "h264_videotoolbox" => a.extend(["-realtime", "1", "-allow_sw", "1", "-profile:v", "high"].map(String::from)),
        "libx264" => a.extend(["-preset", "veryfast", "-tune", "zerolatency"].map(String::from)),
        "h264_nvenc" => a.extend(["-preset", "p4", "-tune", "ll"].map(String::from)),
        "h264_mf" => a.extend(["-hw_encoding", "1", "-rate_control", "cbr"].map(String::from)),
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
/// `-f dshow -list_devices true -i dummy` (Windows): (video, audio), in FFmpeg's order.
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

/// The device (its position in FFmpeg's list) that a browser label refers to, e.g.
/// "FaceTime HD Camera (Built-in)" → "FaceTime HD Camera": exact, then without the bracketed
/// parts, then one containing the other. Devices in `used` are skipped, so two cameras of the
/// same model ("USB Camera" twice) get the first and the second, never the same one twice.
pub fn match_device(label: &str, names: &[String], used: &HashSet<usize>) -> Option<usize> {
    let l = norm(label);
    if l.is_empty() { return None; }
    let free = |i: &usize| !used.contains(i);
    let idx = |f: &dyn Fn(&String) -> bool| names.iter().enumerate().filter(|(i, n)| free(i) && f(n)).map(|(i, _)| i).next();
    idx(&|n| n.eq_ignore_ascii_case(label))
        .or_else(|| idx(&|n| norm(n) == l))
        .or_else(|| idx(&|n| { let n = norm(n); !n.is_empty() && (n.contains(&l) || l.contains(&n)) }))
}

/// Like `match_device`, for the `slot`-th device with that name: the page knows which of
/// several identical cameras an input is bound to, so the stream opens that one.
pub fn match_device_at(label: &str, names: &[String], used: &HashSet<usize>, slot: Option<usize>) -> Option<usize> {
    if let Some(n) = slot {
        let l = norm(label);
        if !l.is_empty() {
            let tiers: [&dyn Fn(&String) -> bool; 3] = [
                &|x: &String| x.eq_ignore_ascii_case(label),
                &|x: &String| norm(x) == l,
                &|x: &String| { let x = norm(x); !x.is_empty() && (x.contains(&l) || l.contains(&x)) },
            ];
            for f in tiers {
                let all: Vec<usize> = names.iter().enumerate().filter(|(_, x)| f(x)).map(|(i, _)| i).collect();
                if all.is_empty() { continue; }
                if let Some(&i) = all.get(n) { if !used.contains(&i) { return Some(i); } }
                break;
            }
        }
    }
    match_device(label, names, used)
}

/// Can this FFmpeg stream at all: the flv, tee and fifo muxers, RTMP (and RTMPS when a
/// destination needs it), and the filters used here?
pub fn can_stream(ffmpeg: &Path, need_tls: bool) -> bool {
    let out = |args: &[&str]| Command::new(ffmpeg).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let has = |listing: &str, name: &str| listing.lines().any(|l| l.split_whitespace().any(|w| w == name));
    let muxers = out(&["-hide_banner", "-muxers"]);
    let protocols = out(&["-hide_banner", "-protocols"]);
    let filters = out(&["-hide_banner", "-filters"]);
    ["flv", "tee", "fifo"].iter().all(|m| has(&muxers, m)) && has(&protocols, "rtmp") && (!need_tls || has(&protocols, "rtmps"))
        && ["streamselect", "overlay", "crop", "scale", "color", "anullsrc"].iter().all(|f| has(&filters, f))
}

/// The FFmpeg to stream with: Kairo's own when it can, else one on this computer that can.
pub fn streaming_ffmpeg(own: Option<PathBuf>, need_tls: bool) -> Option<PathBuf> {
    if let Some(p) = own.as_ref().filter(|p| can_stream(p, need_tls)) { return Some(p.clone()); }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].iter().map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(exe)).filter(|p| p.is_file() && Some(p) != own.as_ref()).find(|p| can_stream(p, need_tls))
}

// ── The FFmpeg command ───────────────────────────────────────────────────────

/// The address FFmpeg sends to: the server address and the stream key, joined.
pub fn destination_url(d: &Destination) -> String {
    let url = d.url.trim().trim_end_matches('/');
    let key = d.key.trim().trim_start_matches('/');
    if key.is_empty() { url.to_string() } else { format!("{url}/{key}") }
}

/// Tee's own special characters (and the quote), escaped inside one slave address.
fn tee_escape(s: &str) -> String {
    s.chars().flat_map(|c| if "|[]\\'".contains(c) { vec!['\\', c] } else { vec![c] }).collect()
}

/// `rtmp://host[:port]` of an address: what may be shown of it.
fn server_of(url: &str) -> String {
    let u = url.trim();
    let (scheme, rest) = u.split_once("://").unwrap_or(("rtmp", u));
    format!("{scheme}://{}", rest.split('/').next().unwrap_or(""))
}

/// Every destination's full address and key (as typed, joined, and escaped for tee) replaced
/// with its server and "••••", so nothing secret leaves this module — including a key that was
/// pasted into the address itself.
pub fn redact(text: &str, dests: &[Destination]) -> String {
    let mut out = text.to_string();
    for d in dests {
        let full = destination_url(d);
        let shown = format!("{}/••••", server_of(&d.url));
        for secret in [tee_escape(&full), full.clone(), d.url.trim().to_string()] {
            if secret.len() > server_of(&d.url).len() { out = out.replace(&secret, &shown); }
        }
        let k = d.key.trim();
        if k.len() >= 3 { out = out.replace(k, "••••"); }
        // A key in the address: everything after the server's first path part.
        let path: Vec<&str> = d.url.trim().splitn(4, '/').collect();
        if let Some(tail) = path.get(3).and_then(|p| p.split_once('/')).map(|(_, t)| t).filter(|t| t.len() >= 6) {
            out = out.replace(tail, "••••");
        }
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
    /// Camera id → FFmpeg input number, for the cameras FFmpeg is asked to open.
    pub camera_inputs: Vec<(String, usize)>,
    pub audio_input: Option<usize>,
    /// The mixer's channels FFmpeg was asked to open: (channel id, FFmpeg input number).
    pub mix_inputs: Vec<(String, usize)>,
    pub warnings: Vec<String>,
}

/// Which FFmpeg input device kind this computer uses.
fn device_format() -> Option<&'static str> {
    if cfg!(target_os = "macos") { Some("avfoundation") } else if cfg!(target_os = "windows") { Some("dshow") } else { None }
}

/// `skip`: cameras left out (they failed to open last time).
pub fn build_inputs(cfg: &PipelineConfig, graphics_port: u16, video_names: &[String], audio_names: &[String], skip: &HashSet<String>) -> Inputs {
    let mut args: Vec<String> = Vec::new();
    let mut warnings = Vec::new();
    // 0: Kairo's graphics, with transparency, from our own socket. A short queue: these are
    // big raw frames, and a backlog only adds delay.
    args.extend(["-thread_queue_size", "8", "-use_wallclock_as_timestamps", "1", "-f", "rawvideo", "-pix_fmt", "bgra"].map(String::from));
    args.extend(["-s".into(), format!("{}x{}", cfg.width, cfg.height), "-framerate".into(), cfg.fps.to_string()]);
    args.extend(["-i".into(), format!("tcp://127.0.0.1:{graphics_port}?listen=1")]);
    let mut next = 1;
    let mut camera_inputs = Vec::new();
    let test_sources = std::env::var("KAIRO_STREAM_TEST_DEVICES").is_ok();
    let mut used = HashSet::new();
    for cam in &cfg.cameras {
        if skip.contains(&cam.id) {
            warnings.push(format!("\"{}\" couldn't be opened for the stream, so it is left out.", cam.label));
            continue;
        }
        if test_sources {
            args.extend(["-re", "-f", "lavfi", "-i"].map(String::from));
            args.push(format!("testsrc2=size=1280x720:rate={}", cfg.fps));
        } else {
            let Some(fmt) = device_format() else {
                warnings.push(format!("Cameras can't go into the stream on this computer yet ({}).", cam.label));
                continue;
            };
            let Some(i) = match_device_at(&cam.label, video_names, &used, cam.slot) else {
                warnings.push(format!("\"{}\" wasn't found for the stream.", cam.label));
                continue;
            };
            used.insert(i);
            args.extend(["-thread_queue_size", "16", "-f", fmt].map(String::from));
            // The camera's own frame rate: the filter makes it the stream's (a camera that
            // can't do the stream's rate exactly would otherwise refuse to open).
            if fmt == "avfoundation" {
                // By its place in the list: names can repeat and may contain ':'.
                args.extend(["-i".into(), i.to_string()]);
            } else {
                args.extend(["-i".into(), format!("video={}", video_names[i])]);
            }
        }
        camera_inputs.push((cam.id.clone(), next));
        next += 1;
    }
    let mut audio_input = None;
    let mut mix_inputs = Vec::new();
    if !cfg.audio_mix.is_empty() {
        let mut used = HashSet::new();
        for (k, ch) in cfg.audio_mix.iter().enumerate() {
            if test_sources {
                args.extend(["-re", "-f", "lavfi", "-i"].map(String::from));
                args.push(format!("sine=frequency={}:sample_rate=48000", 330 + 110 * k));
            } else {
                let found = device_format().zip(match_device(&ch.label, audio_names, &used));
                let Some((fmt, i)) = found else {
                    warnings.push(format!("The sound input \"{}\" wasn't found; it is left out of the mix.", ch.label));
                    continue;
                };
                used.insert(i);
                args.extend(["-thread_queue_size", "1024", "-f", fmt].map(String::from));
                args.extend(["-i".into(), if fmt == "avfoundation" { format!(":{i}") } else { format!("audio={}", audio_names[i]) }]);
            }
            mix_inputs.push((ch.id.clone(), next));
            next += 1;
        }
    } else if test_sources {
        args.extend(["-re", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"].map(String::from));
        audio_input = Some(next);
    } else if let Some(label) = cfg.audio_input.as_deref().filter(|l| !l.trim().is_empty()) {
        match (device_format(), match_device(label, audio_names, &HashSet::new())) {
            (Some(fmt), Some(i)) => {
                args.extend(["-thread_queue_size", "1024", "-f", fmt].map(String::from));
                args.extend(["-i".into(), if fmt == "avfoundation" { format!(":{i}") } else { format!("audio={}", audio_names[i]) }]);
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
        // Generated silence (also the mixer's floor, so the sound never stops) is made at real-time speed (-re): nothing else paces it, and the
        // destinations' buffers would otherwise let it run far ahead of the picture.
        args.extend(["-re", "-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"].map(String::from));
        audio_input = Some(next);
    }
    Inputs { args, camera_inputs, audio_input, mix_inputs, warnings }
}

/// The selection index for `program` among the opened cameras; the last index is "no camera".
pub fn select_index(program: Option<&str>, camera_inputs: &[(String, usize)]) -> usize {
    program.and_then(|p| camera_inputs.iter().position(|(id, _)| id == p)).unwrap_or(camera_inputs.len())
}

/// The zones the camera is prepared for: the whole frame first, then each layout zone once.
pub fn zone_list(cfg: &PipelineConfig) -> Vec<Option<Zone>> {
    let mut out: Vec<Option<Zone>> = vec![None];
    for z in cfg.zones.iter().chain(cfg.zone.iter()) {
        if !out.iter().any(|o| o.is_some_and(|o| same_zone(o, *z))) { out.push(Some(*z)); }
    }
    out
}

fn same_zone(a: Zone, b: Zone) -> bool {
    (a.x - b.x).abs() < 1.0 && (a.y - b.y).abs() < 1.0 && (a.w - b.w).abs() < 1.0 && (a.h - b.h).abs() < 1.0
}

/// Which prepared picture shows `zone` (0, the whole frame, when it isn't one of them).
pub fn zone_index(zone: Option<Zone>, zones: &[Option<Zone>]) -> usize {
    zone.and_then(|z| zones.iter().position(|o| o.is_some_and(|o| same_zone(o, z)))).unwrap_or(0)
}

pub fn filter_graph(cfg: &PipelineConfig, inputs: &Inputs, selected: usize, zones: &[Option<Zone>], zone_sel: usize) -> String {
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
    // streamselect needs two or more pictures to choose between: with no cameras, just black.
    if n == 1 { g += "[nocam]null[cam];"; } else { g += &format!("{labels}streamselect@sel=inputs={n}:map={selected}[cam];"); }
    // The camera on air, ready in every zone (each on its own black canvas, all the same size).
    if zones.len() == 1 {
        g += "[cam]null[bv];";
    } else {
        let parts: String = (0..zones.len()).map(|i| format!("[z{i}]")).collect();
        g += &format!("[cam]split={}{parts};", zones.len());
        let mut outs = String::new();
        for (i, z) in zones.iter().enumerate() {
            match z {
                None => g += &format!("[z{i}]null[p{i}];"),
                Some(z) => {
                    let p = placement(Some(*z), w, h);
                    g += &format!("[z{i}]crop={}:{},scale={}:{}[s{i}];color=c=black:s={w}x{h}:r={fps},format=yuv420p[k{i}];[k{i}][s{i}]overlay=x={}:y={}:eof_action=pass[p{i}];",
                        p.crop_w, p.crop_h, p.w, p.h, p.x, p.y);
                }
            }
            outs += &format!("[p{i}]");
        }
        g += &format!("{outs}streamselect@zsel=inputs={}:map={zone_sel}[bv];", zones.len());
    }
    // The graphics end the stream when they stop (Kairo has gone): nothing goes out unwatched.
    g += "[0:v]format=rgba[g];[bv][g]overlay=format=auto:eof_action=endall,format=yuv420p[v]";
    // The sound mixer: each channel at its level (volume@m<k>, changed live), all summed over
    // the silent floor so the sound never stops when an input does.
    if !inputs.mix_inputs.is_empty() {
        let mut ins = String::new();
        for (k, (id, n)) in inputs.mix_inputs.iter().enumerate() {
            let vol = cfg.audio_mix.iter().find(|c| &c.id == id).map(mix_volume).unwrap_or(1.0);
            g += &format!(";[{n}:a]aresample=48000,aformat=sample_rates=48000:channel_layouts=stereo,volume@m{k}=volume={vol:.4}:eval=frame[m{k}]");
            ins += &format!("[m{k}]");
        }
        let floor = inputs.audio_input.unwrap_or(1);
        g += &format!(";[{floor}:a]aformat=sample_rates=48000:channel_layouts=stereo[mfloor];{ins}[mfloor]amix=inputs={}:duration=longest:normalize=0:dropout_transition=0[a]", inputs.mix_inputs.len() + 1);
    }
    g
}

/// One destination behind its own fifo: it reconnects by itself, and a slow or dead server
/// only drops its own packets instead of holding up the others.
fn slave(d: &Destination) -> String {
    format!(
        "[f=fifo:fifo_format=flv:drop_pkts_on_overflow=1:attempt_recovery=1:recover_any_error=1:recovery_wait_time=2:restart_with_keyframe=1:onfail=ignore]{}",
        tee_escape(&destination_url(d))
    )
}

pub fn ffmpeg_args(cfg: &PipelineConfig, enc: &str, inputs: &Inputs, selected: usize, zones: &[Option<Zone>], zone_sel: usize) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "warning", "-stats", "-stats_period", "1", "-y"].map(String::from).to_vec();
    a.extend(inputs.args.iter().cloned());
    a.extend(["-filter_complex".into(), filter_graph(cfg, inputs, selected, zones, zone_sel)]);
    let audio = if inputs.mix_inputs.is_empty() { format!("{}:a", inputs.audio_input.unwrap_or(1)) } else { "[a]".into() };
    a.extend(["-map".into(), "[v]".into(), "-map".into(), audio]);
    a.extend(encoder_args(enc, cfg));
    a.extend([
        "-c:a".into(), "aac".into(), "-b:a".into(), format!("{}k", cfg.audio_kbps.clamp(64, 320)),
        "-ar".into(), "48000".into(), "-ac".into(), "2".into(),
        "-flags".into(), "+global_header".into(),
    ]);
    if let Some(path) = cfg.record_to.as_ref() {
        // Fragmented MP4, each piece written out as it is made: everything recorded so far
        // plays even if Kairo or the computer stops.
        a.extend(["-movflags".into(), "+frag_keyframe+empty_moov+default_base_moof".into(), "-flush_packets".into(), "1".into(), "-f".into(), "mp4".into(), path.clone()]);
        return a;
    }
    let slaves: Vec<String> = cfg.destinations.iter().map(slave).collect();
    a.extend(["-f".into(), "tee".into(), slaves.join("|")]);
    a
}

/// A recording's file for each start: the file asked for, then "… part 2", "… part 3".
pub fn record_part(base: &str, part: u32) -> String {
    if part <= 1 { return base.to_string(); }
    let p = Path::new(base);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    p.with_file_name(format!("{stem} part {part}{ext}")).to_string_lossy().to_string()
}

/// FFmpeg commands that put another camera on air and/or show it in another zone: switches
/// between pictures that are already being made, so nothing is resized while running.
pub fn live_commands(from: Option<(usize, usize)>, selected: usize, zone_sel: usize, zones: usize) -> Vec<String> {
    // The zone first: a camera coming on air appears straight in its place.
    let mut c = Vec::new();
    if zones > 1 && from.map(|f| f.1) != Some(zone_sel) { c.push(format!("streamselect@zsel -1 map {zone_sel}")); }
    if from.map(|f| f.0) != Some(selected) { c.push(format!("streamselect@sel -1 map {selected}")); }
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

/// The tee slave a message is about: "Slave muxer #1 failed" (counted from 0).
pub fn slave_index(line: &str) -> Option<usize> {
    let i = line.find("Slave muxer #")? + "Slave muxer #".len();
    line[i..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// The FFmpeg input a start-up error is about: "[in#2 @ 0x…] Error opening input: …".
pub fn failed_input(line: &str) -> Option<usize> {
    let i = line.find("[in#")? + 4;
    let n: usize = line[i..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    let l = line.to_lowercase();
    (l.contains("error") || l.contains("not supported") || l.contains("failed") || l.contains("cannot")).then_some(n)
}

// ── A running pipeline ───────────────────────────────────────────────────────

static RUNS: AtomicU64 = AtomicU64::new(1);

struct Shared {
    cfg: Mutex<PipelineConfig>,
    status: Mutex<PipelineStatus>,
    stop: AtomicBool,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    frame: Mutex<FrameSlot>,
    camera_inputs: Mutex<Vec<(String, usize)>>,
    /// What FFmpeg was last told (camera, zone picture).
    sent: Mutex<Option<(usize, usize)>>,
    /// The zones this FFmpeg prepared the camera for.
    zones: Mutex<Vec<Option<Zone>>>,
    /// When FFmpeg last reported progress.
    progress: Mutex<Instant>,
    /// The mixer channels this FFmpeg opened, in filter order, and the level each was last set to.
    mix_inputs: Mutex<Vec<(String, usize)>>,
    sent_volumes: Mutex<Vec<f32>>,
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
    supervisor: Option<JoinHandle<()>>,
    /// Wakes the one thread that sends FFmpeg its camera commands.
    nudge: mpsc::Sender<()>,
    on_status: OnStatus,
}

pub type OnStatus = Arc<dyn Fn(PipelineStatus) + Send + Sync>;

/// What the status says about cameras added after this stream started.
const PENDING_CAMERAS: &str = "Not in the stream yet:";

impl Pipeline {
    pub fn start(ffmpeg: PathBuf, cfg: PipelineConfig, on_status: OnStatus) -> Pipeline {
        let run = RUNS.fetch_add(1, Ordering::SeqCst);
        let status = PipelineStatus {
            output: cfg.output.clone(),
            run,
            state: "starting".into(),
            destinations: match cfg.record_to.as_ref() {
                Some(path) => vec![DestStatus { id: "recording".into(), name: Path::new(path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), state: "connecting".into(), error: None }],
                None => cfg.destinations.iter().map(|d| DestStatus { id: d.id.clone(), name: d.name.clone(), state: "connecting".into(), error: None }).collect(),
            },
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
            zones: Mutex::new(vec![None]),
            progress: Mutex::new(Instant::now()),
            mix_inputs: Mutex::new(Vec::new()),
            sent_volumes: Mutex::new(Vec::new()),
        });
        let (nudge, wake) = mpsc::channel::<()>();
        let s2 = shared.clone();
        thread::spawn(move || send_commands(&s2, wake));
        let s3 = shared.clone();
        let on_status2 = on_status.clone();
        let supervisor = Some(thread::spawn(move || supervise(ffmpeg, s3, on_status)));
        Pipeline { shared, supervisor, nudge, on_status: on_status2 }
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
        if let Ok(mut cfg) = self.shared.cfg.lock() {
            cfg.program = program;
            cfg.zone = zone;
        }
        let _ = self.nudge.send(());
    }

    /// The cameras set up in Director changed while this is running (one was added, or came
    /// back). The stream is never restarted for it: FFmpeg opens its inputs once, so the new ones
    /// are used from the next start, and the status says which are missing meanwhile.
    pub fn set_cameras(&self, cameras: Vec<Camera>) {
        if let Ok(mut cfg) = self.shared.cfg.lock() { cfg.cameras = cameras.clone(); }
        let open: Vec<String> = self.shared.camera_inputs.lock().map(|c| c.iter().map(|(id, _)| id.clone()).collect()).unwrap_or_default();
        let missing: Vec<&str> = cameras.iter().filter(|c| !open.contains(&c.id)).map(|c| c.label.as_str()).collect();
        set_status(&self.shared, &self.on_status, |s| {
            s.warnings.retain(|w| !w.starts_with(PENDING_CAMERAS));
            // A camera that was already left out says so itself.
            let left_out: Vec<&str> = missing.iter().copied().filter(|l| !s.warnings.iter().any(|w| w.contains(&format!("\"{l}\"")))).collect();
            if !left_out.is_empty() && s.state != "stopped" {
                s.warnings.push(format!("{PENDING_CAMERAS} {}. Stop and Go live again to include {}.", left_out.join(", "), if left_out.len() == 1 { "it" } else { "them" }));
            }
        });
    }

    /// The sound mixer's levels or mutes changed: applied live. (A channel added or removed
    /// is used from the next start.)
    pub fn set_mix(&self, mix: Vec<AudioIn>) {
        if let Ok(mut cfg) = self.shared.cfg.lock() {
            for c in &mut cfg.audio_mix {
                if let Some(n) = mix.iter().find(|n| n.id == c.id) { c.gain_db = n.gain_db; c.muted = n.muted; }
            }
        }
        let _ = self.nudge.send(());
    }

    /// Asks FFmpeg to finish; the supervisor makes sure it is gone within a few seconds.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(mut stdin) = self.shared.stdin.lock() {
            if let Some(w) = stdin.as_mut() { let _ = w.write_all(b"q"); let _ = w.flush(); }
            *stdin = None;
        }
        let _ = self.nudge.send(());
    }

    fn kill(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(mut c) = self.shared.child.lock() { if let Some(ch) = c.as_mut() { let _ = ch.kill(); let _ = ch.wait(); } }
    }

    /// Waits (up to `limit`) for the supervisor to end, killing FFmpeg if it takes too long.
    fn join(mut self, limit: Duration) {
        let Some(h) = self.supervisor.take() else { return };
        let until = Instant::now() + limit;
        while !h.is_finished() && Instant::now() < until { thread::sleep(Duration::from_millis(50)); }
        if !h.is_finished() { self.kill(); }
        let _ = h.join();
    }
}

/// The one thread per pipeline that tells FFmpeg where the camera goes. It always works from
/// the latest wish, so fast switches can never be applied out of order.
fn send_commands(shared: &Shared, wake: mpsc::Receiver<()>) {
    while wake.recv().is_ok() {
        while wake.try_recv().is_ok() {}
        if shared.stop.load(Ordering::SeqCst) { return; }
        let (selected, zone_sel, nzones) = {
            let Ok(cfg) = shared.cfg.lock() else { return };
            let cams = shared.camera_inputs.lock().map(|c| c.clone()).unwrap_or_default();
            let zones = shared.zones.lock().map(|z| z.clone()).unwrap_or_default();
            (select_index(cfg.program.as_deref(), &cams), zone_index(cfg.zone, &zones), zones.len())
        };
        let from = shared.sent.lock().ok().and_then(|s| *s);
        let mut cmds = live_commands(from, selected, zone_sel, nzones);
        let volumes = wanted_volumes(shared);
        let sent_vol = shared.sent_volumes.lock().map(|v| v.clone()).unwrap_or_default();
        cmds.extend(volume_commands(&sent_vol, &volumes));
        if cmds.is_empty() { continue; }
        // FFmpeg polls its keyboard about every 100 ms and the last command of a burst waits
        // for the next key, so they go one at a time and harmless repeats follow them.
        let last = cmds.last().cloned().unwrap_or_default();
        cmds.push(last.clone());
        cmds.push(last);
        let mut ok = true;
        for c in cmds {
            {
                let Ok(mut stdin) = shared.stdin.lock() else { return };
                let Some(w) = stdin.as_mut() else { ok = false; break };
                if w.write_all(format!("c{c}\n").as_bytes()).and_then(|_| w.flush()).is_err() { ok = false; break; }
            }
            thread::sleep(Duration::from_millis(110));
        }
        if ok {
            if let Ok(mut s) = shared.sent.lock() { *s = Some((selected, zone_sel)); }
            if let Ok(mut v) = shared.sent_volumes.lock() { *v = volumes; }
        }
    }
}

/// The level each opened mixer channel should be at now, in filter order.
fn wanted_volumes(shared: &Shared) -> Vec<f32> {
    let mix = shared.cfg.lock().map(|c| c.audio_mix.clone()).unwrap_or_default();
    let opened = shared.mix_inputs.lock().map(|m| m.clone()).unwrap_or_default();
    opened.iter().map(|(id, _)| mix.iter().find(|c| &c.id == id).map(mix_volume).unwrap_or(1.0)).collect()
}

/// FFmpeg commands for the mixer channels whose level changed.
pub fn volume_commands(sent: &[f32], wanted: &[f32]) -> Vec<String> {
    wanted.iter().enumerate()
        .filter(|(k, v)| sent.get(*k).is_none_or(|s| (s - **v).abs() > 0.0005))
        .map(|(k, v)| format!("volume@m{k} -1 volume {v:.4}"))
        .collect()
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
    let Some(enc) = working_encoder(&ffmpeg, &encoders) else {
        set_status(&shared, &on_status, |s| { s.state = "failed".into(); s.error = Some("No H.264 encoder works with this copy of FFmpeg on this computer.".into()); });
        return;
    };
    // Mixing needs FFmpeg's amix and volume filters; without them the first channel that is
    // on is used alone.
    let filters = Command::new(&ffmpeg).args(["-hide_banner", "-filters"]).stdin(Stdio::null()).stderr(Stdio::null()).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let can_mix = ["amix", "volume"].iter().all(|f| filters.lines().any(|l| l.split_whitespace().any(|w| w == *f)));
    let mut backoff = 2u64;
    let mut skip: HashSet<String> = HashSet::new();
    // Starts in a row that never got going without saying which camera was the trouble.
    let mut unnamed_failures = 0u32;
    let mut part = 0u32;
    while !shared.stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        let mut cfg = match shared.cfg.lock() { Ok(c) => c.clone(), Err(_) => return };
        if let Some(base) = cfg.record_to.clone() {
            // Never over a file already there (an earlier part, or someone else's).
            part += 1;
            while Path::new(&record_part(&base, part)).exists() { part += 1; }
            let file = record_part(&base, part);
            cfg.record_to = Some(file.clone());
            let name = Path::new(&file).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            set_status(&shared, &on_status, |s| { s.files.push(file); if let Some(d) = s.destinations.first_mut() { d.name = name; } });
        }
        // Listed again each time: a camera plugged in since is found.
        let (video_names, audio_names) = list_devices(&ffmpeg);
        let port = match TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) { Ok(a) => a.port(), Err(e) => {
            set_status(&shared, &on_status, |s| { s.state = "failed".into(); s.error = Some(format!("No local port for the picture: {e}")); });
            return;
        } };
        let mut mix_warning = None;
        if !can_mix && !cfg.audio_mix.is_empty() {
            cfg.audio_input = cfg.audio_mix.iter().find(|c| !c.muted).map(|c| c.label.clone());
            cfg.audio_mix.clear();
            mix_warning = Some("This copy of FFmpeg can't mix sound, so only the first sound input that is on is used.".to_string());
        }
        let mut inputs = build_inputs(&cfg, port, &video_names, &audio_names, &skip);
        inputs.warnings.extend(mix_warning);
        let selected = select_index(cfg.program.as_deref(), &inputs.camera_inputs);
        let zones = zone_list(&cfg);
        let zone_sel = zone_index(cfg.zone, &zones);
        if let Ok(mut c) = shared.camera_inputs.lock() { *c = inputs.camera_inputs.clone(); }
        if let Ok(mut z) = shared.zones.lock() { *z = zones.clone(); }
        if let Ok(mut s) = shared.sent.lock() { *s = Some((selected, zone_sel)); }
        if let Ok(mut m) = shared.mix_inputs.lock() { *m = inputs.mix_inputs.clone(); }
        if let Ok(mut v) = shared.sent_volumes.lock() {
            *v = inputs.mix_inputs.iter().map(|(id, _)| cfg.audio_mix.iter().find(|c| &c.id == id).map(mix_volume).unwrap_or(1.0)).collect();
        }
        let args = ffmpeg_args(&cfg, enc, &inputs, selected, &zones, zone_sel);
        let mut child = match Command::new(&ffmpeg).args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn() {
            Ok(c) => c,
            Err(e) => {
                set_status(&shared, &on_status, |s| { s.state = "failed".into(); s.error = Some(format!("FFmpeg didn't start: {e}")); });
                return;
            }
        };
        let stderr = child.stderr.take();
        if let Ok(mut s) = shared.stdin.lock() { *s = child.stdin.take(); }
        // Kept before anything else can happen, so a quit at any moment finds it.
        if let Ok(mut c) = shared.child.lock() { *c = Some(child); }
        if shared.stop.load(Ordering::SeqCst) { break; }
        let warnings = inputs.warnings.clone();
        set_status(&shared, &on_status, |s| {
            s.state = if s.state == "retrying" { "retrying".into() } else { "starting".into() };
            s.encoder = enc.into(); s.warnings = warnings;
            for d in &mut s.destinations { d.state = "connecting".into(); d.error = None; }
        });
        if let Ok(mut p) = shared.progress.lock() { *p = Instant::now(); }

        // The picture: our socket → FFmpeg's graphics input.
        let s_writer = shared.clone();
        let writer = thread::spawn(move || write_graphics(port, &s_writer));
        // A stuck FFmpeg (no progress while live) is ended, so it is started again.
        let s_dog = shared.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();
        let watchdog = thread::spawn(move || {
            while !done2.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(500));
                let stuck = s_dog.progress.lock().map(|p| p.elapsed() > STALL).unwrap_or(false);
                if stuck && !s_dog.stop.load(Ordering::SeqCst) {
                    if let Ok(mut c) = s_dog.child.lock() { if let Some(ch) = c.as_mut() { let _ = ch.kill(); } }
                    break;
                }
            }
        });
        // FFmpeg's messages: progress, each destination's trouble, cameras that won't open.
        let mut early_failures = Vec::new();
        let mut saw_progress = false;
        if let Some(err) = stderr {
            read_messages(err, &shared, &on_status, &cfg.destinations, started, &mut early_failures, &mut saw_progress);
        }
        done.store(true, Ordering::SeqCst);
        let code = shared.child.lock().ok().and_then(|mut c| c.as_mut().and_then(|ch| ch.wait().ok()));
        if let Ok(mut c) = shared.child.lock() { *c = None; }
        if let Ok(mut s) = shared.stdin.lock() { *s = None; }
        let _ = writer.join();
        let _ = watchdog.join();
        if shared.stop.load(Ordering::SeqCst) { break; }
        // It never got going: leave out the cameras it couldn't open (or all of them, when it
        // didn't say which) so graphics and sound still go out.
        if saw_progress {
            // It was running: a camera left out earlier (unplugged then, maybe back now) gets
            // another try with this restart, and a start that goes wrong names it again.
            skip.clear();
            unnamed_failures = 0;
        } else if !inputs.camera_inputs.is_empty() {
            let named: Vec<String> = inputs.camera_inputs.iter().filter(|(_, n)| early_failures.contains(n)).map(|(id, _)| id.clone()).collect();
            if named.is_empty() {
                // One start that fails without a word may not be a camera at all (the network, an
                // encoder starting up): only a second one takes every camera out.
                unnamed_failures += 1;
                if unnamed_failures >= 2 { skip.extend(inputs.camera_inputs.iter().map(|(id, _)| id.clone())); }
            } else {
                unnamed_failures = 0;
                skip.extend(named);
            }
            backoff = 1;
        }
        if started.elapsed() > Duration::from_secs(60) { backoff = 2; }
        let why = code.map(|c| format!("FFmpeg stopped ({c})")).unwrap_or_else(|| "FFmpeg stopped".into());
        set_status(&shared, &on_status, |s| { s.state = "retrying".into(); if s.error.is_none() { s.error = Some(why); } });
        for _ in 0..backoff * 10 {
            if shared.stop.load(Ordering::SeqCst) { break; }
            thread::sleep(Duration::from_millis(100));
        }
        backoff = (backoff * 2).min(30);
    }
    // Asked to stop: give FFmpeg a moment to close the streams cleanly, then make sure.
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let gone = shared.child.lock().map(|mut c| c.as_mut().is_none_or(|ch| ch.try_wait().ok().flatten().is_some())).unwrap_or(true);
        if gone { break; }
        if Instant::now() > until {
            if let Ok(mut c) = shared.child.lock() { if let Some(ch) = c.as_mut() { let _ = ch.kill(); let _ = ch.wait(); } }
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    set_status(&shared, &on_status, |s| { s.state = "stopped".into(); s.fps = 0.0; s.kbps = 0.0; });
}

fn read_messages(err: impl Read, shared: &Shared, on_status: &OnStatus, dests: &[Destination], started: Instant, early_failures: &mut Vec<usize>, saw_progress: &mut bool) {
    let mut reader = BufReader::new(err);
    let mut buf = Vec::new();
    let mut last_emit = Instant::now() - Duration::from_secs(5);
    // A destination is shown reconnecting for a while after its last error.
    let mut trouble: Vec<Option<Instant>> = vec![None; dests.len()];
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
            *saw_progress = true;
            if let Ok(mut p) = shared.progress.lock() { *p = Instant::now(); }
            let emit = last_emit.elapsed() >= Duration::from_secs(1);
            let up = started.elapsed().as_secs();
            let mut snapshot = None;
            if let Ok(mut s) = shared.status.lock() {
                s.fps = fps; s.kbps = kbps; s.dropped = drop; s.uptime_secs = up;
                if s.state == "starting" || s.state == "retrying" { s.state = "live".into(); s.error = None; }
                for (i, d) in s.destinations.iter_mut().enumerate() {
                    let quiet = trouble.get(i).copied().flatten().is_none_or(|t| t.elapsed() > Duration::from_secs(15));
                    if quiet && d.state != "live" { d.state = "live".into(); d.error = None; }
                }
                if emit { snapshot = Some(s.clone()); }
            }
            if let Some(snap) = snapshot { last_emit = Instant::now(); on_status(snap); }
            continue;
        }
        if line.starts_with("Enter command") || line.starts_with("Command reply") { if std::env::var("KAIRO_STREAM_DEBUG").is_ok() { eprintln!("[Stream] {line}"); } continue; }
        eprintln!("[Stream] {line}");
        if !*saw_progress { if let Some(n) = failed_input(&line) { early_failures.push(n); } }
        let lower = line.to_lowercase();
        let is_error = lower.contains("error") || lower.contains("failed") || lower.contains("refused") || lower.contains("timed out") || lower.contains("broken pipe");
        if !is_error { continue; }
        // Which destination: the tee slave's number, else a server name only one of them uses.
        let by_host = || {
            let hits: Vec<usize> = dests.iter().enumerate().filter(|(_, d)| {
                let host = server_of(&d.url).split("://").nth(1).unwrap_or("").to_lowercase();
                !host.is_empty() && lower.contains(&host)
            }).map(|(i, _)| i).collect();
            (hits.len() == 1).then(|| hits[0])
        };
        let which = slave_index(&line).filter(|i| *i < dests.len()).or_else(by_host);
        if let Some(i) = which { trouble[i] = Some(Instant::now()); }
        set_status(shared, on_status, |s| {
            match which.and_then(|i| s.destinations.get_mut(i)) {
                Some(ds) => { ds.state = if *saw_progress { "reconnecting".into() } else { "connecting".into() }; ds.error = Some(line.clone()); }
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
    let mut scratch = vec![0u8; size];
    let period = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next = Instant::now();
    loop {
        if shared.stop.load(Ordering::SeqCst) { return; }
        let bytes = next_frame(shared, w, h, &mut scratch);
        let out: &[u8] = match &bytes {
            Some(NextFrame::Ready(b)) if b.len() == size => b,
            Some(NextFrame::Mixed) => &scratch,
            _ => &clear,
        };
        if sock.write_all(out).is_err() { return; }
        next += period;
        let now = Instant::now();
        if next > now { thread::sleep(next - now); } else { next = now; }
    }
}

enum NextFrame {
    /// The newest snapshot, ready to send.
    Ready(Arc<Vec<u8>>),
    /// A fade between two snapshots, mixed into the writer's own buffer.
    Mixed,
}

/// The frame to send now: the newest snapshot (fading in from the one before), with plain
/// (not premultiplied) transparency, which FFmpeg's overlay expects. The mixing is done
/// outside the lock, so a new snapshot is never kept waiting.
fn next_frame(shared: &Shared, w: u32, h: u32, scratch: &mut [u8]) -> Option<NextFrame> {
    let (cur, prev, t) = {
        let mut slot = shared.frame.lock().ok()?;
        let cur = slot.current.clone()?;
        if cur.width != w || cur.height != h { return None; }
        match (slot.previous.clone(), slot.fade_from) {
            (Some(prev), Some(from)) => {
                let t = from.elapsed().as_secs_f32() * 1000.0 / slot.fade_ms.max(1) as f32;
                if t < 1.0 && prev.width == w && prev.height == h {
                    (cur, Some(prev), t)
                } else {
                    slot.previous = None;
                    slot.fade_from = None;
                    (cur, None, 1.0)
                }
            }
            _ => (cur, None, 1.0),
        }
    };
    if let Some(prev) = prev {
        blend(scratch, &prev.bgra, &cur.bgra, t);
        unpremultiply_in_place(scratch);
        return Some(NextFrame::Mixed);
    }
    let ready = shared.frame.lock().ok().and_then(|s| s.ready.clone());
    let ready = match ready {
        Some(r) => r,
        None => {
            let r = Arc::new(cur.unpremultiplied());
            if let Ok(mut s) = shared.frame.lock() {
                // Still the same snapshot: keep the ready copy.
                if s.current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cur)) { s.ready = Some(r.clone()); }
            }
            r
        }
    };
    Some(NextFrame::Ready(ready))
}

fn unpremultiply_in_place(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] { *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8; }
        }
    }
}

// ── All the pipelines ────────────────────────────────────────────────────────

#[derive(Default)]
pub struct StreamState {
    pipelines: Mutex<Vec<Pipeline>>,
    /// One start at a time: a second Go live (a double-click) waits for the first.
    starting: Mutex<()>,
    /// Bumped by every start and stop: a start that is no longer the newest when its wait for
    /// the old stream ends (Stop was pressed meanwhile, or another start came) doesn't go live.
    generation: AtomicU64,
}

impl StreamState {
    pub fn start(&self, ffmpeg: PathBuf, configs: Vec<PipelineConfig>, on_status: OnStatus) -> Result<(), String> {
        let _one = self.starting.lock().map_err(|e| e.to_string())?;
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // The previous stream is finished first, so the same stream keys are never sent to
        // twice at once.
        let old: Vec<Pipeline> = self.pipelines.lock().map_err(|e| e.to_string())?.drain(..).collect();
        for p in &old { p.stop(); }
        for p in old { p.join(Duration::from_secs(4)); }
        let mut p = self.pipelines.lock().map_err(|e| e.to_string())?;
        if self.generation.load(Ordering::SeqCst) != mine {
            return Err("Stopped before it went live.".into());
        }
        for cfg in configs {
            if cfg.destinations.is_empty() && cfg.record_to.is_none() { continue; }
            p.push(Pipeline::start(ffmpeg.clone(), cfg, on_status.clone()));
        }
        if p.is_empty() { return Err("No destination is switched on.".into()); }
        Ok(())
    }

    /// Ends every stream. Returns at once; FFmpeg is given a moment to close each stream
    /// cleanly and is killed after that.
    pub fn stop(&self) {
        // A start still waiting for the old stream to end is cancelled too.
        self.generation.fetch_add(1, Ordering::SeqCst);
        let old: Vec<Pipeline> = self.pipelines.lock().map(|mut p| p.drain(..).collect()).unwrap_or_default();
        for p in &old { p.stop(); }
        thread::spawn(move || { for p in old { p.join(Duration::from_secs(4)); } });
    }

    /// Kairo is quitting: end every FFmpeg now, so no stream outlives the app.
    pub fn kill_now(&self) {
        if let Ok(mut p) = self.pipelines.lock() {
            for x in p.drain(..) { x.kill(); }
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

    pub fn set_mix(&self, mix: Vec<AudioIn>) {
        if let Ok(p) = self.pipelines.lock() { for x in p.iter() { x.set_mix(mix.clone()); } }
    }

    pub fn set_program(&self, output: Option<&str>, program: Option<String>, zone: Option<Zone>) {
        if let Ok(p) = self.pipelines.lock() {
            for x in p.iter().filter(|x| output.is_none_or(|o| x.output() == o)) { x.set_program(program.clone(), zone); }
        }
    }

    pub fn set_cameras(&self, cameras: Vec<Camera>) {
        if let Ok(p) = self.pipelines.lock() { for x in p.iter() { x.set_cameras(cameras.clone()); } }
    }
}

/// Which of the two ways the same engine is used: a live stream to destinations, or a
/// recording to a file. Both run `Pipeline` (one FFmpeg command from `ffmpeg_args`, the same
/// graphics, cameras and sound); they differ only in `record_to` / `destinations` and in which
/// status event the page listens to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Stream,
    Record,
}

impl Kind {
    /// The event this kind's status goes to every window as.
    pub fn event(self) -> &'static str {
        match self {
            Kind::Stream => "stream-status",
            Kind::Record => "record-status",
        }
    }
}

/// The stream and the recording, side by side: either starts and stops without touching the
/// other, and everything that reaches the engine (a captured frame, the camera on air, the
/// sound mix, quitting) goes to both.
#[derive(Default)]
pub struct Engines {
    stream: StreamState,
    record: StreamState,
}

impl Engines {
    pub fn of(&self, kind: Kind) -> &StreamState {
        match kind {
            Kind::Stream => &self.stream,
            Kind::Record => &self.record,
        }
    }

    fn both(&self) -> [&StreamState; 2] {
        [&self.stream, &self.record]
    }

    pub fn push_frame(&self, capture_id: &str, frame: Arc<Frame>, fade_ms: u32) {
        for e in self.both() { e.push_frame(capture_id, frame.clone(), fade_ms); }
    }

    pub fn set_program(&self, output: Option<&str>, program: Option<String>, zone: Option<Zone>) {
        for e in self.both() { e.set_program(output, program.clone(), zone); }
    }

    pub fn set_mix(&self, mix: Vec<AudioIn>) {
        for e in self.both() { e.set_mix(mix.clone()); }
    }

    pub fn set_cameras(&self, cameras: Vec<Camera>) {
        for e in self.both() { e.set_cameras(cameras.clone()); }
    }

    pub fn kill_now(&self) {
        for e in self.both() { e.kill_now(); }
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
            cameras: vec![Camera { id: "cam1".into(), label: "FaceTime HD Camera (Built-in)".into(), slot: None }, Camera { id: "cam2".into(), label: "Missing".into(), slot: None }],
            program: Some("cam1".into()), zone: None, audio_input: Some("MacBook Pro Microphone (Built-in)".into()),
            zones: vec![Zone { x: 60.0, y: 120.0, w: 640.0, h: 840.0 }],
            record_to: None,
            audio_mix: Vec::new(),
        }
    }

    #[test]
    fn a_stop_during_a_start_cancels_it() {
        let st = StreamState::default();
        // A start that finds the generation moved on (a Stop came while it waited) goes nowhere.
        let before = st.generation.load(Ordering::SeqCst);
        st.stop();
        assert_eq!(st.generation.load(Ordering::SeqCst), before + 1);
        // With nothing to start it answers as before, and nothing is left running.
        assert!(st.start(PathBuf::from("ffmpeg"), vec![], Arc::new(|_| {})).is_err());
        assert!(st.status().is_empty());
    }

    #[test]
    fn no_cameras_is_just_black() {
        let mut c = cfg();
        c.cameras.clear();
        let inputs = Inputs { args: vec![], camera_inputs: vec![], audio_input: Some(1), mix_inputs: vec![], warnings: vec![] };
        let g = filter_graph(&c, &inputs, 0, &zone_list(&c), 0);
        assert!(!g.contains("streamselect@sel"), "one picture can't be selected between: {g}");
        assert!(g.contains("[nocam]null[cam]"), "{g}");
    }

    #[test]
        fn mixer_levels_and_live_changes() {
        let ch = |id: &str, db: f32, muted: bool| AudioIn { id: id.into(), label: id.into(), gain_db: db, muted };
        assert!((mix_volume(&ch("a", 0.0, false)) - 1.0).abs() < 1e-4);
        assert!((mix_volume(&ch("a", -6.0, false)) - 0.501).abs() < 1e-3);
        assert_eq!(mix_volume(&ch("a", 0.0, true)), 0.0);
        assert!(mix_volume(&ch("a", 99.0, false)) < 4.0, "kept to +12 dB");
        assert_eq!(volume_commands(&[1.0, 0.5], &[1.0, 0.0]), vec!["volume@m1 -1 volume 0.0000"]);
        assert!(volume_commands(&[1.0], &[1.0]).is_empty());
        let mut c = cfg();
        c.audio_mix = vec![ch("desk", 0.0, false), ch("mic", -6.0, true)];
        let inputs = Inputs { args: vec![], camera_inputs: vec![], audio_input: Some(3), mix_inputs: vec![("desk".into(), 1), ("mic".into(), 2)], warnings: vec![] };
        let g = filter_graph(&c, &inputs, 0, &zone_list(&c), 0);
        assert!(g.contains("volume@m0=volume=1.0000") && g.contains("volume@m1=volume=0.0000"), "{g}");
        assert!(g.contains("amix=inputs=3"), "two channels over the silent floor: {g}");
        let a = ffmpeg_args(&c, "libx264", &inputs, 0, &zone_list(&c), 0);
        assert!(a.windows(2).any(|w| w[0] == "-map" && w[1] == "[a]"));
    }

    #[test]
    fn recording_writes_a_file_and_never_over_one() {
        assert_eq!(record_part("/m/Kairo 2026-10-15 19-30.mp4", 1), "/m/Kairo 2026-10-15 19-30.mp4");
        assert_eq!(record_part("/m/Kairo 2026-10-15 19-30.mp4", 3), "/m/Kairo 2026-10-15 19-30 part 3.mp4");
        let mut c = cfg();
        c.record_to = Some("/m/Kairo.mp4".into());
        let inputs = build_inputs(&c, 5000, &[], &[], &HashSet::new());
        let a = ffmpeg_args(&c, "libx264", &inputs, 0, &zone_list(&c), 0);
        assert_eq!(a.last().map(String::as_str), Some("/m/Kairo.mp4"));
        assert!(!a.iter().any(|x| x == "tee"), "a recording sends nowhere");
        assert!(a.windows(2).any(|w| w[0] == "-movflags" && w[1].contains("frag_keyframe")), "plays even if cut off");
        assert!(a.windows(2).any(|w| w[0] == "-flush_packets" && w[1] == "1"), "written out as it goes");
    }

    #[test]
    fn encoders_in_order() {
        let listing = " V....D libx264  libx264 H.264\n V....D h264_videotoolbox VideoToolbox\n V....D h264_mf MF\n";
        assert_eq!(encoder_candidates(listing), vec!["h264_videotoolbox", "h264_mf", "libx264"]);
        assert!(encoder_candidates(" V....D mpeg4 x\n").is_empty());
    }

    #[test]
    fn urls_and_keys() {
        let c = cfg();
        assert_eq!(destination_url(&c.destinations[0]), "rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234");
        assert_eq!(destination_url(&c.destinations[1]), "rtmps://x.example.com:443/app");
        let r = redact("failed rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234 x", &c.destinations);
        assert!(!r.contains("abcd-efgh-1234"), "{r}");
        assert!(r.contains("rtmp://a.rtmp.youtube.com"), "{r}");
        assert_eq!(tee_escape("rtmp://h/a|b[c]'d"), "rtmp://h/a\\|b\\[c\\]\\'d");
        // A key pasted into the address, with no key field.
        let pasted = vec![Destination { id: "f".into(), name: "FB".into(), url: "rtmps://live-api-s.facebook.com:443/rtmp/FB-1234567890-abc".into(), key: "".into() }];
        let r = redact("rtmps://live-api-s.facebook.com:443/rtmp/FB-1234567890-abc: I/O error", &pasted);
        assert!(!r.contains("FB-1234567890"), "{r}");
        // A key with tee's special characters is hidden in its escaped form too.
        let odd = vec![Destination { id: "o".into(), name: "O".into(), url: "rtmp://h.example/app".into(), key: "k|ey[1]".into() }];
        let r = redact(&format!("x {} y", tee_escape("rtmp://h.example/app/k|ey[1]")), &odd);
        assert!(!r.contains("ey"), "{r}");
    }

    #[test]
    fn matches_browser_labels_to_devices() {
        let names = vec!["FaceTime HD Camera".to_string(), "USB Camera".to_string(), "USB Camera".to_string()];
        let mut used = HashSet::new();
        assert_eq!(match_device("FaceTime HD Camera (Built-in)", &names, &used), Some(0));
        assert_eq!(match_device("Nope", &names, &used), None);
        // Two of the same model: the first, then the second, never the same one twice.
        let first = match_device("USB Camera", &names, &used).unwrap();
        used.insert(first);
        let second = match_device("USB Camera", &names, &used).unwrap();
        assert_eq!((first, second), (1, 2));
        used.insert(second);
        assert_eq!(match_device("USB Camera", &names, &used), None);
    }

    #[test]
    fn two_cameras_of_one_model_are_told_apart_by_their_slot() {
        let names = vec!["FaceTime HD Camera".to_string(), "USB Camera".to_string(), "USB Camera".to_string()];
        let mut used = HashSet::new();
        // The input bound to the second USB camera opens the second one, even when it is
        // asked for first.
        assert_eq!(match_device_at("USB Camera", &names, &used, Some(1)), Some(2));
        used.insert(2);
        assert_eq!(match_device_at("USB Camera", &names, &used, Some(0)), Some(1));
        // No slot, or a slot that isn't there: the first free one, as before.
        assert_eq!(match_device_at("USB Camera", &names, &used, None), Some(1));
        assert_eq!(match_device_at("USB Camera", &names, &used, Some(5)), Some(1));
        assert_eq!(match_device_at("Nope", &names, &used, Some(0)), None);
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
        assert_eq!(placement(None, 1280, 720), Placement { crop_w: 1280, crop_h: 720, w: 1280, h: 720, x: 0, y: 0 });
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
        let audio = vec!["BlackHole 2ch".to_string(), "MacBook Pro Microphone".to_string()];
        let inputs = build_inputs(&c, 5555, &video, &audio, &HashSet::new());
        let zones = zone_list(&c);
        assert_eq!(zones.len(), 2);
        let args = ffmpeg_args(&c, "h264_videotoolbox", &inputs, 0, &zones, 0).join(" ");
        assert!(args.contains("streamselect@zsel=inputs=2:map=0"), "{args}");
        if cfg!(target_os = "macos") {
            assert_eq!(inputs.camera_inputs, vec![("cam1".to_string(), 1)]);
            assert_eq!(inputs.audio_input, Some(2));
            assert!(inputs.warnings.iter().any(|w| w.contains("Missing")));
            // By position: "-i 0" for the camera, "-i :1" for the microphone; no forced rate.
            assert!(args.contains("-f avfoundation -i 0"), "{args}");
            assert!(args.contains("-f avfoundation -i :1"), "{args}");
        }
        assert!(args.contains("tcp://127.0.0.1:5555?listen=1"));
        assert!(args.contains("[f=fifo:fifo_format=flv:"));
        assert!(args.contains("rtmp://a.rtmp.youtube.com/live2/abcd-efgh-1234|[f=fifo"));
        assert!(args.contains("eof_action=endall"));
        assert!(args.contains("-b:v 4500k"));
        // A camera that failed last time is left out.
        let skip: HashSet<String> = ["cam1".to_string()].into_iter().collect();
        let without = build_inputs(&c, 5555, &video, &audio, &skip);
        assert!(without.camera_inputs.is_empty());
        assert!(without.warnings.iter().any(|w| w.contains("left out")));
    }

    #[test]
    fn reads_ffmpeg_messages() {
        let l = "frame=  120 fps= 30 q=-0.0 size=    1024KiB time=00:00:04.00 bitrate=2998.4kbits/s dup=0 drop=3 speed=   1x";
        assert_eq!(parse_stats(l), Some((30.0, 2998.4, 3)));
        assert_eq!(parse_stats("Error opening output"), None);
        assert_eq!(slave_index("[tee @ 0x1] Slave muxer #1 failed: Broken pipe, continuing with 1/2 slaves."), Some(1));
        assert_eq!(slave_index("nothing"), None);
        assert_eq!(failed_input("[in#2 @ 0x7f] Error opening input: Input/output error"), Some(2));
        assert_eq!(failed_input("[in#2 @ 0x7f] Stream #0: Video: rawvideo"), None);
    }

    #[test]
    fn live_commands_move_the_camera() {
        let z = Zone { x: 100.0, y: 50.0, w: 960.0, h: 540.0 };
        let zones = vec![None, Some(z)];
        assert_eq!(zone_index(Some(z), &zones), 1);
        assert_eq!(zone_index(None, &zones), 0);
        assert_eq!(zone_index(Some(Zone { x: 1.0, y: 1.0, w: 2.0, h: 2.0 }), &zones), 0);
        assert_eq!(live_commands(Some((0, 0)), 1, 1, 2), vec!["streamselect@zsel -1 map 1".to_string(), "streamselect@sel -1 map 1".to_string()]);
        assert_eq!(live_commands(Some((1, 1)), 0, 1, 2), vec!["streamselect@sel -1 map 0".to_string()]);
        assert!(live_commands(Some((0, 1)), 0, 1, 2).is_empty());
        // No layout zones: never a zone command.
        assert_eq!(live_commands(Some((0, 0)), 0, 0, 1), Vec::<String>::new());
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
        thread::sleep(Duration::from_millis(1500));
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
        let mut bgra = vec![0u8; 640 * 360 * 4];
        for y in 120..240 { for x in 220..420 { let i = (y * 640 + x) * 4; bgra[i..i + 4].copy_from_slice(&[0, 0, 128, 128]); } }
        p.push_frame(Arc::new(Frame { width: 640, height: 360, bgra }), 0);
        thread::sleep(Duration::from_secs(4));
        p.set_program(Some("cam1".into()), Some(Zone { x: 60.0, y: 120.0, w: 640.0, h: 840.0 }));
        thread::sleep(Duration::from_secs(4));
        let live = p.status();
        p.stop();
        p.join(Duration::from_secs(6));
        thread::sleep(Duration::from_secs(1));
        let _ = l1.wait(); let _ = l2.wait();
        println!("status while live: {live:?}");
        assert_eq!(live.state, "live", "{live:?}");
        assert!(live.destinations.iter().all(|d| d.state == "live"), "{live:?}");
        let all = statuses.lock().unwrap();
        assert_eq!(all.last().map(|s| s.state.as_str()), Some("stopped"));
        let text = format!("{all:?}");
        assert!(!text.contains("key19351") && !text.contains("key19352"), "a key reached the status");
        for f in ["a.flv", "b.flv"] {
            let probe = Command::new(&ffmpeg).args(["-hide_banner", "-i"]).arg(dir.join(f)).output().unwrap();
            let info = String::from_utf8_lossy(&probe.stderr).to_string();
            println!("{f}: {}", info.lines().filter(|l| l.contains("Stream #") || l.contains("Duration")).collect::<Vec<_>>().join(" | "));
            assert!(info.contains("Video: h264"), "{info}");
            assert!(info.contains("Audio: aac"), "{info}");
        }
        let _ = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y", "-sseof", "-1", "-i"]).arg(dir.join("a.flv"))
            .args(["-frames:v", "1"]).arg(dir.join("a.png")).status();
        println!("frames in {}", dir.display());
    }

    /// Mixes two test tones, mutes both live and checks the end of the recording is silent:
    /// `KAIRO_STREAM_E2E=1 cargo test -p kairo --lib mix_e2e -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mix_e2e() {
        if std::env::var("KAIRO_STREAM_E2E").is_err() { return; }
        std::env::set_var("KAIRO_STREAM_TEST_DEVICES", "1");
        let ffmpeg = PathBuf::from(std::env::var("KAIRO_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
        let dir = std::env::temp_dir().join(format!("kairo-mix-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mix.mp4");
        let ch = |id: &str, muted: bool| AudioIn { id: id.into(), label: id.into(), gain_db: 0.0, muted };
        let mut c = cfg();
        c.width = 640; c.height = 360; c.destinations.clear(); c.program = None; c.cameras.clear();
        c.record_to = Some(file.to_string_lossy().to_string());
        c.audio_mix = vec![ch("desk", false), ch("mic", false)];
        let p = Pipeline::start(ffmpeg.clone(), c, Arc::new(|_| {}));
        p.push_frame(Arc::new(Frame { width: 640, height: 360, bgra: vec![90u8; 640 * 360 * 4] }), 0);
        thread::sleep(Duration::from_secs(4));
        p.set_mix(vec![ch("desk", true), ch("mic", true)]);
        thread::sleep(Duration::from_secs(4));
        let st = p.status();
        p.stop();
        p.join(Duration::from_secs(6));
        assert_eq!(st.state, "live", "{st:?}");
        let level = |from: &str, len: &str| {
            let o = Command::new(&ffmpeg).args(["-hide_banner", "-ss", from, "-t", len, "-i"]).arg(&file).args(["-af", "volumedetect", "-f", "null", "-"]).output().unwrap();
            let t = String::from_utf8_lossy(&o.stderr).to_string();
            t.lines().find_map(|l| l.split("max_volume: ").nth(1)).and_then(|v| v.trim_end_matches(" dB").trim().parse::<f32>().ok()).unwrap_or(-999.0)
        };
        let (before, after) = (level("1", "2"), level("6", "1.5"));
        println!("max level before mute {before} dB, after {after} dB");
        assert!(before > -20.0, "the tones are heard: {before}");
        assert!(after < -60.0, "muted live: {after}");
    }

    /// Records ~4 s to a file with test sources and checks it plays, even cut off by a kill:
    /// `KAIRO_STREAM_E2E=1 cargo test -p kairo --lib record_e2e -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn record_e2e() {
        if std::env::var("KAIRO_STREAM_E2E").is_err() { return; }
        std::env::set_var("KAIRO_STREAM_TEST_DEVICES", "1");
        let ffmpeg = PathBuf::from(std::env::var("KAIRO_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
        let dir = std::env::temp_dir().join(format!("kairo-record-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Kairo test.mp4");
        std::fs::write(&file, b"someone else's").unwrap();
        let mut c = cfg();
        c.width = 640; c.height = 360; c.destinations.clear(); c.program = None;
        c.record_to = Some(file.to_string_lossy().to_string());
        let p = Pipeline::start(ffmpeg.clone(), c, Arc::new(|_| {}));
        p.push_frame(Arc::new(Frame { width: 640, height: 360, bgra: vec![90u8; 640 * 360 * 4] }), 0);
        thread::sleep(Duration::from_secs(5));
        let live = p.status();
        p.kill();
        p.join(Duration::from_secs(2));
        println!("status: {live:?}");
        assert_eq!(live.state, "live", "{live:?}");
        assert_eq!(std::fs::read(&file).unwrap(), b"someone else's", "an existing file is never written over");
        let written = dir.join("Kairo test part 2.mp4");
        assert_eq!(live.files, vec![written.to_string_lossy().to_string()]);
        let probe = Command::new(&ffmpeg).args(["-hide_banner", "-i"]).arg(&written).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr).to_string();
        println!("{}", info.lines().filter(|l| l.contains("Stream #") || l.contains("Duration")).collect::<Vec<_>>().join(" | "));
        assert!(info.contains("Video: h264") && info.contains("Audio: aac"), "{info}");
    }
}
