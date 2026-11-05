//! An interpreter in the room.
//!
//! When a message is interpreted, the interpreter speaks in turn with the preacher, in another
//! language. The speech model listens in the preacher's language only, so the interpretation
//! comes out as nothing, or a few garbled fragments ("is ta combosotros"). It must never
//! reach the detectors: it would feed them noise, and it never advances anything. Nor is it
//! transcribed: the transcript shows "(interpretation)" and the preacher's language is picked
//! straight back up.
//!
//! How it is told apart: the audio is a voice (Silero VAD — it tells a voice from applause and
//! noise) for several seconds while hardly any *real word* of the preacher's language was
//! recognised in them. Real preaching, even a slow prayer, yields real words all along; the
//! fragments made of another language are mostly not words. It ends the moment a sentence of
//! real words arrives again (or after a long silence). While a song slide is live it stays out
//! of the way (singing, worship in another language).
//!
//! One clock: the audio's own (samples heard), so the same code runs live and over a recording
//! (the evaluation tools drive it exactly as the app does).

use std::path::Path;

/// Voice in the last [`WINDOW_S`] seconds needed before it can be an interpreter.
const START_VOICED_MS: u64 = 8_000;
/// The window the voice and the real words are counted over.
const WINDOW_S: usize = 12;
/// Real words recognised per second of voice below which the voice is another language. Slow
/// preaching and prayer run at 0.7 or more; the fragments of another language, 0.2 or less.
const MAX_WORDS_PER_S: f64 = 0.35;
/// A sentence of the preacher's language (to end an interpretation): this many words, this share real.
const SENTENCE_WORDS: usize = 4;
const SENTENCE_REAL: f64 = 0.7;
/// The preacher must have been heard this recently for a voiced stretch to be an interpreter
/// answering (not the start of a service, not a prayer in another language).
const PREACHER_RECENT_MS: u64 = 60_000;
/// Silence that ends an interpretation on its own.
const END_SILENCE_MS: u64 = 8_000;
/// After the speech engine dropped audio to catch up, the voice it never heard as words is not
/// another language: no turn starts for this long (live, 2026-10-30: four false turns, one 24 s).
const AFTER_DROP_MS: u64 = 15_000;
const SAMPLE_RATE: i32 = 16_000;

/// The commonest function words of each service language: a sentence of the preacher's language
/// has several; the fragments the model makes of another language almost never do.
fn function_words(lang: &str) -> &'static [&'static str] {
    match lang.get(..2).unwrap_or("en") {
        "es" => &["el", "la", "los", "las", "de", "que", "y", "en", "un", "una", "es", "por", "con", "para", "su", "se", "lo", "al", "del", "como", "pero", "mi", "tu", "dios", "señor"],
        "fr" => &["le", "la", "les", "de", "des", "du", "et", "en", "un", "une", "est", "que", "qui", "pour", "dans", "ce", "il", "je", "vous", "nous", "pas", "dieu", "seigneur"],
        "pt" => &["o", "a", "os", "as", "de", "do", "da", "que", "e", "em", "um", "uma", "é", "por", "com", "para", "se", "não", "mas", "meu", "seu", "deus", "senhor"],
        "de" => &["der", "die", "das", "und", "ist", "zu", "den", "dem", "ein", "eine", "nicht", "mit", "von", "ich", "du", "sie", "wir", "er", "es", "gott", "herr"],
        _ => &["the", "and", "of", "to", "in", "that", "you", "i", "is", "it", "for", "we", "he", "my", "your", "are", "was", "with", "this", "not", "but", "have", "what", "so", "they", "all", "will", "god", "lord", "be", "a", "on", "me", "his"],
    }
}

/// Whether a word is a real word of the service's language.
pub type Known = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub struct Interpreter {
    vad: Option<sherpa_onnx::VoiceActivityDetector>,
    known: Option<Known>,
    clock_ms: u64,
    /// The last [`WINDOW_S`] seconds, a bucket each: ms of voice, real words recognised.
    voiced: [u64; WINDOW_S],
    words: [u64; WINDOW_S],
    /// The second (of the audio clock) the buckets are at.
    second: u64,
    /// Real words in the text last seen (an utterance grows; a final one ends it).
    last_real: usize,
    silent_ms: u64,
    /// When a sentence of the preacher's language was last recognised (audio clock).
    sentence_at: Option<u64>,
    active: bool,
    song_live: bool,
    /// Text heard during a turn that reads as the preacher's real words (a crowd's "Hallelujah"
    /// is no interpreter): kept, and handed back if the turn ends on his sentence.
    held: Vec<(String, bool)>,
    released: Vec<(String, bool)>,
    /// The service's spoken language (settings: sttLanguage, "en-US").
    lang: String,
    /// No turn starts before this (audio clock): the speech engine dropped audio just before.
    quiet_until: u64,
}

