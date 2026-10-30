//! A service's state, grouped by what it's about. Time always comes from the
//! event being handled.

use std::collections::{HashMap, HashSet, VecDeque};

use kairo_bible::{Book, VerseId};

use super::types::Method;
use super::words::WordBuffer;

/// How long finished speech is kept.
const TRANSCRIPT_KEEP_MS: u64 = 90_000;
/// The sentence buffer flushes after this long without a new final.
pub const SENTENCE_TIMEOUT_MS: u64 = 4_000;

#[derive(Clone, Debug)]
pub struct Final {
    pub text: String,
    pub at: u64,
    pub words: usize,
    pub has_ref: bool,
}

/// Finished segments grouped into sentences before the slower text searches
/// run on them: flushed at sentence-ending punctuation, at the end of an
/// utterance, or after a quiet spell.
#[derive(Clone, Debug, Default)]
pub struct SentenceBuffer {
    buf: String,
    last_append: u64,
}

impl SentenceBuffer {
    pub fn append(&mut self, text: &str, now: u64) -> Option<String> {
        if text.is_empty() {
            return None;
        }
        if !self.buf.is_empty() {
            self.buf.push(' ');
        }
        self.buf.push_str(text);
        self.last_append = now;
        let t = self.buf.trim_end();
        if t.ends_with(['.', '!', '?']) { self.flush() } else { None }
    }

    pub fn timed_out(&mut self, now: u64) -> Option<String> {
        if self.buf.is_empty() || self.last_append == 0 || now.saturating_sub(self.last_append) < SENTENCE_TIMEOUT_MS {
            return None;
        }
        self.flush()
    }

    pub fn flush(&mut self) -> Option<String> {
        let t = std::mem::take(&mut self.buf).trim().to_string();
        self.last_append = 0;
        (!t.is_empty()).then_some(t)
    }

    pub fn has_content(&self) -> bool {
        !self.buf.is_empty()
    }

    pub fn last_append(&self) -> u64 {
        self.last_append
    }
}

#[derive(Clone, Debug, Default)]
pub struct Transcript {
    /// Every word heard, however the recogniser cut it into segments.
    pub words: WordBuffer,
    pub finals: VecDeque<Final>,
    /// Final words heard this service — corrections are bounded by words said, not time.
    pub words_heard: u64,
    /// The interim in progress.
    pub interim: String,
    /// Words of the stretch in progress the engine locked itself (Engine::on_speech): a recogniser that
    /// only ends an utterance at a pause sends one ever-growing interim, and this keeps all but its
    /// newest words moving through everything that works on finished words.
    pub self_locked: Vec<String>,
    pub sentence: SentenceBuffer,
    /// Citation key → when an interim last parsed it (interim stability).
    pub interim_seen: HashMap<String, u64>,
    /// Verses already sent from this utterance's interims.
    pub interim_sent: HashSet<VerseId>,
    pub stream_watermark: usize,
    /// When the utterance in progress began (its first interim).
    pub utterance_start: Option<u64>,
    /// When the segment being handled began to be heard: a pause is measured
    /// from the previous final to here, not to this segment's end.
    pub segment_started: u64,
    pub last_interim_verbatim: u64,
    pub last_fingerprint: u64,
}

impl Transcript {
    pub fn push_final(&mut self, text: &str, now: u64) {
        let words = text.split_whitespace().count();
        self.finals.push_back(Final { text: text.to_string(), at: now, words, has_ref: false });
        while self.finals.front().is_some_and(|f| f.at + TRANSCRIPT_KEEP_MS < now) {
            self.finals.pop_front();
        }
    }

    /// The last `n` words of finished speech.
    pub fn last_words(&self, n: usize) -> String {
        let all: Vec<&str> = self.finals.iter().flat_map(|f| f.text.split_whitespace()).collect();
        all[all.len().saturating_sub(n)..].join(" ")
    }

