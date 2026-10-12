//! Offline transcription: NVIDIA Nemotron Speech Streaming through
//! sherpa-onnx, on this machine, into the same detection funnel as Deepgram.
//!
//! Words are locked in a few at a time while the sentence runs on (the
//! newest few stay live, so a recogniser still revising them can), and the
//! utterance ends at a pause.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::{json, Value};
use sherpa_onnx::{OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineTransducerModelConfig};

use crate::Shared;

const SAMPLE_RATE: i32 = 16_000;
const FEATURE_DIM: i32 = 128;
const POLL: Duration = Duration::from_millis(120);
/// The newest words kept live; words are locked in runs of at least this many.
const LIVE_TAIL_WORDS: usize = 4;
const MIN_LOCK_WORDS: usize = 3;
const MAX_LIVE_WORDS: usize = LIVE_TAIL_WORDS + 4;
/// The recogniser restarts its context after an utterance this long.
const RESET_AFTER_WORDS: usize = 300;
/// The loaded model is kept this long after listening stops (a restart is instant).
const RELEASE_AFTER: Duration = Duration::from_secs(120);
const UTTERANCE_END_SECS: f32 = 1.2;
const REQUIRED: [&str; 4] = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];

static NUMBER_WORD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)^(\d+|zero|oh|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|first|second|third|fourth|fifth)$").unwrap()
});
static LANGUAGE_TAG: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\s*<[a-z]{2,3}(?:-[A-Za-z]{2,4})?>").unwrap());

/// The model for a spoken language: English, or the multilingual one with the language pinned.
pub fn model_for(stt_language: &str) -> (&'static str, Option<String>) {
    if stt_language.to_lowercase().starts_with("en") {
        return ("en", None);
    }
    let lang = match stt_language {
        "multi" => "auto",
        "id" => "id-ID",
        "ja" => "ja-JP",
        "zh" => "zh-CN",
        other => other,
    };
    ("multi", Some(lang.to_string()))
}

pub fn model_dir(app: &Shared, model: &str) -> std::path::PathBuf {
    let override_var = if model == "multi" { "KAIRO_OFFLINE_MODEL_MULTI" } else { "KAIRO_OFFLINE_MODEL" };
    if let Some(p) = std::env::var_os(override_var) {
        return p.into();
    }
    app.cfg.models_dir.join(if model == "multi" { "sherpa-streaming-multi" } else { "sherpa-streaming-en" })
}

pub fn present(dir: &std::path::Path) -> bool {
    REQUIRED.iter().all(|f| dir.join(f).exists()) && std::fs::metadata(dir.join("encoder.int8.onnx")).is_ok_and(|m| m.len() > 5_000_000)
}

/// One loaded recogniser, shared by sessions, released a while after the last.
struct Loaded {
    dir: std::path::PathBuf,
    recognizer: Arc<OnlineRecognizer>,
}
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn recognizer(dir: &std::path::Path) -> Result<Arc<OnlineRecognizer>, String> {
    let mut l = LOADED.lock().unwrap();
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(r) = l.as_ref().filter(|r| r.dir == dir) {
        return Ok(r.recognizer.clone());
    }
    *l = None;
    let p = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(2).clamp(1, 4) as i32;
    let mut cfg = OnlineRecognizerConfig {
        model_config: OnlineModelConfig {
            transducer: OnlineTransducerModelConfig { encoder: p("encoder.int8.onnx"), decoder: p("decoder.int8.onnx"), joiner: p("joiner.int8.onnx") },
            tokens: p("tokens.txt"),
            num_threads: threads,
            provider: Some("cpu".into()),
            ..Default::default()
        },
        decoding_method: Some("greedy_search".into()),
        enable_endpoint: true,
        rule1_min_trailing_silence: 2.4,
        rule2_min_trailing_silence: UTTERANCE_END_SECS,
        rule3_min_utterance_length: 1e6,
        ..Default::default()
    };
    cfg.feat_config.sample_rate = SAMPLE_RATE;
    cfg.feat_config.feature_dim = FEATURE_DIM;
    let r = Arc::new(OnlineRecognizer::create(&cfg).ok_or("the offline model could not be loaded")?);
    *l = Some(Loaded { dir: dir.to_path_buf(), recognizer: r.clone() });
    Ok(r)
}

fn release_later() {
    let generation = GENERATION.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(RELEASE_AFTER);
        if GENERATION.load(Ordering::SeqCst) == generation {
            *LOADED.lock().unwrap() = None;
        }
    });
}

pub struct Session {
    stop: Arc<AtomicBool>,
    done: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    pub fn alive(&self) -> bool {
        self.done.as_ref().is_some_and(|h| !h.is_finished())
    }

    pub async fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.done.take() {
            let _ = tokio::task::spawn_blocking(move || h.join()).await;
        }
    }
}

fn state(app: &Shared, state: &str, error: Option<(&str, String)>) {
    let mut m = json!({ "type": "connection-state", "state": state, "engine": "offline" });
    if let Some((kind, msg)) = error {
        m["error"] = json!(msg);
        m["errorKind"] = json!(kind);
    }
    app.hub.broadcast(m);
}

