//! The text-matching paths: followed word by word (stream), read word for
//! word (verbatim), and loosely by identifying words (fingerprint) —
//! `detection-spec/03` and `06`.

use std::collections::{HashMap, HashSet};

use kairo_bible::{Book, VerseId, VerseRef};

use super::core::{Candidate, Engine, CONTEXT_WINDOW_MS, NAMED_PERSON_WINDOW_MS};
use super::correction::Match;
use super::decide::CORROBORATING_QUOTE_COVERAGE;
use super::event::Effect;
use super::evidence::explained_better_by;
use super::scoring::{SAME_BOOK_WINDOW_MS, STREAM_IDF_FULL_CONFIDENCE, VERBATIM_CERTAIN_IDF};
use super::twins::TwinOutcome;
use super::types::{Evidence, Method, Target};
use crate::lexical::{fingerprint, topic_library, verbatim, Confidence, RecentCitation, VerbatimHit};
use crate::reference::book_mentions;

const VERBATIM_AUTOSEND_MIN: f32 = 0.95;
/// A quote this complete feeds the sermon's context and reading mode.
const READ_SIMILARITY: f32 = 0.92;
const CLAUSE_WINDOW_WORDS: usize = 14;
const CLAUSE_WINDOW_STEP: usize = 7;
const VERBATIM_MIN_WORDS: usize = 6;
const VERBATIM_LIMIT: usize = 3;
/// Shared opening wording: two confirmed alignments this close are tied.
const TIE_MATCHED: u32 = 2;
const TIE_BREAK_CONTINUITY_WINDOW_MS: u64 = 8_000;
const CROSSCHECK_MIN_IDF: f32 = 10.0;
const CROSSCHECK_COOLDOWN_MS: u64 = 20_000;
const CROSSCHECK_WORDS: usize = 100;
const STREAM_MIN_WEIGHTY_WORDS: u32 = 3;
const STREAM_RELATIVE_MS: u64 = 10_000;
const SHOWN_RECENTLY_MS: u64 = 60_000;
const ANCHOR_DEPTH: u32 = 4;
/// Fingerprint suggestions.
const SUGGESTION_MIN_SCORE: f32 = 0.87;
const SCOPED_MIN_SCORE: f32 = 0.60;
const SCOPED_MARGIN: f32 = 0.03;
const DIRECT_REF_SUPPRESS_MS: u64 = 4_000;
const FINGERPRINT_MIN_WORDS: usize = 4;
const FINGERPRINT_FALLBACK_WORDS: usize = 40;
const FINGERPRINT_LIMIT: usize = 5;
const TOPIC_BUILD_INTERVAL_MS: u64 = 60_000;
const TOPIC_WORD_MIN_COUNT: f32 = 2.0;
const TOPIC_WORD_MIN_IDF: f32 = 3.0;
const TOPIC_MAX_WORDS: usize = 20;
const TOPIC_DECAY: f32 = 0.5;
const READING_MODE_WINDOW_MS: u64 = 30_000;
const READING_MODE_HITS: u32 = 3;

/// A stream hit, as ranked and remembered.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamHit {
    pub verse: VerseId,
    pub confirmed: bool,
    pub matched: u32,
    pub matched_idf: f32,
    pub via_backward: bool,
    pub weighty: u32,
    pub df: u8,
    pub idf: f32,
    pub similarity: f32,
}

impl StreamHit {
    fn evidence(&self) -> Evidence {
        Evidence {
            similarity: Some(self.similarity),
            matched_idf: self.confirmed.then_some(self.matched_idf),
            confirmed: self.confirmed,
            matched: Some(self.matched),
            df: (!self.confirmed).then_some(self.df),
            idf: (!self.confirmed).then_some(self.idf),
            ..Default::default()
        }
    }

    fn weight(&self) -> f32 {
        if self.confirmed { self.matched_idf } else { self.idf }
    }
}

#[derive(Default)]
pub(crate) struct PathState {
    /// Verses the stream anchored on lately, for siblings and rivals.
    anchors: HashMap<VerseId, (StreamHit, u64)>,
    crosscheck_rejected: HashMap<VerseId, u64>,
    topic_counts: HashMap<String, f32>,
    topic_built_at: u64,
    pub(crate) topic_library: Option<HashSet<VerseId>>,
}

