//! A moving background under an NDI / Syphon / DeckLink output, at full
//! frame rate. Those outputs get the output's page as snapshots (capture.rs),
//! taken when it changes — fine for words, but a playing video would only
//! ever update as fast as a snapshot can be taken (~64 ms at 1080p).
//!
//! So when the page's background is a full-frame video, the page stops
//! drawing it (display.html, capture mode) and its snapshots carry only what
//! sits on top, with transparency. Here the bundled ffmpeg plays the video
//! file at the output's size and rate, and every frame the latest snapshot is
//! laid over it — blending old into new for a Fade — and handed to the
//! output's senders.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::capture::{blend, Frame};

/// What's laid over the video: the latest snapshot, and the one before it
/// while a Fade runs.
#[derive(Default)]
struct Overlay {
    to: Option<Arc<Frame>>,
    from: Option<Arc<Frame>>,
    started: Option<Instant>,
    fade: Duration,
}

pub struct VideoBg {
    /// The video file playing, to tell a new request for the same one apart.
    pub file: PathBuf,
    pub fit: String,
    stop: Arc<AtomicBool>,
    overlay: Arc<Mutex<Overlay>>,
    child: Arc<Mutex<Option<Child>>>,
}

impl VideoBg {
    /// Plays `file` (looping) at `width`×`height`, `fps` frames a second,
    /// fitted as the page fitted it (`cover`, `contain` or `fill`), and calls
    /// `push` with each finished frame.
    pub fn start(ffmpeg: &Path, file: PathBuf, fit: &str, width: u32, height: u32, fps: u32, push: impl Fn(Arc<Frame>) + Send + 'static) -> Result<VideoBg, String> {
        let (w, h) = (width as usize, height as usize);
        let scale = match fit {
            "fill" => format!("scale={width}:{height}"),
            "cover" => format!("scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}"),
            _ => format!("scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black"),
        };
        let mut child = Command::new(ffmpeg)
            .args(["-nostdin", "-loglevel", "error", "-re", "-stream_loop", "-1", "-i"])
            .arg(&file)
            .args(["-an", "-vf", &format!("{scale},format=bgra"), "-r", &fps.to_string(), "-f", "rawvideo", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("couldn't start ffmpeg: {e}"))?;
        let mut out = child.stdout.take().ok_or("ffmpeg gave no output")?;
        let stop = Arc::new(AtomicBool::new(false));
        let overlay = Arc::new(Mutex::new(Overlay::default()));
        let latest: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

        // Decoder: whole frames from ffmpeg into `latest`. The first one says
        // ffmpeg can do this at all (an older bundled ffmpeg can't).
        let (first_tx, first_rx) = std::sync::mpsc::sync_channel::<bool>(1);
        {
            let (stop, latest) = (stop.clone(), latest.clone());
            thread::spawn(move || {
                let mut buf = vec![0u8; w * h * 4];
                let mut first = Some(first_tx);
                while !stop.load(Ordering::Relaxed) {
                    if out.read_exact(&mut buf).is_err() {
                        if let Some(tx) = first.take() { let _ = tx.send(false); }
                        break;
                    }
                    if let Some(tx) = first.take() { let _ = tx.send(true); }
                    if let Ok(mut l) = latest.lock() {
                        match l.as_mut() {
                            Some(v) => v.copy_from_slice(&buf),
                            None => *l = Some(buf.clone()),
                        }
                    }
                }
            });
        }

        if first_rx.recv_timeout(Duration::from_secs(3)) != Ok(true) {
            stop.store(true, Ordering::Relaxed);
            let _ = child.kill();
            let _ = child.wait();
            return Err("this copy of Kairo's ffmpeg can't play video backgrounds".into());
        }

        // Mixer: every frame, the video with the overlay on top.
        {
            let (stop, overlay, latest) = (stop.clone(), overlay.clone(), latest.clone());
            let period = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
            thread::spawn(move || {
                let mut next = Instant::now();
                let mut mixed_over = vec![0u8; w * h * 4];
                // Diagnostics: KAIRO_VIDEO_DUMP=<dir> saves one mixed frame a second as a PNG.
                let dump = std::env::var_os("KAIRO_VIDEO_DUMP").map(PathBuf::from);
                let (mut frames, mut last_dump) = (0u64, Instant::now());
                while !stop.load(Ordering::Relaxed) {
                    let video = latest.lock().ok().and_then(|l| l.clone());
                    if let Some(mut px) = video {
                        let ov = overlay.lock().ok().map(|o| (o.to.clone(), o.from.clone(), o.started, o.fade));
                        if let Some((Some(to), from, started, fade)) = ov {
                            let t = started.map_or(1.0, |s| s.elapsed().as_secs_f32() / fade.as_secs_f32().max(0.001));
                            match from {
                                Some(f) if t < 1.0 && f.bgra.len() == to.bgra.len() => {
                                    blend(&mut mixed_over, &f.bgra, &to.bgra, t);
                                    over(&mut px, &mixed_over);
                                }
                                _ => over(&mut px, &to.bgra),
                            }
                        }
                        frames += 1;
                        if let Some(dir) = dump.as_ref().filter(|_| last_dump.elapsed() >= Duration::from_secs(1)) {
                            last_dump = Instant::now();
                            let rgba: Vec<u8> = px.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
                            if let Some(img) = image::RgbaImage::from_raw(width, height, rgba) {
                                let _ = img.save(dir.join(format!("mix-{frames:06}.png")));
                            }
                        }
                        push(Arc::new(Frame { width, height, bgra: px }));
                    }
                    next += period;
                    let now = Instant::now();
                    if next > now { thread::sleep(next - now) } else { next = now }
                }
            });
        }

        Ok(VideoBg { file, fit: fit.to_string(), stop, overlay, child: Arc::new(Mutex::new(Some(child))) })
    }

    /// A new snapshot of what sits on top (blended in over `fade_ms`).
    pub fn set_overlay(&self, frame: Arc<Frame>, fade_ms: u32) {
        if let Ok(mut o) = self.overlay.lock() {
            let previous = o.to.take();
            o.from = if fade_ms > 0 { previous } else { None };
            o.to = Some(frame);
            o.started = (fade_ms > 0).then(Instant::now);
            o.fade = Duration::from_millis(fade_ms as u64);
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut child) = c.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

impl Drop for VideoBg {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `dst` (opaque video) with premultiplied `top` laid over it.
fn over(dst: &mut [u8], top: &[u8]) {
    for (d, t) in dst.chunks_exact_mut(4).zip(top.chunks_exact(4)) {
        let a = t[3] as u32;
        if a == 0 {
            continue;
        }
        if a == 255 {
            d.copy_from_slice(t);
            continue;
        }
        let k = 255 - a;
        for c in 0..3 {
            d[c] = (t[c] as u32 + (d[c] as u32 * k + 127) / 255).min(255) as u8;
        }
        d[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn overlay_composites_premultiplied_over_opaque() {
        // video: opaque blue; overlay: transparent, opaque white, half-white
        let mut dst = vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        let top = vec![0, 0, 0, 0, 255, 255, 255, 255, 128, 128, 128, 128];
        super::over(&mut dst, &top);
        assert_eq!(&dst[0..4], &[255, 0, 0, 255]);
        assert_eq!(&dst[4..8], &[255, 255, 255, 255]);
        assert_eq!(&dst[8..12], &[255, 128, 128, 255]);
    }
}
