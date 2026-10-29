//! A cited range being read (`detection-spec/07-ranges-triggers-context.md`).

use std::collections::HashSet;

use kairo_bible::{VerseId, VerseRef};

use super::core::{Candidate, Engine};
use super::decide::RangeAdvance;
use super::event::Effect;
use super::evidence::{meaningful_words, plain_tokens};
use super::scoring::SAME_BOOK_WINDOW_MS;
use super::types::{Method, Target};
use crate::lexical::stem::heal;
use crate::lexical::text::{is_stop_word, norm};
use crate::reference::book_mentions;

const ADVANCE_COOLDOWN_MS: u64 = 1_200;
/// A text match earlier in the chapter on screen is promoted only with this share of its words said
/// in the last stretch of speech (reading on, later in the chapter, keeps the plain rule).
const BACK_IN_CHAPTER_SHARE: f32 = 0.5;
const BACK_IN_CHAPTER_SPEECH_MS: u64 = 15_000;
/// A range not moved for this long has paused; skipping ahead then takes this much identifying
/// weight in the verse's own (not everyday) words.
const PAUSED_READING_MS: u64 = 25_000;
const PAUSED_SKIP_MIN_IDF: f32 = 9.0;
/// The next verse's first words are looked for among this many of the newest words (plus their own).
const OPENING_LOOKBACK: usize = 4;
const HEARD_MAX_WORDS: usize = 160;
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
        self.range.soft_quoted = false;
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

    /// Every way a range moves goes through here, so two triggers can't both advance it. `why` names
    /// the trigger in the explanation (a move otherwise shows as a bare direct send).
    pub(crate) fn request_range_advance(&mut self, why: &str, now: u64) -> bool {
        let Some(next) = self.range.queue.pop_front() else { return false };
        self.range.last_advance_at = now;
        let from = self.range.current;
        self.note(|e| format!("range moves to {}: {why}", e.label(next)));
        self.range.current = Some(next);
        self.range_moved(from, now);
        if self.range.soft {
            // Read on: hold the next ones again, and it's a reading now, not a quote.
            self.range.soft_quoted = false;
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
            self.request_range_advance("the verse read to its end", now);
        }
    }

    /// The current verse read to its end: followed word by word, in order, in any of its wordings.
    /// Its last words heard anywhere isn't enough — "him", "said" or "Abel" said early in the verse
    /// put Genesis 4:9 up while 4:8 was still being read (path audit).
    fn current_ending_heard(&self) -> bool {
        let Some(current) = self.range.current else { return false };
        let heard: Vec<String> = plain_tokens(&self.range_heard_words().join(" "));
        self.verse_wordings(current).into_iter().any(|text| read_to_end(&heard, &plain_tokens(text)))
    }

    /// The next verse's opening heard — words of its own, not the current verse's.
    pub(crate) fn advance_range_on_next_opening(&mut self, fresh: &str, now: u64) {
        if !self.can_advance(now) {
            return;
        }
        let current = self.range.current.unwrap();
        let next = *self.range.queue.front().unwrap();
        // A verse as short as its own opening ("Jesus wept") is read once it's up: the words that
        // put it there were all of it (11:36 came 4 s late after 11:35).
        let short = self.verse_wordings(current).into_iter().all(|t| plain_tokens(t).len() <= SHORT_VERSE_WORDS);
        let ended = short || self.current_ending_heard();
        // A held continuation: the verse on screen read to its end first, then the next one's own opening.
        if self.range.soft && !ended {
            return;
        }
        // Read word for word: the next verse's first words, in order, are what was just said ("And
        // said, O" for 2 Chronicles 20:6). Everyday words in sequence after the verse's end say more
        // than one rare word anywhere; waiting for two of its rare words held 20:6 to its 8th word,
        // most of the verse (owner, live 2026-10-27). Four after a quote, three in a cited passage.
        // Only once the verse up has been read to its end: a called range's next verse opening "And
        // the LORD" mustn't move it when the verse being read says "and the LORD" mid-way (review).
        let k = if self.range.soft_quoted { 4 } else if ended { 2 } else { 3 };
        let said_now = plain_tokens(fresh);
        // Among the newest words: they arrive a chunk at a time ("…court and said", then "said O LORD").
        let recent = &said_now[said_now.len().saturating_sub(k + OPENING_LOOKBACK)..];
        // In any wording: Ezekiel 12:25 read in the NLT opens "For I am the LORD! If I say it".
        // Two words once the verse was read to its end, if they aren't both everyday ones ("and the").
        let opens = self.verse_wordings(next).into_iter().any(|t| {
            let tokens = plain_tokens(t);
            // "And said, O LORD": two everyday words open it, so three are needed after all.
            let k = if k == 2 && tokens.iter().take(2).all(|w| is_stop_word(w)) { 3 } else { k };
            let start: Vec<String> = tokens.into_iter().take(k).collect();
            start.len() == k && recent.windows(k).any(|w| w == &start[..])
        });
        if opens && (self.range.soft || ended) {
            self.request_range_advance("the next verse's opening words", now);
            return;
        }
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
        // Held after a quote, one word of the next verse is chance: Galatians 3:13 quoted in passing,
        // then "blessing" put 3:14 up; Deuteronomy 12:13, then "place", 12:14 (dev sermons). Two.
        // Held verses too: "…God has put in your heart" put 2 Chronicles 26:16 ("his heart was lifted
        // up") up 20 s after 26:15 was read (live recording, 2026-10-28).
        let need = if self.range.soft { 2 } else { 1 };
        let matched = if identifying.len() >= need { hit(&identifying) >= need } else { hit(&own) >= need + 1 };
        if matched {
            self.request_range_advance("the next verse's own words", now);
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
        // Everyday words don't skip a reading ahead: a prayer after Mark 8:25, "…in the name of
        // Jesus. The Bible says that you should ask…", shared "Jesus", "ask", "say" and "that" with
        // Mark 8:27 and put it up (YouTube captions, Hallelujah Challenge 2026-10-27).
        let weight = |e: &Engine, v: VerseId| e.shared_weight(v, &text).0;
        let best = ahead.iter().copied().fold(ahead[0], |a, b| if weight(self, b) > weight(self, a) { b } else { a });
        let (wb, wc) = (weight(self, best), weight(self, current));
        if wb < CHAPTER_KEYWORD_MIN_IDF || wb - wc < CHAPTER_KEYWORD_MIN_MARGIN {
            return;
        }
        // Long after the last move the reading has paused, and everyday words don't resume it: a
        // prayer 36 s after Mark 8:25, "…in the name of Jesus. The Bible says that you should ask…",
        // put 8:27 up on "Jesus", "ask", "say" (YouTube captions, Hallelujah Challenge 2026-10-27).
        // Mid-reading, a few words are enough ("what God has promised it was": Romans 4:21).
        let paused = now.saturating_sub(self.range.moved_at) > PAUSED_READING_MS;
        if paused && self.shared_weight_meaningful(best, &text) < PAUSED_SKIP_MIN_IDF {
            return;
        }
        // A paused reading resumes with a stretch of the verse read word for word; a prayer reusing its
        // words doesn't. Daniel 9:5–6 read in the NLT, then "we are sorry, we have sinned, we have done
        // wrong… we have rebelled against you… we have not followed your word" put 9:8 and 9:9 up,
        // passing over 9:7 (live, 2026-10-28). "The Lord shall alter his voice before his army" is
        // Joel 2:11 read on (a word misheard). Mid-reading, a few words still carry it ("that what God
        // has promised, he was also able to perform" is three of Romans 4:21's own words).
        let said = plain_tokens(&text);
        let read = !paused || self.verse_wordings(best).into_iter().any(|t| {
            let (run, own) = read_run(&said, &plain_tokens(t));
            run >= RESUME_RUN_WORDS && own >= RESUME_RUN_OWN
        });
        if !read {
            self.note(|e| format!("range stays on {}: {} shares words, not read word for word", e.label(current), e.label(best)));
            return;
        }
        while self.range.queue.front().is_some_and(|v| *v != best) {
            self.range.queue.pop_front();
        }
        self.request_range_advance("its words read", now);
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

    /// `shared_weight` without the everyday words ("the", "that", "say").
    fn shared_weight_meaningful(&self, v: VerseId, text: &str) -> f32 {
        let said: HashSet<String> = norm(text).split(' ').filter(|w| w.len() >= 3 && !is_stop_word(w)).map(heal).collect();
        let mut seen: HashSet<String> = HashSet::new();
        norm(self.verse_text(v))
            .split(' ')
            .filter(|w| w.len() >= 3 && !is_stop_word(w) && seen.insert(w.to_string()) && said.contains(&heal(w)))
            .map(|w| self.k.lexicon.idf_of(w))
            .sum()
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
        self.request_range_advance("its verse called", now)
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
        // Held after a quote, a meaning search alone doesn't read on: Revelation 12:11 quoted, then a
        // prayer ("…no cause for hindrance… entering into your celebration") put 12:12 up on a meaning
        // match (YouTube captions, 2026-10-27). Words of the verse do.
        if self.range.soft_quoted && matches!(c.method, Method::Semantic | Method::Paraphrase | Method::Fingerprint) {
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
        if self.request_range_advance("found by detection", now) { RangeAdvance::Moved } else { RangeAdvance::No }
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
        if v.book != u.book || v.chapter != u.chapter || v.verse == u.verse {
            return false;
        }
        // Back in the chapter, a few shared words aren't the preacher going back: "how much more
        // will your Father in heaven give" (Matthew 7:11) put Luke 12:28 up after 12:32 (YouTube
        // captions, Hallelujah Challenge 2026-10-27). Going back takes the verse's own words.
        if v.verse < u.verse && c.method.is_text_match() && self.share_said(c.verses[0], &self.t.words.recent(now, BACK_IN_CHAPTER_SPEECH_MS)) < BACK_IN_CHAPTER_SHARE {
            return false;
        }
        true
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

/// `verse` read through to its end in `heard`: its words followed in order (a few may be skipped or
/// misheard), at least READ_THROUGH of them, the last or the one before it among them. Everyday words
/// only count where they're expected next, so "and the" said before the reading can't carry it ahead.
fn read_to_end(heard: &[String], verse: &[String]) -> bool {
    if verse.is_empty() {
        return false;
    }
    // The reading may be joined partway: the words that moved the range onto this verse are spent
    // already, so the first of its own words heard (in its first half) is where the reader is.
    let Some((first, start)) = heard.iter().enumerate().find_map(|(i, w)| (!is_stop_word(w)).then(|| verse.iter().position(|v| v == w).filter(|p| *p <= verse.len() / 2).map(|p| (i, p))).flatten()) else {
        return false;
    };
    let (mut at, mut matched, mut last) = (start, 0usize, None);
    let need = verse.len() - start;
    for w in &heard[first..] {
        let reach = if is_stop_word(w) { 1 } else { READ_SKIP + 1 };
        if let Some(gap) = (0..reach).find(|g| verse.get(at + g) == Some(w)) {
            last = Some(at + gap);
            at += gap + 1;
            matched += 1;
            if at >= verse.len() {
                break;
            }
        }
    }
    last.is_some_and(|l| l + 2 >= verse.len()) && matched as f32 >= READ_THROUGH * need as f32
}

/// The longest stretch of `verse` read nearly word for word in `said`: its words in order with at most
/// RUN_GAP words missed or misheard between two of them, on either side. Returns the words matched
/// and how many of them aren't everyday words.
pub(super) fn read_run(said: &[String], verse: &[String]) -> (usize, usize) {
    let mut run = vec![vec![(0usize, 0usize); verse.len()]; said.len()];
    let mut best = (0, 0);
    for i in 0..said.len() {
        for j in 0..verse.len() {
            if said[i] != verse[j] {
                continue;
            }
            let mut prev = (0, 0);
            for pi in i.saturating_sub(RUN_GAP + 1)..i {
                for pj in j.saturating_sub(RUN_GAP + 1)..j {
                    prev = prev.max(run[pi][pj]);
                }
            }
            run[i][j] = (prev.0 + 1, prev.1 + usize::from(!is_stop_word(&said[i])));
            best = best.max(run[i][j]);
        }
    }
    best
}

/// A paused reading resumes on a run of this many of the verse's words, this many not everyday ones.
const RESUME_RUN_WORDS: usize = 5;
const RESUME_RUN_OWN: usize = 2;
const RUN_GAP: usize = 2;

/// read_to_end: verse words that may be skipped between two heard ones, and the share that must be heard.
const READ_SKIP: usize = 3;
/// A verse this short is read by the time it's up.
const SHORT_VERSE_WORDS: usize = 4;
const READ_THROUGH: f32 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;

    fn run(said: &str, verse: &str) -> (usize, usize) {
        read_run(&plain_tokens(said), &plain_tokens(verse))
    }

    #[test]
    fn a_reading_runs_a_prayer_scatters() {
        // Joel 2:11 read on, "utter" misheard.
        let joel = "And the LORD shall utter his voice before his army: for his camp is very great";
        assert!(run("said the Lord shall alter his voice before his army", joel).0 >= RESUME_RUN_WORDS);
        // A prayer in Daniel 9:8's words (NLT): its words, not its reading.
        let dan = "O LORD, we and our kings, princes, and ancestors are covered with shame because we have sinned against you.";
        let prayer = "Lord Father, we are sorry, Lord, oh God, we are sorry we have sinned, we have done wrong, Lord, oh God, we have done abominable things, oh God all mandible sanctory and we have rebelled against you";
        assert!(run(prayer, dan).0 < RESUME_RUN_WORDS);
    }
}
