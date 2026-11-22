//! Listening without the web view: the sound input (a microphone, an audio
//! interface, BlackHole or another loopback) is captured here, through the
//! system's own audio layer (Core Audio, WASAPI, ALSA via cpal), and fed into
//! the same path as the page's 16 kHz frames (ws.rs forward_audio).
//!
//! Why: captured in the page, WebKit's audio could go to digital silence for
//! the rest of the session when Director mode closed its cameras — the player
//! still playing into BlackHole, Kairo hearing zeros, reopening the input not
//! helping (owner's dev build, 2026-10-17 17:47). Nothing the windows do with
//! cameras reaches this capture.
//!
//! What reaches the engines is the same as before: mono, 16 kHz, 16-bit, in
//! 1024-sample frames, level-controlled exactly as audio_level.js does it
//! (raised when quiet, a loopback further than a microphone, never clipped).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::json;

use crate::Shared;

const RATE: u32 = 16_000;
const FRAME: usize = 1024;

/// How long to wait before the next try at an input that dropped: at once the first time (a glitch on
/// Windows, a sample-rate change by another app, a jack touched, is back in a moment), then 0.25 s, 0.5 s,
/// 1 s, and every 2 s after that. It used to wait 2 s before even the first try, so any one-second hiccup
/// left a hole in the words at least that long (church PC, 15 Nov 2026: "audio input randomly goes off").
fn retry_after(failed_tries: u32) -> Duration {
    match failed_tries {
        0 => Duration::ZERO,
        1 => Duration::from_millis(250),
        2 => Duration::from_millis(500),
        3 => Duration::from_secs(1),
        _ => Duration::from_secs(2),
    }
}

/// The gain each input settled on, by name: a new session starts from it instead of 1.0, as the
/// page's capture does (app.js), so its first sentences aren't heard while the gain climbs back.
static SETTLED_GAIN: std::sync::Mutex<Option<std::collections::HashMap<String, f32>>> = std::sync::Mutex::new(None);

fn settled_gain(name: &str) -> f32 {
    crate::util::lock(&SETTLED_GAIN).as_ref().and_then(|m| m.get(name).copied()).unwrap_or(1.0)
}

fn remember_gain(name: &str, gain: f32) {
    crate::util::lock(&SETTLED_GAIN).get_or_insert_with(Default::default).insert(name.to_string(), gain);
}