#[derive(Default)]
pub(crate) struct ReadingMode {
    active: bool,
    chapter: Option<(Book, u16)>,
    hits: u32,
    last_reset: u64,
}

impl ReadingMode {
    pub fn chapter(&self) -> Option<(Book, u16)> {
        if self.active { self.chapter } else { None }
    }
}

/// Clauses at punctuation (6+ words), long ones also cut into overlapping
/// windows, and the whole text last.
fn clause_list(text: &str, min_words: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for clause in text.split(['.', '!', '?', ',', ';']).map(str::trim) {
        let words: Vec<&str> = clause.split_whitespace().collect();
        if words.len() < min_words {
            continue;
        }
        out.push(clause.to_string());
        if words.len() > CLAUSE_WINDOW_WORDS + CLAUSE_WINDOW_STEP {
            let mut i = 0;
            while i + CLAUSE_WINDOW_WORDS < words.len() {
                out.push(words[i..i + CLAUSE_WINDOW_WORDS].join(" "));
                i += CLAUSE_WINDOW_STEP;
            }
            out.push(words[words.len() - CLAUSE_WINDOW_WORDS..].join(" "));
        }
    }
    out.push(text.to_string());
    let mut seen = HashSet::new();
    out.into_iter().filter(|c| c.split_whitespace().count() >= min_words && seen.insert(c.clone())).collect()
}

impl Engine {
    /// The best verbatim results over several texts (stopping at a near-complete quote).
    /// The best results over several texts, and the text they came from.
    fn verbatim_batch(&self, texts: &[String], min_words: usize) -> (Vec<VerbatimHit>, String) {
        // Another verse carries the same weight of what was said: this much of the speech can't
        // tell them apart yet (John 3:15 and 3:16 share "should not perish, but have").
        let ambiguous = |r: &[VerbatimHit]| {
            r.get(1).is_some_and(|b| matches!((r[0].matched_idf, b.matched_idf), (Some(x), Some(y)) if y >= x * 0.98))
        };
        let mut best: (Vec<VerbatimHit>, String) = (Vec::new(), String::new());
        for t in texts {
            let r = verbatim(&self.k.lexicon, t, min_words, VERBATIM_LIMIT);
            if r.is_empty() {
                continue;
            }
            // A clear result beats an ambiguous one: the longer text that follows tells them apart.
            if best.0.is_empty() || r[0].score > best.0[0].score || (ambiguous(&best.0) && !ambiguous(&r)) {
                best = (r, t.clone());
            }
            // A near-complete, unambiguous quote ends the search.
            if best.0[0].score >= READ_SIMILARITY && !ambiguous(&best.0) {
                break;
            }
        }
        best
    }

