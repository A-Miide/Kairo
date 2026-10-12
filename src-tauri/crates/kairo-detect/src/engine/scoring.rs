//! One score per candidate: B (how certain the method's own evidence is, on
//! a common scale) + D (how near it is to what's on screen) + A (independent
//! corroboration), with policy caps — see `detection-spec/01-decision.md`.
//!
//! The core principle: the text being read is ground truth; a spoken citation
//! is a strong hint (0.93), not unoverridable. Evidence strong enough on its
//! own (B ≥ the screen bar) isn't weighed against distance at all — distance
//! can only veto a candidate that needed help to clear the bar.

use std::collections::{HashMap, HashSet};

use kairo_bible::{Book, VerseRef};

use super::types::{Evidence, Method, OnScreen, Target};
use crate::lexical::Confidence;

/// The screen bar.
pub const VIEWER_MIN_SCORE: f32 = 0.80;
/// Below this, not worth showing anywhere.
pub const SUGGESTION_FLOOR: f32 = 0.50;
/// Matched weight that's certain despite partial coverage.
pub const VERBATIM_CERTAIN_IDF: f32 = 18.0;
/// Matched weight worth offering.
pub const VERBATIM_MODERATE_IDF: f32 = 10.0;
/// A stream alignment's confirmation weight.
pub const STREAM_IDF_FULL_CONFIDENCE: f32 = 12.0;
/// How long the verse on screen pulls nearby candidates.
pub const SAME_BOOK_WINDOW_MS: u64 = 60_000;
/// Corroboration window.
pub const LEDGER_WINDOW_MS: u64 = 20_000;
/// Per extra independent method agreeing.
pub const ENSEMBLE_BOOST: f32 = 0.08;
/// A person named recently who appears in the candidate's chapter.
pub const NAMED_ENTITY_BOOST: f32 = 0.15;
/// A single method trusted alone at this raw score (semantic cosine;
/// fingerprint coverage at high confidence).
pub const VERY_HIGH_RAW_CONFIDENCE: f32 = 0.95;
/// Where a hard cap lands: below the screen bar, still offered.
const CAPPED: f32 = VIEWER_MIN_SCORE - 0.05;

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

pub fn very_high_raw_confidence(method: Method, e: &Evidence) -> bool {
    let sim = e.similarity.unwrap_or(0.0);
    match method {
        Method::Semantic => sim >= VERY_HIGH_RAW_CONFIDENCE,
        Method::Fingerprint => sim >= VERY_HIGH_RAW_CONFIDENCE && e.confidence == Some(Confidence::High),
        _ => false,
    }
}

/// B: each method's evidence on one 0–1 scale.
pub fn calibrate(method: Method, e: &Evidence) -> f32 {
    let sim = e.similarity.unwrap_or(0.0);
    match method {
        Method::Direct | Method::Continuation => 0.93,
        Method::ContextCitation => 0.85,
        // A chapter was just named; only the verse is a guess.
        Method::ChapterKeyword => 0.82,
        // Never alone: the context it leans on could be stale.
        Method::DirectPartial => 0.75,
        Method::Verbatim => match e.matched_idf {
            Some(m) if m >= VERBATIM_CERTAIN_IDF => sim.max(0.90 + ((m - VERBATIM_CERTAIN_IDF) * 0.005).min(0.07)),
            // A short, real excerpt of a longer verse reaches a human, never the screen alone.
            Some(m) if m >= VERBATIM_MODERATE_IDF => {
                let t = clamp01((m - VERBATIM_MODERATE_IDF) / (VERBATIM_CERTAIN_IDF - VERBATIM_MODERATE_IDF));
                sim.max(0.55 + t * 0.05)
            }
            _ => sim,
        },
        Method::Stream => {
            if e.confirmed {
                return (0.90 + ((e.matched.unwrap_or(6) as f32 - 6.0) * 0.01).min(0.07)).min(0.97);
            }
            match (e.df, e.idf) {
                // A phrase only one verse has, carrying certain weight.
                (Some(1), Some(idf)) if idf >= VERBATIM_CERTAIN_IDF => e.similarity.unwrap_or(0.90).max(0.90),
                // Real but not certain: offered only (the raw anchor overlap isn't a score to trust).
                (Some(1), Some(idf)) if idf >= STREAM_IDF_FULL_CONFIDENCE => {
                    0.55 + clamp01((idf - STREAM_IDF_FULL_CONFIDENCE) / (VERBATIM_CERTAIN_IDF - STREAM_IDF_FULL_CONFIDENCE)) * 0.05
                }
                _ => 0.0,
            }
        }
        Method::Fingerprint => {
            if very_high_raw_confidence(method, e) {
                return 0.85;
            }
            let mult = match e.confidence {
                Some(Confidence::High) => 1.0,
                Some(Confidence::Medium) => 0.85,
                Some(Confidence::None) => 0.0,
                _ => 0.65,
            };
            (0.35 + sim * 0.5).min(0.75) * mult
        }
        Method::Semantic => {
            if very_high_raw_confidence(method, e) {
                return 0.85;
            }
            (clamp01(sim) * 0.7).min(0.60)
        }
        Method::Paraphrase => clamp01(e.paraphrase_score.unwrap_or(0.0)),
        Method::NamedPassage => 0.70,
    }
}

