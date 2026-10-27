//! A cited range being read (`detection-spec/07-ranges-triggers-context.md`).

use std::collections::HashSet;

use kairo_bible::{VerseId, VerseRef};

use super::core::{Candidate, Engine};
use super::decide::RangeAdvance;
use super::event::Effect;
use super::evidence::meaningful_words;
use super::scoring::SAME_BOOK_WINDOW_MS;
use super::types::{Method, Target};
use crate::lexical::stem::heal;
use crate::lexical::text::norm;
use crate::reference::book_mentions;

const ADVANCE_COOLDOWN_MS: u64 = 1_200;
const HEARD_MAX_WORDS: usize = 160;
const ENDING_SPAN: usize = 8;
const READING_SLACK: usize = 15;
const PREFIX_MIN_IDF: f32 = 3.5;
/// "…go through chapter four verse five" counts this soon after the citation, for at most this many verses.
const THROUGH_CHAPTER_MS: u64 = 20_000;
const THROUGH_CHAPTER_MAX_VERSES: usize = 60;
/// A range this long is the sermon's passage.
const ANNOUNCED_MIN_VERSES: usize = 3;
const ADVANCE_MIN_SCORE: f32 = 0.50;
const CONTENT_LOOKAHEAD: usize = 3;
/// What "just said" means for skipping ahead: at least this many words.
const CONTENT_MIN_WORDS: usize = 10;
/// A chapter's verse is chosen from speech only with this much identifying
/// weight, and this lead over the next best.
pub const CHAPTER_KEYWORD_MIN_IDF: f32 = 14.0;
pub const CHAPTER_KEYWORD_MIN_MARGIN: f32 = 2.0;
/// The verse a range left can't come straight back without its own words for this long.
const LEFT_VERSE_HOLD_MS: u64 = 8_000;
/// A single cited verse holds this many verses after it, topped up as each is read.
const SOFT_AHEAD: usize = 2;
/// A held continuation lets go when nothing has moved it for this long.
const SOFT_RANGE_TTL_MS: u64 = 120_000;

impl Engine {
    pub(crate) fn set_range(&mut self, verses: Vec<VerseId>, now: u64) {
        self.range.soft = false;
        self.range.all = verses.clone();
        self.range.current = verses.first().copied();
        self.range.queue = verses.iter().skip(1).copied().collect();
        self.range_moved(None, now);
        // A passage of several verses is what the sermon is on.
        if let (Some(first), Some(last)) = (verses.first(), verses.last()) {
            let (a, b) = (self.r(*first), self.r(*last));
            if verses.len() >= ANNOUNCED_MIN_VERSES && a.book == b.book && a.chapter == b.chapter {
                self.announce(a.book, a.chapter, now);
            }
        }
        if let Some(current) = self.range.current {
            self.effect(Effect::RangeSet { verses, current });
        }
    }

    /// "Second Timothy chapter three. We're going to start at verse fourteen and go through chapter
    /// four verse five" (Piper, dev build 2026-10-21): the verse just cited is read on into the
    /// next chapter, and both chapters are the sermon's passage.
    pub(crate) fn read_through_chapter(&mut self, text: &str, prev: Option<&(String, u64)>, now: u64) -> bool {
        let Some((up, at)) = self.screen.up else { return false };
        if now.saturating_sub(at) > THROUGH_CHAPTER_MS || self.screen.last_citation.is_none_or(|(c, _, _)| c != up) {
            return false;
        }
        let joined = match prev {
            Some((p, t)) if now.saturating_sub(*t) < THROUGH_CHAPTER_MS => format!("{p} {text}"),
            _ => text.to_string(),
        };
        let words = super::citations::tokens(&joined);
        let w: Vec<&str> = words.iter().map(String::as_str).filter(|x| !matches!(*x, "uh" | "um")).collect();
        // "through", not "to": "now turn to chapter four verse five" is a new citation, not reading on.
        let Some(i) = w.windows(2).position(|p| matches!(p[0], "through" | "thru" | "until" | "till") && p[1] == "chapter") else { return false };
        let Some(ch) = crate::reference::numbers::consume_number(&w, i + 2, Some(150)) else { return false };
        let j = i + 2 + ch.consumed;
        if w.get(j) != Some(&"verse") {
            return false;
        }
        let Some(vs) = crate::reference::numbers::consume_number(&w, j + 1, Some(176)) else { return false };
        let u = self.r(up);
        let ch = ch.value as u16;
        if ch <= u.chapter || ch > u.chapter + 2 {
            return false;
        }
        let Some(end) = self.k.id(VerseRef::new(u.book, ch, vs.value as u16)) else { return false };
        let verses: Vec<VerseId> = (up.0..=end.0).map(VerseId).collect();
        if verses.len() > THROUGH_CHAPTER_MAX_VERSES {
            return false;
        }
        self.note(|e| format!("reading on from {} through {}", e.label(up), e.label(end)));
        self.set_range(verses, now);
        for c in u.chapter..=ch {
            self.announce(u.book, c, now);
        }
        true
    }