    pub(crate) fn process_verbatim(&mut self, text: &str, rescan: bool, latest: &str, now: u64) {
        let texts = clause_list(text, VERBATIM_MIN_WORDS);
        if texts.is_empty() {
            return;
        }
        let (results, found_in) = self.verbatim_batch(&texts, VERBATIM_MIN_WORDS);
        let Some(first) = results.first().copied() else { return };
        let certain = |h: &VerbatimHit| h.score >= VERBATIM_AUTOSEND_MIN || h.matched_idf.is_some_and(|m| m >= VERBATIM_CERTAIN_IDF);
        let screen: Vec<VerbatimHit> = results.iter().copied().filter(certain).collect();
        let evidence = |h: &VerbatimHit| Evidence { similarity: Some(h.score), matched_idf: h.matched_idf, ..Default::default() };

        let Some(&top) = screen.first() else {
            // Below the bar: offered (the scorer's floor drops weak ones).
            self.decide(Candidate::new(vec![first.verse], Method::Verbatim, first.score, Target::Suggestions).evidence(evidence(&first)).from_text(&found_in).capped(), now);
            return;
        };
        let steps_back = self.screen.up_verse().is_some_and(|u| {
            let (u, v) = (self.r(u), self.r(top.verse));
            u.book == v.book && u.chapter == v.chapter && v.verse < u.verse
        });
        // The end-of-utterance sweep mustn't resurrect a quote the reading has moved past.
        if rescan && (!self.shares_words_with(top.verse, latest) || steps_back) {
            self.decide(Candidate::new(vec![top.verse], Method::Verbatim, top.score, Target::Suggestions).evidence(evidence(&top)).from_text(&found_in).capped(), now);
            return;
        }
        let as_match = |h: &VerbatimHit| Match { verse: h.verse, similarity: h.score, matched_idf: h.matched_idf, method: Method::Verbatim };
        let alternates: Vec<Match> = results.iter().filter(|h| h.verse != top.verse).map(as_match).collect();
        if self.maybe_correct_miscitation(&as_match(&top), text, &alternates, now) {
            return;
        }
        let via_idf = top.score < VERBATIM_AUTOSEND_MIN;
        // A partial quote: another verse the speech touched may explain more of it.
        if via_idf && top.score < CORROBORATING_QUOTE_COVERAGE {
            let mut rivals: Vec<VerseId> = self.stream_anchors_near(top.verse, now);
            rivals.extend(results.iter().map(|h| h.verse).filter(|v| *v != top.verse));
            let texts: Vec<String> = rivals.iter().map(|v| self.match_text(*v)).collect();
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            if explained_better_by(&self.match_text(top.verse), &refs, if rescan { latest } else { text }).is_some() {
                self.decide(Candidate::new(vec![top.verse], Method::Verbatim, top.score, Target::Suggestions).evidence(evidence(&top)).from_text(&found_in).capped(), now);
                return;
            }
        }
        let score = if via_idf { top.score.max(0.90) } else { top.score };
        let corroborated = self.ledger.has_corroboration(self.r(top.verse));
        // Certain by weight alone (a partial quote) and no other method agrees:
        // offered, never put on screen alone (03-verbatim-and-correction.md).
        // Capped, not just "intent": `intent` is never read, so a four-word
        // phrase like "Jesus Christ from the dead" (weight 18.3) put 1 Peter 1:3
        // on screen on its own (live test, 2026-10-14).
        let weak = via_idf && !corroborated;
        let intent = if weak { Target::Suggestions } else { Target::Screen };
        let verses = screen.iter().map(|h| h.verse).collect();
        let candidate = Candidate::new(verses, Method::Verbatim, score, intent).evidence(evidence(&top)).from_text(&found_in);
        self.decide(if weak { candidate.capped() } else { candidate }, now);
        if top.score >= READ_SIMILARITY {
            let r = self.r(top.verse);
            self.note_cited(r.book, Some(r.chapter), Some(r.verse), false, now);
            self.track_reading_mode(top.verse, top.score, now);
        }
    }

    /// Feeds only the words not yet fed; the offline engine's newest interim
    /// word may still be growing.
    pub(crate) fn stream_new_words(&mut self, text: &str, is_final: bool, continues_live: bool, now: u64) {
        let mut words: Vec<&str> = text.split_whitespace().collect();
        if !is_final && self.cfg.offline {
            words.pop();
        }
        let fresh: Vec<String> = words.get(self.t.stream_watermark.min(words.len())..).unwrap_or(&[]).iter().map(|w| w.to_string()).collect();
        self.t.stream_watermark = if !is_final {
            words.len()
        } else if continues_live {
            self.t.stream_watermark.saturating_sub(words.len())
        } else {
            0
        };
        if !fresh.is_empty() {
            self.process_stream(&fresh, now);
        }
    }

