//! Blackmagic DeckLink output (SDI/HDMI from a DeckLink card): the output's
//! captured theme frames (capture.rs) played out at the card's own frame rate,
//! blending old into new for a Fade — the same frames NDI and Syphon send.
//! With keying on, the card's external keyer puts out fill and key, so a
//! vision mixer can lay lower thirds over camera; the fill is premultiplied
//! (an ATEM's default "Pre Multiplied Key").
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
    Stop,
}

#[derive(Default)]
pub struct DeckLinkHandle {
    tx: Option<Sender<Cmd>>,
    thread: Option<thread::JoinHandle<()>>,
    /// The size the card plays, which captures are made at.
    pub size: (u32, u32),
}

impl DeckLinkHandle {
    pub fn is_running(&self) -> bool {
        self.tx.is_some()
    }

    pub fn update_frame(&self, frame: Arc<Frame>, fade_ms: u32) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(Cmd::Frame { frame, fade_ms });
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
        let (tx, rx) = bounded::<Cmd>(8);
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
            let mut next = Instant::now();
            loop {
                let mut stop = false;
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        Cmd::Stop => stop = true,
                        Cmd::Frame { frame, fade_ms } => {
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
                    }
                }
                if stop {
                    break;
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

    #[test]
    fn starting_without_a_card_fails_cleanly() {
        let h = std::sync::Arc::new(std::sync::Mutex::new(super::DeckLinkHandle::default()));
        let r = super::start(0, 0x48703539 /* bmdModeHD1080p5994 */, false, 59.94, h.clone());
        assert!(r.is_err());
        println!("start without card: {:?}", r.err());
        assert!(!h.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_running());
    }
}
