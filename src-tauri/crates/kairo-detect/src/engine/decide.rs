//! The one place a candidate becomes a decision (`detection-spec/01-decision.md`).

use kairo_bible::VerseId;

use super::core::{Candidate, Engine, Held};
use super::evidence::fits_first_at_least_as_well;
use super::event::Effect;
use super::scoring::{self, ScoreContext, SAME_BOOK_WINDOW_MS};
use super::twins::TwinOutcome;
use super::types::{Method, OnScreen, Target};

const SUGGESTION_DEDUP_MS: u64 = 30_000;
const DETECT_DEDUP_MS: u64 = 45_000;
const DIRECT_DEDUP_MS: u64 = 5_000;
const SEND_DEDUP_MS: u64 = 8_000;
const CORRECTED_AWAY_WINDOW_MS: u64 = 15_000;
const LATE_CORROBORATION_WINDOW_MS: u64 = 20_000;
/// A partial quote corroborates another method only with half the verse heard in order.
pub const CORROBORATING_QUOTE_COVERAGE: f32 = 0.5;
/// A near-complete quote is the verse being said; the on-screen comparison doesn't apply.
const ON_SCREEN_GUARD_MAX_COVERAGE: f32 = 0.8;
/// A chapter just named holds text matches elsewhere this long.
const CHAPTER_NAMED_HOLD_MS: u64 = 30_000;
const VERBATIM_AUTOSEND_MIN: f32 = 0.95;