    fn process_stream(&mut self, words: &[String], now: u64) {
        let mut confirmed: Vec<StreamHit> = Vec::new();
        let mut anchors: Vec<StreamHit> = Vec::new();
        for w in words {
            let tick = self.stream.push(&self.k.lexicon, w, now);
            for a in tick.anchors {
                let hit = StreamHit { verse: a.verse, confirmed: false, matched: ANCHOR_DEPTH, matched_idf: 0.0, via_backward: false, weighty: 0, df: a.df, idf: a.idf, similarity: anchor_similarity(a.df) };
                match anchors.iter_mut().find(|h| h.verse == a.verse) {
                    Some(h) if a.df < h.df => *h = hit,
                    Some(_) => {}
                    None => anchors.push(hit),
                }
            }
            for c in tick.confirmed {
                let similarity = (0.90 + (c.matched as f32 - 6.0) * 0.01).min(0.97);
                let hit = StreamHit { verse: c.verse, confirmed: true, matched: c.matched, matched_idf: c.matched_idf, via_backward: c.via_backward_extension, weighty: c.weighty_words, df: 0, idf: 0.0, similarity };
                match confirmed.iter_mut().find(|h| h.verse == c.verse) {
                    Some(h) if c.matched > h.matched => *h = hit,
                    Some(_) => {}
                    None => confirmed.push(hit),
                }
            }
        }
        anchors.retain(|a| !confirmed.iter().any(|c| c.verse == a.verse));
        let mut ranked: Vec<StreamHit> = confirmed.into_iter().chain(anchors).collect();
        if ranked.is_empty() {
            return;
        }
        let ranks: Vec<f32> = ranked.iter().map(|h| self.stream_rank(h, now)).collect();
        let mut order: Vec<usize> = (0..ranked.len()).collect();
        order.sort_by(|a, b| ranks[*b].total_cmp(&ranks[*a]));
        ranked = order.into_iter().map(|i| ranked[i]).collect();
        for h in &ranked {
            self.paths.anchors.insert(h.verse, (*h, now));
        }
        self.paths.anchors.retain(|_, (_, at)| now.saturating_sub(*at) <= STREAM_RELATIVE_MS);

        let text = words.join(" ");
        let confirmed: Vec<StreamHit> = ranked.iter().copied().filter(|h| h.confirmed).collect();
        if !confirmed.is_empty() {
            self.stream_confirmed(confirmed, &text, now);
        }
        let anchors: Vec<StreamHit> = ranked.iter().copied().filter(|h| !h.confirmed).collect();
        if let Some(unique) = anchors.iter().copied().find(|a| a.df == 1 && a.idf >= STREAM_IDF_FULL_CONFIDENCE) {
            let top = Match { verse: unique.verse, similarity: unique.similarity, matched_idf: Some(unique.idf), method: Method::Stream };
            let others: Vec<Match> = anchors.iter().filter(|a| a.verse != unique.verse).map(|a| Match { verse: a.verse, similarity: a.similarity, matched_idf: Some(a.idf), method: Method::Stream }).collect();
            if !self.maybe_correct_miscitation(&top, &text, &others, now) {
                self.decide(Candidate::new(vec![unique.verse], Method::Stream, unique.similarity.max(0.90), Target::Screen).evidence(unique.evidence()), now);
            }
        }
    }

    /// Confirmed first; then longer; the topic library; near a recent citation.
    fn stream_rank(&self, h: &StreamHit, now: u64) -> f32 {
        let mut score = if h.confirmed { 1000.0 } else { 0.0 };
        score += h.matched as f32 * 10.0;
        if self.paths.topic_library.as_ref().is_some_and(|l| l.contains(&h.verse)) {
            score += 50.0;
        }
        let r = self.r(h.verse);
        for c in self.recently_cited(now) {
            if c.book != r.book {
                continue;
            }
            let delta = (c.chapter.unwrap_or(0) as i32 - r.chapter as i32).unsigned_abs();
            if delta > 5 {
                continue;
            }
            let recency = (1.0 - now.saturating_sub(c.at) as f32 / CONTEXT_WINDOW_MS as f32).max(0.0);
            score += (40.0 - delta as f32 * 6.0) * recency;
        }
        score
    }

