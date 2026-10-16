//! Does the interpreter ever hold back a preacher? Runs the app's interpreter over a recording's
//! audio, with the text timeline of an existing offline transcript (`offline-transcribe`), without
//! decoding the audio again (fast). On a service with no interpreter there must be no marks.
//!
//! `interp-check <audio> <transcript name> [lang]` — reads `parity/offline/<name>.json`.

use std::io::Read;
use std::process::{Command, Stdio};

use kairo_eval::root;
use kairo_server::interpretation::{model_path, Change, Interpreter, Verdict};
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(audio), Some(name)) = (args.first(), args.get(1)) else {
        eprintln!("usage: interp-check <audio> <transcript name> [lang]");
        std::process::exit(2);
    };
    let t: Value = serde_json::from_str(&std::fs::read_to_string(root().join(format!("parity/offline/{name}.json"))).unwrap()).unwrap();
    let inputs: Vec<(u64, String, bool)> = t["inputs"].as_array().unwrap().iter().map(|i| (i["t"].as_u64().or(i["audioMs"].as_u64()).unwrap_or(0), i["text"].as_str().unwrap_or("").to_string(), i["isFinal"].as_bool().unwrap_or(false))).collect();
    let mut it = Interpreter::load(&model_path());
    let k = std::sync::Arc::new(kairo_eval::knowledge());
    it.set_known(std::sync::Arc::new(move |w: &str| k.lexicon.vocab().id(w).is_some()));
    it.reset(args.get(2).map_or("en-US", String::as_str));
    let mut ff = Command::new("ffmpeg").args(["-v", "error", "-i", audio, "-ac", "1", "-ar", "16000", "-f", "s16le", "-"]).stdout(Stdio::piped()).spawn().expect("ffmpeg");
    let mut pcm = ff.stdout.take().unwrap();
    let step = 1024usize;
    let mut buf = vec![0u8; step * 2];
    let (mut next, mut started, mut held, mut spans) = (0usize, 0u64, 0usize, Vec::<(u64, u64)>::new());
    loop {
        let mut n = 0;
        while n < buf.len() {
            match pcm.read(&mut buf[n..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => n += k,
            }
        }
        n -= n % 2;
        if n == 0 {
            break;
        }
        let s: Vec<f32> = buf[..n].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let change = it.audio(&s);
        let now = it.clock_ms();
        match change {
            Some(Change::Started) => started = now,
            Some(Change::Ended) => spans.push((started, now)),
            None => {}
        }
        while next < inputs.len() && inputs[next].0 <= now {
            let (v, c) = it.text(&inputs[next].1, inputs[next].2);
            if v == Verdict::Interpretation {
                held += 1;
            }
            // Kept aside as possibly the preacher's and handed back: not lost.
            held -= it.take_released().len().min(held);
            match c {
                Some(Change::Ended) => spans.push((started, now)),
                _ => {}
            }
            next += 1;
        }
    }
    if it.is_active() {
        spans.push((started, it.clock_ms()));
    }
    let hours = it.clock_ms() as f64 / 3_600_000.0;
    println!("{name}: {:.2} h, {} marked turns, {} pieces of text lost", hours, spans.len(), held);
    for (a, b) in spans {
        println!("   {:>6.1} min – {:>6.1} min", a as f64 / 60000.0, b as f64 / 60000.0);
    }
}
