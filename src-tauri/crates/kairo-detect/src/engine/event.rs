//! What goes into the engine and what comes out.

use kairo_bible::VerseId;

use super::types::{Method, Target};

/// One recognised stretch of speech.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    /// Settled (true) or still growing (an interim).
    pub is_final: bool,
    /// The engine heard the end of an utterance (endpointing).
    pub speech_final: bool,
    /// The live text continues straight on from this final (an engine that
    /// locks words a few at a time while the sentence runs on).
    pub continues_live: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Speech from the primary recogniser.
    Speech(Segment),
    /// A final from an extra, time-offset recogniser stream (citation voting).
    SecondarySpeech { text: String, stream: u8 },
    /// The operator put a verse up (Live Queue send, search).
    OperatorSent(VerseId),
    /// A song, announcement or other non-scripture slide went live.
    NonScriptureLive,
    /// The screen was cleared or stepped back.
    ScreenCleared,
    RangeNext,
    RangeJump(VerseId),
    RangeClear,
    /// A timer the engine asked for has fired.
    Timer(TimerId),
    /// A model request has been answered.
    Answer(Answer),
    /// A new service: forget everything about the last one.
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimerId {
    /// A chapter said twice: the hold for the words read after it is over.
    RestatedChapter(u32),
    /// The sentence buffer has gone quiet.
    SentenceBuffer,
}

/// What the server shows or tells the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Show {
        verses: Vec<VerseId>,
        method: Method,
        score: f32,
        target: Target,
        /// The verse this replaces as a correction, if it is one.
        corrected_from: Option<VerseId>,
    },
    /// A range was set (all its verses) or moved (the current one).
    RangeSet { verses: Vec<VerseId>, current: VerseId },
    RangeMoved { current: VerseId, remaining: usize },
    RangeCleared,
    ReadingMode { active: bool, chapter: Option<VerseId> },
    TopicLibrary { size: usize, words: Vec<String> },
    /// Ask to be woken with `Event::Timer(id)` at `at_ms`.
    WakeAt { id: TimerId, at_ms: u64 },
}

/// Model-backed work the runtime does off the detection thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Meaning search over several windows of recent speech.
    Paraphrase { id: u32, windows: Vec<String>, limit: usize },
    /// Meaning search restricted to these verses.
    Scoped { id: u32, text: String, verses: Vec<VerseId>, limit: usize },
    /// Meaning and cross-encoder scores of a few verses against speech.
    Compare { id: u32, texts: Vec<String>, verses: Vec<VerseId> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Paraphrase { id: u32, candidates: Vec<MeaningHit> },
    Scoped { id: u32, hits: Vec<(VerseId, f32)> },
    Compare { id: u32, scores: Vec<(Option<f32>, Option<f32>)> },
}

/// A verse found by meaning, with the evidence the paraphrase decision weighs.
#[derive(Clone, Debug, PartialEq)]
pub struct MeaningHit {
    pub verse: VerseId,
    /// Best cosine over the windows.
    pub cos: f32,
    /// Which window found it.
    pub window: usize,
    /// Cross-encoder against the KJV and against a modern wording.
    pub rerank: Option<f32>,
    pub rerank_modern: Option<f32>,
}

/// Everything one event produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub effects: Vec<Effect>,
    pub requests: Vec<Request>,
    /// Why each decision went where it did (when `Config::explain` is on).
    pub notes: Vec<String>,
}
