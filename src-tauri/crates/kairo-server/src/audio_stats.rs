//! Where the audio is, every 3 s while listening, in debug.log: what reached
//! the server from the app, its level, anything dropped because the engine's
//! queue was full, and what the engine took in and gave back. With the app's
//! own line (client:audio-pipeline, app.js) a silent spell can be traced to
//! the step it started at — the input, the app, the connection, the queue or
//! the engine.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Duration;

use serde_json::json;

use crate::Shared;

#[derive(Default)]
pub struct AudioStats {
    /// 1024-sample frames that arrived from the app, and how many were dropped (queue full).
    pub frames_in: AtomicU64,
    pub dropped: AtomicU64,
    /// Loudest sample that arrived (int16 scale), and frames that were all zeros.
    pub peak_in: AtomicU64,
    pub zero_frames: AtomicU64,
    /// Frames the engine took in (offline model, or sent to Deepgram), and what came back.
    pub engine_frames: AtomicU64,
    pub engine_results: AtomicU64,
}

pub static STATS: std::sync::LazyLock<AudioStats> = std::sync::LazyLock::new(AudioStats::default);

/// A frame from the app: counted, its level noted.
pub fn note_frame(pcm: &[u8]) {
    let mut peak = 0u64;
    for s in pcm.chunks_exact(2) {
        let v = i16::from_le_bytes([s[0], s[1]]).unsigned_abs() as u64;
        if v > peak { peak = v; }
    }
    STATS.frames_in.fetch_add(1, Relaxed);
    STATS.peak_in.fetch_max(peak, Relaxed);
    if peak == 0 { STATS.zero_frames.fetch_add(1, Relaxed); }
}

/// Runs for the life of the server; writes a line only while listening.
pub fn spawn_reporter(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        loop {
            tick.tick().await;
            let take = |a: &AtomicU64| a.swap(0, Relaxed);
            let (frames, dropped, peak, zeros, engine, results) =
                (take(&STATS.frames_in), take(&STATS.dropped), take(&STATS.peak_in), take(&STATS.zero_frames), take(&STATS.engine_frames), take(&STATS.engine_results));
            let listening = app.audio.lock().await.is_some();
            if !listening && frames == 0 {
                continue;
            }
            let peak_db = if peak == 0 { None } else { Some(((peak as f64 / 32768.0).log10() * 20.0).round()) };
            crate::routes::write_debug_line(&app, "server:audio", json!({
                "framesIn": frames, "zeroFrames": zeros, "peakDb": peak_db, "dropped": dropped,
                "engineFrames": engine, "engineResults": results, "listening": listening,
            }));
        }
    });
}