    /// The verses after `v` in its chapter, at most `n`.
    fn verses_after(&self, v: VerseId, n: usize) -> Vec<VerseId> {
        let r = self.r(v);
        let mut out = Vec::new();
        let mut id = v.0 + 1;
        while out.len() < n && (id as usize) < self.k.corpus.len() {
            let next = VerseId(id);
            let nr = self.r(next);
            if nr.book != r.book || nr.chapter != r.chapter {
                break;
            }
            out.push(next);
            id += 1;
        }
        out
    }

    /// A single verse cited ("Ezra 4:4"): the next verses wait in case the preacher reads on. Unlike a
    /// cited range it isn't shown as one, and it moves only when the verse's end and then the next
    /// verse's own opening words are heard (or the next verse is found or called outright).
    pub(crate) fn set_soft_range(&mut self, v: VerseId, now: u64) {
        let next = self.verses_after(v, SOFT_AHEAD);
        if next.is_empty() {
            self.clear_range(now);
            return;
        }
        let had = self.range.current.is_some() && !self.range.soft;
        self.range.soft = true;
        self.range.all = std::iter::once(v).chain(next.iter().copied()).collect();
        self.range.current = Some(v);
        self.range.queue = next.into_iter().collect();
        self.range_moved(None, now);
        if had {
            self.effect(Effect::RangeCleared);
        }
    }

    pub(crate) fn clear_range(&mut self, now: u64) {
        let had = self.range.current.is_some() && !self.range.soft;
        self.range.soft = false;
        self.range.all.clear();
        self.range.queue.clear();
        self.range.current = None;
        self.range_moved(None, now);
        if had {
            self.effect(Effect::RangeCleared);
        }
    }

    /// One stretch of speech moves a range once: what was said before the
    /// move is spent.
    fn range_moved(&mut self, from: Option<VerseId>, now: u64) {
        self.range.left = from;
        self.range.moved_at = now;
        self.range.heard.clear();
        self.range.heard_open.clear();
        self.range.spent = self.t.interim.split_whitespace().count();
    }

