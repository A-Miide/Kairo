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
/// How often the recogniser is asked what it heard.
pub const POLL: Duration = Duration::from_millis(120);
pub const SAMPLE_RATE_HZ: usize = SAMPLE_RATE as usize;
/// The newest words kept live; words are locked in runs of at least this many.
const LIVE_TAIL_WORDS: usize = 4;
const MIN_LOCK_WORDS: usize = 3;
const MAX_LIVE_WORDS: usize = LIVE_TAIL_WORDS + 4;
/// The recogniser restarts its context after an utterance this long.
const RESET_AFTER_WORDS: usize = 300;
const UTTERANCE_END_SECS: f32 = 1.2;
/// Live speech stays live: audio still waiting to be decoded when it is older than this is
/// dropped and recognition starts again at the newest audio. On a computer too busy to decode
/// in real time the backlog otherwise grew for the whole service (measured 2026-10-16: 5.4
/// minutes behind after an hour), detection reacting to things said minutes before. The live
/// worker and `offline-transcribe --realtime` both use this rule (see [`too_far_behind`]).
pub const MAX_BEHIND: Duration = Duration::from_secs(4);

/// The catch-up rule: audio that arrived `waited` ago and isn't decoded yet is too old to use.
pub fn too_far_behind(waited: Duration) -> bool {
    waited > MAX_BEHIND
}
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

/// How the recogniser decodes. Beam search can lean towards words a service
/// is likely to hear (the Bible's book names, which the model otherwise
/// mishears: "Proverbate", "someone 25"); greedy is the fastest.
///
/// Beam search for NeMo streaming models arrived in sherpa-onnx after 1.13.8
/// (k2-fsa/sherpa-onnx#3895); 1.13.8 exits the process when asked for it.
/// Until a release carries it the app decodes greedily, and only
/// `offline-transcribe --beam` / `--hotwords` (for measuring it) ask for beam.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoding {
    pub beam: bool,
    /// How strongly the book names are favoured (0: not at all).
    pub hotwords_score: f32,
    /// Taken off the "nothing said" score at every step. The model otherwise
    /// hears a phrase right after a pause as silence ("Romans five five" |
    /// "…shed abroad in our heart" lost); at 1.0 drops of 4+ words fell from
    /// 39 to 23 on two sermons, wrong sends 4 -> 1, at no extra CPU.
    pub blank_penalty: f32,
}

/// Measured on the eval sermons (`offline-transcribe --blank-penalty`).
pub const BLANK_PENALTY: f32 = 1.0;

impl Default for Decoding {
    fn default() -> Decoding {
        Decoding { beam: false, hotwords_score: 0.0, blank_penalty: BLANK_PENALTY }
    }
}

/// Book names that aren't everyday words — favouring "Acts" or "Numbers"
/// would put them where nobody said them.
const EVERYDAY_WORDS: &[&str] = &["Acts", "Numbers", "Judges", "Kings", "Job", "Mark", "John", "James", "Jude", "Ruth"];

/// The book names as the model's own word pieces, one per line.
pub fn hotwords(tokens: &std::path::Path) -> String {
    let vocab: std::collections::HashSet<String> = std::fs::read_to_string(tokens)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.rsplit_once(' ').map(|(t, _)| t.to_string()))
        .collect();
    let mut names: Vec<String> = Vec::new();
    for b in kairo_bible::Book::all() {
        let name = b.name().trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ').to_string();
        if EVERYDAY_WORDS.contains(&name.as_str()) || names.contains(&name) {
            continue;
        }
        if name == "Psalms" {
            names.push("Psalm".into());
        }
        names.push(name);
    }
    names.iter().filter_map(|n| pieces(n, &vocab)).map(|p| p.join(" ")).collect::<Vec<_>>().join("\n")
}

/// A phrase in the vocabulary's pieces, longest first ("▁Jer" "em" "iah").
fn pieces(phrase: &str, vocab: &std::collections::HashSet<String>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for word in phrase.split_whitespace() {
        let chars: Vec<char> = format!("\u{2581}{word}").chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let piece = (i + 1..=chars.len()).rev().map(|j| chars[i..j].iter().collect::<String>()).find(|p| vocab.contains(p))?;
            i += piece.chars().count();
            out.push(piece);
        }
    }
    Some(out)
}