/// What a stretch of audio changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Started,
    Ended,
}

/// What to do with a piece of recognised text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The preacher's: on to the detectors.
    Preacher,
    /// The interpreter's: not transcribed, not detected.
    Interpretation,
}

impl Interpreter {
    /// With the speech detector (without it nothing is ever called interpretation).
    pub fn load(vad_model: &Path) -> Interpreter {
        Interpreter {
            vad: make_vad(vad_model),
            known: None,
            clock_ms: 0,
            voiced: [0; WINDOW_S],
            words: [0; WINDOW_S],
            second: 0,
            last_real: 0,
            silent_ms: 0,
            sentence_at: None,
            active: false,
            song_live: false,
            held: Vec::new(),
            released: Vec::new(),
            lang: "en".into(),
            quiet_until: 0,
        }
    }

    /// The speech engine dropped audio to catch up with live speech: that voice was never heard as
    /// words, so it says nothing about another language. Forget it, end a turn it may have started,
    /// and start none for a while.
    pub fn speech_dropped(&mut self) -> Option<Change> {
        self.voiced = [0; WINDOW_S];
        self.words = [0; WINDOW_S];
        self.quiet_until = self.clock_ms + AFTER_DROP_MS;
        self.held.clear();
        self.end()
    }

    /// The words the service's language has (the Bible's vocabulary). Without it nothing is
    /// ever called interpretation.
    pub fn set_known(&mut self, known: Known) {
        self.known = Some(known);
    }

    /// A new listening session, in the service's spoken language.
    pub fn reset(&mut self, lang: &str) {
        let (vad, known, song_live) = (self.vad.take(), self.known.take(), self.song_live);
        *self = Interpreter { vad, known, song_live, lang: lang.to_lowercase(), ..Interpreter::load(Path::new("")) };
        if let Some(v) = &self.vad {
            v.reset();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The audio clock, in ms (samples heard).
    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }

    /// A song slide is live (or not): singing in another language is not an interpreter.
    pub fn set_song_live(&mut self, live: bool) -> Option<Change> {
        self.song_live = live;
        if live { self.end() } else { None }
    }

    /// Moves the buckets to the audio clock's second (the ones passed are emptied).
    fn tick(&mut self) {
        let now = self.clock_ms / 1000;
        while self.second < now {
            self.second += 1;
            let i = self.second as usize % WINDOW_S;
            self.voiced[i] = 0;
            self.words[i] = 0;
        }
    }

    /// Real words recognised per second of voice over the window, and the voice in it (ms).
    fn window(&self) -> (f64, u64) {
        let voiced: u64 = self.voiced.iter().sum();
        let words: u64 = self.words.iter().sum();
        (words as f64 / (voiced as f64 / 1000.0).max(1.0), voiced)
    }

    /// 16 kHz mono samples in [-1, 1], in order.
    pub fn audio(&mut self, samples: &[f32]) -> Option<Change> {
        let ms = (samples.len() as u64 * 1000) / SAMPLE_RATE as u64;
        self.clock_ms += ms;
        self.tick();
        let voiced = match self.vad.as_ref() {
            Some(v) => {
                v.accept_waveform(samples);
                let d = v.detected();
                while !v.is_empty() {
                    v.pop();
                }
                d
            }
            None => return None,
        };
        if voiced {
            self.voiced[self.second as usize % WINDOW_S] += ms;
            self.silent_ms = 0;
        } else {
            self.silent_ms += ms;
        }
        if self.active {
            return if self.silent_ms >= END_SILENCE_MS { self.end() } else { None };
        }
        let (rate, voiced_ms) = self.window();
        let preacher_recent = self.sentence_at.is_some_and(|t| self.clock_ms.saturating_sub(t) <= PREACHER_RECENT_MS);
        if self.known.is_some() && !self.song_live && preacher_recent && self.clock_ms >= self.quiet_until && voiced_ms >= START_VOICED_MS && rate < MAX_WORDS_PER_S {
            self.active = true;
            return Some(Change::Started);
        }
        None
    }

    /// Recognised text (interim or final) — the preacher's, or the interpreter's.
    pub fn text(&mut self, text: &str, is_final: bool) -> (Verdict, Option<Change>) {
        let Some(known) = self.known.clone() else { return (Verdict::Preacher, None) };
        let words: Vec<String> = text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase()).filter(|w| !w.is_empty()).collect();
        let common = function_words(&self.lang);
        let real = words.iter().filter(|w| known(w) || common.contains(&w.as_str())).count();
        let sentence = words.len() >= SENTENCE_WORDS && real as f64 >= SENTENCE_REAL * words.len() as f64;
        if self.active {
            // The preacher is back: a whole sentence of his language, not a fragment.
            if sentence {
                self.last_real = if is_final { 0 } else { real };
                self.sentence_at = Some(self.clock_ms);
                let held = std::mem::take(&mut self.held);
                let change = self.end();
                self.released = held;
                return (Verdict::Preacher, change);
            }
            if !words.is_empty() && real as f64 >= SENTENCE_REAL * words.len() as f64 && self.held.len() < 40 {
                self.held.push((text.to_string(), is_final));
            }
            return (Verdict::Interpretation, None);
        }
        self.tick();
        // The real words new in this text (an utterance grows; a shorter text is a new one).
        let fresh = if real >= self.last_real { real - self.last_real } else { real };
        self.last_real = if is_final { 0 } else { real };
        self.words[self.second as usize % WINDOW_S] += fresh as u64;
        if sentence {
            self.sentence_at = Some(self.clock_ms);
        }
        (Verdict::Preacher, None)
    }

