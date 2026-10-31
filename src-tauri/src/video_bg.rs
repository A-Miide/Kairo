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
//!
//! On a computer with a usable GPU (docs/DIRECTOR_COMPOSITOR.md; `KAIRO_GPU_COMPOSITOR=0` turns
//! it off) the laying over happens on the GPU instead: ffmpeg hands over NV12 rather than converting to BGRA, the
//! snapshot is uploaded only when it changes, and the fade and the composite are drawn by
//! kairo-compositor. Without a usable GPU the CPU path below runs as before.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::capture::{blend, Frame};
use kairo_compositor::{Canvas, Gpu, Layer, Rect, Source};

/// The GPU compositor, when it's switched on and there's a GPU to run it.
fn gpu() -> Option<Arc<Gpu>> {
    static GPU: std::sync::OnceLock<Option<Arc<Gpu>>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        // On unless KAIRO_GPU_COMPOSITOR=0 (the owner's call, 2026-10-31: the app is young enough
        // to make the change now). No usable GPU falls back to the CPU path.
        if std::env::var("KAIRO_GPU_COMPOSITOR").ok().as_deref() == Some("0") {
            return None;
        }
        match Gpu::new() {
            Ok(g) => {
                eprintln!("[Video] GPU compositor on {}", g.adapter);
                Some(g)
            }
            Err(e) => {
                eprintln!("[Video] GPU compositor unavailable ({e}); compositing on the CPU");
                None
            }
        }
    })
    .clone()
}

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
    /// A camera's frames, held while this output shows it (camera.rs).
    _camera: Option<crate::camera::Held>,
}

