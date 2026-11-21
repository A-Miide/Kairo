//! Blackmagic DeckLink output (SDI/HDMI from a DeckLink card): the output's
//! captured theme frames (capture.rs) played out at the card's own frame rate,
//! blending old into new for a Fade — the same frames NDI and Syphon send.
//! With keying on, the card's external keyer puts out fill and key, so a
//! vision mixer can lay lower thirds over camera; the fill is premultiplied
//! (a video mixer's default "Pre Multiplied Key").
//!
//! Where output windows can't be captured (Windows), no frames would ever
//! arrive and the card played black. There the output draws the words, media
//! and timer itself with NDI's renderer, under its Output Control switches, as
//! NDI and Spout do; a captured frame, once one arrives, takes over as on macOS.
//!
//! The C side is native/decklink.cpp over vendor/decklink; built when the
//! compiler step succeeded (cfg `kairo_decklink`), otherwise every call
//! reports DeckLink as unavailable.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Sender};
use serde_json::{json, Value};

use crate::capture::Frame;

#[cfg(kairo_decklink)]
mod ffi {
    use std::os::raw::{c_char, c_int, c_void};
    extern "C" {
        pub fn kdl_list(out: *mut c_char, cap: c_int) -> c_int;
        pub fn kdl_open(index: c_int, mode: u32, keying: c_int, err: *mut c_char, errcap: c_int) -> *mut c_void;
        pub fn kdl_frame(handle: *mut c_void, bgra: *const u8, width: c_int, height: c_int) -> c_int;
        pub fn kdl_size(handle: *mut c_void, width: *mut c_int, height: *mut c_int);
        pub fn kdl_close(handle: *mut c_void);
    }
}

/// The cards and the video modes each can play, or why there are none.
pub fn list() -> Value {
    #[cfg(kairo_decklink)]
    {
        let mut buf = vec![0u8; 256 * 1024];
        unsafe { ffi::kdl_list(buf.as_mut_ptr().cast(), buf.len() as i32) };
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        serde_json::from_slice(&buf[..end]).unwrap_or_else(|e| json!({ "ok": false, "error": format!("DeckLink list unreadable: {e}") }))
    }
    #[cfg(not(kairo_decklink))]
    {
        json!({ "ok": false, "error": "This build of Kairo has no DeckLink support." })
    }
}

enum Cmd {
    Frame { frame: Arc<Frame>, fade_ms: u32 },
    Text { verse: String, reference: String, bible: bool },
    Media(Option<Vec<u8>>),
    Timer(String),
    Layers(crate::native_layers::Layers),
    Stop,
}

/// What the output draws itself until a captured frame arrives (see the module comment). The
/// font system is made on first use: an output fed captured frames never pays for it.
#[derive(Default)]
#[cfg_attr(not(kairo_decklink), allow(dead_code))]
struct Drawn {
    verse: String,
    reference: String,
    bible: bool,
    media: Option<image::RgbaImage>,
    timer: String,
    layers: crate::native_layers::Layers,
    fonts: Option<(cosmic_text::FontSystem, cosmic_text::SwashCache)>,
}

#[cfg_attr(not(kairo_decklink), allow(dead_code))]
impl Drawn {
    /// Takes a command; true when the picture needs drawing again.
    fn take(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Text { verse, reference, bible } => (self.verse, self.reference, self.bible) = (verse, reference, bible),
            Cmd::Timer(text) => self.timer = text,
            Cmd::Layers(l) => self.layers = l,
            Cmd::Media(bytes) => {
                self.media = bytes.and_then(|b| match image::load_from_memory(&b) {
                    Ok(img) => Some(img.to_rgba8()),
                    Err(e) => { eprintln!("[DeckLink] Media decode failed: {e}"); None }
                });
            }
            Cmd::Frame { .. } | Cmd::Stop => return false,
        }
        true
    }
    fn draw(&mut self, buf: &mut [u8], w: i32, h: i32) {
        let (fs, sc) = self.fonts.get_or_insert_with(|| (cosmic_text::FontSystem::new(), cosmic_text::SwashCache::new()));
        let (verse, reference) = self.layers.words(self.bible, &self.verse, &self.reference);
        crate::ndi::render_frame(buf, w, h, verse, reference, self.layers.media(self.media.as_ref()), self.layers.timer(&self.timer), fs, sc);
    }
}

#[derive(Default)]
pub struct DeckLinkHandle {
    tx: Option<Sender<Cmd>>,
    thread: Option<thread::JoinHandle<()>>,
    /// The size the card plays, which captures are made at.
    pub size: (u32, u32),
}