    fn stream_confirmed(&mut self, confirmed: Vec<StreamHit>, text: &str, now: u64) {
        let mut top: Vec<StreamHit> = confirmed.into_iter().take(5).collect();
        let behind = |e: &Engine, v: VerseId| {
            e.screen.up_verse().is_some_and(|u| {
                let (u, r) = (e.r(u), e.r(v));
                r.book == u.book && r.chapter == u.chapter && r.verse < u.verse
            })
        };
        // Shared opening wording: a different verse aligned nearly as far.
        if let Some(runner) = top.iter().copied().find(|h| h.verse != top[0].verse) {
            if top[0].matched.abs_diff(runner.matched) < TIE_MATCHED {
                let fresh = self.screen.up.is_some() && self.screen.last_sent_book.is_some_and(|(_, at)| now.saturating_sub(at) < TIE_BREAK_CONTINUITY_WINDOW_MS);
                let up_book = self.screen.up_verse().map(|u| self.r(u).book);
                let active = |e: &Engine, v: VerseId| fresh && up_book == Some(e.r(v).book) && !behind(e, v);
                let (top_active, runner_active) = (active(self, top[0].verse), active(self, runner.verse));
                let winner = if top_active != runner_active {
                    if top_active { top[0].verse } else { runner.verse }
                } else {
                    let tied: Vec<VerseId> = top.iter().filter(|h| top[0].matched.abs_diff(h.matched) < TIE_MATCHED).map(|h| h.verse).collect();
                    match self.settle_stream_tie(&tied, now) {
                        Some(w) => w,
                        None => return, // near-duplicate wording: hold both back
                    }
                };
                top.retain(|h| h.verse == winner);
            }
        }
        let lead = top[0];
        let alternates: Vec<Match> = top[1..].iter().map(|h| Match { verse: h.verse, similarity: h.similarity, matched_idf: Some(h.matched_idf), method: Method::Stream }).collect();
        let as_match = Match { verse: lead.verse, similarity: lead.similarity, matched_idf: Some(lead.matched_idf), method: Method::Stream };
        if self.maybe_correct_miscitation(&as_match, text, &alternates, now) {
            return;
        }
        let verses: Vec<VerseId> = top.iter().map(|h| h.verse).collect();
        let candidate = |verses: Vec<VerseId>, intent: Target| Candidate::new(verses, Method::Stream, lead.similarity, intent).evidence(lead.evidence());
        let lead_ref = self.r(lead.verse);
        let corroborated = self.ledger.has_corroboration(lead_ref);
        let mut intent = Target::Screen;
        if lead.via_backward && !corroborated {
            intent = Target::Suggestions;
            self.hold_for_corroboration(candidate(verses.clone(), Target::Screen), now);
        }

        // A verbatim search of recent speech picking another verse disagrees
        // (not for reading on in the chapter on screen).
        let sequential = self.screen.last_sent_book.is_some_and(|(_, at)| now.saturating_sub(at) < SAME_BOOK_WINDOW_MS)
            && self.screen.up_verse().is_some_and(|u| {
                let u = self.r(u);
                lead_ref.book == u.book && lead_ref.chapter == u.chapter && lead_ref.verse >= u.verse
            });
        let mut disagreed = false;
        if intent == Target::Screen && self.paths.crosscheck_rejected.get(&lead.verse).is_some_and(|at| now.saturating_sub(*at) < CROSSCHECK_COOLDOWN_MS) {
            intent = Target::Suggestions;
            disagreed = true;
        } else if intent == Target::Screen && !sequential {
            let recent = self.t.last_words(CROSSCHECK_WORDS);
            if !recent.is_empty() {
                let v = verbatim(&self.k.lexicon, &recent, 4, VERBATIM_LIMIT);
                if v.first().is_some_and(|h| h.verse != lead.verse && h.matched_idf.is_some_and(|m| m >= CROSSCHECK_MIN_IDF)) {
                    intent = Target::Suggestions;
                    self.paths.crosscheck_rejected.insert(lead.verse, now);
                    disagreed = true;
                }
            }
        }

        // Thin on its own: nothing points at it and few weighty words aligned.
        let pointed_at = self.passage_pointed_at(lead.verse, now);
        let thin = intent == Target::Screen && !pointed_at && lead.weighty < STREAM_MIN_WEIGHTY_WORDS && !corroborated;
        let mut twin_took = false;
        if thin {
            let better = match self.resolve_twin(lead.verse, now) {
                Some(TwinOutcome::Switch(t)) => Some(t),
                _ => {
                    let near = self.stream_anchors_near(lead.verse, now);
                    let texts: Vec<String> = near.iter().map(|v| self.match_text(*v)).collect();
                    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                    let said = self.recent_speech_seconds(40).join(" ");
                    explained_better_by(&self.match_text(lead.verse), &refs, &said).map(|(i, _)| near[i])
                }
            };
            if let Some(other) = better {
                let mut c = candidate(vec![other], Target::Screen);
                c.twin_resolved = true;
                twin_took = self.decide(c, now) == Target::Screen;
            }
        }
        if thin && !twin_took {
            intent = Target::Suggestions;
            self.hold_for_corroboration(candidate(verses.clone(), Target::Screen), now);
        }
        if twin_took {
            return;
        }
        let mut c = candidate(verses, intent);
        c.verbatim_disagreed = disagreed;
        c.cap = thin;
        let sent = self.decide(c, now);
        if sent == Target::Screen && !pointed_at && lead.matched_idf < VERBATIM_CERTAIN_IDF {
            let relatives = self.stream_anchors_near(lead.verse, now);
            if !relatives.is_empty() {
                self.start_citation_check(lead.verse, "", false, Some(relatives), now);
            }
        }
    }

