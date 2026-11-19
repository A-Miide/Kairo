//! A non-streaming recogniser (Parakeet or Moonshine) over each sentence of a recording, as a
//! second pass would hear it: Silero cuts the sentences, each is transcribed once it ends, and the
//! text lands at the sentence's end on the audio clock. Writes `parity/offline/<name>.json` for
//! `replay --offline <name>`, and prints how much work it took.
//!
//!   cargo run --release -p kairo-eval --bin second-pass -- <audio> <name> --parakeet <dir> | --moonshine <dir>
//!       [--hotwords <file> --hotwords-score <x>] [--threads <n>]

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Instant;

use kairo_eval::root;
use serde_json::{json, Value};
use sherpa_onnx::{OfflineMoonshineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, VadModelConfig, VoiceActivityDetector};

const RATE: i32 = 16_000;
/// A sentence longer than this is cut (a second pass must answer while it still matters).
const MAX_SENTENCE_S: f32 = 15.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let (audio, name) = (&args[0], &args[1]);
    let threads: i32 = opt("--threads").and_then(|s| s.parse().ok()).unwrap_or(2);
    let p = |dir: &str, f: &str| Some(format!("{dir}/{f}"));

    let mut cfg = OfflineRecognizerConfig::default();
    let model = if let Some(dir) = opt("--parakeet") {
        cfg.model_config.transducer = OfflineTransducerModelConfig { encoder: p(&dir, "encoder.int8.onnx"), decoder: p(&dir, "decoder.int8.onnx"), joiner: p(&dir, "joiner.int8.onnx") };
        cfg.model_config.tokens = p(&dir, "tokens.txt");
        cfg.model_config.model_type = Some("nemo_transducer".into());
        "parakeet"
    } else if let Some(dir) = opt("--moonshine") {
        cfg.model_config.moonshine = OfflineMoonshineModelConfig {
            preprocessor: p(&dir, "preprocess.onnx"),
            encoder: p(&dir, "encode.int8.onnx"),
            uncached_decoder: p(&dir, "uncached_decode.int8.onnx"),
            cached_decoder: p(&dir, "cached_decode.int8.onnx"),
            merged_decoder: None,
        };
        cfg.model_config.tokens = p(&dir, "tokens.txt");
        "moonshine"
    } else {
        panic!("--parakeet <dir> or --moonshine <dir>");
    };
    cfg.model_config.num_threads = threads;
    cfg.model_config.provider = Some("cpu".into());
    if let Some(file) = opt("--hotwords") {
        cfg.decoding_method = Some("modified_beam_search".into());
        cfg.max_active_paths = 4;
        cfg.hotwords_file = Some(file);
        cfg.hotwords_score = opt("--hotwords-score").and_then(|s| s.parse().ok()).unwrap_or(1.5);
    }
    let recognizer = OfflineRecognizer::create(&cfg).expect("load the recogniser");

    let mut vcfg = VadModelConfig::default();
    vcfg.silero_vad.model = Some(root().join("databases/vad/silero_vad.onnx").to_string_lossy().into_owned());
    vcfg.silero_vad.threshold = 0.5;
    vcfg.silero_vad.min_silence_duration = 0.5;
    vcfg.silero_vad.min_speech_duration = 0.25;
    vcfg.silero_vad.window_size = 512;
    vcfg.silero_vad.max_speech_duration = MAX_SENTENCE_S;
    vcfg.sample_rate = RATE;
    vcfg.num_threads = 1;
    let vad = VoiceActivityDetector::create(&vcfg, 60.0).expect("load Silero");

    let mut ff = Command::new("ffmpeg").args(["-v", "error", "-i", audio, "-ac", "1", "-ar", "16000", "-f", "f32le", "-"]).stdout(Stdio::piped()).spawn().expect("ffmpeg");
    let mut bytes = Vec::new();
    ff.stdout.take().unwrap().read_to_end(&mut bytes).expect("decode");
    let _ = ff.wait();
    let samples: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();

    let mut cues: Vec<Value> = Vec::new();
    let mut inputs: Vec<Value> = Vec::new();
    let mut work = 0.0f64;
    let mut decode = |seg_start: i32, seg: &[f32], cues: &mut Vec<Value>, inputs: &mut Vec<Value>| {
        let t = Instant::now();
        let stream = recognizer.create_stream();
        stream.accept_waveform(RATE, seg);
        recognizer.decode(&stream);
        let text = stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default();
        work += t.elapsed().as_secs_f64();
        if text.is_empty() {
            return;
        }
        let end_ms = ((seg_start as i64 + seg.len() as i64) * 1000 / RATE as i64) as u64;
        let cue = cues.len();
        cues.push(json!({ "audioMs": end_ms, "text": text }));
        inputs.push(json!({ "audioMs": end_ms, "t": end_ms, "cue": cue, "text": text, "isFinal": true, "speechFinal": true, "continues": false }));
    };
    for chunk in samples.chunks(512) {
        vad.accept_waveform(chunk);
        while let Some(seg) = vad.front() {
            decode(seg.start(), seg.samples(), &mut cues, &mut inputs);
            vad.pop();
        }
    }
    vad.flush();
    while let Some(seg) = vad.front() {
        decode(seg.start(), seg.samples(), &mut cues, &mut inputs);
        vad.pop();
    }
    let secs = samples.len() as f64 / RATE as f64;
    eprintln!("[second-pass] {model}: {:.0} min of audio, {} sentences, {:.0} s of work ({:.1}% of one thread per thread used: {threads})", secs / 60.0, cues.len(), work, work / secs * 100.0);
    let out = json!({ "source": name, "kind": "offline", "offline": true, "model": model, "cues": cues, "inputs": inputs, "stable": [], "unstable": [], "decoding": { "secondPass": true, "hotwords": opt("--hotwords").is_some() } });
    std::fs::write(root().join("parity/offline").join(format!("{name}.json")), serde_json::to_string(&out).unwrap()).expect("write");
}