impl Engine {
    /// Decides a candidate and applies the decision. Returns where it landed.
    pub(crate) fn decide(&mut self, mut c: Candidate, now: u64) -> Target {
        let Some(&top) = c.verses.first() else { return Target::Drop };

        // 1. A cited range's next verse moves the range, from any method.
        match self.try_range_advance_by_detection(&c, now) {
            RangeAdvance::Moved => return Target::Screen,
            RangeAdvance::Held => c.cap = true,
            RangeAdvance::No => {}
        }

        // 2. A verse continuing the chapter on screen is promoted.
        let continuity = !c.verbatim_disagreed && !c.cap && self.continues_chapter(&c, now);
        if continuity {
            c.intent = Target::Screen;
        }

        // 3. One score.
        let r = self.r(top);
        let on_screen = self.screen.up.map(|(v, _)| OnScreen { verse: self.r(v), at_ms: self.screen.last_sent_book.map_or(0, |(_, t)| t) });
        let range_collision = match self.range.current {
            Some(_) if !self.range.all.contains(&top) => Some(!self.speech_favours_over_range(top)),
            _ => None,
        };
        let ctx = ScoreContext {
            now_ms: now,
            on_screen,
            already_shown: self.screen.shown_this_book.contains(&top),
            range_book: self.range.current.map(|v| self.r(v).book),
            range_collision,
            named_person_in_chapter: self.person_in_chapter(r, now),
            confusable_with_screen: on_screen.is_some_and(|s| self.k.confusable((r.book, r.chapter), (s.verse.book, s.verse.chapter))),
        };
        let s = scoring::score(r, c.method, &c.evidence, &ctx, &self.ledger);
        let corroborated = self.ledger.has_corroboration(r);
        let decided = scoring::target(s.total, c.method, corroborated, s.very_high);
        let second_opinion = (decided != Target::Drop || continuity)
            && !(c.method == Method::Verbatim && c.evidence.similarity.unwrap_or(0.0) < CORROBORATING_QUOTE_COVERAGE);
        let evidence = c.source.as_deref().map(|s| self.evidence_key(top, s));
        self.ledger.record_evidence(r, c.method, now, second_opinion, evidence);

        // 4. A stream match held for want of a second method: now it has one.
        if c.method != Method::Stream && second_opinion {
            if let Some(held) = self.awaiting.remove(&top) {
                if now.saturating_sub(held.at) < LATE_CORROBORATION_WINDOW_MS {
                    self.redo.push(held.candidate);
                }
            }
        }

        self.note(|e| {
            format!(
                "{} {} {:.3}: B {:.2} D {:+.2} A {:+.2} = {:.2} → {decided:?}{}{}{}",
                c.method.wire(),
                e.label(top),
                c.score,
                s.b,
                s.d,
                s.a,
                s.total,
                if continuity { " (continues the chapter)" } else { "" },
                if corroborated { " (corroborated)" } else { "" },
                if c.cap { " (capped)" } else { "" }
            )
        });
        let mut target = if continuity { Target::Screen } else { decided };
        let score = if continuity { c.score.max(s.total) } else { s.total };

        // 5. What the path knew and the score can't see only ever demotes.
        if target == Target::Screen && ((c.verbatim_disagreed && !continuity) || c.cap) {
            target = Target::Suggestions;
        }

        // 6.
        if target == Target::Drop {
            return Target::Drop;
        }

        // 7. A twin of the passage in play is explained by it.
        if target == Target::Suggestions && c.method.is_text_match() && self.twin_in_play(top, now) {
            return Target::Drop;
        }

        // 8. Near-duplicate twins: the better-supported one, or both offered.
        if target == Target::Screen && !c.twin_resolved && !c.method.names_its_verse() {
            match self.resolve_twin(top, now) {
                Some(TwinOutcome::Switch(twin)) => {
                    let mut t = c.clone();
                    t.verses = vec![twin];
                    t.twin_resolved = true;
                    t.intent = Target::Screen;
                    return self.decide(t, now);
                }
                Some(TwinOutcome::Hold(twin)) => {
                    target = Target::Suggestions;
                    c.verses = vec![top, twin];
                }
                None => {}
            }
        }

        // 9. A text match doesn't replace what's on screen unless what's being said favours it.
        if target == Target::Screen && c.method.is_text_match() && self.text_match_held(&c, top, now) {
            target = Target::Suggestions;
        }

        // 10. Offer a verse once per half minute.
        if target == Target::Suggestions {
            let fresh: Vec<VerseId> = c
                .verses
                .iter()
                .copied()
                .filter(|v| self.screen.recent_suggestions.get(v).is_none_or(|t| now.saturating_sub(*t) >= SUGGESTION_DEDUP_MS))
                .collect();
            if self.screen.recent_suggestions.len() > 200 {
                self.screen.recent_suggestions.retain(|_, t| now.saturating_sub(*t) < SUGGESTION_DEDUP_MS);
            }
            for v in &fresh {
                self.screen.recent_suggestions.insert(*v, now);
            }
            if fresh.is_empty() {
                return Target::Drop;
            }
            c.verses = fresh;
        }

        // 11. A citation can't resurrect what was corrected away, or drag the
        // screen back over a verse already read past (a re-parse of old text).
        if target == Target::Screen && c.method == Method::Direct {
            let corrected_back = self.screen.corrected_away.is_some_and(|ca| {
                ca.verse == top && now.saturating_sub(ca.at) < CORRECTED_AWAY_WINDOW_MS && self.screen.up_verse() == Some(ca.replaced_by)
            });
            let behind = self.screen.up_verse().is_some_and(|u| {
                let (ur, cr) = (self.r(u), self.r(top));
                ur.book == cr.book && ur.chapter == cr.chapter && cr.verse < ur.verse && self.screen.shown_this_book.contains(&top)
            });
            if corrected_back || behind {
                target = Target::Suggestions;
            }
        }

        // 12. Don't flood the screen with the same verse.
        if target == Target::Screen {
            let spoken = matches!(c.method, Method::Direct | Method::Continuation);
            let window = if spoken { DIRECT_DEDUP_MS } else { DETECT_DEDUP_MS };
            let same_book_continuation = self.screen.last_sent_book.is_some_and(|(b, t)| b == r.book && now.saturating_sub(t) < SAME_BOOK_WINDOW_MS)
                && self.screen.last_detected.map(|(v, _, _)| v) != Some(top);
            let overrides_weaker = spoken && self.screen.last_detected.is_some_and(|(_, _, m)| !matches!(m, Method::Direct | Method::Continuation));
            let wider_range = c.method == Method::Direct && c.verses.len() > 1 && self.screen.last_detected.map(|(v, _, _)| v) == Some(top) && c.verses.len() > self.range.all.len();
            if !same_book_continuation && !overrides_weaker && !wider_range {
                if self.screen.last_detected.is_some_and(|(v, t, _)| v == top && now.saturating_sub(t) < window) {
                    return Target::Drop;
                }
                if self.screen.recent_detections.get(&top).is_some_and(|t| now.saturating_sub(*t) < window) {
                    return Target::Drop;
                }
            }
            self.screen.last_detected = Some((top, now, c.method));
            self.screen.recent_detections.insert(top, now);
            if self.screen.recent_detections.len() > 100 {
                self.screen.recent_detections.retain(|_, t| now.saturating_sub(*t) <= DETECT_DEDUP_MS);
            }
        }

        // 13.
        self.effect(Effect::Show { verses: c.verses.clone(), method: c.method, score, target, corrected_from: c.corrected_from });
        if target == Target::Screen && self.cfg.auto_send {
            let fresh_send = self.screen.last_sent.is_none_or(|(v, t)| v != top || now.saturating_sub(t) >= SEND_DEDUP_MS);
            if fresh_send {
                self.screen.last_sent = Some((top, now));
                self.screen.last_sent_book = Some((r.book, now));
                if c.method == Method::Direct {
                    self.screen.last_citation = Some((top, now, self.t.words_heard));
                } else if c.method != Method::Continuation {
                    self.note_replaced_citation(top, now);
                }
                self.screen.non_scripture_live = false;
                // "verse N" now means a verse of the passage on screen.
                if c.method != Method::Direct && c.method != Method::DirectPartial {
                    self.reading.update(r.book, Some(r.chapter as u32), now);
                }
                self.mark_shown(top, now);
            }
        }
        target
    }