    /// What was kept aside during a turn that turned out to be the preacher after all (in order).
    pub fn take_released(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.released)
    }

    fn end(&mut self) -> Option<Change> {
        if !self.active {
            return None;
        }
        self.active = false;
        self.held.clear();
        self.voiced = [0; WINDOW_S];
        self.words = [0; WINDOW_S];
        Some(Change::Ended)
    }
}

fn make_vad(model: &Path) -> Option<sherpa_onnx::VoiceActivityDetector> {
    if !model.is_file() {
        return None;
    }
    let mut cfg = sherpa_onnx::VadModelConfig::default();
    cfg.silero_vad.model = Some(model.to_string_lossy().into_owned());
    cfg.silero_vad.threshold = 0.5;
    cfg.silero_vad.min_silence_duration = 0.8;
    cfg.silero_vad.min_speech_duration = 0.15;
    cfg.silero_vad.window_size = 512;
    cfg.silero_vad.max_speech_duration = 3600.0;
    cfg.sample_rate = SAMPLE_RATE;
    cfg.num_threads = 1;
    sherpa_onnx::VoiceActivityDetector::create(&cfg, 30.0)
}

/// Where the speech detector's model is: KAIRO_VAD_MODEL, else databases/vad beside the working directory.
pub fn model_path() -> std::path::PathBuf {
    if let Some(p) = MODEL.get() {
        return p.clone();
    }
    std::env::var_os("KAIRO_VAD_MODEL").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("databases/vad/silero_vad.onnx"))
}

static MODEL: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The model in this install (beside the Bible data), set at start.
pub fn set_model_path(p: std::path::PathBuf) {
    let _ = MODEL.set(p);
}

/// The one interpreter of the running app: audio arrives on one task, text on another.
pub static APP: std::sync::LazyLock<std::sync::Mutex<Interpreter>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(Interpreter::load(&model_path())));

pub fn with<R>(f: impl FnOnce(&mut Interpreter) -> R) -> R {
    f(&mut crate::util::lock(&APP))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard() -> Interpreter {
        let mut i = Interpreter::load(Path::new(""));
        i.set_known(std::sync::Arc::new(|w: &str| matches!(w, "faith" | "grace" | "church" | "obedience" | "blessing" | "jesus")));
        i
    }

    #[test]
    fn nothing_is_held_back_until_an_interpreter_is_told() {
        let mut i = heard();
        assert_eq!(i.text("is ta combosotros", false).0, Verdict::Preacher);
        assert_eq!(i.text("faith and grace of the church", true).0, Verdict::Preacher);
    }

    #[test]
    fn an_interpreters_turn_is_held_back_until_a_sentence_of_real_words() {
        let mut i = heard();
        i.active = true;
        // Fragments of another language: held, not transcribed.
        assert_eq!(i.text("is ta combosotros", false).0, Verdict::Interpretation);
        assert_eq!(i.text("esufficiente paracada uno de nosotros", false).0, Verdict::Interpretation);
        assert!(i.is_active());
        // The preacher's sentence: through at once, and the turn is over.
        let (v, change) = i.text("faith and grace of the church", false);
        assert_eq!(v, Verdict::Preacher);
        assert_eq!(change, Some(Change::Ended));
        assert!(!i.is_active());
    }

    #[test]
    fn audio_the_speech_engine_dropped_is_no_interpreter() {
        let mut i = heard();
        i.active = true;
        // Dropped to catch up: the turn it caused ends, and none starts for a while after.
        assert_eq!(i.speech_dropped(), Some(Change::Ended));
        assert!(!i.is_active());
        assert!(i.quiet_until > i.clock_ms);
    }

    #[test]
    fn a_song_slide_ends_and_prevents_a_turn() {
        let mut i = heard();
        i.active = true;
        assert_eq!(i.set_song_live(true), Some(Change::Ended));
        assert!(!i.is_active());
    }
}