/// The names of the sound inputs, as the system names them.
pub fn inputs() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices().map(|ds| ds.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

/// A loopback carries no hiss, so it may be raised further (audio_level.js).
fn is_digital(name: &str) -> bool {
    let n = name.to_lowercase();
    ["blackhole", "loopback", "soundflower", "vb-cable", "cable output", "voicemeeter", "virtual", "monitor of"].iter().any(|k| n.contains(k))
}

/// An input's name without what Windows adds when it finds a device again: "Microphone (2- USB
/// Audio)" is the "Microphone (USB Audio)" that dropped. Looked for by its old name, a USB adapter
/// that came back renamed was never found again, and every restart asked for the old one (church
/// PC, 22 Nov 2026: "total loss of audio input… took multiple restarts").
pub fn base_name(name: &str) -> String {
    static RENUMBERED: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\(\s*\d+\s*-\s*").expect("pattern"));
    let n = name.trim().trim_start_matches("Default - ").trim_start_matches("Communications - ").to_lowercase();
    // The page's names carry a USB id the system's don't: "Microphone (USB Audio) (0d8c:0014)".
    static USB_ID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\s*\([0-9a-f]{4}:[0-9a-f]{4}\)$").expect("pattern"));
    RENUMBERED.replace_all(&USB_ID.replace(&n, ""), "(").trim().to_string()
}

/// The input to open: the one named (as the page showed it, or as it was before Windows renamed
/// it), else the system's default.
fn pick(host: &cpal::Host, want: &str) -> Option<cpal::Device> {
    let want = base_name(want);
    if !want.is_empty() {
        if let Ok(devs) = host.input_devices() {
            let devs: Vec<cpal::Device> = devs.collect();
            if let Some(d) = devs.iter().find(|d| d.name().is_ok_and(|n| base_name(&n) == want)) {
                return Some(d.clone());
            }
            if let Some(d) = devs.iter().find(|d| d.name().is_ok_and(|n| { let n = base_name(&n); n.contains(&want) || want.contains(&n) })) {
                return Some(d.clone());
            }
        }
        return None;   // named but not found: the page's capture is used instead
    }
    host.default_input_device()
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub device: String,
}

impl Capture {
    pub fn stop(mut self) {
        self.halt();
    }
    fn halt(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Opens `want` (or the default input) and, with `feed`, feeds it to the listening engine until
/// stopped; without, it only reports its level (the Settings meter while not listening).
pub fn start(app: Shared, want: &str, feed: bool) -> Result<Capture, String> {
    let rt = tokio::runtime::Handle::current();
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
    let want = want.to_string();
    let stop_t = stop.clone();
    let thread = std::thread::Builder::new()
        .name("kairo-capture".into())
        .spawn(move || {
            let host = cpal::default_host();
            let mut opened = match open(&host, &want) {
                Ok(o) => o,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let mut name = opened.name.clone();
            crate::routes::write_debug_global("server:native-capture-opened", json!({ "device": name, "rate": opened.rate, "channels": opened.channels, "format": opened.format }));
            let _ = ready_tx.send(Ok(name.clone()));

            let mut lowpass = Biquad::lowpass(opened.rate as f32, 7000.0);
            let mut resample = Resampler::new(opened.rate, RATE);
            let mut level = LevelControl::new(is_digital(&name));
            level.gain = settled_gain(&name).clamp(1.0, level.max_gain);
            let mut pending: Vec<f32> = Vec::with_capacity(FRAME * 2);
            let mut resampled = Vec::with_capacity(FRAME);
            let mut frames = 0u64;
            let mut last_block = std::time::Instant::now();
            let mut lost_since: Option<std::time::Instant> = None;
            // Tries at the input that failed since it dropped, and whether the page was told.
            let mut failed_tries = 0u32;
            let mut told = false;
            while !stop_t.load(Ordering::Relaxed) {
                // The input went away (unplugged, or the system stopped it): open it again at once, then
                // at growing gaps; the page is told only if the first try doesn't bring it back, so a
                // glitch that mends in a moment doesn't flash "No audio!".
                let silent_for = last_block.elapsed();
                let errored = opened.failed.load(Ordering::Relaxed);
                let gone = errored || silent_for > Duration::from_secs(3);
                if gone {
                    if lost_since.is_none() {
                        lost_since = Some(std::time::Instant::now());
                        failed_tries = 0;
                        told = false;
                        // Why, in the log: the system's own words for an error, else how long nothing came.
                        let why = if errored { crate::util::lock(&opened.why).clone() } else { format!("no sound arrived for {:.1} s", silent_for.as_secs_f32()) };
                        crate::routes::write_debug_global("server:native-capture-lost", json!({ "device": name, "why": why, "rate": opened.rate, "channels": opened.channels, "format": opened.format }));
                    }
                    let wait = retry_after(failed_tries);
                    let until = std::time::Instant::now() + wait;
                    while std::time::Instant::now() < until && !stop_t.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    match open(&host, &name) {
                        Ok(o) => {
                            // It may have come back under a new name ("2- USB Audio"): the meters and log use it.
                            if o.name != name {
                                crate::routes::write_debug_global("server:native-capture-renamed", json!({ "from": name, "to": o.name }));
                                name = o.name.clone();
                            }
                            lowpass = Biquad::lowpass(o.rate as f32, 7000.0);
                            resample = Resampler::new(o.rate, RATE);
                            let (rate, channels, format) = (o.rate, o.channels, o.format.clone());
                            opened = o;
                            last_block = std::time::Instant::now();
                            let away_ms = lost_since.take().map_or(0, |t| t.elapsed().as_millis() as u64);
                            crate::routes::write_debug_global("server:native-capture-back", json!({ "device": name, "awayMs": away_ms, "tries": failed_tries + 1, "rate": rate, "channels": channels, "format": format }));
                            if told {
                                app.hub.broadcast(json!({ "type": "input-back", "device": name }));
                            }
                        }
                        Err(e) => {
                            failed_tries += 1;
                            // The first few failures and then now and again, so a long outage isn't a long log.
                            if failed_tries <= 3 || failed_tries % 15 == 0 {
                                // What the system has now, so the log shows a device that came back renamed.
                                crate::routes::write_debug_global("server:native-capture-retry", json!({ "device": name, "tries": failed_tries, "error": e, "inputs": inputs() }));
                            }
                            if !told {
                                told = true;
                                app.hub.broadcast(json!({ "type": "input-lost", "device": name }));
                            }
                        }
                    }
                    continue;
                }
                let Ok(mut block) = opened.rx.recv_timeout(Duration::from_millis(100)) else { continue };
                if opened.rate != RATE {
                    block.iter_mut().for_each(|s| *s = lowpass.run(*s));
                }
                resampled.clear();
                resample.push(&block, &mut resampled);
                pending.extend_from_slice(&resampled);
                while pending.len() >= FRAME {
                    let mut frame: Vec<f32> = pending.drain(..FRAME).collect();
                    let (peak, clipped) = level.apply(&mut frame);
                    let bytes: Vec<u8> = frame.iter().flat_map(|s| ((s * 32767.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()).collect();
                    // Waits for the engine rather than drop a frame.
                    if feed {
                        rt.block_on(crate::ws::forward_audio_lossless(&app, bytes::Bytes::from(bytes)));
                    }
                    frames += 1;
                    // The page's meters and silence watch, about six times a second.
                    if frames % 3 == 0 {
                        remember_gain(&name, level.gain);
                        app.hub.broadcast(json!({ "type": "input-level", "peak": peak, "clipped": clipped, "gain": level.gain, "device": name }));
                    }
                }
                // "Gone" counts only time spent waiting for the input. Time spent waiting for a busy
                // engine above is not the input going quiet: counted, a stall of 3 s reopened the
                // input and threw away the audio queued meanwhile (review, 2026-10-18).
                last_block = std::time::Instant::now();
            }
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(device)) => Ok(Capture { stop, thread: Some(thread), device }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            Err("the input didn't start".into())
        }
    }
}

/// An input being captured: its blocks arrive on `rx`, mono, at `rate`.
struct Opened {
    name: String,
    rate: u32,
    channels: usize,
    /// The sample format the system gave ("F32", "I16"…), for the log.
    format: String,
    rx: mpsc::Receiver<Vec<f32>>,
    failed: Arc<AtomicBool>,
    /// What the system said when the stream failed.
    why: Arc<Mutex<String>>,
    _stream: Keep,
}

/// What keeps an input capturing while it's open.
enum Keep {
    Cpal(#[allow(dead_code)] cpal::Stream),
    #[cfg(windows)]
    Raw(#[allow(dead_code)] crate::wasapi_raw::RawCapture),
}

fn open(host: &cpal::Host, want: &str) -> Result<Opened, String> {
    // On Windows, raw first: none of Windows' processing (Voice Clarity, Voice Focus, enhancements)
    // between the device and Kairo (wasapi_raw.rs). Where that can't open the input, cpal does, as before.
    #[cfg(windows)]
    {
        match open_raw(want) {
            Ok(o) => return Ok(o),
            Err(e) => crate::routes::write_debug_global("server:native-capture-raw-failed", json!({ "device": want, "error": e })),
        }
    }
    open_cpal(host, want)
}

#[cfg(windows)]
fn open_raw(want: &str) -> Result<Opened, String> {
    let base = base_name(want);
    // The same choice as pick(): the named input (as it was before Windows renamed it), else the default.
    let matches = move |n: &str| {
        let n = base_name(n);
        n == base || n.contains(&base) || base.contains(&n)
    };
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let failed = Arc::new(AtomicBool::new(false));
    let why = Arc::new(Mutex::new(String::new()));
    let o = crate::wasapi_raw::open(matches, want.trim().is_empty(), tx, failed.clone(), why.clone())?;
    // "F32 raw": Windows' processing is off; "F32 processed": the device refused that.
    let format = format!("F32 {}", if o.raw { "raw" } else { "processed" });
    Ok(Opened { name: o.name, rate: o.rate, channels: o.channels, format, rx, failed, why, _stream: Keep::Raw(o.capture) })
}

fn open_cpal(host: &cpal::Host, want: &str) -> Result<Opened, String> {
    let Some(dev) = pick(host, want) else { return Err(format!("no input named {want}")) };
    let name = dev.name().unwrap_or_else(|_| "Input".into());
    let cfg = dev.default_input_config().map_err(|e| format!("{name}: {e}"))?;
    let channels = cfg.channels() as usize;
    let rate = cfg.sample_rate().0;
    // Unbounded: a block is never dropped here (owner: no audio or words may be lost).
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let failed = Arc::new(AtomicBool::new(false));
    let f2 = failed.clone();
    let why = Arc::new(Mutex::new(String::new()));
    let w2 = why.clone();
    let err_fn = move |e: cpal::StreamError| {
        eprintln!("[Audio] capture error: {e}");
        *crate::util::lock(&w2) = e.to_string();
        f2.store(true, Ordering::Relaxed);
    };
    let format = format!("{:?}", cfg.sample_format());
    let sc: cpal::StreamConfig = cfg.clone().into();
    // Each block becomes mono f32 and is queued; sending never blocks the audio thread.
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_input_stream(&sc, move |d: &[f32], _: &_| { let _ = tx.send(mono(d, channels, |s| s)); }, err_fn, None),
        cpal::SampleFormat::I16 => dev.build_input_stream(&sc, move |d: &[i16], _: &_| { let _ = tx.send(mono(d, channels, |s| s as f32 / 32768.0)); }, err_fn, None),
        cpal::SampleFormat::I32 => dev.build_input_stream(&sc, move |d: &[i32], _: &_| { let _ = tx.send(mono(d, channels, |s| s as f32 / 2_147_483_648.0)); }, err_fn, None),
        cpal::SampleFormat::U16 => dev.build_input_stream(&sc, move |d: &[u16], _: &_| { let _ = tx.send(mono(d, channels, |s| (s as f32 - 32768.0) / 32768.0)); }, err_fn, None),
        other => return Err(format!("{name}: unsupported sample format {other:?}")),
    }
    .map_err(|e| format!("{name}: {e}"))?;
    stream.play().map_err(|e| format!("{name}: {e}"))?;
    Ok(Opened { name, rate, channels, format, rx, failed, why, _stream: Keep::Cpal(stream) })
}

pub(crate) fn mono<T: Copy>(data: &[T], channels: usize, f: impl Fn(T) -> f32) -> Vec<f32> {
    if channels <= 1 {
        return data.iter().map(|&s| f(s)).collect();
    }
    data.chunks(channels).map(|c| c.iter().map(|&s| f(s)).sum::<f32>() / c.len() as f32).collect()
}

/// Linear resampling, its position carried from block to block.
struct Resampler {
    step: f64,
    pos: f64,
    buf: Vec<f32>,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        Resampler { step: from as f64 / to as f64, pos: 0.0, buf: Vec::new() }
    }
    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        while self.pos + 1.0 < self.buf.len() as f64 {
            let i = self.pos.floor() as usize;
            let f = (self.pos - i as f64) as f32;
            out.push(self.buf[i] * (1.0 - f) + self.buf[i + 1] * f);
            self.pos += self.step;
        }
        let used = (self.pos.floor() as usize).min(self.buf.len());
        self.buf.drain(..used);
        self.pos -= used as f64;
    }
}

/// A low-pass ahead of the resampler, so nothing above 8 kHz folds into speech.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn lowpass(rate: f32, cutoff: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * cutoff.min(rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Biquad { b0: (1.0 - c) / 2.0 / a0, b1: (1.0 - c) / a0, b2: (1.0 - c) / 2.0 / a0, a1: -2.0 * c / a0, a2: (1.0 - alpha) / a0, z1: 0.0, z2: 0.0 }
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// audio_level.js's LevelControl, the same rules and numbers (see its notes for
/// the measurements behind them), at 16 kHz.
struct LevelControl {
    segs: Vec<f32>,
    seg_idx: usize,
    seg_fill: usize,
    seg_max: f32,
    seg_count: usize,
    quiet: f32,
    max_gain: f32,
    gate: f32,
    gain: f32,
}

const SEG_LEN: usize = (RATE as usize) / 10;   // 100 ms
const TARGET: f32 = 0.3;
const CEILING: f32 = 0.9;
const GATE_DIGITAL: f32 = 0.0003;
const SPEECH_SWING: f32 = 4.0;
const RISE_DB_S: f32 = 6.0;
const CATCH_UP_DB_S: f32 = 24.0;
const FALL_DB_S: f32 = 40.0;
fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl LevelControl {
    fn new(digital: bool) -> Self {
        LevelControl {
            segs: vec![0.0; 50],
            seg_idx: 0,
            seg_fill: 0,
            seg_max: 0.0,
            seg_count: 0,
            quiet: 0.0,
            max_gain: if digital { 1000.0 } else { 31.6 },
            gate: if digital { GATE_DIGITAL } else { 0.005 },
            gain: 1.0,
        }
    }
    fn window_peak(&self) -> f32 {
        self.segs.iter().copied().fold(self.seg_max, f32::max)
    }
    fn note(&mut self, peak: f32, n: usize) {
        self.seg_max = self.seg_max.max(peak);
        self.seg_fill += n;
        if self.seg_fill >= SEG_LEN {
            self.segs[self.seg_idx] = self.seg_max;
            self.seg_idx = (self.seg_idx + 1) % self.segs.len();
            self.seg_max = 0.0;
            self.seg_fill = 0;
            if self.seg_count < self.segs.len() {
                self.seg_count += 1;
            }
            let mut part: Vec<f32> = self.segs[..self.seg_count].to_vec();
            part.sort_by(f32::total_cmp);
            self.quiet = part[self.seg_count / 5];
        }
    }
    fn open(&self, w: f32) -> bool {
        w >= self.gate || (w >= GATE_DIGITAL && self.seg_count >= 20 && w >= SPEECH_SWING * self.quiet)
    }
    /// Level-controls `block` in place; returns its raw peak and whether it reached full scale.
    fn apply(&mut self, block: &mut [f32]) -> (f32, bool) {
        let n = block.len();
        let peak = block.iter().fold(0f32, |m, s| m.max(s.abs()));
        self.note(peak, n);
        let w = self.window_peak();
        let want = if !self.open(w) { self.gain } else { (TARGET / w).max(1.0).min(self.max_gain).min(CEILING / w) };
        let dt = n as f32 / RATE as f32;
        let from = self.gain;
        let rise = if want > from * 4.0 { CATCH_UP_DB_S } else { RISE_DB_S };
        let mut to = if want > from { want.min(from * db_to_gain(rise * dt)) } else { want.max(from / db_to_gain(FALL_DB_S * dt)) };
        if peak * to > CEILING {
            to = CEILING / peak;
        }
        let start = if peak * from > CEILING { to } else { from };
        let step = (to - start) / n as f32;
        for (i, s) in block.iter_mut().enumerate() {
            *s = (*s * (start + step * (i as f32 + 1.0))).clamp(-1.0, 1.0);
        }
        self.gain = to;
        (peak, peak >= 0.999)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_windows_renamed_is_the_same_input() {
        assert_eq!(base_name("Microphone (2- USB Audio)"), base_name("Microphone (USB Audio)"));
        assert_eq!(base_name("Default - Microphone (USB Audio)"), "microphone (usb audio)");
        assert_eq!(base_name("Microphone (2- USB Audio) (0d8c:0014)"), "microphone (usb audio)");
        assert_eq!(base_name("Line In (12- Behringer UMC202HD)"), "line in (behringer umc202hd)");
        assert_ne!(base_name("Microphone (USB Audio)"), base_name("Microphone (Realtek Audio)"));
    }

    #[test]
    fn a_dropped_input_is_tried_again_at_once_then_less_often() {
        assert_eq!(retry_after(0), Duration::ZERO, "the first try is immediate");
        assert!(retry_after(1) < retry_after(3));
        assert_eq!(retry_after(4), Duration::from_secs(2));
        assert_eq!(retry_after(400), Duration::from_secs(2), "never longer than 2 s");
    }

    #[test]
    fn resampling_48k_to_16k_keeps_a_third_of_the_samples() {
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            r.push(&vec![0.5; 4800], &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "{}", out.len());
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn a_quiet_loopback_is_raised_and_nothing_clips() {
        let mut l = LevelControl::new(true);
        let mut last = 0.0;
        for k in 0..200 {
            // A 300 Hz tone 40 dB down, in 1024-sample frames.
            let mut f: Vec<f32> = (0..FRAME).map(|i| 0.01 * ((k * FRAME + i) as f32 * 2.0 * std::f32::consts::PI * 300.0 / RATE as f32).sin()).collect();
            l.apply(&mut f);
            last = f.iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!(last <= CEILING + 1e-6);
        }
        assert!(last > 0.2, "raised to {last}");
    }

    #[test]
    fn digital_silence_is_not_raised() {
        let mut l = LevelControl::new(true);
        for _ in 0..100 {
            let mut f = vec![0.0; FRAME];
            l.apply(&mut f);
        }
        assert_eq!(l.gain, 1.0);
    }
}