    /// Holds a stream match until a second method lands on the same verse.
    pub(crate) fn hold_for_corroboration(&mut self, c: Candidate, now: u64) {
        let top = c.verses[0];
        let mut held = c;
        held.cap = false;
        held.intent = Target::Screen;
        self.awaiting.insert(top, Held { candidate: held, at: now });
    }

    /// The text-match guards (rule 9).
    fn text_match_held(&self, c: &Candidate, top: VerseId, now: u64) -> bool {
        if self.screen.non_scripture_live {
            return true;
        }
        if self.shown_verse_returns_without_new_evidence(top, now) || self.range_left_verse_returns_without_new_evidence(top, now) {
            return true;
        }
        if self.chapter_just_named_elsewhere(top, c, now) {
            return true;
        }
        let near_complete = c.method == Method::Verbatim && c.evidence.similarity.unwrap_or(0.0) >= ON_SCREEN_GUARD_MAX_COVERAGE;
        !near_complete && self.on_screen_explains_speech_better(top, c.source.as_deref(), now)
    }

    fn chapter_just_named_elsewhere(&self, v: VerseId, c: &Candidate, now: u64) -> bool {
        let Some(p) = &self.chapter_pending else { return false };
        if now.saturating_sub(p.at) > CHAPTER_NAMED_HOLD_MS {
            return false;
        }
        let r = self.r(v);
        if r.book == p.book && r.chapter == p.chapter {
            return false;
        }
        !(c.method == Method::Verbatim && c.evidence.similarity.unwrap_or(0.0) >= VERBATIM_AUTOSEND_MIN)
    }

    /// The words the match was found in (else the newest speech) fit the verse
    /// on screen at least as well.
    fn on_screen_explains_speech_better(&self, v: VerseId, source: Option<&str>, now: u64) -> bool {
        let Some((up, at)) = self.screen.up else { return false };
        if now.saturating_sub(at) > SAME_BOOK_WINDOW_MS || up == v || self.range.all.contains(&v) {
            return false;
        }
        let newest = if self.t.interim.is_empty() { self.t.finals.back().map_or("", |f| f.text.as_str()) } else { &self.t.interim };
        fits_first_at_least_as_well(self.verse_text(up), self.verse_text(v), source.unwrap_or(newest))
    }

    /// What a sighting rests on: the verse's own words found in the speech.
    fn evidence_key(&self, v: VerseId, source: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let heard = super::evidence::evidence_words(source);
        let mut words: Vec<String> = super::evidence::evidence_words(&self.match_text(v)).into_iter().filter(|w| heard.contains(w)).collect();
        words.sort();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        words.hash(&mut h);
        h.finish()
    }

    /// A verse already shown and replaced goes back up only if the words said
    /// since the current verse went up are more its own than the current verse's.
    fn shown_verse_returns_without_new_evidence(&self, v: VerseId, _now: u64) -> bool {
        let Some((up, at)) = self.screen.up else { return false };
        if v == up || (!self.screen.shown_this_book.contains(&v) && !self.screen.recent_detections.contains_key(&v)) {
            return false;
        }
        let last_since = self.t.finals.iter().filter(|f| f.at > at).last().map_or("", |f| f.text.as_str());
        let heard = super::evidence::evidence_words(&format!("{last_since} {}", self.t.interim));
        let (cand, cur) = (super::evidence::evidence_words(self.verse_text(v)), super::evidence::evidence_words(self.verse_text(up)));
        let (cand_only, cur_only) = super::evidence::own_words_heard(&cand, &cur, &heard);
        cand_only == 0 || cand_only <= cur_only
    }

    /// Replaces the just-cited verse with a text match: remember it, so a
    /// re-parse of the citation can't put it back.
    fn note_replaced_citation(&mut self, by: VerseId, now: u64) {
        let (Some((up, _)), Some((cited, _, words))) = (self.screen.up, self.screen.last_citation) else { return };
        if up != cited || self.t.words_heard.saturating_sub(words) > super::correction::CORRECTION_WINDOW_WORDS {
            return;
        }
        let (u, b) = (self.r(up), self.r(by));
        if b.book == u.book && b.chapter == u.chapter && b.verse >= u.verse {
            return;
        }
        self.screen.corrected_away = Some(super::state::CorrectedAway { verse: up, replaced_by: by, at: now });
    }
}

pub(crate) enum RangeAdvance {
    Moved,
    Held,
    No,
}