    /// Among tied alignments, exactly one in the passage in play (not behind
    /// the verse on screen) wins.
    fn settle_stream_tie(&self, tied: &[VerseId], now: u64) -> Option<VerseId> {
        let behind = |v: VerseId| {
            self.screen.up_verse().is_some_and(|u| {
                let (u, r) = (self.r(u), self.r(v));
                r.book == u.book && r.chapter == u.chapter && r.verse < u.verse
            })
        };
        let scored: Vec<(VerseId, f32)> =
            tied.iter().map(|v| (*v, if !behind(*v) && self.passage_pointed_at(*v, now) { self.passage_affinity(*v, now) } else { 0.0 })).collect();
        let best = scored.iter().map(|s| s.1).fold(0.0, f32::max);
        let top: Vec<VerseId> = scored.iter().filter(|s| s.1 == best).map(|s| s.0).collect();
        (best >= 2.0 && top.len() == 1).then(|| top[0])
    }

    /// Other verses the stream anchored on lately: most specific phrase
    /// first, then most identifying, then most recent — not the verse's twins,
    /// nor verses on screen in the last minute.
    pub(crate) fn stream_anchors_near(&self, v: VerseId, now: u64) -> Vec<VerseId> {
        let twins: HashSet<VerseId> = self.k.lexicon.twins(v).iter().map(|t| t.verse).collect();
        let mut near: Vec<&(StreamHit, u64)> = self
            .paths
            .anchors
            .iter()
            .filter(|(k, (_, at))| {
                **k != v
                    && now.saturating_sub(*at) <= STREAM_RELATIVE_MS
                    && self.screen.recent_detections.get(*k).is_none_or(|t| now.saturating_sub(*t) >= SHOWN_RECENTLY_MS)
                    && !twins.contains(*k)
            })
            .map(|(_, a)| a)
            .collect();
        let df = |h: &StreamHit| if h.confirmed { 0 } else { h.df };
        near.sort_by(|(x, xa), (y, ya)| df(x).cmp(&df(y)).then(y.weight().total_cmp(&x.weight())).then(ya.cmp(xa)).then(x.verse.cmp(&y.verse)));
        near.into_iter().take(5).map(|(h, _)| h.verse).collect()
    }

    /// Words the sermon keeps coming back to, and the verses most about them.
    pub(crate) fn note_topic_words(&mut self, text: &str, now: u64) {
        for w in crate::lexical::text::norm(text).split(' ').filter(|w| w.len() >= 5) {
            *self.paths.topic_counts.entry(w.to_string()).or_default() += 1.0;
        }
        if now.saturating_sub(self.paths.topic_built_at) < TOPIC_BUILD_INTERVAL_MS {
            return;
        }
        self.paths.topic_built_at = now;
        let candidates: Vec<String> = self.paths.topic_counts.iter().filter(|(_, c)| **c >= TOPIC_WORD_MIN_COUNT).map(|(w, _)| w.clone()).collect();
        self.paths.topic_counts.retain(|_, c| {
            *c *= TOPIC_DECAY;
            *c >= 1.0
        });
        if candidates.len() < 3 {
            return;
        }
        let lex = &self.k.lexicon;
        let counts = &self.paths.topic_counts;
        let mut words: Vec<(String, f32)> = candidates.into_iter().map(|w| (lex.idf_of(&w), w)).filter(|(idf, _)| *idf >= TOPIC_WORD_MIN_IDF).map(|(idf, w)| (w, idf)).collect();
        words.sort_by(|a, b| {
            let (fa, fb) = (counts.get(&a.0).copied().unwrap_or(0.0), counts.get(&b.0).copied().unwrap_or(0.0));
            (fb * b.1).total_cmp(&(fa * a.1)).then(a.0.cmp(&b.0))
        });
        words.truncate(TOPIC_MAX_WORDS);
        if words.len() < 2 {
            return;
        }
        let list: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
        if let Some(library) = topic_library(lex, &list) {
            if !library.is_empty() {
                let size = library.len();
                self.paths.topic_library = Some(library);
                self.effect(Effect::TopicLibrary { size, words: list.iter().take(6).map(|w| w.to_string()).collect() });
            }
        }
    }

