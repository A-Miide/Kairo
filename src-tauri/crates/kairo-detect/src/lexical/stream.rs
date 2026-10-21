//! A verse being read aloud, followed word by word as it is spoken.
//!
//! Every distinctive 4-word phrase in the Bible is an anchor. As each word
//! arrives, the last four are looked up; a hit opens an alignment candidate
//! that then follows the verse: each next word either matches the verse's
//! next word, matches the one after (a word dropped or inserted), or costs one
//! of a few misses. A candidate is confirmed once enough words have aligned
//! AND they carry enough identifying weight — and the words past the anchor
//! add weight of their own (an anchor can carry the whole bar by itself; a
//! stray "and the" after it must not complete a match).
//!
//! Words said just before the anchor can be credited too (the anchor started
//! a word late), but a confirmation that only happened because of that credit
//! is flagged, so the scorer can ask for corroboration.

use std::collections::{HashMap, HashSet};

use kairo_bible::VerseId;

use super::lexicon::{Lexicon, WordId};
use super::text::PREACHING_COMMON_WORDS;

const ANCHOR_N: usize = 4;
/// The same verse can't fire again from the same wording within this.
const HIT_DEDUP_MS: u64 = 12_000;
/// Words aligned before a candidate can be confirmed.
const ALIGN_CONFIRM_AT: u32 = 6;
/// Identifying weight aligned before a candidate can be confirmed.
pub const ANCHOR_CONFIRM_IDF: f32 = 12.0;
/// Weight the words past the anchor must add themselves.
const ALIGN_EXTENSION_MIN_IDF: f32 = 2.0;
/// A word weighing at least this is more than glue.
const STREAM_WEIGHTY_IDF: f32 = 3.0;
const ALIGN_MISS_BUDGET: u32 = 3;
const ALIGN_AGE_MS: u64 = 20_000;
const MAX_CANDIDATES: usize = 40;
/// Backward credit: only words that identify something, and only a couple.
const BACKWARD_WORD_IDF: f32 = 2.0;
const BACKWARD_MAX_WORDS: u32 = 2;
const WORD_HISTORY: usize = 24;

