//! Writes the Core ML encoder's program with some of its int8 weights kept int8 on the CPU
//! (kairo-server coreml_encoder.rs, the rewrite the app does where the model is installed), to
//! measure other cut points with offline-transcribe and replay.
//!
//! `cargo run --release -p kairo-eval --bin compact-encoder -- <encoder_int8.mlmodelc/model.mil> <out model.mil> [--only <regex>]`
//! `--only`: the weights whose name matches ("." for all 193 linears); without it, the ones Kairo
//! ships rewritten. Put the output in a copy of the model folder and point KAIRO_OFFLINE_MODEL at it.

use kairo_server::coreml_encoder::{rewrite_with, SHIPPED};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(src), Some(dst)) = (args.first(), args.get(1)) else {
        eprintln!("usage: compact-encoder <model.mil> <out model.mil> [--only <regex>]");
        std::process::exit(2);
    };
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).map_or(SHIPPED, String::as_str);
    let only = regex::Regex::new(only).unwrap_or_else(|e| {
        eprintln!("--only: {e}");
        std::process::exit(2);
    });
    let text = std::fs::read_to_string(src).expect("the encoder's model.mil");
    let out = rewrite_with(&text, &only);
    let kept = out.matches(" = gather(").count();
    if kept == 0 {
        eprintln!("nothing to rewrite: not the encoder's model.mil, or no weight matches");
        std::process::exit(1);
    }
    std::fs::write(dst, out).expect("written");
    eprintln!("{kept} weights kept int8");
}
