//! The engine and its funnel: every segment of speech, from any recogniser,
//! goes through `on_speech` — one path, so every improvement reaches every
//! engine (`detection-spec/00-funnel.md`).

use std::collections::HashMap;
use std::sync::Arc;

use kairo_bible::{Book, VerseId, VerseRef};

use super::check::CitationCheck;
use super::citations::CitationState;
use super::event::{Answer, Effect, Event, Output, Segment, TimerId};
use super::meaning::{MeaningState, MeaningThresholds};
use super::paths::{PathState, ReadingMode};
use super::twins::TwinState;
use super::scoring::Ledger;
use super::state::{Cited, Range, Screen, Transcript};
use super::types::{Evidence, Method, Target};
use crate::knowledge::Knowledge;
use crate::lexical::StreamMatcher;
use crate::reference::{Active, Lang, ReadingContext};

#[derive(Clone)]
pub struct Config {
    pub lang: Lang,
    pub auto_send: bool,
    pub auto_correct: bool,
    /// The offline recogniser: its newest interim word may still be growing.
    pub offline: bool,
    /// The runtime answers model requests (meaning search, cross-encoder).
    pub meaning: bool,
    /// Say why each decision went where it did (`Output::notes`).
    pub explain: bool,
    pub meaning_thresholds: MeaningThresholds,
}

impl Default for Config {
    fn default() -> Self {
        Config { lang: Lang::En, auto_send: true, auto_correct: true, offline: false, meaning: false, explain: false, meaning_thresholds: MeaningThresholds::LEGACY }
    }
}

/// A verse (or several) a path proposes, and how sure it is.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub verses: Vec<VerseId>,
    pub method: Method,
    /// The path's own score for the top verse.
    pub score: f32,
    /// Where the path would put it.
    pub intent: Target,
    pub evidence: Evidence,
    /// The path judged its evidence too weak to send alone.
    pub cap: bool,
    /// The full-index cross-check picked another verse.
    pub verbatim_disagreed: bool,
    /// Already chosen over its near-duplicate twin.
    pub twin_resolved: bool,
    pub corrected_from: Option<VerseId>,
    /// The speech the path found it in, when that's a stretch of its own.
    pub source: Option<String>,
}

impl Candidate {
    pub fn new(verses: Vec<VerseId>, method: Method, score: f32, intent: Target) -> Candidate {
        Candidate {
            verses,
            method,
            score,
            intent,
            evidence: Evidence { similarity: Some(score), ..Default::default() },
            cap: false,
            verbatim_disagreed: false,
            twin_resolved: false,
            corrected_from: None,
            source: None,
        }
    }

    pub fn from_text(mut self, text: &str) -> Candidate {
        self.source = Some(text.to_string());
        self
    }

    pub fn evidence(mut self, e: Evidence) -> Candidate {
        self.evidence = e;
        self
    }

    pub fn capped(mut self) -> Candidate {
        self.cap = true;
        self
    }
}

/// A chapter named with no verse yet, and what's been said since.
#[derive(Clone, Debug)]
pub struct ChapterPending {
    pub book: Book,
    pub chapter: u16,
    pub text: String,
    pub at: u64,
}