pub async fn start(app: Shared) -> Result<Session, Value> {
    let Some((_, detector)) = app.ready() else { return Err(json!({ "error": "The detection engine is still loading" })) };
    let detector = detector.clone();
    let spoken = app.settings.str("sttLanguage").await.unwrap_or_else(|| "en-US".into());
    let (model, language) = model_for(&spoken);
    let dir = model_dir(&app, model);
    state(&app, "connecting", None);
    if !present(&dir) {
        let msg = if model == "multi" {
            "Offline listening in this language needs the other-languages model. Open Settings → Speech Engine → Offline and hit \"Download other languages\", or set Spoken language to English."
        } else {
            "Offline model missing. Open Settings → Speech Engine → Offline and hit \"Download offline model\"."
        };
        state(&app, "error", Some(("model-missing", msg.into())));
        return Err(json!({ "error": msg }));
    }
    let rec = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || recognizer(&dir)).await.map_err(|e| e.to_string()).and_then(|r| r)
    };
    let rec = match rec {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Offline engine failed to start: {e}");
            state(&app, "error", Some(("unknown", msg.clone())));
            return Err(json!({ "error": msg }));
        }
    };
    detector.send(kairo_detect::engine::Event::Reset);
    detector.configure(|c| c.offline = true);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Bytes>(1024);
    *app.audio.lock().await = Some(tx);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let hub = app.hub.clone();
    let worker = std::thread::Builder::new()
        .name("offline-speech".into())
        .spawn(move || {
            let stream = rec.create_stream();
            let pin = |s: &sherpa_onnx::OnlineStream| {
                if let Some(l) = &language {
                    s.set_option("language", l);
                }
            };
            pin(&stream);
            let mut locked = 0usize;
            let mut last_partial = String::new();
            let mut last_pump = Instant::now();
            let words_of = |rec: &OnlineRecognizer, s: &sherpa_onnx::OnlineStream| -> Vec<String> {
                let text = rec.get_result(s).map(|r| r.text).unwrap_or_default();
                LANGUAGE_TAG.replace_all(&text, "").split_whitespace().map(str::to_string).collect()
            };
            loop {
                let stopping = flag.load(Ordering::SeqCst);
                let mut got = false;
                while let Ok(chunk) = rx.try_recv() {
                    got = true;
                    let samples: Vec<f32> = chunk.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
                    stream.accept_waveform(SAMPLE_RATE, &samples);
                }
                if stopping {
                    stream.input_finished();
                    while rec.is_ready(&stream) {
                        rec.decode(&stream);
                    }
                    let words = words_of(&rec, &stream);
                    let rest = words.get(locked.min(words.len())..).unwrap_or(&[]).join(" ");
                    if !rest.is_empty() {
                        detector.speech(&hub, &rest, true, 0.9, true, None, false);
                    }
                    break;
                }
                if last_pump.elapsed() < POLL {
                    if !got {
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    continue;
                }
                last_pump = Instant::now();
                while rec.is_ready(&stream) {
                    rec.decode(&stream);
                }
                let words = words_of(&rec, &stream);
                locked = locked.min(words.len());
                if rec.is_endpoint(&stream) {
                    let rest = words[locked..].join(" ");
                    if !rest.is_empty() {
                        detector.speech(&hub, &rest, true, 0.9, true, None, false);
                    }
                    last_partial.clear();
                    locked = words.len();
                    if words.len() >= RESET_AFTER_WORDS {
                        rec.reset(&stream);
                        pin(&stream);
                        locked = 0;
                    }
                    continue;
                }
                // Lock in all but the newest few words — and not a number its
                // own sound may still be growing.
                let mut up_to = words.len().saturating_sub(LIVE_TAIL_WORDS);
                while up_to > locked && words.len() - up_to < MAX_LIVE_WORDS && NUMBER_WORD.is_match(&words[up_to - 1]) {
                    up_to -= 1;
                }
                if up_to >= locked + MIN_LOCK_WORDS {
                    detector.speech(&hub, &words[locked..up_to].join(" "), true, 0.9, false, None, true);
                    locked = up_to;
                }
                let live = words[locked..].join(" ");
                if !live.is_empty() && live != last_partial {
                    last_partial = live.clone();
                    detector.speech(&hub, &live, false, 0.85, false, None, false);
                }
            }
            release_later();
        })
        .map_err(|e| json!({ "error": e.to_string() }))?;
    state(&app, "connected", None);
    eprintln!("[Offline] Listening ({model} model).");
    Ok(Session { stop, done: Some(worker) })
}

/// After a session: the audio channel closed, the state told.
pub async fn stopped(app: &Shared) {
    *app.audio.lock().await = None;
    if let Some((_, d)) = app.ready() {
        d.configure(|c| c.offline = false);
    }
    state(app, "disconnected", None);
}

#[cfg(test)]
mod tests {
    #[test]
    fn languages_choose_the_model() {
        assert_eq!(super::model_for("en-US").0, "en");
        assert_eq!(super::model_for("ja"), ("multi", Some("ja-JP".to_string())));
    }
}