    /// The newest finals: at least `finals` of them and at least `words`
    /// words — a Deepgram sentence, or the several short runs the offline
    /// engine locks in while the same sentence is spoken.
    pub fn recent_finals(&self, finals: usize, words: usize) -> String {
        let (mut taken, mut count) = (Vec::new(), 0);
        for f in self.finals.iter().rev() {
            if taken.len() >= finals && count >= words {
                break;
            }
            count += f.words;
            taken.push(f.text.as_str());
        }
        taken.reverse();
        taken.join(" ")
    }

    /// Finished speech within the last `ms`.
    pub fn since(&self, now: u64, ms: u64) -> Vec<&Final> {
        self.finals.iter().filter(|f| now.saturating_sub(f.at) < ms).collect()
    }

    /// Words per minute, as a factor stretching windows that wait for the
    /// preacher's next words: 1 at ≥ 120 wpm, up to 2.
    pub fn pace_factor(&self) -> f64 {
        if self.finals.len() < 3 {
            return 1.0;
        }
        let minutes = (self.finals.back().unwrap().at - self.finals.front().unwrap().at) as f64 / 60_000.0;
        if minutes < 0.75 {
            return 1.0;
        }
        let words: usize = self.finals.iter().map(|f| f.words).sum();
        (120.0 / (words as f64 / minutes).max(1.0)).clamp(1.0, 2.0)
    }
}

/// A citation corrected away, so a re-parse of it can't put it back while
/// the correction is up.
#[derive(Clone, Copy, Debug)]
pub struct CorrectedAway {
    pub verse: VerseId,
    pub replaced_by: VerseId,
    pub at: u64,
}

/// What's on screen and how it got there.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// The verse up, and when it went up.
    pub up: Option<(VerseId, u64)>,
    /// A short range sent whole, up together: reading through it keeps it up whole (owner,
    /// 2026-10-30: "it shows both verses called"), not one verse at a time.
    pub together: Vec<VerseId>,
    /// Verses shown while the current book has been up.
    pub shown_this_book: HashSet<VerseId>,
    pub last_sent_book: Option<(Book, u64)>,
    pub last_sent: Option<(VerseId, u64)>,
    /// The last decision that targeted the screen (even if de-duplicated).
    pub last_detected: Option<(VerseId, u64, Method)>,
    /// Verse → when it last targeted the screen.
    pub recent_detections: HashMap<VerseId, u64>,
    /// The last spoken citation sent, when, and the words heard at that moment.
    pub last_citation: Option<(VerseId, u64, u64)>,
    pub corrected_away: Option<CorrectedAway>,
    /// A song or other non-scripture slide is live.
    pub non_scripture_live: bool,
    /// People are singing (heard, not a slide): scripture in the lyrics is only offered.
    pub singing: bool,
    /// Verse → when it was last offered.
    pub recent_suggestions: HashMap<VerseId, u64>,
    /// When a spoken citation last went through (suppresses redundant suggestions).
    pub last_citation_at: u64,
}

impl Screen {
    pub fn up_verse(&self) -> Option<VerseId> {
        self.up.map(|(v, _)| v)
    }
}

/// A cited passage of several verses.
#[derive(Clone, Debug, Default)]
pub struct Range {
    pub all: Vec<VerseId>,
    pub queue: VecDeque<VerseId>,
    pub current: Option<VerseId>,
    pub left: Option<VerseId>,
    pub moved_at: u64,
    pub last_advance_at: u64,
    /// Speech since the range last moved: finished segments, the open one.
    pub heard: VecDeque<String>,
    pub heard_open: String,
    /// Words of the open segment already said when it moved.
    pub spent: usize,
    /// A single verse cited, with the next verses held in case they are read on (owner, 2026-10-19):
    /// moved only on strong evidence, shown nowhere as a range, and let go after a while.
    pub soft: bool,
    /// Held after a verse found in the words (a quote), not one cited: moving on takes more of
    /// the next verse's opening (ranges.rs advance_range_on_next_opening).
    pub soft_quoted: bool,
}

/// A passage cited or read recently (the sermon's context).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cited {
    pub book: Book,
    pub chapter: Option<u16>,
    pub verse: Option<u16>,
    pub at: u64,
    /// Named (true) rather than matched from what was read.
    pub named: bool,
}
