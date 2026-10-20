//! Runs the YAMNet sound classifier over a recording and prints, for each
//! 0.48 s frame, how much it hears speech, singing and music — for checking
//! whether singing can be told from preaching before it is wired into the app.
//!
//!   cargo run --release -p kairo-eval --bin music-check -- <yamnet.onnx> <audio> > frames.csv
//!
//! Needs `ffmpeg` on the PATH to decode the audio to 16 kHz mono.

use std::io::Read;
use std::process::{Command, Stdio};

use ort::session::Session;
use ort::value::Tensor;

const RATE: usize = 16_000;
/// Audio given to the model at once (the frames it yields are 0.48 s apart).
const CHUNK_SECS: usize = 30;
const HOP: usize = 7_680;
const SPEECH: usize = 0;
const SINGING: &[usize] = &[24, 25, 29, 30]; // Singing, Choir, Child singing, Synthetic singing
const MUSIC: usize = 132;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (model, audio) = (&args[0], &args[1]);
    let mut session = Session::builder().and_then(|mut b| b.commit_from_file(model)).expect("load YAMNet");
    let mut ff = Command::new("ffmpeg")
        .args(["-v", "error", "-i", audio, "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("ffmpeg");
    let mut out = ff.stdout.take().unwrap();
    let mut bytes = Vec::new();
    out.read_to_end(&mut bytes).expect("decode");
    let samples: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    println!("t,speech,singing,music");
    let chunk = CHUNK_SECS * RATE;
    let mut start = 0;
    while start < samples.len() {
        let end = (start + chunk).min(samples.len());
        let wave = samples[start..end].to_vec();
        let n = wave.len();
        let outputs = session.run(vec![("waveform".to_string(), Tensor::from_array(([n], wave)).unwrap().into_dyn())]).expect("run");
        let (shape, scores) = outputs[0].try_extract_tensor::<f32>().unwrap();
        let (frames, classes) = (shape[0] as usize, shape[1] as usize);
        for f in 0..frames {
            let row = &scores[f * classes..(f + 1) * classes];
            let sing = SINGING.iter().map(|c| row[*c]).fold(0.0, f32::max);
            let t = (start + f * HOP) as f64 / RATE as f64;
            println!("{t:.2},{:.3},{sing:.3},{:.3}", row[SPEECH], row[MUSIC]);
        }
        start += chunk;
    }
    let _ = ff.wait();
}
