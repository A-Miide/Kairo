//! Translating slide text that isn't scripture, on this machine: one small
//! translation model per language, downloaded the first time that language
//! is chosen (a French-only church never downloads Portuguese), then fully
//! offline. Scripture never comes here — verses use the Bibles.
//!
//! French and Spanish are Opus-MT (Marian) models, ~107 MB each. Portuguese
//! is NLLB-200 (~890 MB): there's no small, trustworthy en→pt conversion,
//! and NLLB takes its target language as a token rather than a text prefix.
//! Both are encoder–decoder models run the same way: encode a sentence once,
//! then pick the likeliest next piece until the end of the sentence, the
//! decoder carrying what it has worked out forward (its "merged" export
//! takes its own state back) rather than re-reading the translation so far.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::{json, Value};
use tokenizers::Tokenizer;

use crate::installer::{fetch_verified, ndjson_stream, Progress};
use crate::Shared;

struct File {
    /// In the model's repository.
    remote: &'static str,
    /// In the language's folder here.
    local: &'static str,
    size: u64,
    sha256: &'static str,
}

#[derive(Clone, Copy)]
enum Family {
    /// Starts from the padding token, which it must never produce.
    Marian,
    /// Starts from end-of-sentence, then the target language's token.
    Nllb { target: &'static str },
}

struct Model {
    lang: &'static str,
    repo: &'static str,
    /// Pinned: what's downloaded can't change under the checksums.
    revision: &'static str,
    dir: &'static str,
    family: Family,
    files: [File; 4],
}

const fn file(remote: &'static str, local: &'static str, size: u64, sha256: &'static str) -> File {
    File { remote, local, size, sha256 }
}

const MODELS: [Model; 3] = [
    Model {
        lang: "fr",
        repo: "Xenova/opus-mt-en-fr",
        revision: "28726206f80896b90035bd99cccd5cc1e151f916",
        dir: "opus-mt-en-fr",
        family: Family::Marian,
        files: [
            file("config.json", "config.json", 1411, "b522b73fcc86f77981349c1df2a2e041eed0dbcbea4acf2635019cf21d51ffc0"),
            file("tokenizer.json", "tokenizer.json", 5637839, "8391785c1a2139e7af4678571ccd8dc654ecbb72e4be186940f65d7c604f0246"),
            file("onnx/encoder_model_quantized.onnx", "encoder.onnx", 50090398, "0a81bdba62f53223740a8c6c6f58e716eba8ea1f92dfa569caf27ed5e3b6a0f7"),
            file("onnx/decoder_model_merged_quantized.onnx", "decoder.onnx", 57381512, "333b244bce16023df04541c8cf9fd60aec9b0569da393c4b831d561897b0bda8"),
        ],
    },
    Model {
        lang: "es",
        repo: "Xenova/opus-mt-en-es",
        revision: "4b002a4c7edd54a7ced58877258b87f7efd3f892",
        dir: "opus-mt-en-es",
        family: Family::Marian,
        files: [
            file("config.json", "config.json", 1468, "499eae1f97eeba63172fc884f92fd727ae803b7ceb79c0f653aa4b011152af2f"),
            file("tokenizer.json", "tokenizer.json", 6262682, "285eb29e7155ee48851a77960797813f86a125f70d2c1a124f613f1fbd2b19c3"),
            file("onnx/encoder_model_quantized.onnx", "encoder.onnx", 52899742, "03434edd1147e6c49e22066e9dc5888235e99cc35a6711d1c4ea5b06455d5da9"),
            file("onnx/decoder_model_merged_quantized.onnx", "decoder.onnx", 60212804, "58ee5ddd6e22d1693d8722b90c1486afb93700a4376cf28fd74175052ad530ae"),
        ],
    },
    Model {
        lang: "pt",
        repo: "Xenova/nllb-200-distilled-600M",
        revision: "261c31d1a5732c67cdd16d80e8d6088507c7ccea",
        dir: "nllb-200-distilled-600M",
        family: Family::Nllb { target: "por_Latn" },
        files: [
            file("config.json", "config.json", 873, "52f035acb54ac80e5ef7fe78d6967f8ddf8e8799f078d1a92a2c8168d8ff4a20"),
            file("tokenizer.json", "tokenizer.json", 17331224, "8ac789ad7dabea44d41537822d48c516ba358374c51813e2cba78c006e150c94"),
            file("onnx/encoder_model_quantized.onnx", "encoder.onnx", 419120483, "5cde664eacba07a62f198857ec6c06e09572b1ebb77c8137f1fa99ac604a3a28"),
            file("onnx/decoder_model_merged_quantized.onnx", "decoder.onnx", 475505771, "dd66608c2a4194e78f95548fa0e64f24302303698c5b09fa8e1f9e16ec00676b"),
        ],
    },
];

/// A translation this long (in the model's pieces) is cut short: a slide
/// line never needs it, and a model repeating itself stops here.
const MAX_PIECES: usize = 256;
const THREADS: usize = 2;

fn model(lang: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.lang == lang)
}