/// One loaded recogniser, shared by sessions, released a while after the last.
struct Loaded {
    dir: std::path::PathBuf,
    decoding: Decoding,
    recognizer: Arc<OnlineRecognizer>,
}
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn recognizer(dir: &std::path::Path, decoding: &Decoding) -> Result<Arc<OnlineRecognizer>, String> {
    let mut l = LOADED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(r) = l.as_ref().filter(|r| r.dir == dir && r.decoding == *decoding) {
        return Ok(r.recognizer.clone());
    }
    *l = None;
    let p = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
    // Two at most: measured on a 3-minute sermon under a full service's load (2026-10-16), 2
    // threads decoded fastest; 4 took longer and twice the CPU (the runtime's idle threads
    // spin), which is what a small laptop can't spare. KAIRO_ASR_THREADS overrides it.
    let threads = std::env::var("KAIRO_ASR_THREADS").ok().and_then(|v| v.parse::<i32>().ok()).filter(|n| (1..=8).contains(n))
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(2).clamp(1, 2) as i32);
    let mut cfg = OnlineRecognizerConfig {
        model_config: OnlineModelConfig {
            transducer: OnlineTransducerModelConfig { encoder: p("encoder.int8.onnx"), decoder: p("decoder.int8.onnx"), joiner: p("joiner.int8.onnx") },
            tokens: p("tokens.txt"),
            num_threads: threads,
            provider: Some("cpu".into()),
            ..Default::default()
        },
        decoding_method: Some(if decoding.beam { "modified_beam_search" } else { "greedy_search" }.into()),
        max_active_paths: 4,
        enable_endpoint: true,
        rule1_min_trailing_silence: 2.4,
        rule2_min_trailing_silence: UTTERANCE_END_SECS,
        rule3_min_utterance_length: 1e6,
        blank_penalty: decoding.blank_penalty,
        ..Default::default()
    };
    if decoding.beam && decoding.hotwords_score > 0.0 {
        cfg.hotwords_buf = Some(hotwords(&dir.join("tokens.txt")).into_bytes());
        cfg.hotwords_score = decoding.hotwords_score;
    }
    cfg.feat_config.sample_rate = SAMPLE_RATE;
    cfg.feat_config.feature_dim = FEATURE_DIM;
    let r = Arc::new(OnlineRecognizer::create(&cfg).ok_or("the offline model could not be loaded")?);
    *l = Some(Loaded { dir: dir.to_path_buf(), decoding: decoding.clone(), recognizer: r.clone() });
    Ok(r)
}


/// What the recogniser has heard, as the detection funnel takes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    /// The words still live (the recogniser may revise them).
    Interim(String),
    /// Words locked in while the sentence runs on.
    Locked(String),
    /// The rest of an utterance, ended by a pause.
    Final(String),
}

/// One recognition stream with the locking rules: audio in, [`Heard`] out.
/// The app's offline sessions and the evaluation tools both use it.
pub struct Transcriber {
    rec: Arc<OnlineRecognizer>,
    stream: sherpa_onnx::OnlineStream,
    language: Option<String>,
    locked: usize,
    last_partial: String,
    /// Restart the recogniser's context at a pause once this many words are in it.
    pub reset_after: usize,
}

impl Transcriber {
    pub fn new(rec: Arc<OnlineRecognizer>, language: Option<String>) -> Transcriber {
        let stream = rec.create_stream();
        let t = Transcriber { rec, stream, language, locked: 0, last_partial: String::new(), reset_after: RESET_AFTER_WORDS };
        t.pin();
        t
    }

    fn pin(&self) {
        if let Some(l) = &self.language {
            self.stream.set_option("language", l);
        }
    }

    /// 16 kHz mono samples in [-1, 1].
    pub fn accept(&self, samples: &[f32]) {
        self.stream.accept_waveform(SAMPLE_RATE, samples);
    }

    fn words(&self) -> Vec<String> {
        let text = self.rec.get_result(&self.stream).map(|r| r.text).unwrap_or_default();
        LANGUAGE_TAG.replace_all(&text, "").split_whitespace().map(str::to_string).collect()
    }

    fn decode(&self) {
        while self.rec.is_ready(&self.stream) {
            self.rec.decode(&self.stream);
        }
    }