impl DeckLinkHandle {
    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.tx.is_some()
    }

    pub fn update_frame(&self, frame: Arc<Frame>, fade_ms: u32) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(Cmd::Frame { frame, fade_ms });
        }
    }

    /// The words to draw where no captured frames come (see the module comment). Unlike a
    /// frame, which the next one replaces, a change of words is never dropped.
    pub fn update_text(&self, verse: String, reference: String, bible: bool) {
        self.send(Cmd::Text { verse, reference, bible });
    }
    pub fn update_media(&self, bytes: Option<Vec<u8>>) {
        self.send(Cmd::Media(bytes));
    }
    pub fn update_timer(&self, text: String) {
        self.send(Cmd::Timer(text));
    }
    pub fn set_layers(&self, layers: crate::native_layers::Layers) {
        self.send(Cmd::Layers(layers));
    }
    fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Cmd::Stop);
        }
        if let Some(t) = self.thread.take() {
            let started = Instant::now();
            while !t.is_finished() && started.elapsed() < Duration::from_secs(2) {
                thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

/// Opens `device`'s output in `mode` (and its keyer if `keying`) on its own
/// thread, playing at `fps`. Returns the frame size the card wants.
#[allow(unused_variables)]
pub fn start(device: i32, mode: u32, keying: bool, fps: f64, shared: Arc<Mutex<DeckLinkHandle>>) -> Result<(u32, u32), String> {
    #[cfg(not(kairo_decklink))]
    {
        Err("This build of Kairo has no DeckLink support.".into())
    }
    #[cfg(kairo_decklink)]
    {
        let (tx, rx) = bounded::<Cmd>(32);
        let (ready_tx, ready_rx) = bounded::<Result<(u32, u32), String>>(1);
        let period = Duration::from_secs_f64(1.0 / fps.clamp(1.0, 120.0));
        let t = thread::spawn(move || unsafe {
            let mut err = vec![0u8; 512];
            let h = ffi::kdl_open(device, mode, keying as i32, err.as_mut_ptr().cast(), err.len() as i32);
            if h.is_null() {
                let end = err.iter().position(|b| *b == 0).unwrap_or(0);
                let _ = ready_tx.send(Err(String::from_utf8_lossy(&err[..end]).into_owned()));
                return;
            }
            let (mut w, mut hgt) = (0i32, 0i32);
            ffi::kdl_size(h, &mut w, &mut hgt);
            let _ = ready_tx.send(Ok((w as u32, hgt as u32)));
            let mut buf = vec![0u8; (w * hgt * 4) as usize]; // black, fully keyed out
            let mut fade: Option<(Vec<u8>, Arc<Frame>, Instant, Duration)> = None;
            let mut drawn = Drawn::default();
            let mut captured = false;
            let mut next = Instant::now();
            loop {
                let mut stop = false;
                let mut redraw = false;
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        Cmd::Stop => stop = true,
                        Cmd::Frame { frame, fade_ms } => {
                            captured = true;
                            if frame.width as i32 != w || frame.height as i32 != hgt {
                                continue;
                            }
                            if fade_ms > 0 {
                                fade = Some((buf.clone(), frame, Instant::now(), Duration::from_millis(fade_ms as u64)));
                            } else {
                                buf.copy_from_slice(&frame.bgra);
                                fade = None;
                            }
                        }
                        other => redraw |= drawn.take(other),
                    }
                }
                if stop {
                    break;
                }
                if redraw && !captured {
                    drawn.draw(&mut buf, w, hgt);
                }
                if let Some((from, to, started, dur)) = &fade {
                    let t = started.elapsed().as_secs_f32() / dur.as_secs_f32().max(0.001);
                    if t >= 1.0 {
                        buf.copy_from_slice(&to.bgra);
                        fade = None;
                    } else {
                        crate::capture::blend(&mut buf, from, &to.bgra, t);
                    }
                }
                ffi::kdl_frame(h, buf.as_ptr(), w, hgt);
                next += period;
                let now = Instant::now();
                if next > now {
                    thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
            ffi::kdl_close(h);
        });
        let size = ready_rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "The DeckLink card didn't respond.".to_string())??;
        let mut g = shared.lock().map_err(|e| e.to_string())?;
        g.tx = Some(tx);
        g.thread = Some(t);
        g.size = size;
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    /// Without Blackmagic Desktop Video installed (as on a build machine),
    /// listing says so cleanly instead of failing.
    #[test]
    fn listing_without_a_driver_is_a_clear_answer() {
        let v = super::list();
        assert!(v["ok"].is_boolean(), "{v}");
        if v["ok"] == false {
            assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{v}");
        } else {
            assert!(v["devices"].is_array(), "{v}");
        }
        println!("decklink list: {v}");
    }

    /// Where no captured frames come, the card gets the words, under the output's switches.
    #[test]
    fn the_words_are_drawn_under_the_outputs_switches() {
        use super::{Cmd, Drawn};
        let (w, h) = (1280, 720); // the renderer's type sizes are fixed: smaller frames crop the verse
        let lit = |d: &mut Drawn| {
            let mut buf = vec![0u8; (w * h * 4) as usize];
            d.draw(&mut buf, w, h);
            buf.chunks_exact(4).filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count()
        };
        let mut d = Drawn::default();
        let blank = lit(&mut d);
        assert!(d.take(Cmd::Text { verse: "Jesus wept.".into(), reference: "John 11:35".into(), bible: true }));
        let words = lit(&mut d);
        assert!(words > blank, "the verse is drawn ({words} white pixels vs {blank})");
        d.take(Cmd::Layers(crate::native_layers::Layers { bible: false, ..Default::default() }));
        assert_eq!(lit(&mut d), blank, "Scripture switched off: no words");
    }

    #[test]
    fn starting_without_a_card_fails_cleanly() {
        let h = std::sync::Arc::new(std::sync::Mutex::new(super::DeckLinkHandle::default()));
        let r = super::start(0, 0x48703539 /* bmdModeHD1080p5994 */, false, 59.94, h.clone());
        assert!(r.is_err());
        println!("start without card: {:?}", r.err());
        assert!(!h.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_running());
    }
}
