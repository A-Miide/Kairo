//! A preacher who cites one verse and reads another: the verse read replaces
//! the citation (`detection-spec/03-verbatim-and-correction.md`).

use std::collections::HashSet;

use kairo_bible::VerseId;

use super::core::Engine;
use super::event::Effect;
use super::evidence::{fits_first_at_least_as_well, meaningful_words};
use super::state::CorrectedAway;
use super::types::{Method, Target};
use crate::reference::book_mentions;

/// Words heard after a citation during which a reading can correct it.
pub const CORRECTION_WINDOW_WORDS: u64 = 35;
const CORRECTION_MIN_SIMILARITY: f32 = 0.95;
const CORRECTION_MIN_IDF: f32 = 15.0;
const CORRECTION_IMMUNITY_MS: u64 = 3_000;
/// Reading on a few verses past the citation is reading, not a miscitation.
const FORWARD_EXEMPT_VERSES: u16 = 5;
/// An alternate in another book this close in weight makes the reading ambiguous.
const COMPARABLE_ALTERNATE: f32 = 0.7;
/// Speech this much the cited verse's own words is still the citation being read.
const CITED_OVERLAP: f32 = 0.7;

/// A text match offered as the verse really being read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Match {
    pub verse: VerseId,
    pub similarity: f32,
    pub matched_idf: Option<f32>,
    pub method: Method,
}

impl Engine {
    /// Replaces the citation on screen with `top` when every condition holds.
    pub(crate) fn maybe_correct_miscitation(&mut self, top: &Match, source: &str, alternates: &[Match], now: u64) -> bool {
        if !self.cfg.auto_correct {
            return false;
        }
        let Some((cited, cited_at, cited_words)) = self.screen.last_citation else { return false };
        if self.screen.up_verse().is_some_and(|u| u != cited) {
            return false;
        }
        if self.t.words_heard.saturating_sub(cited_words) > CORRECTION_WINDOW_WORDS || now.saturating_sub(cited_at) < CORRECTION_IMMUNITY_MS {
            return false;
        }
        let (c, t) = (self.r(cited), self.r(top.verse));
        // The cited book confirmed again since: the citation stands.
        if self.screen.last_detected.is_some_and(|(v, at, _)| self.r(v).book == c.book && at > cited_at) {
            return false;
        }
        if self.screen.last_detected.is_some_and(|(v, _, _)| v == top.verse) || cited == top.verse {
            return false;
        }
        // The citation decides between twins.
        if self.k.lexicon.twins(cited).iter().any(|tw| tw.verse == top.verse) {
            return false;
        }
        // Naming the book aloud is a fresh citation still being parsed.
        if t.book != c.book && book_mentions(source, self.k.books, self.cfg.lang).contains(&t.book) {
            return false;
        }
        let strong = top.similarity >= CORRECTION_MIN_SIMILARITY || top.matched_idf.is_some_and(|m| m >= CORRECTION_MIN_IDF);
        if !strong {
            return false;
        }
        if let Some(top_idf) = top.matched_idf {
            if alternates.iter().any(|a| self.r(a.verse).book != t.book && a.matched_idf.is_some_and(|m| m >= top_idf * COMPARABLE_ALTERNATE)) {
                return false;
            }
        }
        if self.k.confusable((c.book, c.chapter), (t.book, t.chapter)) {
            return false;
        }
        // Already on screen before the citation: stale reading text.
        if self.screen.recent_detections.get(&top.verse).is_some_and(|at| *at < cited_at) {
            return false;
        }
        if t.book == c.book && t.chapter == c.chapter && t.verse > c.verse && t.verse - c.verse <= FORWARD_EXEMPT_VERSES {
            return false;
        }
        if self.range.all.iter().any(|v| {
            let r = self.r(*v);
            r.book == t.book && r.chapter == t.chapter
        }) {
            return false;
        }
        let cited_text = self.verse_text(cited).to_string();
        let spoken = meaningful_words(source);
        if !spoken.is_empty() {
            let cited_words: HashSet<String> = meaningful_words(&cited_text).into_iter().collect();
            let overlap = spoken.iter().filter(|w| cited_words.contains(*w)).count();
            if overlap as f32 / spoken.len() as f32 >= CITED_OVERLAP {
                return false;
            }
        }
        let since: Vec<&str> = self.t.finals.iter().filter(|f| f.at + 1_000 >= cited_at).map(|f| f.text.as_str()).collect();
        let since = format!("{} {}", since.join(" "), self.t.interim);
        if fits_first_at_least_as_well(&cited_text, self.verse_text(top.verse), &since) {
            return false;
        }

        self.screen.corrected_away = Some(CorrectedAway { verse: cited, replaced_by: top.verse, at: now });
        self.screen.last_citation = None;
        self.effect(Effect::Show { verses: vec![top.verse], method: top.method, score: top.similarity, target: Target::Screen, corrected_from: Some(cited) });
        self.screen.last_sent_book = Some((t.book, now));
        self.screen.last_sent = Some((top.verse, now));
        self.screen.last_detected = Some((top.verse, now, top.method));
        self.screen.recent_detections.insert(top.verse, now);
        self.mark_shown(top.verse, now);
        true
    }
}
