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
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::json;

use crate::Shared;

const RATE: u32 = 16_000;
const FRAME: usize = 1024;

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

/// The input to open: the one named (as the page showed it), else the system's default.
fn pick(host: &cpal::Host, want: &str) -> Option<cpal::Device> {
    let want = want.trim().trim_start_matches("Default - ").to_lowercase();
    if !want.is_empty() {
        if let Ok(devs) = host.input_devices() {
            let devs: Vec<cpal::Device> = devs.collect();
            if let Some(d) = devs.iter().find(|d| d.name().is_ok_and(|n| n.to_lowercase() == want)) {
                return Some(d.clone());
            }
            if let Some(d) = devs.iter().find(|d| d.name().is_ok_and(|n| { let n = n.to_lowercase(); n.contains(&want) || want.contains(&n) })) {
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
            let name = opened.name.clone();
            let _ = ready_tx.send(Ok(name.clone()));

            let mut lowpass = Biquad::lowpass(opened.rate as f32, 7000.0);
            let mut resample = Resampler::new(opened.rate, RATE);
            let mut level = LevelControl::new(is_digital(&name));
            let mut pending: Vec<f32> = Vec::with_capacity(FRAME * 2);
            let mut resampled = Vec::with_capacity(FRAME);
            let mut frames = 0u64;
            let mut last_block = std::time::Instant::now();
            let mut lost_since: Option<std::time::Instant> = None;
            while !stop_t.load(Ordering::Relaxed) {
                // The input went away (unplugged, or the system stopped it): say so, and open it
                // again as soon as it can be, every 2 s.
                let gone = opened.failed.load(Ordering::Relaxed) || last_block.elapsed() > Duration::from_secs(3);
                if gone {
                    if lost_since.is_none() {
                        lost_since = Some(std::time::Instant::now());
                        crate::routes::write_debug_global("server:native-capture-lost", json!({ "device": name }));
                        app.hub.broadcast(json!({ "type": "input-lost", "device": name }));
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    if let Ok(o) = open(&host, &name) {
                        lowpass = Biquad::lowpass(o.rate as f32, 7000.0);
                        resample = Resampler::new(o.rate, RATE);
                        opened = o;
                        last_block = std::time::Instant::now();
                        lost_since = None;
                        crate::routes::write_debug_global("server:native-capture-back", json!({ "device": name }));
                        app.hub.broadcast(json!({ "type": "input-back", "device": name }));
                    }
                    continue;
                }
                let Ok(mut block) = opened.rx.recv_timeout(Duration::from_millis(100)) else { continue };
                last_block = std::time::Instant::now();
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
                        app.hub.broadcast(json!({ "type": "input-level", "peak": peak, "clipped": clipped, "gain": level.gain, "device": name }));
                    }
                }
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
    rx: mpsc::Receiver<Vec<f32>>,
    failed: Arc<AtomicBool>,
    _stream: cpal::Stream,
}

fn open(host: &cpal::Host, want: &str) -> Result<Opened, String> {
    let Some(dev) = pick(host, want) else { return Err(format!("no input named {want}")) };
    let name = dev.name().unwrap_or_else(|_| "Input".into());
    let cfg = dev.default_input_config().map_err(|e| format!("{name}: {e}"))?;
    let channels = cfg.channels() as usize;
    let rate = cfg.sample_rate().0;
    // Unbounded: a block is never dropped here (owner: no audio or words may be lost).
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let failed = Arc::new(AtomicBool::new(false));
    let f2 = failed.clone();
    let err_fn = move |e| {
        eprintln!("[Audio] capture error: {e}");
        f2.store(true, Ordering::Relaxed);
    };
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
    Ok(Opened { name, rate, rx, failed, _stream: stream })
}

fn mono<T: Copy>(data: &[T], channels: usize, f: impl Fn(T) -> f32) -> Vec<f32> {
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
