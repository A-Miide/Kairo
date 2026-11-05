//! The native outputs a finished frame can go to (NDI, DeckLink, Syphon on macOS), and the
//! per-output handles they are driven through.
//!
//! Each output the operator adds gets its own sender, keyed by the id the page made for it
//! (settings.ndiOutputs, syphonOutputs, decklinkOutputs). `Handles<T>` is that keyed set, made
//! once for every kind instead of copied per kind; `FrameSender` is what a kind has to offer so
//! the capture path can hand every kind the same frame without knowing which exist.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use tauri::{AppHandle, Manager};

use crate::capture::Frame;

/// One handle per output id, made the first time anything asks for it. The sender's own module
/// only ever sees the one `Arc<Mutex<T>>` it is given.
pub struct Handles<T>(Arc<Mutex<HashMap<String, Arc<Mutex<T>>>>>);

impl<T> Default for Handles<T> {
    fn default() -> Self {
        Handles(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl<T> Handles<T> {
    /// Every handle that was ever asked for (at quit, to release each sender). Works even if a
    /// thread panicked holding the lock: shutdown cleanup must not be skipped for that.
    pub fn each(&self, mut f: impl FnMut(&Arc<Mutex<T>>)) {
        for handle in self.0.lock().unwrap_or_else(|p| p.into_inner()).values() {
            f(handle);
        }
    }
}

impl<T: Default> Handles<T> {
    pub fn handle_for(&self, id: &str) -> Result<Arc<Mutex<T>>, String> {
        let mut map = self.0.lock().map_err(|e| e.to_string())?;
        Ok(map.entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(T::default()))).clone())
    }
}

/// A sender kind that takes finished frames of an output.
pub trait FrameSender: Sync {
    /// What a run of failed sends is reported as ("output-capture-trouble"); None: not reported.
    fn stage(&self) -> Option<&'static str> {
        None
    }
    /// Gives output `id`'s sender of this kind the frame.
    fn send(&self, app: &AppHandle, id: &str, frame: Arc<Frame>, fade_ms: u32) -> Result<(), String>;
}

pub struct Ndi;
impl FrameSender for Ndi {
    fn send(&self, app: &AppHandle, id: &str, frame: Arc<Frame>, fade_ms: u32) -> Result<(), String> {
        let h = app.state::<crate::NdiState>().handle_for(id)?;
        let h = h.lock().map_err(|e| e.to_string())?;
        h.update_frame(frame, fade_ms);
        Ok(())
    }
}

pub struct DeckLink;
impl FrameSender for DeckLink {
    fn send(&self, app: &AppHandle, id: &str, frame: Arc<Frame>, fade_ms: u32) -> Result<(), String> {
        let h = app.state::<crate::DeckLinkState>().handle_for(id)?;
        let h = h.lock().map_err(|e| e.to_string())?;
        h.update_frame(frame, fade_ms);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub struct Syphon;
#[cfg(target_os = "macos")]
impl FrameSender for Syphon {
    fn stage(&self) -> Option<&'static str> {
        Some("syphon")
    }
    fn send(&self, app: &AppHandle, id: &str, frame: Arc<Frame>, fade_ms: u32) -> Result<(), String> {
        let h = app.state::<crate::SyphonState>().handle_for(id)?;
        crate::syphon::update_frame(frame, fade_ms, h).map_err(|e| e.to_string())
    }
}

/// Every sender kind this build has, in the order frames are handed out.
#[cfg(target_os = "macos")]
pub fn all() -> &'static [&'static dyn FrameSender] {
    &[&Ndi, &DeckLink, &Syphon]
}
#[cfg(not(target_os = "macos"))]
pub fn all() -> &'static [&'static dyn FrameSender] {
    &[&Ndi, &DeckLink]
}

/// A media picture from the page: standard base64 of PNG/JPEG bytes, or None to clear the
/// media layer. Decoded here so the sender's render thread only receives raw bytes.
pub fn decode_media(image_base64: Option<String>) -> Result<Option<Vec<u8>>, String> {
    match image_base64 {
        Some(b64) => base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map(Some)
            .map_err(|e| format!("invalid media base64: {e}")),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_is_made_once_per_output_and_shared() {
        let h: Handles<u32> = Handles::default();
        *h.handle_for("a").unwrap().lock().unwrap() = 7;
        assert_eq!(*h.handle_for("a").unwrap().lock().unwrap(), 7);
        assert_eq!(*h.handle_for("b").unwrap().lock().unwrap(), 0);
    }

    #[test]
    fn media_is_decoded_or_cleared() {
        assert_eq!(decode_media(None), Ok(None));
        assert_eq!(decode_media(Some("aGk=".into())), Ok(Some(b"hi".to_vec())));
        assert!(decode_media(Some("!!".into())).is_err());
    }
}
