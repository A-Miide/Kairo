//! The pixels of an output's hidden display window — the same page, theme,
//! ProPresenter layout and media the screens show — for the outputs that
//! aren't screens: NDI, Syphon and DeckLink.
//!
//! The window (`kairo-cap-<id>`, display.html?capture=1) tells us when what it
//! shows has changed; we snapshot it then, and the output keeps sending the
//! last frame at its own rate (blending into a new one for a Fade). Measured
//! on an 8 GB M1: a 1080p snapshot takes ~64 ms, so capturing every frame
//! isn't possible — capturing on change is.

use std::sync::{Arc, Mutex};

/// One captured picture: BGRA bytes, premultiplied alpha, rows packed.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

impl Frame {
    /// The same picture with straight (un-premultiplied) alpha, for outputs
    /// that expect it.
    pub fn unpremultiplied(&self) -> Vec<u8> {
        let mut out = self.bgra.clone();
        for px in out.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
}

/// `out` = `from` blended toward `to` by `t` (0..1). Premultiplied pixels
/// blend correctly channel by channel.
pub fn blend(out: &mut [u8], from: &[u8], to: &[u8], t: f32) {
    let k = (t.clamp(0.0, 1.0) * 256.0) as u32;
    for ((o, a), b) in out.iter_mut().zip(from).zip(to) {
        *o = ((*a as u32 * (256 - k) + *b as u32 * k) >> 8) as u8;
    }
}

type Done = Box<dyn FnOnce(Result<Frame, String>) + Send>;

/// Can this platform capture an output window yet?
pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

/// Snapshots `window` at `width`×`height` pixels and hands the frame to
/// `done` (on the main thread, once).
#[cfg(target_os = "macos")]
pub fn snapshot(window: &tauri::WebviewWindow, width: u32, height: u32, done: impl FnOnce(Result<Frame, String>) + Send + 'static) -> Result<(), String> {
    let done: Arc<Mutex<Option<Done>>> = Arc::new(Mutex::new(Some(Box::new(done))));
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let on_fail = done.clone();
    let r = window.with_webview(move |pv| unsafe {
        use foundation3::{NSError, NSNumber};
        use objc2_app_kit::NSImage;
        use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

        let ptr = pv.inner() as *const WKWebView;
        let Some(wv) = ptr.as_ref() else {
            finish(&done, Err("no web view".into()));
            return;
        };
        let mtm = objc2_06::MainThreadMarker::new_unchecked();
        let cfg = WKSnapshotConfiguration::new(mtm);
        // Points; the snapshot comes back at the screen's pixel density.
        cfg.setSnapshotWidth(Some(&NSNumber::new_f64(width as f64 / scale)));
        let d = done.clone();
        let block = block2_06::RcBlock::new(move |img: *mut NSImage, err: *mut NSError| {
            let r = if img.is_null() {
                let why = err.as_ref().map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "snapshot failed".into());
                Err(why)
            } else {
                to_frame(&*img, width, height)
            };
            finish(&d, r);
        });
        wv.takeSnapshotWithConfiguration_completionHandler(Some(&cfg), &block);
    });
    if let Err(e) = r {
        finish(&on_fail, Err(e.to_string()));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn snapshot(_window: &tauri::WebviewWindow, _width: u32, _height: u32, done: impl FnOnce(Result<Frame, String>) + Send + 'static) -> Result<(), String> {
    done(Err("capturing an output window isn't supported on this platform yet".into()));
    Ok(())
}

fn finish(done: &Arc<Mutex<Option<Done>>>, r: Result<Frame, String>) {
    if let Some(f) = done.lock().ok().and_then(|mut d| d.take()) {
        f(r);
    }
}

/// Draws the snapshot into a BGRA bitmap of exactly `width`×`height`.
#[cfg(target_os = "macos")]
unsafe fn to_frame(img: &objc2_app_kit::NSImage, width: u32, height: u32) -> Result<Frame, String> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo};

    let cg = img.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None).ok_or("no image in the snapshot")?;
    let (w, h) = (width as usize, height as usize);
    let mut bgra = vec![0u8; w * h * 4];
    let space = CGColorSpace::new_device_rgb().ok_or("no colour space")?;
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    let ctx = CGBitmapContextCreate(bgra.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), info).ok_or("no bitmap context")?;
    let rect = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: w as f64, height: h as f64 } };
    CGContext::draw_image(Some(&ctx), rect, Some(&cg));
    drop(ctx);
    Ok(Frame { width, height, bgra })
}