/// A stream match waiting for a second method.
#[derive(Clone, Debug)]
pub struct Held {
    pub candidate: Candidate,
    pub at: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Throttles {
    pub next_verse_at: u64,
    pub bare_number_at: u64,
    pub verse_jump_at: u64,
    pub verse_end_at: u64,
}

pub struct Engine {
    pub(crate) k: Arc<Knowledge>,
    pub(crate) cfg: Config,
    pub(crate) t: Transcript,
    pub(crate) screen: Screen,
    pub(crate) range: Range,
    pub(crate) ledger: Ledger,
    pub(crate) reading: ReadingContext,
    pub(crate) sermon: Vec<Cited>,
    pub(crate) stream: StreamMatcher,
    /// The person most recently named, and when.
    pub(crate) person: Option<(String, u64)>,
    pub(crate) chapter_pending: Option<ChapterPending>,
    pub(crate) chapter_resolved_for: Option<(Book, u16)>,
    pub(crate) awaiting: HashMap<VerseId, Held>,
    pub(crate) throttle: Throttles,
    /// Decisions to re-run once the current one is done (late corroboration).
    pub(crate) redo: Vec<Candidate>,
    pub(crate) cites: CitationState,
    pub(crate) twins: TwinState,
    pub(crate) paths: PathState,
    pub(crate) reading_mode: ReadingMode,
    /// The passages the sermon announced ("Mark chapter four verses twenty one to thirty four"):
    /// book, chapter and when, newest last. They stay in play far longer than a passing citation;
    /// a later one (MacArthur's aside to "chapter six, verses seven to thirteen") doesn't end the first.
    pub(crate) announced: Vec<(Book, u16, u64)>,
    pub(crate) check: Option<CitationCheck>,
    pub(crate) meaning: MeaningState,
    next_id: u32,
    /// The time of the event being handled.
    pub(crate) last_now: u64,
    pub(crate) out: Output,
}

/// A person named this recently still counts.
pub const NAMED_PERSON_WINDOW_MS: u64 = 45_000;
const MAX_CITED: usize = 8;
pub const CONTEXT_WINDOW_MS: u64 = 5 * 60 * 1000;
/// An announced passage stays in play this long (a sermon spends most of its time on its text).
pub const ANNOUNCED_PASSAGE_MS: u64 = 40 * 60 * 1000;
/// Announced passages kept.
pub const ANNOUNCED_MAX: usize = 3;

impl Engine {
    pub fn new(k: Arc<Knowledge>, cfg: Config) -> Engine {
        Engine {
            k,
            cfg,
            t: Transcript::default(),
            screen: Screen::default(),
            range: Range::default(),
            ledger: Ledger::default(),
            reading: ReadingContext::new(),
            sermon: Vec::new(),
            stream: StreamMatcher::new(),
            person: None,
            chapter_pending: None,
            chapter_resolved_for: None,
            awaiting: HashMap::new(),
            throttle: Throttles::default(),
            redo: Vec::new(),
            cites: CitationState::default(),
            twins: TwinState::default(),
            paths: PathState::default(),
            reading_mode: ReadingMode::default(),
            announced: Vec::new(),
            check: None,
            meaning: MeaningState::default(),
            next_id: 0,
            last_now: 0,
            out: Output::default(),
        }
    }

    pub fn knowledge(&self) -> &Knowledge {
        &self.k
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Settings changed mid-service (auto-send, auto-correct, the engine, meaning models).
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.cfg
    }

    /// Handle one event that happened at `now` (ms).
    pub fn handle(&mut self, event: Event, now: u64) -> Output {
        self.last_now = now;
        match event {
            Event::Speech(seg) => self.on_speech(seg, now),
            Event::OperatorSent(v) => {
                self.screen.non_scripture_live = false;
                self.mark_shown(v, now);
            }
            Event::OperatorCited(verses) => self.operator_cited(verses, now),
            Event::Restored { up, taken_down } => self.restored(up, taken_down, now),
            Event::NonScriptureLive => self.screen.non_scripture_live = true,
            Event::Singing(on) => self.screen.singing = on,
            Event::ScreenCleared => self.screen.up = None,
            Event::RangeNext => {
                self.request_range_advance(now);
            }
            Event::RangeJump(v) => {
                self.jump_range_to(v, now);
            }
            Event::RangeClear => self.clear_range(now),
            Event::Reset => self.reset(),
            Event::UtteranceEnd => {
                if let Some(chunk) = self.t.sentence.flush() {
                    self.t.last_fingerprint = now;
                    self.process_verbatim(&chunk, false, "", now);
                    self.process_fingerprint(&chunk, now);
                }
            }
            Event::Reconnected => self.forget_utterance(),
            Event::Timer(TimerId::RestatedChapter(id)) => self.on_restated_timer(id, now),
            Event::Answer(Answer::Compare { id, scores }) => self.on_compare(id, &scores, now),
            Event::Answer(Answer::Paraphrase { id, candidates }) => self.on_paraphrase(id, candidates, now),
            Event::Answer(Answer::Scoped { id, hits }) => self.on_scoped(id, &hits, now),
            Event::Timer(TimerId::SentenceBuffer) | Event::SecondarySpeech { .. } => {}
        }
        while let Some(c) = self.redo.pop() {
            self.decide(c, now);
        }
        let mut out = std::mem::take(&mut self.out);
        self.drop_misheard_in_same_breath(&mut out.effects);
        out
    }

