//! Every line of every transcript we have that the spoken-translation grammar would act on, so a
//! false switch (it changes every verse after it) can be read by hand.
//!
//!   cargo run --release -p kairo-eval --bin translation-scan

use kairo_detect::reference::translation_command::translation_command;
use kairo_eval::root;
use serde_json::Value;

fn main() {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for dir in ["eval/fixtures", "parity/offline", "parity/golden"] {
        if let Ok(rd) = std::fs::read_dir(root().join(dir)) {
            files.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")));
        }
    }
    files.sort();
    let (mut lines, mut hits) = (0, 0);
    for f in files {
        let Ok(v) = serde_json::from_slice::<Value>(&std::fs::read(&f).unwrap_or_default()) else { continue };
        let cues = v["transcript"].as_array().or_else(|| v["cues"].as_array()).cloned().unwrap_or_default();
        let texts: Vec<String> = cues.iter().filter_map(|c| c["text"].as_str().map(str::to_string)).collect();
        // Each line with the one before it, as the detector sees a joined pair.
        for i in 0..texts.len() {
            lines += 1;
            let pair = if i > 0 { format!("{} {}", texts[i - 1], texts[i]) } else { texts[i].clone() };
            if let Some(code) = translation_command(&texts[i]).or_else(|| translation_command(&pair)) {
                hits += 1;
                println!("{code:5} {} | {}", f.file_name().unwrap().to_string_lossy(), pair.chars().take(160).collect::<String>());
            }
        }
    }
    eprintln!("{hits} of {lines} lines would switch");
}