/// D: nearness to the verse on screen, fading over a minute. A verse behind
/// it in its chapter is penalised only if it was already shown (an unshown
/// earlier verse is a catch-up read, the passage read from the top).
pub fn distance(candidate: VerseRef, now_ms: u64, on_screen: Option<OnScreen>, already_shown: bool) -> f32 {
    let Some(s) = on_screen else { return 0.0 };
    let recency = clamp01(1.0 - now_ms.saturating_sub(s.at_ms) as f32 / SAME_BOOK_WINDOW_MS as f32);
    if recency <= 0.0 {
        return 0.0;
    }
    let same_book = candidate.book == s.verse.book;
    let same_chapter = same_book && candidate.chapter == s.verse.chapter;
    if same_chapter && candidate.verse >= s.verse.verse {
        let delta = (candidate.verse - s.verse.verse) as f32;
        return 0.15 * recency * (1.0 - delta / 40.0).max(0.0);
    }
    if same_chapter {
        return if already_shown { -0.20 * recency } else { 0.0 };
    }
    if same_book { -0.10 * recency } else { -0.15 * recency }
}

#[derive(Clone, Debug, Default)]
struct VerseEntry {
    methods: HashSet<Method>,
    landed: HashSet<Method>,
    count: u32,
    /// The evidence each sighting rested on: the same words found again
    /// (a growing sentence re-searched) aren't a new sighting.
    evidence: HashSet<u64>,
    last_at: u64,
}

#[derive(Clone, Debug, Default)]
struct ChapterEntry {
    verses: HashSet<u16>,
    last_at: u64,
}

/// A: corroboration over the last 20 seconds — independent methods agreeing
/// on a verse, the same verse found again, and several verses of one chapter
/// found (a passage being read).
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    by_verse: HashMap<VerseRef, VerseEntry>,
    by_chapter: HashMap<(Book, u16), ChapterEntry>,
}

impl Ledger {
    /// Every hit counts toward the bonuses; only one that `landed` (was worth
    /// showing somewhere) counts as a second opinion.
    pub fn record(&mut self, v: VerseRef, method: Method, now_ms: u64, landed: bool) {
        self.record_evidence(v, method, now_ms, landed, None);
    }

    /// As `record`; `evidence` identifies what the sighting rested on, and a
    /// sighting on evidence already counted doesn't count as a repeat.
    pub fn record_evidence(&mut self, v: VerseRef, method: Method, now_ms: u64, landed: bool, evidence: Option<u64>) {
        self.prune(now_ms);
        let e = self.by_verse.entry(v).or_default();
        e.methods.insert(method);
        if landed {
            e.landed.insert(method);
        }
        if evidence.is_none_or(|k| e.evidence.insert(k)) {
            e.count += 1;
        }
        e.last_at = now_ms;
        let c = self.by_chapter.entry((v.book, v.chapter)).or_default();
        c.verses.insert(v.verse);
        c.last_at = now_ms;
    }

    pub fn agreement_bonus(&self, v: VerseRef) -> f32 {
        match self.by_verse.get(&v) {
            Some(e) if e.methods.len() >= 2 => e.methods.len().min(2) as f32 * ENSEMBLE_BOOST,
            _ => 0.0,
        }
    }

    pub fn repeat_bonus(&self, v: VerseRef) -> f32 {
        self.by_verse.get(&v).map_or(0.0, |e| 0.05 * (e.count.saturating_sub(1)).min(3) as f32)
    }