    /// Three near-complete quotes in half a minute: the preacher is reading.
    fn track_reading_mode(&mut self, v: VerseId, similarity: f32, now: u64) {
        if similarity < READ_SIMILARITY {
            return;
        }
        let m = &mut self.reading_mode;
        m.hits += 1;
        if now.saturating_sub(m.last_reset) < READING_MODE_WINDOW_MS {
            return;
        }
        m.last_reset = now;
        let was = m.active;
        m.active = m.hits >= READING_MODE_HITS;
        let r = self.k.verse_ref(v);
        if m.active {
            m.chapter = Some((r.book, r.chapter));
        }
        m.hits = 0;
        if was != m.active {
            let (active, chapter) = (m.active, m.active.then_some(v));
            self.effect(Effect::ReadingMode { active, chapter });
        }
    }

    pub(crate) fn process_fingerprint(&mut self, segment: &str, now: u64) {
        let buffer: Vec<&str> = self.t.finals.iter().filter(|f| !f.has_ref).map(|f| f.text.as_str()).collect();
        let combined = format!("{} {segment}", buffer.join(" ")).trim().to_string();
        let names_book = |t: &str| t.split_whitespace().any(|w| self.k.books.is_single_word(&w.to_lowercase().chars().filter(|c| c.is_ascii_lowercase()).collect::<String>()));
        let mut texts: Vec<String> = combined
            .split(['.', '!', '?', ',', ';'])
            .map(str::trim)
            .filter(|c| c.split_whitespace().count() >= FINGERPRINT_MIN_WORDS && !names_book(c))
            .map(str::to_string)
            .collect();
        texts.reverse();
        let words: Vec<&str> = combined.split_whitespace().collect();
        let fallback = words[words.len().saturating_sub(FINGERPRINT_FALLBACK_WORDS)..].join(" ");
        if !names_book(&fallback) {
            texts.push(fallback);
        }
        let mut seen = HashSet::new();
        texts.retain(|t| t.split_whitespace().count() >= FINGERPRINT_MIN_WORDS && seen.insert(t.clone()));
        if texts.is_empty() {
            return;
        }

        let recent = self.recent_citations_for_nudge(now);
        let lex = &self.k.lexicon;
        let corpus = &self.k.corpus;
        let mut best = crate::lexical::FingerprintResult { hits: Vec::new(), confidence: Confidence::None };
        let mut found_in = String::new();
        if let Some(library) = &self.paths.topic_library {
            for t in &texts {
                let r = fingerprint(lex, corpus, t, FINGERPRINT_LIMIT, &recent, Some(library));
                if r.confidence > best.confidence {
                    best = r;
                    found_in = t.clone();
                }
                if best.confidence == Confidence::High {
                    break;
                }
            }
        }
        if best.confidence < Confidence::Medium {
            for t in &texts {
                let r = fingerprint(lex, corpus, t, FINGERPRINT_LIMIT, &recent, None);
                if r.confidence > best.confidence {
                    best = r;
                    found_in = t.clone();
                }
                if best.confidence == Confidence::High {
                    break;
                }
            }
        }
        if best.hits.is_empty() || best.confidence == Confidence::None {
            return;
        }
        let recent_direct = now.saturating_sub(self.screen.last_citation_at) < DIRECT_REF_SUPPRESS_MS;
        let confidence = best.confidence;
        let evidence = move |score: f32| Evidence { similarity: Some(score), confidence: Some(confidence), ..Default::default() };
        if !recent_direct {
            let qualifying: Vec<(VerseId, f32)> = best.hits.iter().copied().filter(|(_, s)| *s >= SUGGESTION_MIN_SCORE).take(3).collect();
            if let Some(&(v, s)) = qualifying.first() {
                let verses = qualifying.iter().map(|h| h.0).collect();
                self.decide(Candidate::new(verses, Method::Fingerprint, s, Target::Suggestions).evidence(evidence(s)).from_text(&found_in), now);
                let r = self.r(v);
                self.note_cited(r.book, Some(r.chapter), Some(r.verse), false, now);
            }
        }
        // Within the chapters the sermon points at.
        if let Some(scope) = self.scope_verses(now) {
            let r = fingerprint(&self.k.lexicon, &self.k.corpus, &texts[0], FINGERPRINT_LIMIT, &recent, Some(&scope));
            if let Some(&(v, s)) = r.hits.first() {
                let margin = r.hits.get(1).map_or(1.0, |h| s - h.1);
                let repeats = !recent_direct && best.hits.iter().any(|h| h.0 == v);
                if s >= SCOPED_MIN_SCORE && margin >= SCOPED_MARGIN && !repeats {
                    let c = r.confidence;
                    self.decide(
                        Candidate::new(vec![v], Method::Fingerprint, s, Target::Suggestions)
                            .evidence(Evidence { similarity: Some(s), confidence: Some(c), ..Default::default() })
                            .from_text(&texts[0]),
                        now,
                    );
                }
            }
        }
    }

