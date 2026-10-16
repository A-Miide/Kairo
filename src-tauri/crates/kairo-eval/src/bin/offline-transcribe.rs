//! Transcribes a recording with the offline engine exactly as a live session
//! would (same model, same locking, the recogniser asked every 120 ms of
//! audio), faster than real time, into a file the replay scores.
//!
//! `cargo run --release -p kairo-eval --bin offline-transcribe -- <audio> <name> [--beam] [--hotwords <score>] [--blank-penalty <x>] [--reset-after <words>]`
//! writes `parity/offline/<name>.json`; then `replay --offline <name>`.
//! `--beam` decodes with beam search; `--hotwords` also favours the book names.

use std::cell::RefCell;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Instant;

use kairo_eval::root;
use kairo_server::interpretation::{model_path, Change, Interpreter, Verdict};
use kairo_server::offline::{recognizer, Decoding, Heard, Transcriber, POLL, SAMPLE_RATE_HZ};
use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(audio), Some(name)) = (args.first(), args.get(1)) else {
        eprintln!("usage: offline-transcribe <audio file> <name>");
        std::process::exit(2);
    };
    let model = std::env::var_os("KAIRO_OFFLINE_MODEL").map(std::path::PathBuf::from).unwrap_or_else(|| root().join("models/sherpa-streaming-en"));
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let hotwords_score: f32 = opt("--hotwords").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let blank_penalty: f32 = opt("--blank-penalty").and_then(|s| s.parse().ok()).unwrap_or(kairo_server::offline::BLANK_PENALTY);
    let decoding = Decoding { beam: hotwords_score > 0.0 || args.iter().any(|a| a == "--beam"), hotwords_score, blank_penalty };
    let rec = recognizer(&model, &decoding).expect("offline model");
    let mut heard = Transcriber::new(rec, None);
    if let Some(n) = opt("--reset-after").and_then(|s| s.parse().ok()) {
        heard.reset_after = n;
    }

    let mut ffmpeg = Command::new("ffmpeg")
        .args(["-v", "error", "-i", audio, "-ac", "1", "-ar", "16000", "-f", "s16le", "-"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("ffmpeg");
    let mut pcm = ffmpeg.stdout.take().unwrap();

    let step = SAMPLE_RATE_HZ * POLL.as_millis() as usize / 1000;
    let mut buf = vec![0u8; step * 2];
    let mut fed = 0usize;
    let mut cues: Vec<Value> = Vec::new();
    let mut inputs: Vec<Value> = Vec::new();
    let mut cue_start: Option<u64> = None;
    let started = Instant::now();
    // The interpreter, exactly as the app runs it: an interpreter's turn is not transcribed and
    // never reaches the detectors (KAIRO_INTERPRETER=off turns it off for comparison runs).
    let interp = RefCell::new(Interpreter::load(&model_path()));
    {
        // The Bible's vocabulary tells real words from the fragments of another language.
        let k = std::sync::Arc::new(kairo_eval::knowledge());
        interp.borrow_mut().set_known(std::sync::Arc::new(move |w: &str| k.lexicon.vocab().id(w).is_some()));
    }
    let interp_on = std::env::var("KAIRO_INTERPRETER").map_or(true, |v| v != "off");
    let marks: RefCell<Vec<Value>> = RefCell::new(Vec::new());
    let mut record = |h: Heard, t: u64, cues: &mut Vec<Value>, inputs: &mut Vec<Value>| {
        let (text, fin, speech_final, continues) = match h {
            Heard::Interim(s) => (s, false, false, false),
            Heard::Locked(s) => (s, true, false, true),
            Heard::Final(s) => (s, true, true, false),
        };
        // What the interpreter held back as possibly the preacher's goes through first when the turn ends on his sentence.
        let mut batch: Vec<(String, bool, bool, bool)> = Vec::new();
        if interp_on && !text.trim().is_empty() {
            let (verdict, change, released) = {
                let mut i = interp.borrow_mut();
                let (v, c) = i.text(&text, fin);
                (v, c, i.take_released())
            };
            if let Some(c) = change {
                marks.borrow_mut().push(json!({ "audioMs": t, "active": c == Change::Started }));
            }
            if verdict == Verdict::Interpretation {
                marks.borrow_mut().push(json!({ "audioMs": t, "dropped": text }));
                if fin {
                    cue_start = None;
                }
                return;
            }
            batch.extend(released.into_iter().map(|(x, f)| (x, f, false, false)));
        }
        batch.push((text, fin, speech_final, continues));
        for (text, fin, speech_final, continues) in batch {
        let cue = cues.len();
        let start = *cue_start.get_or_insert(t);
        inputs.push(json!({ "t": t, "audioMs": t, "cue": cue, "text": text, "isFinal": fin, "speechFinal": speech_final, "continues": continues }));
        if fin {
            cues.push(json!({ "text": text, "audioMs": start }));
            cue_start = None;
        }
        }
    };
    // --realtime: the audio arrives at live speed, as in the app, and the app's catch-up rule
    // (offline::too_far_behind / Transcriber::cut) applies: on a computer too slow to keep up
    // the run shows exactly what the live worker would skip. Without it the audio is fed as
    // fast as it decodes (the deterministic default, what the app does whenever it keeps up).
    let realtime = args.iter().any(|a| a == "--realtime");
    let mut skips: Vec<Value> = Vec::new();
    loop {
        let n = read_full(&mut pcm, &mut buf);
        if n == 0 {
            break;
        }
        let samples: Vec<f32> = buf[..n].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        if realtime {
            let arrives = started + std::time::Duration::from_millis((fed * 1000 / SAMPLE_RATE_HZ) as u64);
            let now = Instant::now();
            if now < arrives {
                std::thread::sleep(arrives - now);
            } else if kairo_server::offline::too_far_behind(now - arrives) {
                // Everything that has arrived by now is dropped, as the worker drops its queue.
                let at = (fed * 1000 / SAMPLE_RATE_HZ) as u64;
                let mut dropped = samples.len();
                fed += samples.len();
                while started + std::time::Duration::from_millis((fed * 1000 / SAMPLE_RATE_HZ) as u64) <= Instant::now() {
                    let m = read_full(&mut pcm, &mut buf);
                    if m == 0 { break; }
                    dropped += m / 2;
                    fed += m / 2;
                }
                if let Some(h) = heard.cut() {
                    record(h, at, &mut cues, &mut inputs);
                }
                skips.push(json!({ "audioMs": at, "skippedMs": dropped * 1000 / SAMPLE_RATE_HZ }));
                continue;
            }
        }
        fed += samples.len();
        heard.accept(&samples);
        let t = (fed * 1000 / SAMPLE_RATE_HZ) as u64;
        if interp_on {
            if let Some(c) = interp.borrow_mut().audio(&samples) {
                marks.borrow_mut().push(json!({ "audioMs": t, "active": c == Change::Started }));
            }
        }
        for h in heard.pump() {
            record(h, t, &mut cues, &mut inputs);
        }
    }
    if realtime {
        let total: u64 = skips.iter().filter_map(|s| s["skippedMs"].as_u64()).sum();
        eprintln!("[offline] realtime: {} skips, {:.1} s of audio dropped to stay live", skips.len(), total as f64 / 1000.0);
    }
    let t = (fed * 1000 / SAMPLE_RATE_HZ) as u64;
    if let Some(h) = heard.finish() {
        record(h, t, &mut cues, &mut inputs);
    }
    let _ = ffmpeg.wait();

    let out = json!({
        "source": name,
        "kind": "offline",
        "offline": true,
        "model": model.file_name().map(|f| f.to_string_lossy().into_owned()),
        "decoding": { "beam": decoding.beam, "hotwordsScore": decoding.hotwords_score },
        "cues": cues,
        "inputs": inputs,
        "stable": [],
        "unstable": [],
        "skips": skips,
        "interpretation": marks.into_inner(),
    });
    let dir = root().join("parity/offline");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_string(&out).unwrap()).unwrap();
    let secs = t as f64 / 1000.0;
    eprintln!(
        "[offline] {name}: {:.0} min of audio in {:.0}s ({:.0}× real time), {} utterances → {}",
        secs / 60.0,
        started.elapsed().as_secs_f64(),
        secs / started.elapsed().as_secs_f64(),
        out["cues"].as_array().map_or(0, |c| c.len()),
        path.display()
    );
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    n - n % 2
}