    /// A citation misheard and corrected in the same moment never reaches the screen: a
    /// number misheard within the chapter ("Genesis 24 verse 53" for the 24:63 being read) or a
    /// book misheard for one with the same chapter and verse (Ephesians 2:10 for Philippians
    /// 2:10). Only the corrected verse goes up; the citation as heard stays one click away in
    /// Possible Matches, and is not counted as shown.
    fn drop_misheard_in_same_breath(&mut self, effects: &mut [Effect]) {
        let fixes: Vec<(VerseId, VerseId)> = effects.iter().filter_map(|e| match e {
            Effect::Show { target: Target::Screen, corrected_from: Some(from), verses, .. } if verses.len() == 1 => Some((*from, verses[0])),
            _ => None,
        }).collect();
        if fixes.is_empty() { return; }
        let misheard = |e: &Engine, from: VerseId, to: VerseId| {
            let (a, b) = (e.r(from), e.r(to));
            (a.book == b.book && a.chapter == b.chapter) || (a.chapter == b.chapter && a.verse == b.verse)
        };
        let mut demoted = Vec::new();
        for e in effects.iter_mut() {
            if let Effect::Show { target, verses, corrected_from: None, .. } = e {
                if *target == Target::Screen && verses.len() == 1 && fixes.iter().any(|(from, to)| *from == verses[0] && misheard(self, *from, *to)) {
                    *target = Target::Suggestions;
                    demoted.push(verses[0]);
                }
            }
        }
        for v in demoted {
            self.screen.shown_this_book.remove(&v);
            self.screen.recent_detections.remove(&v);
        }
    }

    /// What belongs to the utterance being heard, not the sermon.
    fn forget_utterance(&mut self) {
        self.t.interim.clear();
        self.t.interim_sent.clear();
        self.t.interim_seen.clear();
        self.t.stream_watermark = 0;
        self.t.utterance_start = None;
        self.t.sentence = Default::default();
        self.t.last_interim_verbatim = 0;
        self.t.last_fingerprint = 0;
        self.stream.reset();
        self.check = None;
        self.cites = Default::default();
        self.range.heard.clear();
        self.range.heard_open.clear();
        self.range.spent = 0;
        self.screen.last_citation_at = 0;
    }

    fn reset(&mut self) {
        let (k, cfg) = (self.k.clone(), std::mem::take(&mut self.cfg));
        *self = Engine::new(k, cfg);
    }

    pub(crate) fn effect(&mut self, e: Effect) {
        self.out.effects.push(e);
    }

    /// The text a verse is compared with speech in: the church's own wording,
    /// else the KJV.
    pub(crate) fn verse_text(&self, v: VerseId) -> &str {
        self.k.lexicon.own().and_then(|o| o.text(v)).or_else(|| self.k.corpus.kjv().text(v)).unwrap_or("")
    }

    pub(crate) fn r(&self, v: VerseId) -> VerseRef {
        self.k.verse_ref(v)
    }

    pub(crate) fn active(&self, now: u64) -> Active {
        self.reading.active(now)
    }

    /// Records a passage as in play: named (a citation) or read (a strong text match).
    pub(crate) fn note_cited(&mut self, book: Book, chapter: Option<u16>, verse: Option<u16>, named: bool, now: u64) {
        let entry = Cited { book, chapter, verse, at: now, named };
        if let Some(i) = self.sermon.iter().position(|c| c.book == book && c.chapter == chapter && c.verse == verse) {
            // A reading of a named passage leaves the citation as it was.
            if !named && self.sermon[i].named {
                return;
            }
            self.sermon.remove(i);
        }
        self.sermon.push(entry);
        self.sermon.retain(|c| now.saturating_sub(c.at) <= CONTEXT_WINDOW_MS);
        while self.sermon.len() > MAX_CITED {
            let at = self.sermon.iter().position(|c| !c.named).unwrap_or(0);
            self.sermon.remove(at);
        }
    }

    /// The announced passage's chapter is `book` `chapter`.
    pub(crate) fn in_announced_passage(&self, book: Book, chapter: u16) -> bool {
        self.announced.iter().any(|(b, c, at)| *b == book && *c == chapter && self.last_now.saturating_sub(*at) <= ANNOUNCED_PASSAGE_MS)
    }

    pub(crate) fn recently_cited(&self, now: u64) -> impl Iterator<Item = &Cited> {
        self.sermon.iter().filter(move |c| now.saturating_sub(c.at) <= CONTEXT_WINDOW_MS)
    }

