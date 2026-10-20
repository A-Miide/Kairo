//! Singing heard in the room. YAMNet (a sound classifier, `databases/vad/yamnet.onnx`) scores the
//! last second of audio every 0.48 s on its own thread; `kairo_detect::music::MusicGate` turns its
//! speech and music scores into singing on/off, and the detector is told (`Event::Singing`), so
//! scripture in the lyrics is only offered. Without the model file it stays off.
//!
//! Cost measured on a 64-minute service: 12.5 s of work on an M-series Mac, about 1% of one core.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;

use kairo_detect::engine::Event;
use kairo_detect::music::MusicGate;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::json;

use crate::Shared;

/// One model frame: 0.975 s of 16 kHz audio, scored every 0.48 s.
const FRAME: usize = 15_600;
const HOP: usize = 7_680;
const SPEECH: usize = 0;
const MUSIC: usize = 132;

static AUDIO: OnceLock<SyncSender<Vec<f32>>> = OnceLock::new();

/// Starts the classifier when the model is installed.
pub fn start(model: &Path, app: Shared) {
    if !model.exists() {
        return;
    }
    let session = match Session::builder().and_then(|b| Ok(b.with_intra_threads(1)?)).and_then(|mut b| b.commit_from_file(model)) {
        Ok(s) => s,
        Err(e) => {
            crate::routes::write_debug_global("server:singing-model", json!({ "error": e.to_string() }));
            return;
        }
    };
    let (tx, rx) = sync_channel::<Vec<f32>>(64);
    if AUDIO.set(tx).is_err() {
        return;
    }
    let _ = std::thread::Builder::new().name("singing".into()).spawn(move || run(session, rx, app));
}

/// 16 kHz mono samples as they are heard. Never blocks the audio path: a full queue drops the frame.
pub fn audio(samples: &[f32]) {
    if let Some(tx) = AUDIO.get() {
        let _ = tx.try_send(samples.to_vec());
    }
}

fn run(mut session: Session, rx: Receiver<Vec<f32>>, app: Shared) {
    let mut buf: VecDeque<f32> = VecDeque::with_capacity(FRAME + HOP);
    let mut fresh = 0;
    let mut gate = MusicGate::default();
    for chunk in rx {
        fresh += chunk.len();
        buf.extend(chunk);
        while buf.len() > FRAME {
            buf.pop_front();
        }
        if buf.len() < FRAME || fresh < HOP {
            continue;
        }
        fresh = 0;
        let wave: Vec<f32> = buf.iter().copied().collect();
        let Ok(input) = Tensor::from_array(([FRAME], wave)) else { continue };
        let Ok(out) = session.run(vec![("waveform".to_string(), input.into_dyn())]) else { continue };
        let Ok((shape, scores)) = out[0].try_extract_tensor::<f32>() else { continue };
        let classes = shape.get(1).copied().unwrap_or(0) as usize;
        if classes <= MUSIC || scores.len() < classes {
            continue;
        }
        if let Some(on) = gate.push(scores[SPEECH], scores[MUSIC]) {
            crate::routes::write_debug_global("server:singing", json!({ "active": on }));
            if let Some((_, d)) = app.ready() {
                d.send(Event::Singing(on));
            }
        }
    }
}