#[derive(Clone, Debug, PartialEq)]
pub struct AnchorHit {
    pub verse: VerseId,
    /// Verses the phrase occurs in.
    pub df: u8,
    /// Identifying weight of the phrase plus any backward credit.
    pub idf: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Confirmed {
    pub verse: VerseId,
    pub matched: u32,
    pub matched_idf: f32,
    /// Would not have confirmed without credit for words before the anchor.
    pub via_backward_extension: bool,
    /// Aligned words that weigh something and aren't everyday preaching words.
    pub weighty_words: u32,
    /// The weightiest aligned word.
    pub peak_idf: f32,
    /// The weightiest aligned word that the other verses sharing the opening phrase don't have:
    /// "I heard a voice from heaven" opens four verses of Revelation, and "…the voice of" told
    /// none of them apart (Revelation 14:2 sent on "I hear a voice from heaven, and it's the voice
    /// of mercy", dev build 2026-10-20).
    pub distinct_peak_idf: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamTick {
    pub anchors: Vec<AnchorHit>,
    pub confirmed: Vec<Confirmed>,
}

#[derive(Clone, Debug)]
struct Candidate {
    verse: VerseId,
    own: bool,
    cursor: usize,
    matched: u32,
    matched_idf: f32,
    back_matched: u32,
    back_idf: f32,
    seed_idf: f32,
    counted: HashSet<WordId>,
    misses: u32,
    confirmed: bool,
    fired_at: u64,
    /// The opening phrase it was found by.
    gram: u32,
}

/// One service's streaming state.
#[derive(Default)]
pub struct StreamMatcher {
    last_four: Vec<Option<WordId>>,
    candidates: Vec<Candidate>,
    fired: HashMap<(VerseId, bool), u64>,
    history: Vec<Option<WordId>>,
}

impl StreamMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// One spoken word.
    pub fn push(&mut self, lex: &Lexicon, word: &str, now_ms: u64) -> StreamTick {
        let normalized = super::text::norm(word);
        if normalized.is_empty() {
            return StreamTick::default();
        }
        let stem = lex.vocab().id(&super::stem::heal(&normalized.replace(' ', "")));
        self.history.push(stem);
        if self.history.len() > WORD_HISTORY {
            self.history.remove(0);
        }
        let mut tick = StreamTick::default();

        // Follow the open candidates.
        let mut kept = Vec::with_capacity(self.candidates.len());
        for mut c in std::mem::take(&mut self.candidates) {
            if now_ms.saturating_sub(c.fired_at) > ALIGN_AGE_MS {
                continue;
            }
            let stems = lex.slot_stems(c.verse, c.own);
            let words = lex.slot_words(c.verse, c.own);
            if c.cursor >= stems.len() {
                continue;
            }
            let mut aligned = false;
            if Some(stems[c.cursor]) == stem {
                let w = words[c.cursor];
                credit(lex, &mut c, w);
                c.cursor += 1;
                c.matched += 1;
                aligned = true;
            } else if c.misses > 0 && c.cursor + 1 < stems.len() && Some(stems[c.cursor + 1]) == stem {
                let w = words[c.cursor + 1];
                credit(lex, &mut c, w);
                c.cursor += 2;
                c.matched += 1;
                c.misses -= 1;
                aligned = true;
            } else if c.misses > 0 {
                c.misses -= 1;
            } else {
                continue;
            }
            if aligned && !c.confirmed && c.matched >= ALIGN_CONFIRM_AT && c.matched_idf >= ANCHOR_CONFIRM_IDF && c.matched_idf - c.seed_idf >= ALIGN_EXTENSION_MIN_IDF {
                c.confirmed = true;
                let without_back = c.matched - c.back_matched >= ALIGN_CONFIRM_AT && c.matched_idf - c.back_idf >= ANCHOR_CONFIRM_IDF;
                let weighty_words = c
                    .counted
                    .iter()
                    .filter(|w| lex.idf(**w) >= STREAM_WEIGHTY_IDF && !PREACHING_COMMON_WORDS.contains(lex.vocab().word(**w)))
                    .count() as u32;
                let peak_idf = c.counted.iter().map(|w| lex.idf(*w)).fold(0.0, f32::max);
                let siblings: HashSet<WordId> = lex
                    .anchor_entries(c.gram)
                    .iter()
                    .filter(|e| e.verse != c.verse)
                    .flat_map(|e| lex.slot_words(e.verse, e.own).iter().copied())
                    .collect();
                let distinct_peak_idf = c.counted.iter().filter(|w| !siblings.contains(*w)).map(|w| lex.idf(*w)).fold(0.0, f32::max);
                tick.confirmed.push(Confirmed {
                    verse: c.verse,
                    matched: c.matched,
                    matched_idf: c.matched_idf,
                    via_backward_extension: c.back_matched > 0 && !without_back,
                    weighty_words,
                    peak_idf,
                    distinct_peak_idf,
                });
            }
            kept.push(c);
        }
        self.candidates = kept;

        // Open new anchors from the last four words.
        self.last_four.push(stem);
        if self.last_four.len() > ANCHOR_N {
            self.last_four.remove(0);
        }
        if let [Some(a), Some(b), Some(c), Some(d)] = self.last_four[..] {
            if let Some(gram) = lex.anchor(&[a, b, c, d]) {
                self.fire(lex, gram, now_ms, &mut tick);
            }
        }
        if self.candidates.len() > MAX_CANDIDATES {
            let drop = self.candidates.len() - MAX_CANDIDATES;
            self.candidates.drain(..drop);
        }
        tick
    }

    fn fire(&mut self, lex: &Lexicon, gram: u32, now_ms: u64, tick: &mut StreamTick) {
        for e in lex.anchor_entries(gram) {
            let key = (e.verse, e.own);
            if self.fired.get(&key).is_some_and(|t| now_ms.saturating_sub(*t) < HIT_DEDUP_MS) {
                continue;
            }
            self.fired.insert(key, now_ms);
            let words = lex.slot_words(e.verse, e.own);
            let pos = e.pos as usize;
            let idf = lex.idf_span(words, pos, ANCHOR_N);
            let back = self.extend_backward(lex, e.verse, e.own, pos);
            let total_idf = idf + back.idf;
            tick.anchors.push(AnchorHit { verse: e.verse, df: lex.anchor_df(gram), idf: total_idf });
            let mut counted: HashSet<WordId> = words.iter().skip(pos).take(ANCHOR_N).copied().collect();
            counted.extend(back.words);
            self.candidates.push(Candidate {
                verse: e.verse,
                own: e.own,
                cursor: pos + ANCHOR_N,
                matched: ANCHOR_N as u32 + back.matched,
                matched_idf: total_idf,
                back_matched: back.matched,
                back_idf: back.idf,
                seed_idf: total_idf,
                counted,
                misses: ALIGN_MISS_BUDGET,
                confirmed: false,
                fired_at: now_ms,
                gram,
            });
        }
    }

    /// Credit for the verse's words just before the anchor, if the preacher
    /// said them just before it — tolerating one dropped or extra word, like
    /// the forward walk.
    fn extend_backward(&self, lex: &Lexicon, verse: VerseId, own: bool, pos: usize) -> Back {
        let stems = lex.slot_stems(verse, own);
        let words = lex.slot_words(verse, own);
        let mut back = Back::default();
        if pos == 0 || self.history.len() <= ANCHOR_N {
            return back;
        }
        let mut v = pos as isize - 1;
        let mut h = self.history.len() as isize - 1 - ANCHOR_N as isize;
        let mut misses = ALIGN_MISS_BUDGET;
        let take = |back: &mut Back, w: WordId| {
            if lex.idf(w) >= BACKWARD_WORD_IDF && !back.words.contains(&w) {
                back.idf += lex.idf(w);
                back.words.push(w);
                back.matched += 1;
            }
        };
        while v >= 0 && h >= 0 && back.matched < BACKWARD_MAX_WORDS {
            let heard = self.history[h as usize];
            if Some(stems[v as usize]) == heard {
                take(&mut back, words[v as usize]);
                v -= 1;
                h -= 1;
            } else if misses > 0 && v >= 1 && Some(stems[v as usize - 1]) == heard {
                take(&mut back, words[v as usize - 1]);
                v -= 2;
                h -= 1;
                misses -= 1;
            } else if misses > 0 {
                misses -= 1;
                h -= 1;
            } else {
                break;
            }
        }
        back
    }
}

#[derive(Default)]
struct Back {
    matched: u32,
    idf: f32,
    words: Vec<WordId>,
}

/// Adds a verse word's weight once, however often it recurs.
fn credit(lex: &Lexicon, c: &mut Candidate, w: WordId) {
    if c.counted.insert(w) {
        c.matched_idf += lex.idf(w);
    }
}