    /// Notes a segment for the range checks; returns its unspent words.
    pub(crate) fn note_range_speech(&mut self, text: &str, is_final: bool, continues_live: bool) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        let fresh = words.get(self.range.spent.min(words.len())..).unwrap_or(&[]).join(" ");
        if !is_final {
            self.range.heard_open = fresh.clone();
            return fresh;
        }
        self.range.spent = if continues_live { self.range.spent.saturating_sub(words.len()) } else { 0 };
        self.range.heard_open.clear();
        if !fresh.is_empty() && self.range.current.is_some() {
            self.range.heard.push_back(fresh.clone());
            let mut total: usize = self.range.heard.iter().map(|t| t.split_whitespace().count()).sum();
            while self.range.heard.len() > 1 && total > HEARD_MAX_WORDS {
                total -= self.range.heard.pop_front().map_or(0, |t| t.split_whitespace().count());
            }
        }
        fresh
    }

    fn range_heard_words(&self) -> Vec<String> {
        self.range.heard.iter().map(String::as_str).chain(std::iter::once(self.range.heard_open.as_str())).flat_map(|t| t.split_whitespace()).map(str::to_string).collect()
    }

    /// Every way a range moves goes through here, so two triggers can't both advance it.
    pub(crate) fn request_range_advance(&mut self, now: u64) -> bool {
        let Some(next) = self.range.queue.pop_front() else { return false };
        self.range.last_advance_at = now;
        let from = self.range.current;
        self.range.current = Some(next);
        self.range_moved(from, now);
        if self.range.soft {
            // Read on: hold the next ones again.
            let last = self.range.queue.back().copied().unwrap_or(next);
            let more = self.verses_after(last, SOFT_AHEAD.saturating_sub(self.range.queue.len()));
            self.range.all.extend(more.iter().copied());
            self.range.queue.extend(more);
        } else {
            self.effect(Effect::RangeMoved { current: next, remaining: self.range.queue.len() });
        }
        self.decide(Candidate::new(vec![next], Method::Direct, 1.0, Target::Screen).reading_on(), now);
        true
    }

    /// A verse inside the range repositions it; outside, nothing changes.
    pub(crate) fn jump_range_to(&mut self, v: VerseId, now: u64) -> bool {
        let Some(i) = self.range.all.iter().position(|x| *x == v) else { return false };
        let from = self.range.current;
        self.range.current = Some(v);
        self.range.queue = self.range.all[i + 1..].iter().copied().collect();
        self.range_moved(from, now);
        if !self.range.soft {
            self.effect(Effect::RangeMoved { current: v, remaining: self.range.queue.len() });
        }
        true
    }

    fn can_advance(&self, now: u64) -> bool {
        self.range.current.is_some()
            && !self.range.queue.is_empty()
            && now.saturating_sub(self.range.last_advance_at) >= ADVANCE_COOLDOWN_MS
            && !(self.range.soft && now.saturating_sub(self.range.moved_at) > SOFT_RANGE_TTL_MS)
    }

    /// Most of the current verse's last three words heard just now — each as
    /// many times as the verse says it.
    pub(crate) fn advance_range_on_last_words(&mut self, now: u64) {
        // A held continuation never moves on the verse's end alone: the preacher may stop there.
        if !self.can_advance(now) || self.range.soft {
            return;
        }
        if self.current_ending_heard() {
            self.request_range_advance(now);
        }
    }

    /// The current verse's last words heard just now (most of its last three, each as often as it says it).
    fn current_ending_heard(&self) -> bool {
        let Some(current) = self.range.current else { return false };
        let text = self.verse_text(current).to_string();
        let verse_words = meaningful_words(&text);
        let last3: Vec<&String> = verse_words.iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        let ending: Vec<&String> = { let mut e: Vec<&String> = Vec::new(); for w in &last3 { if !e.contains(w) { e.push(w); } } e };
        if ending.is_empty() {
            return false;
        }
        let heard_all = self.range_heard_words();
        let keep = text.split_whitespace().count() + READING_SLACK;
        let reading = heard_all[heard_all.len().saturating_sub(keep)..].join(" ");
        let heard = meaningful_words(&reading);
        let recent: HashSet<&String> = heard.iter().rev().take(ENDING_SPAN).collect();
        let times = |list: &[String], w: &String| list.iter().filter(|x| *x == w).count();
        let reached = ending.iter().filter(|w| recent.contains(**w) && times(&heard, w) >= times(&verse_words, w)).count();
        reached >= ending.len().min(2)
    }

    /// The next verse's opening heard — words of its own, not the current verse's.
    pub(crate) fn advance_range_on_next_opening(&mut self, fresh: &str, now: u64) {
        if !self.can_advance(now) {
            return;
        }
        // A held continuation: the verse on screen read to its end first, then the next one's own opening.
        if self.range.soft && !self.current_ending_heard() {
            return;
        }
        let current = self.range.current.unwrap();
        let next = *self.range.queue.front().unwrap();
        let opening: Vec<String> = meaningful_words(self.verse_text(next)).into_iter().take(4).collect();
        if opening.is_empty() {
            return;
        }
        let current_words: HashSet<String> = meaningful_words(self.verse_text(current)).into_iter().collect();
        let own: Vec<&String> = opening.iter().filter(|w| !current_words.contains(*w)).collect();
        let identifying: Vec<&String> = own.iter().copied().filter(|w| self.k.lexicon.idf_of(w) >= PREFIX_MIN_IDF).collect();
        if identifying.is_empty() && own.len() < 2 {
            return;
        }
        let book = self.r(current).book;
        if book_mentions(fresh, self.k.books, self.cfg.lang).iter().any(|b| *b != book) {
            return;
        }
        // The verse on screen read first: Mark 4:21–34 cited, "…the kind of soil that produces
        // nothing" (the background, before any reading) moved it to 4:22's "nothing hid" (dev build).
        let next_words: HashSet<String> = meaningful_words(self.verse_text(next)).into_iter().collect();
        let current_own: Vec<String> = current_words.iter().filter(|w| !next_words.contains(*w) && self.k.lexicon.idf_of(w) >= PREFIX_MIN_IDF).cloned().collect();
        if !current_own.is_empty() {
            let heard: HashSet<String> = meaningful_words(&format!("{} {fresh}", self.range_heard_words().join(" "))).into_iter().collect();
            if !current_own.iter().any(|w| heard.contains(w)) {
                return;
            }
        }
        let said: HashSet<String> = meaningful_words(fresh).into_iter().collect();
        let hit = |list: &[&String]| list.iter().collect::<HashSet<_>>().into_iter().filter(|w| said.contains(**w)).count();
        let matched = if !identifying.is_empty() { hit(&identifying) >= 1 } else { hit(&own) >= 2 };
        if matched {
            self.request_range_advance(now);
        }
    }

    /// The reading skipped ahead: one of the next few verses fits what was
    /// just said clearly better than the verse on screen.
    pub(crate) fn advance_range_by_content(&mut self, now: u64) {
        if !self.can_advance(now) {
            return;
        }
        let current = self.range.current.unwrap();
        let cr = self.r(current);
        let ahead: Vec<VerseId> = self.range.queue.iter().take(CONTENT_LOOKAHEAD).copied().filter(|v| {
            let r = self.r(*v);
            r.book == cr.book && r.chapter == cr.chapter
        }).collect();
        if ahead.is_empty() {
            return;
        }
        // The last two segments, or more when they're short (the offline
        // engine locks words in a few at a time).
        let mut taken: Vec<&str> = Vec::new();
        let mut words = 0;
        for seg in self.range.heard.iter().rev() {
            if taken.len() >= 2 && words >= CONTENT_MIN_WORDS {
                break;
            }
            words += seg.split_whitespace().count();
            taken.push(seg);
        }
        taken.reverse();
        let text = taken.join(" ");
        if text.is_empty() || book_mentions(&text, self.k.books, self.cfg.lang).iter().any(|b| *b != cr.book) {
            return;
        }
        let weight = |e: &Engine, v: VerseId| e.shared_weight(v, &text).0;
        let best = ahead.iter().copied().fold(ahead[0], |a, b| if weight(self, b) > weight(self, a) { b } else { a });
        let (wb, wc) = (weight(self, best), weight(self, current));
        if wb < CHAPTER_KEYWORD_MIN_IDF || wb - wc < CHAPTER_KEYWORD_MIN_MARGIN {
            return;
        }
        while self.range.queue.front().is_some_and(|v| *v != best) {
            self.range.queue.pop_front();
        }
        self.request_range_advance(now);
    }

    /// Identifying weight (IDF sum) and count of a verse's words whose stems
    /// were said.
    pub(crate) fn shared_weight(&self, v: VerseId, text: &str) -> (f32, usize) {
        let said: HashSet<String> = norm(text).split(' ').filter(|w| w.len() >= 3).map(heal).collect();
        let mut seen: HashSet<String> = HashSet::new();
        let (mut idf, mut hits) = (0.0, 0);
        for w in norm(self.verse_text(v)).split(' ') {
            if w.len() < 3 || !seen.insert(w.to_string()) {
                continue;
            }
            if said.contains(&heal(w)) {
                hits += 1;
                idf += self.k.lexicon.idf_of(w);
            }
        }
        (idf, hits)
    }

    /// The best two verses of a chapter for what was said, by identifying weight.
    pub(crate) fn score_chapter(&self, book: kairo_bible::Book, chapter: u16, text: &str) -> Vec<(VerseId, f32)> {
        let mut rows = self.score_chapter_all(book, chapter, text);
        rows.truncate(2);
        rows
    }

    /// Every verse of the chapter, best first (equal scores keep verse order).
    pub(crate) fn score_chapter_all(&self, book: kairo_bible::Book, chapter: u16, text: &str) -> Vec<(VerseId, f32)> {
        let Some(n) = self.k.corpus.verse_count(book, chapter) else { return Vec::new() };
        let mut rows: Vec<(VerseId, f32)> = (1..=n)
            .filter_map(|v| self.k.id(kairo_bible::VerseRef::new(book, chapter, v)))
            .map(|id| (id, self.shared_weight(id, text).0))
            .collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        rows
    }

    /// A citation of exactly the range's next verse ("verse two") moves it.
    pub(crate) fn try_range_advance_by_ref(&mut self, verses: &[VerseId], now: u64) -> bool {
        if self.range.current.is_none() || verses.len() != 1 || self.range.queue.front() != Some(&verses[0]) {
            return false;
        }
        self.request_range_advance(now)
    }

    /// A range's next verse found by any method moves the range. A text match
    /// must rest on a word only the next verse says, heard since the move.
    pub(crate) fn try_range_advance_by_detection(&mut self, c: &Candidate, now: u64) -> RangeAdvance {
        if self.range.current.is_none() || self.range.queue.is_empty() || c.score < ADVANCE_MIN_SCORE || c.verses.len() != 1 {
            return RangeAdvance::No;
        }
        let next = *self.range.queue.front().unwrap();
        if c.verses[0] != next {
            return RangeAdvance::No;
        }
        if c.method.is_text_match() {
            let current: HashSet<String> = meaningful_words(self.verse_text(self.range.current.unwrap())).into_iter().collect();
            let next_only: HashSet<String> = meaningful_words(self.verse_text(next)).into_iter().filter(|w| !current.contains(w)).collect();
            let heard = meaningful_words(&self.range_heard_words().join(" "));
            if !heard.iter().any(|w| next_only.contains(w)) {
                return RangeAdvance::Held;
            }
        }
        if self.request_range_advance(now) { RangeAdvance::Moved } else { RangeAdvance::No }
    }

    /// No range, a verse up within the minute, and the candidate is another
    /// verse of its chapter (meaning-only matches need very high confidence).
    pub(crate) fn continues_chapter(&self, c: &Candidate, now: u64) -> bool {
        if self.range.current.is_some() || c.verses.len() != 1 {
            return false;
        }
        let Some((up, _)) = self.screen.up else { return false };
        if self.screen.last_sent_book.is_none_or(|(_, t)| now.saturating_sub(t) >= SAME_BOOK_WINDOW_MS) {
            return false;
        }
        let min = if matches!(c.method, Method::Semantic | Method::Fingerprint) { super::scoring::VERY_HIGH_RAW_CONFIDENCE } else { ADVANCE_MIN_SCORE };
        if c.score < min {
            return false;
        }
        let (v, u) = (self.r(c.verses[0]), self.r(up));
        v.book == u.book && v.chapter == u.chapter && v.verse != u.verse
    }

    /// Does what was just said hold more of the candidate's words than of any one range verse?
    pub(crate) fn speech_favours_over_range(&self, v: VerseId) -> bool {
        if self.range.all.is_empty() {
            return false;
        }
        let said: HashSet<String> = self.recent_speech_words(30).into_iter().map(|w| super::evidence::stem_lite(&w)).collect();
        if said.is_empty() {
            return false;
        }
        let hits = |text: &str| meaningful_words(text).into_iter().map(|w| super::evidence::stem_lite(&w)).collect::<HashSet<_>>().iter().filter(|w| said.contains(*w)).count();
        let cand = hits(self.verse_text(v));
        let best = self.range.all.iter().map(|r| hits(self.verse_text(*r))).max().unwrap_or(0);
        cand > best
    }

    /// The verse a range just left needs its own words, said since the move.
    pub(crate) fn range_left_verse_returns_without_new_evidence(&self, v: VerseId, now: u64) -> bool {
        let Some(left) = self.range.left else { return false };
        if self.range.current.is_none() || now.saturating_sub(self.range.moved_at) > LEFT_VERSE_HOLD_MS || left != v {
            return false;
        }
        let words = meaningful_words(self.verse_text(left));
        let ending: HashSet<&String> = words.iter().rev().take(3).collect();
        let current: HashSet<String> = meaningful_words(self.verse_text(self.range.current.unwrap())).into_iter().collect();
        let own: HashSet<&String> = words.iter().filter(|w| !ending.contains(w) && !current.contains(*w)).collect();
        !meaningful_words(&self.range_heard_words().join(" ")).iter().any(|w| own.contains(w))
    }

    /// The words just spoken: the latest final (a sentence's worth) and the open interim.
    pub(crate) fn recent_speech_words(&self, max: usize) -> Vec<String> {
        let w = &self.t.words;
        let words = meaningful_words(&w.text(w.newest_from().saturating_sub(super::decide::RECENT_SPEECH_WORDS as u64)..w.end()));
        words[words.len().saturating_sub(max)..].to_vec()
    }
}