    pub fn momentum_bonus(&self, v: VerseRef) -> f32 {
        match self.by_chapter.get(&(v.book, v.chapter)) {
            Some(c) if c.verses.len() >= 2 => 0.06,
            _ => 0.0,
        }
    }

    pub fn total(&self, v: VerseRef) -> f32 {
        self.agreement_bonus(v) + self.repeat_bonus(v) + self.momentum_bonus(v)
    }

    /// Two or more methods have independently landed this verse. Read it
    /// before recording the candidate being decided — nothing corroborates itself.
    pub fn has_corroboration(&self, v: VerseRef) -> bool {
        self.by_verse.get(&v).is_some_and(|e| e.landed.len() >= 2)
    }

    pub fn prune(&mut self, now_ms: u64) {
        self.by_verse.retain(|_, e| now_ms.saturating_sub(e.last_at) <= LEDGER_WINDOW_MS);
        self.by_chapter.retain(|_, c| now_ms.saturating_sub(c.last_at) <= LEDGER_WINDOW_MS);
    }

    pub fn clear(&mut self) {
        *self = Ledger::default();
    }
}

/// Everything the score needs to know about the moment.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScoreContext {
    pub now_ms: u64,
    pub on_screen: Option<OnScreen>,
    /// The candidate was already shown while its book has been up.
    pub already_shown: bool,
    /// The book of a cited range being read, if any.
    pub range_book: Option<Book>,
    /// For a candidate outside the range: does what was just said fit a range
    /// verse at least as well (`Some(true)`, a collision) or fit the candidate
    /// better (`Some(false)`, a real cross-reference)? `None`: decide by book.
    pub range_collision: Option<bool>,
    /// A person named in the last minute appears in the candidate's chapter.
    pub named_person_in_chapter: bool,
    /// The candidate's chapter and the one on screen share wording (twins),
    /// in different books.
    pub confusable_with_screen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    pub total: f32,
    pub b: f32,
    pub d: f32,
    pub a: f32,
    pub very_high: bool,
}

pub fn score(candidate: VerseRef, method: Method, e: &Evidence, ctx: &ScoreContext, ledger: &Ledger) -> Score {
    let b = calibrate(method, e);
    let colliding_with_range = ctx.range_book.is_some()
        && match ctx.range_collision {
            Some(c) => c,
            None => ctx.range_book != Some(candidate.book),
        };
    let explicit = method.is_explicit_book_citation();
    let self_sufficient = b >= VIEWER_MIN_SCORE && !(colliding_with_range && !explicit);
    let d = if self_sufficient { 0.0 } else { distance(candidate, ctx.now_ms, ctx.on_screen, ctx.already_shown) };
    let a = ledger.total(candidate) + if ctx.named_person_in_chapter { NAMED_ENTITY_BOOST } else { 0.0 };
    let mut total = clamp01(b + d + a);
    // A cited range holds against coincidental collisions; a near-duplicate
    // passage never takes over from its twin chapter on screen. Absolute: no
    // amount of time or repetition buys past them; a new citation always does.
    if !explicit && (colliding_with_range || (ctx.confusable_with_screen && ctx.on_screen.is_some_and(|s| s.verse.book != candidate.book))) {
        total = total.min(CAPPED);
    }
    Score { total, b, d, a, very_high: very_high_raw_confidence(method, e) }
}

/// Screen, Possible Matches or drop. Meaning-only matches reach the screen
/// only corroborated or at very high confidence. A verse's identifying words
/// said in any order (fingerprint) never put it up alone: in a sermon those
/// words recur every few seconds, and a short verse made of them matches the
/// preaching itself — it needs a second method (or the chapter on screen).
pub fn target(total: f32, method: Method, corroborated: bool, very_high: bool) -> Target {
    if method == Method::Semantic && total >= VIEWER_MIN_SCORE && !(corroborated || very_high) {
        return Target::Suggestions;
    }
    if method == Method::Fingerprint && total >= VIEWER_MIN_SCORE && !corroborated {
        return Target::Suggestions;
    }
    if total >= VIEWER_MIN_SCORE {
        Target::Screen
    } else if total >= SUGGESTION_FLOOR {
        Target::Suggestions
    } else {
        Target::Drop
    }
}