    pub(crate) fn note(&mut self, f: impl FnOnce(&Engine) -> String) {
        if self.cfg.explain {
            let n = f(self);
            self.out.notes.push(n);
        }
    }

    pub(crate) fn label(&self, v: VerseId) -> String {
        let r = self.r(v);
        format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
    }

    pub(crate) fn next_request_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    /// The funnel.
    fn on_speech(&mut self, seg: Segment, now: u64) {
        let text = fold_accents(&seg.text);
        if seg.is_final && !text.is_empty() {
            self.t.words_heard += text.split_whitespace().count() as u64;
        }
        self.note_named_person(&text, now);

        if !seg.is_final {
            let started = *self.t.utterance_start.get_or_insert(now);
            self.t.segment_started = started;
            self.t.interim = text.clone();
            self.stream_new_words(&text, false, false, now);
            self.next_verse_trigger(&text, now);
            let fresh = self.note_range_speech(&text, false, false);
            self.advance_range_on_last_words(now);
            self.advance_range_on_next_opening(&fresh, now);
            let found = self.process_references(&text, false, false, now);
            self.check_citation_siblings(&text, false, false, now);
            if !found && self.reading.is_valid(now) && !self.announces_different_book(&text, self.t.prev_final.clone(), now) {
                self.interim_partial(&text, now);
            } else if !found {
                self.on_screen_partial(&text, false, now);
            }
            self.spoken_correction(&text, false, now);
            if !found {
                let words = text.split_whitespace().count();
                if words >= 6 && now.saturating_sub(self.t.last_interim_verbatim) > 500 {
                    self.t.last_interim_verbatim = now;
                    self.process_verbatim(&text, false, "", now);
                }
                if words >= 6 && now.saturating_sub(self.t.last_fingerprint) > 1000 {
                    self.t.last_fingerprint = now;
                    self.process_fingerprint(&text, now);
                }
                self.maybe_search_meaning(false, now);
            }
            return;
        }

        self.t.segment_started = self.t.utterance_start.take().unwrap_or(now);
        self.t.interim.clear();
        self.t.push_final(&text, now);
        let pace = self.t.pace_factor();
        self.reading.set_pace(pace);
        self.stream_new_words(&text, true, seg.continues_live, now);
        let sent_from_interim = std::mem::take(&mut self.t.interim_sent);
        self.next_verse_trigger(&text, now);
        self.mark_verse_end(&text, now);
        self.note_topic_words(&text, now);

        let fresh = self.note_range_speech(&text, true, seg.continues_live);
        self.advance_range_on_last_words(now);
        self.advance_range_on_next_opening(&fresh, now);
        self.advance_range_by_content(now);

        let prev_before = self.t.prev_final.clone();
        let through = self.read_through_chapter(&text, prev_before.as_ref(), now);
        let mut found = through || self.process_references_final(&text, &sent_from_interim, seg.continues_live, now);
        self.check_citation_siblings(&text, true, false, now);
        if self.spoken_correction(&text, true, now) {
            found = true;
        } else if !found && self.reading.is_valid(now) {
            found = self.final_partial(&text, prev_before.as_ref(), now);
        } else if !found {
            found = self.on_screen_partial(&text, true, now);
        }
        if !found {
            found = self.bare_number_continuation(&text, now);
        }
        if !found {
            found = self.verse_end_jump(&text, now);
        }
        if !found {
            self.offer_named_passages(&text, now);
        }
        if let Some(last) = self.t.finals.back_mut() {
            last.has_ref = found;
        }

        // The slower text searches: on every final while a range is being
        // read (verse transitions must feel immediate), else per sentence.
        let expensive = if self.range.current.is_some() && !self.range.soft {
            self.t.sentence.flush();
            Some(text.clone())
        } else if seg.speech_final {
            self.t.sentence.append(&text, now);
            self.t.sentence.flush()
        } else {
            self.t.sentence.append(&text, now).or_else(|| self.t.sentence.timed_out(now))
        };
        match &expensive {
            Some(chunk) => {
                let chunk = chunk.clone();
                self.process_verbatim(&chunk, false, "", now);
                if now.saturating_sub(self.t.last_fingerprint) > 1000 {
                    self.t.last_fingerprint = now;
                    self.process_fingerprint(&chunk, now);
                }
                self.maybe_search_meaning(true, now);
            }
            None if text.split_whitespace().count() >= 8 => self.process_verbatim(&text, false, "", now),
            None => {}
        }

        if seg.speech_final {
            // A quote spread over several segments: the last ~60 words.
            let window = self.t.last_words(60);
            if !window.is_empty() && Some(&window) != expensive.as_ref() {
                self.process_verbatim(&window, true, &text, now);
                self.process_fingerprint(&window, now);
            }
        }
    }