fn approx_mb(m: &Model) -> u64 {
    m.files.iter().map(|f| f.size).sum::<u64>().div_ceil(1 << 20)
}

fn dir_of(app: &Shared, m: &Model) -> PathBuf {
    app.cfg.models_dir.join("mt").join(m.dir)
}

/// Every file there, at its full size (each was checked when it arrived).
fn present(dir: &Path, m: &Model) -> bool {
    m.files.iter().all(|f| std::fs::metadata(dir.join(f.local)).is_ok_and(|md| md.len() == f.size))
}

pub fn installed(app: &Shared, lang: &str) -> bool {
    model(lang).is_some_and(|m| present(&dir_of(app, m), m))
}

static INSTALLING: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn installing(lang: &str) -> bool {
    INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&lang)
}

/// GET /api/translate-model/status[?lang=]
pub async fn status_route(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let one = |m: &Model| json!({ "installed": present(&dir_of(&app, m), m), "installing": installing(m.lang), "approxMB": approx_mb(m) });
    if let Some(lang) = q.get("lang") {
        let Some(m) = model(lang) else { return crate::util::bad_request(format!("Unknown language \"{lang}\"")) };
        let mut v = one(m);
        v["modelPath"] = json!(dir_of(&app, m));
        return Json(v).into_response();
    }
    Json(Value::Object(MODELS.iter().map(|m| (m.lang.to_string(), one(m))).collect())).into_response()
}

/// POST /api/translate-model/install {lang} — progress as NDJSON.
pub async fn install_route(State(app): State<Shared>, Json(b): Json<Value>) -> Response {
    let Some(m) = b["lang"].as_str().and_then(model) else {
        return crate::util::bad_request(format!("Unknown language \"{}\"", b["lang"].as_str().unwrap_or("")));
    };
    {
        let mut busy = INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if busy.contains(&m.lang) {
            return (StatusCode::CONFLICT, Json(json!({ "error": "install already in progress" }))).into_response();
        }
        busy.push(m.lang);
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let result = install(&app, m, &tx).await;
        let _ = tx.send(match result {
            Ok(()) => json!({ "phase": "complete", "ok": true }),
            Err(e) => json!({ "phase": "complete", "ok": false, "error": e }),
        });
        INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|l| *l != m.lang);
    });
    ndjson_stream(rx)
}