    /// Decodes what has arrived and says what changed.
    pub fn pump(&mut self) -> Vec<Heard> {
        self.decode();
        let words = self.words();
        self.locked = self.locked.min(words.len());
        let mut out = Vec::new();
        if self.rec.is_endpoint(&self.stream) {
            let rest = words[self.locked..].join(" ");
            if !rest.is_empty() {
                out.push(Heard::Final(rest));
            }
            self.last_partial.clear();
            self.locked = words.len();
            if words.len() >= self.reset_after {
                self.rec.reset(&self.stream);
                self.pin();
                self.locked = 0;
            }
            return out;
        }
        // Lock in all but the newest few words — and not a number its own
        // sound may still be growing.
        let mut up_to = words.len().saturating_sub(LIVE_TAIL_WORDS);
        while up_to > self.locked && words.len() - up_to < MAX_LIVE_WORDS && NUMBER_WORD.is_match(&words[up_to - 1]) {
            up_to -= 1;
        }
        if up_to >= self.locked + MIN_LOCK_WORDS {
            out.push(Heard::Locked(words[self.locked..up_to].join(" ")));
            self.locked = up_to;
        }
        let live = words[self.locked..].join(" ");
        if !live.is_empty() && live != self.last_partial {
            self.last_partial = live.clone();
            out.push(Heard::Interim(live));
        }
        out
    }

    /// Recognition fell behind and the waiting audio was dropped: what was heard so far ends
    /// as a final, and the recogniser starts fresh at the next audio (no half-sentence glued
    /// across the gap).
    pub fn cut(&mut self) -> Option<Heard> {
        self.decode();
        let words = self.words();
        let rest = words.get(self.locked.min(words.len())..).unwrap_or(&[]).join(" ");
        self.rec.reset(&self.stream);
        self.pin();
        self.locked = 0;
        self.last_partial.clear();
        (!rest.is_empty()).then_some(Heard::Final(rest))
    }

    /// The audio has ended: whatever is left, as a final.
    pub fn finish(&mut self) -> Option<Heard> {
        self.stream.input_finished();
        self.decode();
        let words = self.words();
        let rest = words.get(self.locked.min(words.len())..).unwrap_or(&[]).join(" ");
        (!rest.is_empty()).then_some(Heard::Final(rest))
    }
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
    detector.send(kairo_detect::engine::Event::Reset);
    detector.configure(|c| c.offline = true);
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(1024);
    *app.audio.lock().await = Some(tx);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    // The engine runs in its own process (worker_main); this thread starts
    // it, feeds it and restarts it if it dies. Started: Ok once the model is
    // loaded and listening, or why it couldn't.
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
    let app2 = app.clone();
    let worker = std::thread::Builder::new()
        .name("offline-speech".into())
        .spawn(move || supervise(app2, detector, dir, language, rx, flag, started_tx))
        .map_err(|e| json!({ "error": e.to_string() }))?;
    let started = tokio::task::spawn_blocking(move || started_rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| Err("the engine didn't start in time".into())))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    if let Err(e) = started {
        stop.store(true, Ordering::SeqCst);
        let msg = format!("Offline engine failed to start: {e}");
        state(&app, "error", Some(("unknown", msg.clone())));
        return Err(json!({ "error": msg }));
    }
    state(&app, "connected", None);
    eprintln!("[Offline] Listening ({model} model).");
    Ok(Session { stop, done: Some(worker) })
}

// ── The speech engine in its own process ─────────────────────────────────
// sherpa-onnx is C++: an error inside it can't be caught here, and it used
// to end the whole app mid-service (live, 2026-10-14). So the engine runs as
// a second copy of this program (`<exe> --kairo-speech-worker <model dir>
// <language|->`): audio in on stdin (u32 length + 16 kHz 16-bit PCM, a zero
// length to finish), what it heard out on stdout ("K1 " + JSON per line). If
// it dies, it's restarted; the audio meanwhile waits in the session's queue
// (~65 s) and is fed in once it's back, so nothing is lost and the screens
// never notice. Three deaths in a minute: it stops and says so.

/// Run as the first thing in `main`: if this process was started as the
/// speech engine, runs it and returns the exit code.
pub fn worker_main() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(WORKER_FLAG) {
        return None;
    }
    let dir = std::path::PathBuf::from(args.get(2)?);
    let language = args.get(3).filter(|l| l.as_str() != "-").cloned();
    Some(run_worker(&dir, language))
}

pub const WORKER_FLAG: &str = "--kairo-speech-worker";
const RESTARTS_PER_MINUTE: usize = 3;
/// Audio already sent that a restarted engine hears again first: the words
/// the old one hadn't settled, with lead-in for it to find its feet (8 s of
/// 64 ms frames).
const REPLAY_FRAMES: usize = 125;
/// Settled words remembered, so a restarted engine doesn't repeat them.
const REMEMBERED_WORDS: usize = 40;