    fn operator_cited(&mut self, verses: Vec<VerseId>, now: u64) {
        let Some(&first) = verses.first() else { return };
        if verses.len() > 1 {
            self.set_range(verses.clone(), now);
        } else {
            self.set_soft_range(first, now);   // the next verses held in case they're read on
        }
        let r = self.r(first);
        self.note_cited(r.book, Some(r.chapter), Some(r.verse), true, now);
        self.reading.update(r.book, Some(r.chapter as u32), now);
        self.screen.last_citation_at = now;
        self.decide(Candidate::new(verses, Method::Direct, 1.0, Target::Screen), now);
    }

    /// Back: the screen shows `up` again; `taken_down` mustn't come straight back.
    fn restored(&mut self, up: Option<VerseId>, taken_down: Option<VerseId>, now: u64) {
        match up {
            Some(v) => {
                self.mark_shown(v, now);
                if !self.range.all.is_empty() && !self.jump_range_to(v, now) {
                    self.clear_range(now);
                }
            }
            None => {
                self.screen.up = None;
                self.clear_range(now);
            }
        }
        if let Some(d) = taken_down {
            self.screen.recent_detections.insert(d, now);
            self.screen.corrected_away = up.map(|u| super::state::CorrectedAway { verse: d, replaced_by: u, at: now });
        }
    }

    /// The most recent person named in this segment (most recent wins outright).
    fn note_named_person(&mut self, text: &str, now: u64) {
        for w in text.split_whitespace().rev() {
            let w: String = w.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>().to_lowercase();
            if !w.is_empty() && self.k.is_name(&w) {
                self.person = Some((w, now));
                return;
            }
        }
    }

    /// A person named in the last 45 s appears in this verse's chapter.
    pub(crate) fn person_in_chapter(&self, v: VerseRef, now: u64) -> bool {
        self.person.as_ref().is_some_and(|(name, at)| {
            now.saturating_sub(*at) < NAMED_PERSON_WINDOW_MS && self.k.name_chapters(name).is_some_and(|c| c.binary_search(&(v.book, v.chapter)).is_ok())
        })
    }

    /// Puts a verse on screen directly (the operator, an advancing range).
    pub(crate) fn put_up(&mut self, v: VerseId, method: Method, now: u64) {
        self.effect(Effect::Show { verses: vec![v], method, score: 1.0, target: Target::Screen, corrected_from: None });
        // A text match of the same verse a moment later is then a repeat, not a new send,
        // and the next verse of the chapter is reading on (as after any send).
        self.screen.last_detected = Some((v, now, method));
        self.screen.recent_detections.insert(v, now);
        if self.cfg.auto_send {
            self.screen.last_sent = Some((v, now));
            self.screen.last_sent_book = Some((self.r(v).book, now));
        }
        self.mark_shown(v, now);
    }

    /// What going up changes: the verse up, the verses shown this book, a
    /// chapter no longer waiting for its verse.
    pub(crate) fn mark_shown(&mut self, v: VerseId, now: u64) {
        let r = self.r(v);
        if self.screen.up.is_none_or(|(u, _)| self.r(u).book != r.book) {
            self.screen.shown_this_book.clear();
        }
        self.screen.shown_this_book.insert(v);
        self.screen.up = Some((v, now));
        if self.chapter_pending.as_ref().is_some_and(|p| p.book == r.book && p.chapter == r.chapter) {
            self.chapter_pending = None;
        }
    }
}

/// Accents removed, case kept — detection sees "Joao" where the screen shows "João".
pub fn fold_accents(s: &str) -> String {
    if s.is_ascii() {
        return s.to_string();
    }
    use unicode_normalization::UnicodeNormalization;
    s.nfd().filter(|c| !('\u{0300}'..='\u{036f}').contains(c)).collect()
}