/// POST /api/translate-model/remove {lang} — frees its disk space; the
/// language downloads again if chosen later.
pub async fn remove_route(State(app): State<Shared>, Json(b): Json<Value>) -> Response {
    let Some(m) = b["lang"].as_str().and_then(model) else {
        return crate::util::bad_request("Unknown language");
    };
    if installing(m.lang) {
        return (StatusCode::CONFLICT, Json(json!({ "error": "install in progress" }))).into_response();
    }
    app.translator.forget_model(m.lang);
    match tokio::fs::remove_dir_all(dir_of(&app, m)).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Json(json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn install(app: &Shared, m: &'static Model, progress: &Progress) -> Result<(), String> {
    let dir = dir_of(app, m);
    if present(&dir, m) {
        let _ = progress.send(json!({ "phase": "done", "already": true, "modelPath": dir }));
        return Ok(());
    }
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let all: u64 = m.files.iter().map(|f| f.size).sum();
    let mut before = 0u64;
    let _ = progress.send(json!({ "phase": "download", "pct": 0 }));
    for f in &m.files {
        let dest = dir.join(f.local);
        if std::fs::metadata(&dest).is_ok_and(|md| md.len() == f.size) {
            before += f.size;
            continue;
        }
        // One percentage across all the files.
        let report = crate::installer::percent_reporter(progress.clone());
        let on = move |received: u64, _total: u64| report(before + received, all);
        let url = format!("https://huggingface.co/{}/resolve/{}/{}", m.repo, m.revision, f.remote);
        fetch_verified(&client, &url, &dest, f.size, f.sha256, &on, progress).await?;
        before += f.size;
    }
    let _ = progress.send(json!({ "phase": "done", "already": false, "modelPath": dir }));
    Ok(())
}

/// A language's model, loaded.
pub struct Loaded {
    lang: &'static str,
    tokenizer: Tokenizer,
    encoder: Session,
    decoder: Session,
    start: i64,
    forced: Option<i64>,
    eos: i64,
    /// Never produced (Marian's padding token).
    banned: Option<usize>,
    /// The decoder's state inputs ("past_key_values.0.decoder.key", …).
    state: Vec<String>,
    heads: usize,
    head_dim: usize,
}

/// A tensor's shape and values, kept between decoder steps.
struct Held {
    shape: Vec<usize>,
    data: Vec<f32>,
}

/// The tokenizer as published, less an empty "Precompiled" normalizer
/// (Opus-MT's files carry one with no character map, which the tokenizers
/// crate can't read and transformers.js skips).
fn tokenizer(path: &Path) -> Result<Tokenizer, String> {
    let mut t: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if t["normalizer"]["type"] == "Precompiled" && t["normalizer"]["precompiled_charsmap"].is_null() {
        t["normalizer"] = Value::Null;
    }
    let bytes = serde_json::to_vec(&t).map_err(|e| e.to_string())?;
    std::panic::catch_unwind(|| Tokenizer::from_bytes(&bytes)).map_err(|_| "the tokenizer couldn't be read".to_string())?.map_err(|e| e.to_string())
}

fn session(path: &Path) -> Result<Session, String> {
    Session::builder()
        .and_then(|b| Ok(b.with_intra_threads(THREADS)?))
        .and_then(|b| Ok(b.with_inter_threads(1)?))
        .and_then(|mut b| b.commit_from_file(path))
        .map_err(|e| format!("translation model: {e}"))
}

impl Loaded {
    pub fn load(app: &Shared, lang: &str) -> Result<Loaded, String> {
        let m = model(lang).ok_or_else(|| format!("Unsupported language \"{lang}\""))?;
        let dir = dir_of(app, m);
        if !present(&dir, m) {
            return Err("The translation model for this language isn't downloaded".into());
        }
        let config: Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let tokenizer = tokenizer(&dir.join("tokenizer.json"))?;
        let id = |k: &str| config[k].as_i64();
        let (start, forced, banned) = match m.family {
            Family::Marian => (id("decoder_start_token_id").unwrap_or(0), None, id("pad_token_id").map(|p| p as usize)),
            Family::Nllb { target } => (id("decoder_start_token_id").unwrap_or(2), tokenizer.token_to_id(target).map(i64::from), None),
        };
        let decoder = session(&dir.join("decoder.onnx"))?;
        let state = decoder.inputs().iter().map(|i| i.name().to_string()).filter(|n| n.starts_with("past_key_values.")).collect();
        let heads = id("decoder_attention_heads").unwrap_or(8).max(1) as usize;
        let head_dim = id("d_model").unwrap_or(512) as usize / heads;
        Ok(Loaded {
            lang: m.lang,
            tokenizer,
            encoder: session(&dir.join("encoder.onnx"))?,
            decoder,
            start,
            forced,
            eos: id("eos_token_id").unwrap_or(0),
            banned,
            state,
            heads,
            head_dim,
        })
    }

    pub fn lang(&self) -> &str {
        self.lang
    }

    /// Text, line by line (a slide keeps its line breaks), sentence by
    /// sentence (NLLB stops at the end of the first).
    pub fn translate(&mut self, text: &str) -> Result<String, String> {
        let mut lines = Vec::new();
        for line in text.lines() {
            let mut out = Vec::new();
            for s in sentences(line) {
                out.push(self.sentence(s)?);
            }
            lines.push(out.join(" "));
        }
        Ok(lines.join("\n"))
    }

    fn sentence(&mut self, text: &str) -> Result<String, String> {
        let err = |e: ort::Error| e.to_string();
        let enc = self.tokenizer.encode(text, true).map_err(|e| e.to_string())?;
        let ids: Vec<i64> = enc.get_ids().iter().map(|&i| i64::from(i)).collect();
        let n = ids.len();
        let mask = vec![1i64; n];
        let outputs = self
            .encoder
            .run(vec![
                ("input_ids".to_string(), Tensor::from_array(([1, n], ids)).map_err(err)?.into_dyn()),
                ("attention_mask".to_string(), Tensor::from_array(([1, n], mask.clone())).map_err(err)?.into_dyn()),
            ])
            .map_err(err)?;
        let (shape, hidden) = outputs["last_hidden_state"].try_extract_tensor::<f32>().map_err(err)?;
        let width = shape[2] as usize;
        let hidden = hidden.to_vec();
        drop(outputs);

        let mut out: Vec<i64> = vec![self.start];
        out.extend(self.forced);
        let given = out.len();
        let limit = MAX_PIECES.min(n * 3 + 16);
        // Nothing worked out yet: every state input empty along its sequence.
        let mut state: Vec<Held> = self.state.iter().map(|_| Held { shape: vec![1, self.heads, 0, self.head_dim], data: Vec::new() }).collect();
        let mut fed = 0;
        while out.len() - given < limit {
            let new: Vec<i64> = out[fed..].to_vec();
            let mut feed: Vec<(String, ort::value::DynValue)> = vec![
                ("input_ids".to_string(), Tensor::from_array(([1, new.len()], new.clone())).map_err(err)?.into_dyn()),
                ("encoder_attention_mask".to_string(), Tensor::from_array(([1, n], mask.clone())).map_err(err)?.into_dyn()),
                ("encoder_hidden_states".to_string(), Tensor::from_array(([1, n, width], hidden.clone())).map_err(err)?.into_dyn()),
                ("use_cache_branch".to_string(), Tensor::from_array(([1], vec![fed > 0])).map_err(err)?.into_dyn()),
            ];
            for (name, held) in self.state.iter().zip(&state) {
                feed.push((name.clone(), Tensor::from_array((held.shape.clone(), held.data.clone())).map_err(err)?.into_dyn()));
            }
            let outputs = self.decoder.run(feed).map_err(err)?;
            let (shape, logits) = outputs["logits"].try_extract_tensor::<f32>().map_err(err)?;
            let vocab = shape[2] as usize;
            let rows = shape[1] as usize;
            let last = &logits[(rows - 1) * vocab..rows * vocab];
            let next = last
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != self.banned)
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(self.eos, |(i, _)| i as i64);
            // The state, carried forward ("present.0.decoder.key" for
            // "past_key_values.0.decoder.key"; the encoder's stays as it was).
            for (name, held) in self.state.iter().zip(state.iter_mut()) {
                if let Some(v) = outputs.get(format!("present{}", &name["past_key_values".len()..])) {
                    let (shape, data) = v.try_extract_tensor::<f32>().map_err(err)?;
                    if shape.iter().product::<i64>() > 0 || fed == 0 {
                        *held = Held { shape: shape.iter().map(|&d| d as usize).collect(), data: data.to_vec() };
                    }
                }
            }
            fed = out.len();
            if next == self.eos {
                break;
            }
            out.push(next);
        }
        let pieces: Vec<u32> = out[given..].iter().map(|&i| i as u32).collect();
        self.tokenizer.decode(&pieces, true).map(|s| s.trim().to_string()).map_err(|e| e.to_string())
    }
}

/// A line's sentences, each with its own closing punctuation.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let ends = matches!(c, '.' | '!' | '?') && chars.get(k + 1).is_some_and(|&(_, n)| n.is_whitespace());
        if ends {
            let s = line[start..i + c.len_utf8()].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = i + c.len_utf8();
        }
    }
    let rest = line[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_is_pinned_and_checked() {
        for m in &MODELS {
            assert_eq!(m.revision.len(), 40, "{}", m.lang);
            for f in &m.files {
                assert_eq!(f.sha256.len(), 64, "{} {}", m.lang, f.remote);
                assert!(f.size > 0);
            }
        }
        assert_eq!(approx_mb(model("fr").unwrap()), 108);
        assert!(model("de").is_none());
    }

    #[test]
    fn lines_split_into_sentences() {
        assert_eq!(sentences("Welcome to church. Please sit down!"), ["Welcome to church.", "Please sit down!"]);
        assert_eq!(sentences("Service at 6.30pm today"), ["Service at 6.30pm today"]);
        assert!(sentences("   ").is_empty());
    }

    #[test]
    fn an_empty_precompiled_normalizer_is_dropped() {
        let dir = std::env::temp_dir().join(format!("kairo-mt-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tokenizer.json");
        let vocab: Vec<Value> = ["</s>", "<unk>", "\u{2581}hi"].iter().map(|p| json!([p, 0.0])).collect();
        let t = json!({
            "version": "1.0", "truncation": null, "padding": null, "added_tokens": [],
            "normalizer": { "type": "Precompiled", "precompiled_charsmap": null },
            "pre_tokenizer": { "type": "Metaspace", "replacement": "\u{2581}", "prepend_scheme": "always", "split": true },
            "post_processor": null, "decoder": null,
            "model": { "type": "Unigram", "unk_id": 1, "vocab": vocab, "byte_fallback": false }
        });
        std::fs::write(&path, serde_json::to_vec(&t).unwrap()).unwrap();
        let tok = tokenizer(&path).unwrap();
        assert_eq!(tok.encode("hi", false).unwrap().get_ids(), [2]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