fn plain(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// `text` without its opening words that are the end of what was already
/// settled (`said`): a restarted engine re-hears the last few seconds.
fn without_repeat(said: &std::collections::VecDeque<String>, text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let max = words.len().min(said.len());
    let k = (1..=max).rev().find(|&k| said.iter().skip(said.len() - k).zip(&words[..k]).all(|(a, b)| *a == plain(b))).unwrap_or(0);
    words[k..].join(" ")
}

fn run_worker(dir: &std::path::Path, language: Option<String>) -> i32 {
    use std::io::{Read, Write};
    let rec = match recognizer(dir, &Decoding::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[Offline] {e}");
            return 2;
        }
    };
    let out = std::io::stdout();
    let say = |line: String| {
        let mut o = out.lock();
        let _ = writeln!(o, "K1 {line}");
        let _ = o.flush();
    };
    say("ready".into());
    // Each chunk with when it arrived (for the catch-up rule). `gone`: the server closed our
    // input (stopped listening, or the server itself went): no one is listening any more, so
    // nothing still queued is decoded (it used to decode minutes of backlog for no one).
    let (tx, rx) = std::sync::mpsc::channel::<Option<(Instant, Vec<u8>)>>();
    let gone = Arc::new(AtomicBool::new(false));
    let gone_reader = gone.clone();
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut len = [0u8; 4];
            if input.read_exact(&mut len).is_err() {
                gone_reader.store(true, Ordering::SeqCst);
                let _ = tx.send(None);
                return;
            }
            let n = u32::from_le_bytes(len) as usize;
            let mut buf = vec![0u8; n];
            if n == 0 || input.read_exact(&mut buf).is_err() {
                // A zero length is the server's "finish": the last words still matter.
                if n != 0 { gone_reader.store(true, Ordering::SeqCst); }
                let _ = tx.send(None);
                return;
            }
            if tx.send(Some((Instant::now(), buf))).is_err() {
                return;
            }
        }
    });
    let mut heard = Transcriber::new(rec, language);
    let emit = |h: Heard| {
        let (k, t) = match h {
            Heard::Interim(t) => ("i", t),
            Heard::Locked(t) => ("l", t),
            Heard::Final(t) => ("f", t),
        };
        say(json!({ "k": k, "t": t }).to_string());
    };
    let mut last_pump = Instant::now();
    loop {
        if gone.load(Ordering::SeqCst) {
            return 0;
        }
        match rx.recv_timeout(Duration::from_millis(15)) {
            Ok(Some((arrived, chunk))) => {
                if too_far_behind(arrived.elapsed()) {
                    // Behind live speech: drop what is waiting, say so, start fresh at the newest.
                    let mut dropped = chunk.len() / 2;
                    let mut ended = false;
                    while let Ok(next) = rx.try_recv() {
                        match next {
                            Some((_, c)) => dropped += c.len() / 2,
                            None => { ended = true; break; }
                        }
                    }
                    if let Some(h) = heard.cut() {
                        emit(h);
                    }
                    say(json!({ "k": "s", "ms": dropped * 1000 / SAMPLE_RATE_HZ }).to_string());
                    if ended {
                        return 0;
                    }
                    continue;
                }
                let samples: Vec<f32> = chunk.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
                heard.accept(&samples);
            }
            Ok(None) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(h) = heard.finish() {
                    emit(h);
                }
                return 0;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if last_pump.elapsed() >= POLL {
            last_pump = Instant::now();
            for h in heard.pump() {
                emit(h);
            }
        }
    }
}

enum FromWorker {
    Ready,
    Heard(Heard),
    /// Recognition had fallen behind: this much waiting audio was dropped to stay live.
    Skipped(u64),
    Gone,
}

