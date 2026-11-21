//! Native Spout sender for KAIRO (Windows only) — the Windows equivalent of
//! `syphon.rs`'s macOS sender: same-machine, zero-setup video sharing so any
//! compatible app can pick the source up directly — no capture bridge, no hidden window.
//!
//! The transport is `spout2-rs`'s DirectX 11 backend (`Sender`), which owns its
//! own D3D11 device and takes a plain CPU BGRA buffer via `send_image`. Built on
//! that crate rather than a hand-rolled FFI binding to Spout's own C++ interface
//! (SpoutLibrary exports a C++ vtable, not flat C functions the way libndi's
//! are) — see Cargo.toml's own comment on this dependency.
//!
//! Each output has its own thread, as NDI does: the D3D11 sender is made, used
//! and dropped there, and the page's commands only queue work. Drawing and
//! sending used to run inside the Tauri command, on the window's UI thread, so
//! a long verse or a new picture made the whole app stutter. The words are
//! drawn by ndi.rs's renderer, with this output's Output Control switches
//! (native_layers.rs), and sent with straight alpha.

#![cfg(target_os = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Sender};

use crate::native_layers::Layers;

// Same fallback default as ndi.rs/syphon.rs — Output Looks' "add an output"
// flow asks for a real resolution up front, so this only matters when a
// caller passes 0/invalid values.
const DEFAULT_FRAME_W: u32 = 1280;
const DEFAULT_FRAME_H: u32 = 720;

enum Cmd {
    Text { verse: String, reference: String, bible: bool },
    Media(Option<Vec<u8>>),
    Timer(String),
    Layers(Layers),
    Stop,
}

#[derive(Default)]
pub struct SpoutHandle {
    tx: Option<Sender<Cmd>>,
    /// A start has been accepted and its thread isn't sending yet (see ndi.rs's own `starting`).
    starting: bool,
    thread: Option<thread::JoinHandle<()>>,
}

impl SpoutHandle {
    fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }
    /// Stops the sender and waits (up to `timeout`) for its thread to release the D3D11 sender.
    pub fn stop_and_join(&mut self, timeout: Duration) {
        self.starting = false;
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Cmd::Stop);
        }
        if let Some(t) = self.thread.take() {
            let started = Instant::now();
            while !t.is_finished() && started.elapsed() < timeout {
                thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

pub fn is_spout_available() -> bool {
    // spout2-rs's DX backend creates its own D3D11 device on demand rather
    // than exposing a separate capability probe; D3D11 is part of Windows
    // itself, so unconditionally available is the correct answer here.
    true
}

/// `width`/`height`: this output's own configured resolution (0 falls back to DEFAULT_FRAME_W/H).
pub fn start(source_name: &str, width: u32, height: u32, shared: Arc<Mutex<SpoutHandle>>) -> Result<(), String> {
    {
        let mut h = shared.lock().map_err(|e| e.to_string())?;
        // Already sending or starting: a second start is a success (see syphon::start).
        if h.tx.is_some() || h.starting {
            return Ok(());
        }
        h.starting = true;
    }
    let (w, hgt) = (if width > 0 { width } else { DEFAULT_FRAME_W }, if height > 0 { height } else { DEFAULT_FRAME_H });
    let (tx, rx) = bounded::<Cmd>(64);
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
    let name = source_name.to_string();
    let t = thread::spawn(move || {
        let mut sender = match spout2::dx::Sender::new(&name) {
            Ok(s) => s,
            Err(e) => {
                let _ = ready_tx.send(Err(format!("Spout sender create failed: {e}")));
                return;
            }
        };
        let _ = ready_tx.send(Ok(()));
        eprintln!("[Spout] sender '{name}' broadcasting {w}x{hgt}");
        let mut font_system = cosmic_text::FontSystem::new();
        let mut swash_cache = cosmic_text::SwashCache::new();
        let mut buf = vec![0u8; (w * hgt * 4) as usize];
        let (mut verse, mut reference, mut bible, mut timer) = (String::new(), String::new(), false, String::new());
        let mut media: Option<image::RgbaImage> = None;
        let mut layers = Layers::default();
        'run: loop {
            let (v, r) = layers.words(bible, &verse, &reference);
            crate::ndi::render_frame(&mut buf, w as i32, hgt as i32, v, r, layers.media(media.as_ref()), layers.timer(&timer), &mut font_system, &mut swash_cache);
            crate::capture::unpremultiply(&mut buf);
            if let Err(e) = sender.send_image(&buf, w, hgt) {
                eprintln!("[Spout] send_image failed: {e}");
            }
            // Wait for the next change; everything queued meanwhile is taken in one go.
            let Ok(first) = rx.recv() else { break };
            for cmd in std::iter::once(first).chain(rx.try_iter()) {
                match cmd {
                    Cmd::Stop => break 'run,
                    Cmd::Text { verse: v, reference: r, bible: b } => (verse, reference, bible) = (v, r, b),
                    Cmd::Timer(text) => timer = text,
                    Cmd::Layers(l) => layers = l,
                    Cmd::Media(bytes) => {
                        media = bytes.and_then(|b| match image::load_from_memory(&b) {
                            Ok(img) => Some(img.to_rgba8()),
                            Err(e) => { eprintln!("[Spout] Media decode failed: {e}"); None }
                        });
                    }
                }
            }
        }
        eprintln!("[Spout] sender '{name}' stopped");
    });
    let ready = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap_or(Ok(()));
    let mut h = shared.lock().map_err(|e| e.to_string())?;
    if let Err(e) = ready {
        h.starting = false;
        let _ = t.join();
        return Err(e);
    }
    if h.starting {
        h.starting = false;
        h.tx = Some(tx);
        h.thread = Some(t);
    } else {
        // Stopped while it was starting: don't leave a sender nobody can reach.
        let _ = tx.send(Cmd::Stop);
    }
    Ok(())
}

pub fn stop(shared: Arc<Mutex<SpoutHandle>>) -> Result<(), String> {
    shared.lock().map_err(|e| e.to_string())?.stop_and_join(Duration::from_secs(2));
    Ok(())
}

/// `bible`: Scripture (else a slide or song), for the Output Control switch it falls under.
pub fn update(verse: &str, reference: &str, bible: bool, shared: Arc<Mutex<SpoutHandle>>) -> Result<(), String> {
    shared.lock().map_err(|e| e.to_string())?.send(Cmd::Text { verse: verse.to_string(), reference: reference.to_string(), bible });
    Ok(())
}

/// `bytes: None` clears the Media layer.
pub fn update_media(bytes: Option<Vec<u8>>, shared: Arc<Mutex<SpoutHandle>>) -> Result<(), String> {
    shared.lock().map_err(|e| e.to_string())?.send(Cmd::Media(bytes));
    Ok(())
}

pub fn update_timer(text: &str, shared: Arc<Mutex<SpoutHandle>>) -> Result<(), String> {
    shared.lock().map_err(|e| e.to_string())?.send(Cmd::Timer(text.to_string()));
    Ok(())
}

pub fn set_layers(layers: Layers, shared: Arc<Mutex<SpoutHandle>>) -> Result<(), String> {
    shared.lock().map_err(|e| e.to_string())?.send(Cmd::Layers(layers));
    Ok(())
}