impl VideoBg {
    /// Plays `file` (looping) at `width`×`height`, `fps` frames a second,
    /// fitted as the page fitted it (`cover`, `contain` or `fill`), and calls
    /// `push` with each finished frame.
    pub fn start(ffmpeg: &Path, file: PathBuf, fit: &str, width: u32, height: u32, fps: u32, push: impl Fn(Arc<Frame>) + Send + 'static) -> Result<VideoBg, String> {
        let (w, h) = (width as usize, height as usize);
        // NV12 needs even sides; every output size is.
        let gpu = gpu().filter(|_| width % 2 == 0 && height % 2 == 0);
        let frame_len = if gpu.is_some() { w * h * 3 / 2 } else { w * h * 4 };
        let pix = if gpu.is_some() { "nv12" } else { "bgra" };
        let scale = match fit {
            "fill" => format!("scale={width}:{height}"),
            "cover" => format!("scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}"),
            _ => format!("scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black"),
        };
        let mut child = Command::new(ffmpeg)
            // Decoded on the computer's video engine where there is one (VideoToolbox on a Mac,
            // D3D11 on Windows), on the CPU otherwise.
            .args(["-nostdin", "-loglevel", "error", "-hwaccel", "auto", "-re", "-stream_loop", "-1", "-i"])
            .arg(&file)
            .args(["-an", "-vf", &format!("{scale},format={pix}"), "-r", &fps.to_string(), "-f", "rawvideo", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("couldn't start ffmpeg: {e}"))?;
        let mut out = child.stdout.take().ok_or("ffmpeg gave no output")?;
        let stop = Arc::new(AtomicBool::new(false));
        let overlay = Arc::new(Mutex::new(Overlay::default()));
        // The newest decoded frame, and whether the mixer has used it yet.
        let latest: Arc<Mutex<(Vec<u8>, bool)>> = Arc::new(Mutex::new((vec![0u8; frame_len], false)));

        // Decoder: whole frames from ffmpeg into `latest`. The first one says
        // ffmpeg can do this at all (an older bundled ffmpeg can't).
        let (first_tx, first_rx) = std::sync::mpsc::sync_channel::<bool>(1);
        {
            let (stop, latest) = (stop.clone(), latest.clone());
            thread::spawn(move || {
                let mut buf = vec![0u8; frame_len];
                let mut first = Some(first_tx);
                while !stop.load(Ordering::Relaxed) {
                    if out.read_exact(&mut buf).is_err() {
                        if let Some(tx) = first.take() { let _ = tx.send(false); }
                        break;
                    }
                    if let Some(tx) = first.take() { let _ = tx.send(true); }
                    // Handed over by swapping buffers, not copying 8 MB a frame.
                    if let Ok(mut l) = latest.lock() {
                        std::mem::swap(&mut l.0, &mut buf);
                        l.1 = true;
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

        if let Some(gpu) = gpu {
            let (s, o, l) = (stop.clone(), overlay.clone(), latest.clone());
            // ffmpeg already fitted the video to the canvas.
            let feed = move |gpu: &Gpu, video: &mut Option<Source>| -> bool {
                let Ok(mut l) = l.lock() else { return false };
                if !l.1 {
                    return false;
                }
                l.1 = false;
                let v = video.get_or_insert_with(|| Source::nv12(gpu, width, height));
                v.upload_nv12(gpu, &l.0).is_ok()
            };
            thread::spawn(move || mix_on_gpu(gpu, s, o, "fill".into(), feed, width, height, fps, push));
            return Ok(VideoBg { file, fit: fit.to_string(), stop, overlay, child: Arc::new(Mutex::new(Some(child))), _camera: None });
        }

        // Mixer: every frame, the video with the overlay on top.
        {
            let (stop, overlay, latest) = (stop.clone(), overlay.clone(), latest.clone());
            let period = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
            thread::spawn(move || {
                let mut next = Instant::now();
                let mut mixed_over = vec![0u8; w * h * 4];
                // Finished frames are written into ones no sender holds any more, so a frame costs
                // no 8 MB allocation (and the page faults of fresh memory) once the pool is warm.
                let mut pool: Vec<Arc<Frame>> = Vec::new();
                let mut have_video = false;
                // Diagnostics: KAIRO_VIDEO_DUMP=<dir> saves one mixed frame a second as a PNG.
                let dump = std::env::var_os("KAIRO_VIDEO_DUMP").map(PathBuf::from);
                let (mut frames, mut last_dump) = (0u64, Instant::now());
                while !stop.load(Ordering::Relaxed) {
                    // A frame no sender still holds; a new one only while every pooled one is held.
                    let slot = pool.iter_mut().position(|f| Arc::get_mut(f).is_some()).unwrap_or_else(|| {
                        pool.push(Arc::new(Frame { width, height, bgra: vec![0u8; w * h * 4] }));
                        pool.len() - 1
                    });
                    let px = &mut Arc::get_mut(&mut pool[slot]).expect("unshared pooled frame").bgra;
                    if let Ok(mut l) = latest.lock() {
                        have_video |= l.1;
                        if have_video {
                            px.copy_from_slice(&l.0);
                            l.1 = false;
                        }
                    }
                    if have_video {
                        let ov = overlay.lock().ok().map(|o| (o.to.clone(), o.from.clone(), o.started, o.fade));
                        if let Some((Some(to), from, started, fade)) = ov {
                            let t = started.map_or(1.0, |s| s.elapsed().as_secs_f32() / fade.as_secs_f32().max(0.001));
                            match from {
                                Some(f) if t < 1.0 && f.bgra.len() == to.bgra.len() => {
                                    blend(&mut mixed_over, &f.bgra, &to.bgra, t);
                                    over(px, &mixed_over);
                                }
                                _ => over(px, &to.bgra),
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
                        push(pool[slot].clone());
                    }
                    // Past the pool's size, a frame goes once sent: its senders free it.
                    if pool.len() > POOL {
                        pool.swap_remove(slot);
                    }
                    next += period;
                    let now = Instant::now();
                    if next > now { thread::sleep(next - now) } else { next = now }
                }
            });
        }

        Ok(VideoBg { file, fit: fit.to_string(), stop, overlay, child: Arc::new(Mutex::new(Some(child))), _camera: None })
    }

    /// Shows the camera `label` (the shared capture, camera.rs) under the output's page, fitted as
    /// the page fitted it, at `fps`. GPU compositor only: without it the page keeps drawing the
    /// camera itself, as before.
    pub fn start_camera(hub: &Arc<crate::camera::Hub>, ffmpeg: &Path, label: &str, fit: &str, width: u32, height: u32, fps: u32, push: impl Fn(Arc<Frame>) + Send + 'static) -> Result<VideoBg, String> {
        let gpu = gpu().ok_or("the GPU compositor is off")?;
        let held = hub.hold(ffmpeg, label)?;
        let latest = held.latest.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let overlay = Arc::new(Mutex::new(Overlay::default()));
        let mut seen = 0u64;
        let feed = move |gpu: &Gpu, video: &mut Option<Source>| -> bool {
            let Ok(l) = latest.lock() else { return false };
            if l.seq == seen {
                return false;
            }
            seen = l.seq;
            if video.as_ref().is_none_or(|v| (v.width, v.height) != (l.width, l.height)) {
                *video = Some(Source::nv12(gpu, l.width, l.height));
            }
            video.as_ref().is_some_and(|v| v.upload_nv12(gpu, &l.nv12).is_ok())
        };
        let (s, o) = (stop.clone(), overlay.clone());
        let fit_mode = fit.to_string();
        thread::spawn(move || mix_on_gpu(gpu, s, o, fit_mode, feed, width, height, fps, push));
        Ok(VideoBg { file: PathBuf::from(format!("camera:{label}")), fit: fit.to_string(), stop, overlay, child: Arc::new(Mutex::new(None)), _camera: Some(held) })
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

/// Frames kept for reuse: one being written, and the ones senders still hold.
const POOL: usize = 4;

/// The mixer on the GPU: the NV12 video, the latest snapshot over it (faded from the one before),
/// read back into pooled frames for the senders, one frame behind.
#[allow(clippy::too_many_arguments)]
fn mix_on_gpu(
    gpu: Arc<Gpu>,
    stop: Arc<AtomicBool>,
    overlay: Arc<Mutex<Overlay>>,
    fit: String,
    // Uploads the newest video frame into the source (making it, or remaking it at a new size);
    // true when it did.
    mut feed: impl FnMut(&Gpu, &mut Option<Source>) -> bool,
    width: u32,
    height: u32,
    fps: u32,
    push: impl Fn(Arc<Frame>),
) {
    let period = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
    let mut canvas = Canvas::new(&gpu, width, height);
    let mut video: Option<Source> = None;
    // Two page pictures: the one on air, and the one it fades from.
    let pages = [Source::bgra(&gpu, width, height), Source::bgra(&gpu, width, height)];
    let mut page_to: Option<usize> = None;
    let mut uploaded: Option<Arc<Frame>> = None;
    let mut have_video = false;
    let mut pool: Vec<Arc<Frame>> = Vec::new();
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        have_video |= feed(&gpu, &mut video);
        let ov = overlay.lock().ok().map(|o| (o.to.clone(), o.from.is_some(), o.started, o.fade));
        let mut fade_t = None;
        if let Some((Some(to), fading, started, fade)) = ov {
            if to.width == width && to.height == height && !uploaded.as_ref().is_some_and(|u| Arc::ptr_eq(u, &to)) {
                // The new picture goes into the other slot; the one on air stays to fade from.
                let slot = page_to.map_or(0, |s| 1 - s);
                if pages[slot].upload_bgra(&gpu, &to.bgra).is_ok() {
                    page_to = Some(slot);
                    uploaded = Some(to);
                }
            }
            if fading {
                let t = started.map_or(1.0, |s| s.elapsed().as_secs_f32() / fade.as_secs_f32().max(0.001));
                if t < 1.0 {
                    fade_t = Some(t);
                }
            }
        }
        if let (true, Some(video)) = (have_video, video.as_ref()) {
            let full = Rect::full(width, height);
            let (rect, crop) = kairo_compositor::fit(&fit, video.width, video.height, full);
            let mut layers = vec![Layer { source: video, rect, crop: Some(crop), opacity: 1.0, fade_from: None }];
            if let Some(s) = page_to {
                layers.push(Layer { source: &pages[s], rect: full, crop: None, opacity: 1.0, fade_from: fade_t.map(|t| (&pages[1 - s], t)) });
            }
            canvas.render(&gpu, [0.0, 0.0, 0.0, 1.0], &layers);
            let slot = pool.iter_mut().position(|f| Arc::get_mut(f).is_some()).unwrap_or_else(|| {
                pool.push(Arc::new(Frame { width, height, bgra: Vec::new() }));
                pool.len() - 1
            });
            let frame = Arc::get_mut(&mut pool[slot]).expect("unshared pooled frame");
            if canvas.read_bgra(&gpu, &mut frame.bgra) {
                push(pool[slot].clone());
            }
            if pool.len() > POOL {
                pool.swap_remove(slot);
            }
        }
        next += period;
        let now = Instant::now();
        if next > now { thread::sleep(next - now) } else { next = now }
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
    /// Plays KAIRO_TEST_VIDEO through ffmpeg (KAIRO_TEST_FFMPEG) for 10 s with a page fading in over
    /// it, and prints the frames delivered and the CPU this process used. Run once with and once
    /// without KAIRO_GPU_COMPOSITOR=1:
    ///   cargo test --release -p kairo --lib video_bg::tests::plays -- --ignored --nocapture
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn plays() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let (Ok(ff), Ok(video)) = (std::env::var("KAIRO_TEST_FFMPEG"), std::env::var("KAIRO_TEST_VIDEO")) else { return };
        let (w, h) = (1920u32, 1080u32);
        let frames = Arc::new(AtomicUsize::new(0));
        let sample = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (f2, s2) = (frames.clone(), sample.clone());
        let cpu = || {
            let mut u: Rusage = unsafe { std::mem::zeroed() };
            unsafe { getrusage(0, &mut u) };
            u.utime[0] as f64 + u.utime[1] as f64 / 1e6 + u.stime[0] as f64 + u.stime[1] as f64 / 1e6
        };
        let before = cpu();
        let bg = super::VideoBg::start(std::path::Path::new(&ff), video.into(), "cover", w, h, 30, move |f| {
            f2.fetch_add(1, Ordering::Relaxed);
            let i = ((h / 4 * w + w / 4) * 4) as usize;
            *s2.lock().unwrap() = f.bgra[i..i + 4].to_vec();
        })
        .expect("start");
        // A page: a white box in the top-left quarter, faded in over 1 s.
        let mut page = vec![0u8; (w * h * 4) as usize];
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                page[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        bg.set_overlay(Arc::new(super::Frame { width: w, height: h, bgra: page }), 1000);
        std::thread::sleep(std::time::Duration::from_secs(9));
        bg.stop();
        let used = cpu() - before;
        let n = frames.load(Ordering::Relaxed);
        eprintln!("frames {n} in 10 s, CPU {used:.2} s ({:.0}% of a core), pixel under the page {:?}", used * 10.0, sample.lock().unwrap());
        assert!(n > 200, "about 30 fps");
        assert_eq!(*sample.lock().unwrap(), vec![255, 255, 255, 255], "the page is over the video");
    }

    #[cfg(unix)]
    #[repr(C)]
    struct Rusage {
        utime: [i64; 2],
        stime: [i64; 2],
        rest: [i64; 14],
    }
    #[cfg(unix)]
    extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }

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