/// One engine process, and a thread reading what it says.
fn spawn_worker(dir: &std::path::Path, language: &Option<String>) -> Result<(std::process::Child, std::sync::mpsc::Receiver<FromWorker>), String> {
    use std::io::BufRead;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(WORKER_FLAG).arg(dir).arg(language.as_deref().unwrap_or("-"));
    cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start the speech engine: {e}"))?;
    let stdout = child.stdout.take().ok_or("the speech engine has no output")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Some(body) = line.strip_prefix("K1 ") else { continue };
            let msg = if body == "ready" {
                FromWorker::Ready
            } else {
                let Ok(v) = serde_json::from_str::<Value>(body) else { continue };
                let t = v["t"].as_str().unwrap_or("").to_string();
                if v["k"] == "s" {
                    if tx.send(FromWorker::Skipped(v["ms"].as_u64().unwrap_or(0))).is_err() { return; }
                    continue;
                }
                FromWorker::Heard(match v["k"].as_str() {
                    Some("f") => Heard::Final(t),
                    Some("l") => Heard::Locked(t),
                    _ => Heard::Interim(t),
                })
            };
            if tx.send(msg).is_err() {
                return;
            }
        }
        let _ = tx.send(FromWorker::Gone);
    });
    Ok((child, rx))
}