    fn recent_citations_for_nudge(&self, now: u64) -> Vec<RecentCitation> {
        let mut out: Vec<RecentCitation> = self
            .recently_cited(now)
            .filter_map(|c| {
                let chapter = c.chapter?;
                let verse = self.k.id(VerseRef::new(c.book, chapter, c.verse.unwrap_or(1)))?;
                Some(RecentCitation { verse, whole_chapter: c.verse.is_none(), age_ms: now.saturating_sub(c.at) })
            })
            .collect();
        out.reverse(); // most recent first
        out
    }

    /// The chapters the sermon points at: books named in the last few words,
    /// a person's chapters, the chapter on screen and its neighbours.
    pub(crate) fn scope_verses(&self, now: u64) -> Option<HashSet<VerseId>> {
        let mut chapters: HashSet<(Book, u16)> = HashSet::new();
        let recent = self.paraphrase_recent_text();
        let words: Vec<&str> = recent.split_whitespace().collect();
        for b in book_mentions(&words[words.len().saturating_sub(25)..].join(" "), self.k.books, self.cfg.lang) {
            for c in 1..=self.k.corpus.chapter_count(b) {
                chapters.insert((b, c));
            }
        }
        if let Some((name, at)) = &self.person {
            if now.saturating_sub(*at) < NAMED_PERSON_WINDOW_MS {
                if let Some(list) = self.k.name_chapters(name) {
                    chapters.extend(list.iter().copied());
                }
            }
        }
        if let (Some(up), Some((_, at))) = (self.screen.up_verse(), self.screen.last_sent_book) {
            if now.saturating_sub(at) < SAME_BOOK_WINDOW_MS {
                let u = self.r(up);
                for c in [u.chapter.saturating_sub(1), u.chapter, u.chapter + 1] {
                    if c >= 1 {
                        chapters.insert((u.book, c));
                    }
                }
            }
        }
        if chapters.is_empty() {
            return None;
        }
        let mut verses = HashSet::new();
        for (b, c) in chapters {
            for v in 1..=self.k.corpus.verse_count(b, c).unwrap_or(0) {
                if let Some(id) = self.k.id(VerseRef::new(b, c, v)) {
                    verses.insert(id);
                }
            }
        }
        Some(verses)
    }

    /// The last four finals (at least 40 words of them) and the live text, at
    /// most 60 words.
    pub(crate) fn paraphrase_recent_text(&self) -> String {
        let all = format!("{} {}", self.t.recent_finals(4, 40), self.t.interim);
        let words: Vec<&str> = all.split_whitespace().collect();
        words[words.len().saturating_sub(60)..].join(" ")
    }

    /// Words for comparing speech with a verse: the KJV and the church's own wording.
    pub(crate) fn match_text(&self, v: VerseId) -> String {
        let kjv = self.k.corpus.kjv().text(v).unwrap_or("");
        match self.k.lexicon.own().and_then(|o| o.text(v)) {
            Some(own) if own != kjv => format!("{kjv} {own}"),
            _ => kjv.to_string(),
        }
    }
}

/// How much a shared 4-word phrase says, by how many verses have it.
fn anchor_similarity(df: u8) -> f32 {
    match df {
        0 | 1 => 0.85,
        2 => 0.80,
        3 => 0.76,
        4 => 0.72,
        _ => 0.68,
    }
}

#[cfg(test)]
mod tests {
    use super::clause_list;

    #[test]
    fn long_runs_are_cut_into_windows() {
        let run = (0..30).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let list = clause_list(&run, 6);
        assert!(list.iter().any(|c| c.split_whitespace().count() == 14));
        assert_eq!(list.iter().filter(|c| **c == run).count(), 1, "the whole text once");
    }
}