fn supervise(
    app: Shared,
    detector: crate::detect::Detector,
    dir: std::path::PathBuf,
    language: Option<String>,
    mut audio: tokio::sync::mpsc::Receiver<Bytes>,
    stopping: Arc<AtomicBool>,
    started: std::sync::mpsc::SyncSender<Result<(), String>>,
) {
    use std::io::Write;
    let hub = app.hub.clone();
    // Settled words so far (for a restarted engine's overlap), and whether
    // the engine was just restarted and may still be repeating them.
    let mut said: std::collections::VecDeque<String> = Default::default();
    let catching_up = std::cell::Cell::new(false);
    let mut tell = |h: Heard| {
        crate::audio_stats::STATS.engine_results.fetch_add(1, Ordering::Relaxed);
        let h = if catching_up.get() {
            let strip = |t: &str| without_repeat(&said, t);
            match h {
                Heard::Interim(t) => Heard::Interim(strip(&t)),
                Heard::Locked(t) => Heard::Locked(strip(&t)),
                Heard::Final(t) => Heard::Final(strip(&t)),
            }
        } else {
            h
        };
        if let Heard::Locked(t) | Heard::Final(t) = &h {
            if t.is_empty() {
                return;
            }
            catching_up.set(false);
            said.extend(t.split_whitespace().map(plain));
            while said.len() > REMEMBERED_WORDS {
                said.pop_front();
            }
        }
        if let Heard::Interim(t) = &h {
            if t.is_empty() {
                return;
            }
        }
        match h {
            Heard::Final(text) => detector.speech(&hub, &text, true, 0.9, true, None, false),
            Heard::Locked(text) => detector.speech(&hub, &text, true, 0.9, false, None, true),
            Heard::Interim(text) => detector.speech(&hub, &text, false, 0.85, false, None, false),
        }
    };
    let mut started = Some(started);
    let mut deaths: std::collections::VecDeque<Instant> = Default::default();
    let mut recent: std::collections::VecDeque<Bytes> = Default::default();
    loop {
        // Start (or restart) the engine and wait for its model to load. The
        // audio queue isn't read meanwhile: it holds what's said.
        let ready = spawn_worker(&dir, &language).and_then(|(mut child, from)| {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                match from.recv_timeout(Duration::from_millis(100)) {
                    Ok(FromWorker::Ready) => return Ok((child, from)),
                    Ok(FromWorker::Gone) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        let _ = child.wait();
                        return Err("the offline model could not be loaded".to_string());
                    }
                    _ if stopping.load(Ordering::SeqCst) || Instant::now() > deadline => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("the engine didn't start in time".to_string());
                    }
                    _ => {}
                }
            }
        });
        let (mut child, from) = match ready {
            Ok(w) => w,
            Err(e) => {
                if let Some(s) = started.take() {
                    let _ = s.send(Err(e));
                    return;
                }
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                eprintln!("[Offline] Speech engine couldn't restart: {e}");
                state(&app, "error", Some(("unknown", format!("The offline speech engine stopped and couldn't restart: {e}"))));
                return;
            }
        };
        if let Some(s) = started.take() {
            let _ = s.send(Ok(()));
        } else {
            state(&app, "connected", None);
            eprintln!("[Offline] Speech engine back — catching up on the audio held meanwhile");
        }
        let mut input = child.stdin.take();
        let mut dead = false;
        // After a restart: the last seconds again first (see REPLAY_FRAMES).
        if !recent.is_empty() {
            catching_up.set(true);
            let w = input.as_mut().expect("engine input");
            for chunk in &recent {
                if w.write_all(&(chunk.len() as u32).to_le_bytes()).and_then(|_| w.write_all(chunk)).is_err() {
                    dead = true;
                    break;
                }
            }
        }
        loop {
            while let Ok(m) = from.try_recv() {
                match m {
                    FromWorker::Heard(h) => tell(h),
                    FromWorker::Skipped(ms) => {
                        // Said, in the log and to the operator: this computer can't keep up.
                        crate::routes::write_debug_global("server:speech-behind", json!({ "skippedMs": ms }));
                        hub.broadcast(json!({ "type": "speech-behind", "skippedMs": ms }));
                    }
                    FromWorker::Gone => dead = true,
                    FromWorker::Ready => {}
                }
            }
            if dead {
                break;
            }
            if stopping.load(Ordering::SeqCst) {
                // Finish: the last words, then the engine ends itself.
                if let Some(mut w) = input.take() {
                    let _ = w.write_all(&0u32.to_le_bytes());
                }
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline {
                    match from.recv_timeout(Duration::from_millis(50)) {
                        Ok(FromWorker::Heard(h)) => tell(h),
                        Ok(FromWorker::Gone) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        _ => {}
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            let mut got = false;
            while let Ok(chunk) = audio.try_recv() {
                got = true;
                let w = input.as_mut().expect("engine input");
                if w.write_all(&(chunk.len() as u32).to_le_bytes()).and_then(|_| w.write_all(&chunk)).is_err() {
                    dead = true;
                    break;
                }
                crate::audio_stats::STATS.engine_frames.fetch_add(1, Ordering::Relaxed);
                recent.push_back(chunk);
                if recent.len() > REPLAY_FRAMES {
                    recent.pop_front();
                }
            }
            if dead {
                break;
            }
            if !got {
                std::thread::sleep(Duration::from_millis(15));
            }
        }
        // It died. Restart it — unless it keeps dying.
        let status = child.wait().map(|s| s.to_string()).unwrap_or_default();
        let now = Instant::now();
        deaths.push_back(now);
        while deaths.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
            deaths.pop_front();
        }
        crate::routes::write_debug_line(&app, "server:speech-engine", json!({ "stopped": status, "recentStops": deaths.len() }));
        if deaths.len() >= RESTARTS_PER_MINUTE {
            eprintln!("[Offline] Speech engine stopped {} times in a minute ({status}) — giving up", deaths.len());
            state(&app, "error", Some(("unknown", "The offline speech engine keeps stopping. Stop and start listening again; if it keeps happening, switch the Speech Engine in Settings → Audio.".into())));
            return;
        }
        eprintln!("[Offline] Speech engine stopped ({status}) — restarting; what's said meanwhile is kept");
        state(&app, "reconnecting", None);
    }
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
    fn live_speech_stays_live() {
        use std::time::Duration;
        assert!(!super::too_far_behind(Duration::from_millis(500)), "a normal wait is decoded");
        assert!(!super::too_far_behind(super::MAX_BEHIND), "exactly at the limit is still decoded");
        assert!(super::too_far_behind(super::MAX_BEHIND + Duration::from_millis(1)), "older than the limit is dropped");
    }

    #[test]
    fn a_restarted_engine_does_not_repeat_settled_words() {
        let said: std::collections::VecDeque<String> = ["through", "this", "testimony"].iter().map(|w| w.to_string()).collect();
        assert_eq!(super::without_repeat(&said, "this testimony, Ephesians two six came"), "Ephesians two six came");
        assert_eq!(super::without_repeat(&said, "Ephesians two six"), "Ephesians two six");
        assert_eq!(super::without_repeat(&said, "Testimony."), "");
    }

    #[test]
    fn book_names_in_the_models_pieces() {
        let vocab: std::collections::HashSet<String> = ["\u{2581}J", "er", "em", "iah", "\u{2581}Song", "\u{2581}of", "\u{2581}Sol", "omon", "e", "r", "m", "i", "a", "h"].iter().map(|s| s.to_string()).collect();
        assert_eq!(super::pieces("Jeremiah", &vocab).unwrap(), ["\u{2581}J", "er", "em", "iah"]);
        assert_eq!(super::pieces("Song of Solomon", &vocab).unwrap().len(), 4);
        assert!(super::pieces("Zz", &vocab).is_none());
    }

    #[test]
    fn languages_choose_the_model() {
        assert_eq!(super::model_for("en-US").0, "en");
        assert_eq!(super::model_for("ja"), ("multi", Some("ja-JP".to_string())));
    }
}
