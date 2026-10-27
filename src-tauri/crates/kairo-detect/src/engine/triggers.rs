//! Spoken triggers: "next verse", a verse number called on its own, "verse N"
//! against the passage being read (`detection-spec/07-ranges-triggers-context.md`).

use std::collections::HashSet;
use std::sync::LazyLock;

use kairo_bible::{VerseId, VerseRef};

use super::citations::{ends_on_growable_number, interim_looks_incomplete, opens_with_number, tokens, DIRECT_PARTIAL_SCORE, INTERIM_STABLE_WINDOW_MS, PREV_FINAL_JOIN_WINDOW_MS};
use super::core::{Candidate, Engine};
use super::evidence::{meaningful_words, stem_lite};
use super::types::{Hold, Method, Target};
use crate::re::re;
use crate::reference::i18n::localize;
use crate::reference::numbers::{consume_number, is_ambiguous_number_word};
use crate::reference::partial::BARE_NUMBER_FILLER;
use crate::reference::{Active, Partial, Verses};

const NEXT_VERSE_COOLDOWN_MS: u64 = 4_000;
const NEXT_VERSE_MAX_GAP_MS: u64 = 180_000;
const BARE_NUMBER_COOLDOWN_MS: u64 = 2_000;
const BARE_NUMBER_TAIL_WORDS: usize = 4;
const BARE_NUMBER_MAX_GAP_MS: u64 = 15_000;
const VERSE_END_NUMBER_WINDOW_MS: u64 = 8_000;
const VERSE_JUMP_MAX_WORDS: usize = 4;
const VERSE_JUMP_COOLDOWN_MS: u64 = 2_000;
/// A word this identifying, heard, lets a bare "verse N" in a chapter never named go up.
const PARTIAL_OWN_WORD_MIN_IDF: f32 = 4.0;
/// A verse of the same chapter on screen this recently also lets a bare "verse N" go up.
const PARTIAL_CHAPTER_UP_MS: u64 = 60_000;
/// A "verse N" resolution renews the context only if it reached the screen,
/// or that verse is already up from this recently.
const PARTIAL_CONFIRM_MAX_AGE_MS: u64 = 15_000;
/// After the reading context lapses, "verse N" still means a verse of the
/// passage on screen for this long after it went up.
const ON_SCREEN_CONTEXT_MS: u64 = 120_000;

static NEXT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"\b(next verse|following verse|verse after (that|this|it))\b"));
static BACK: LazyLock<regex::Regex> = LazyLock::new(|| re(r"\b(previous verse|verse before|preceding verse)\b"));
/// A preacher taking back the number just said: "Romans 8:28, sorry, verse 31".
static CORRECTION_WORD: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?i)\b(sorry|i mean|i meant|rather|correction|pardon me|excuse me|my mistake)\b[\s,.;:!-]*"));
static VERSE_OR_CHAPTER_NEXT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?i)^(verses?|chapter)\b"));

fn filler(w: &str) -> bool {
    BARE_NUMBER_FILLER.contains(&w)
}

impl Engine {
    fn verse_in_chapter(&self, of: VerseId, verse: i64) -> Option<VerseId> {
        let r = self.r(of);
        u16::try_from(verse).ok().filter(|v| *v >= 1).and_then(|v| self.k.id(VerseRef::new(r.book, r.chapter, v)))
    }

    /// "next verse" / "previous verse" in the last 8 words.
    pub(crate) fn next_verse_trigger(&mut self, text: &str, now: u64) {
        let tail: Vec<&str> = text.split_whitespace().collect();
        let tail = localize(&tail[tail.len().saturating_sub(8)..].join(" "), self.cfg.lang).to_lowercase();
        let back = BACK.is_match(&tail);
        if !back && !NEXT.is_match(&tail) {
            return;
        }
        if now.saturating_sub(self.throttle.next_verse_at) < NEXT_VERSE_COOLDOWN_MS {
            return;
        }
        self.throttle.next_verse_at = now;
        if !self.range.queue.is_empty() && !back {
            self.request_range_advance(now);
            return;
        }
        let Some((up, at)) = self.screen.up else { return };
        if now.saturating_sub(at) > NEXT_VERSE_MAX_GAP_MS {
            return;
        }
        let verse = self.r(up).verse as i64 + if back { -1 } else { 1 };
        if let Some(v) = self.verse_in_chapter(up, verse) {
            if back {
                self.jump_range_to(v, now);
            }
            self.decide(Candidate::new(vec![v], Method::Continuation, 1.0, Target::Screen), now);
        }
    }

    /// "…so do you, 51" with verse 50 up: the next verse called by its number.
    pub(crate) fn bare_number_continuation(&mut self, text: &str, now: u64) -> bool {
        if now.saturating_sub(self.throttle.bare_number_at) < BARE_NUMBER_COOLDOWN_MS {
            return false;
        }
        let words = tokens(text);
        let tail: Vec<&str> = words[words.len().saturating_sub(BARE_NUMBER_TAIL_WORDS)..].iter().map(String::as_str).filter(|w| !filler(w)).collect();
        if tail.is_empty() {
            return false;
        }
        let Some(num) = (0..tail.len()).find_map(|s| consume_number(&tail, s, None).filter(|n| s + n.consumed == tail.len())) else { return false };
        if num.consumed == 1 && is_ambiguous_number_word(tail[tail.len() - 1]) {
            return false; // "…waiting for" is not verse 4
        }
        let clause = text.split(['.', '!', '?', ';', ',']).map(str::trim).rfind(|c| !c.is_empty()).unwrap_or("");
        if tokens(clause).iter().filter(|w| !filler(w)).count() != num.consumed {
            return false;
        }
        let Some(base) = self.range.current.or(self.screen.up_verse()) else { return false };
        if num.value != self.r(base).verse as i64 + 1 {
            return false;
        }
        if self.range.queue.is_empty() && self.screen.up.is_none_or(|(_, at)| now.saturating_sub(at) > BARE_NUMBER_MAX_GAP_MS) {
            return false;
        }
        self.throttle.bare_number_at = now;
        if !self.range.queue.is_empty() {
            self.request_range_advance(now);
            return true;
        }
        if let Some(v) = self.verse_in_chapter(base, num.value) {
            self.decide(Candidate::new(vec![v], Method::Continuation, 1.0, Target::Screen), now);
        }
        true
    }

    /// The verse on screen was just read to its end.
    pub(crate) fn mark_verse_end(&mut self, text: &str, now: u64) {
        let Some(up) = self.screen.up_verse() else { return };
        let words = meaningful_words(self.verse_text(up));
        let tail3 = words[words.len().saturating_sub(3)..].join(" ");
        if !tail3.is_empty() && meaningful_words(text).join(" ").contains(&tail3) {
            self.throttle.verse_end_at = now;
        }
    }

    /// Just after finishing a verse, a number on its own jumps to that verse.
    pub(crate) fn verse_end_jump(&mut self, text: &str, now: u64) -> bool {
        if self.throttle.verse_end_at == 0 || now.saturating_sub(self.throttle.verse_end_at) > VERSE_END_NUMBER_WINDOW_MS {
            return false;
        }
        if now.saturating_sub(self.throttle.verse_jump_at) < VERSE_JUMP_COOLDOWN_MS {
            return false;
        }
        let Some(up) = self.screen.up_verse() else { return false };
        let all = tokens(text);
        let words: Vec<&str> = all.iter().map(String::as_str).filter(|w| !filler(w)).collect();
        if words.is_empty() || words.len() > VERSE_JUMP_MAX_WORDS {
            return false;
        }
        let Some(num) = consume_number(&words, 0, None).filter(|n| n.consumed == words.len()) else { return false };
        if num.consumed == 1 && is_ambiguous_number_word(words[0]) {
            return false;
        }
        if num.value == self.r(up).verse as i64 {
            return false;
        }
        self.throttle.verse_end_at = 0;
        self.throttle.verse_jump_at = now;
        let Some(v) = self.verse_in_chapter(up, num.value) else { return false };
        if !self.jump_range_to(v, now) {
            self.clear_range(now);
        }
        self.decide(Candidate::new(vec![v], Method::Continuation, 1.0, Target::Screen), now);
        true
    }

    /// "verse N" in speech still in progress (never a bare number there: an
    /// outline's "number two?" isn't a verse).
    pub(crate) fn interim_partial(&mut self, text: &str, now: u64) {
        let Some(p) = self.k.partials(self.cfg.lang).resolve(text, self.verse_call_context(now), false) else { return };
        let p = self.announced_instead(p, text);
        let Verses::One(verse) = p.citation.verses else { return };
        if interim_looks_incomplete(text, self.cfg.lang) {
            if ends_on_growable_number(text, self.cfg.lang) {
                return;
            }
            let key = format!("partial|{}|{}|{verse}", p.citation.book.index(), p.citation.chapter);
            let seen = self.t.interim_seen.insert(key, now);
            if seen.is_none_or(|t| now.saturating_sub(t) >= INTERIM_STABLE_WINDOW_MS) {
                return;
            }
        }
        let Some(v) = self.k.id(VerseRef::new(p.citation.book, p.citation.chapter as u16, verse as u16)) else { return };
        self.jump_range_to(v, now);
        let candidate = Candidate::new(vec![v], self.partial_method(&p), DIRECT_PARTIAL_SCORE, Target::Screen);
        let sent = self.decide(if self.partial_may_send(&p, &[v], text) { candidate } else { candidate.held(Hold::StrayVerseNumber) }, now);
        self.partial_confirms_context(sent, v, &p, now);
    }

    /// A bare "verse N" goes up when its chapter was named aloud. A chapter in play only because a
    /// quote of it was matched may be one the recogniser lost the book of ("Psalm one ten, verse one
    /// to three" heard as "someone wants the rule, verse one to three" with Matthew 24 in play): then,
    /// as for a lapsed context, only the verse's own words in what was just said put it up.
    fn partial_may_send(&self, p: &Partial, verses: &[VerseId], text: &str) -> bool {
        if p.chapter_given || self.sermon.iter().any(|c| c.named && c.book == p.citation.book && c.chapter == Some(p.citation.chapter as u16)) {
            return true;
        }
        // The chapter named without a verse, waiting for it: "go to Ezekiel twelve…", then "go from
        // verse twenty one" is that chapter's verse 21, called out (live, 2026-10-27).
        if self.chapter_pending.as_ref().is_some_and(|c| c.book == p.citation.book && c.chapter as u32 == p.citation.chapter) {
            return true;
        }
        // The sermon's announced passage, however long ago: "Mark chapter four verses twenty one
        // to thirty four" at the start, "verse twenty four" eight minutes in (MacArthur, dev build).
        if self.in_announced_passage(p.citation.book, p.citation.chapter as u16) {
            return true;
        }
        // A verse of the chapter went up moments ago: the reading is still there ("…verse 12 and
        // then gets us up on our high places, verse 14", Isaiah 58:12 up 9 s before).
        if let Some((up, at)) = self.screen.up {
            let u = self.r(up);
            if u.book == p.citation.book && u.chapter as u32 == p.citation.chapter && self.last_now.saturating_sub(at) < PARTIAL_CHAPTER_UP_MS {
                return true;
            }
        }
        self.own_word_heard(verses, text)
    }

    /// "verse N" taken against the chapter cited in passing, when the words fit verse N of the
    /// sermon's announced passage better: Luke 6:38 cited, then "whoever has, verse twenty five, to
    /// him more shall be given" is Mark 4:25, not Luke 6:25 (MacArthur, dev build 2026-10-20).
    fn announced_instead(&self, mut p: Partial, text: &str) -> Partial {
        let Verses::One(n) = p.citation.verses else { return p };
        if p.chapter_given || self.in_announced_passage(p.citation.book, p.citation.chapter as u16) {
            return p;
        }
        let spoken = format!("{} {text}", self.recent_speech_seconds(20).join(" "));
        let here = self.k.id(VerseRef::new(p.citation.book, p.citation.chapter as u16, n as u16)).map_or(0.0, |h| self.shared_weight(h, &spoken).0);
        let best = self
            .announced
            .iter()
            .filter(|(b, c, _)| self.in_announced_passage(*b, *c))
            .filter_map(|(b, c, _)| self.k.id(VerseRef::new(*b, *c, n as u16)).map(|id| (*b, *c, self.shared_weight(id, &spoken).0)))
            .max_by(|x, y| x.2.total_cmp(&y.2));
        if let Some((book, chapter, w)) = best {
            if w >= here + super::ranges::CHAPTER_KEYWORD_MIN_MARGIN * 2.0 {
                p.citation.book = book;
                p.citation.chapter = chapter as u32;
            }
        }
        p
    }

    /// An identifying word of one of the verses in what was just said, not two everyday ones
    /// ("one", "down" for Matthew 24:2 in "verse one to three. Sit down").
    fn own_word_heard(&self, verses: &[VerseId], text: &str) -> bool {
        let spoken = format!("{} {text}", self.recent_speech_seconds(20).join(" "));
        let said: HashSet<String> = meaningful_words(&spoken).iter().map(|w| stem_lite(w)).collect();
        verses.iter().any(|v| meaningful_words(self.verse_text(*v)).iter().any(|w| self.k.lexicon.idf_of(w) >= PARTIAL_OWN_WORD_MIN_IDF && said.contains(&stem_lite(w))))
    }

    /// "Romans 8:28 — sorry, verse 31": the verse named after a correction word
    /// replaces the one just cited in the same breath. (Said in a later
    /// segment, "sorry, verse 31" is an ordinary "verse N" already.)
    pub(crate) fn spoken_correction(&mut self, text: &str, is_final: bool, now: u64) -> bool {
        let Some(m) = CORRECTION_WORD.find_iter(text).last() else { return false };
        let tail = text[m.end()..].trim_start();
        if !VERSE_OR_CHAPTER_NEXT.is_match(tail) || !self.reading.is_valid(now) {
            return false;
        }
        if is_final {
            self.final_partial(tail, None, now)
        } else {
            self.interim_partial(tail, now);
            false
        }
    }

    /// "verse N" said once the reading context has lapsed, against the
    /// passage still on screen ("…direct thy paths. Verse six"). Only with
    /// "verse" said: a bare number long after the reading means nothing.
    pub(crate) fn on_screen_partial(&mut self, text: &str, is_final: bool, now: u64) -> bool {
        let Some((up, at)) = self.screen.up else { return false };
        if self.reading.is_valid(now) || now.saturating_sub(at) > ON_SCREEN_CONTEXT_MS {
            return false;
        }
        if !is_final && interim_looks_incomplete(text, self.cfg.lang) {
            return false;
        }
        let r = self.r(up);
        let active = Active { book: Some(r.book), chapter: Some(r.chapter as u32) };
        let Some(p) = self.k.partials(self.cfg.lang).resolve(text, active, false) else { return false };
        let verses = self.citation_verses(&p.citation);
        if verses.is_empty() || verses.contains(&up) {
            return false;
        }
        // The context has lapsed, so the preacher may have moved to a passage
        // never named: only an identifying word of the verse in what was just
        // said puts it up; otherwise it is offered. Not in the sermon's announced passage: that is
        // where the preacher is (MacArthur's "go back to verse thirteen" in Mark 4).
        if !self.in_announced_passage(r.book, r.chapter) && !self.own_word_heard(&verses, text) {
            self.decide(Candidate::new(verses, partial_method(&p), DIRECT_PARTIAL_SCORE, Target::Suggestions).held(Hold::VerseNumberAfterLapse), now);
            return true;
        }
        if verses.len() > 1 {
            self.set_range(verses.clone(), now);
        }
        let sent = self.decide(Candidate::new(verses.clone(), partial_method(&p), DIRECT_PARTIAL_SCORE, Target::Screen), now);
        self.partial_confirms_context(sent, verses[0], &p, now);
        true
    }

    /// "verse N" (or "verses 3 to 5") in a final, against the passage being read.
    pub(crate) fn final_partial(&mut self, text: &str, prev: Option<&(String, u64)>, now: u64) -> bool {
        if self.announces_different_book(text, prev.cloned(), now) {
            return false;
        }
        // "…start from verse" | "one": joined across the split, but only if the
        // verse fits what's being said.
        let prev_tail = prev.map(|(p, _)| p.trim().trim_end_matches(['.', ',', '!', '?']).split_whitespace().last().unwrap_or("").to_lowercase()).unwrap_or_default();
        let carried = matches!(prev_tail.as_str(), "verse" | "verses")
            && prev.is_some_and(|(_, at)| self.t.segment_started.saturating_sub(*at) < PREV_FINAL_JOIN_WINDOW_MS)
            && opens_with_number(text);
        let query = if carried { format!("verse {text}") } else { text.to_string() };
        let Some(p) = self.k.partials(self.cfg.lang).resolve(&query, self.verse_call_context(now), true) else { return false };
        let p = self.announced_instead(p, &query);
        let verses = self.citation_verses(&p.citation);
        if verses.is_empty() {
            return false;
        }
        if carried && matches!(p.citation.verses, Verses::One(_)) {
            let spoken = format!("{} {text}", prev.map_or("", |(t, _)| t.as_str()));
            if !self.shares_words_with(verses[0], &spoken) {
                return false;
            }
        }
        let mut outside_range = false;
        if verses.len() > 1 {
            self.set_range(verses.clone(), now);
        } else {
            outside_range = !self.jump_range_to(verses[0], now);
        }
        let candidate = Candidate::new(verses.clone(), self.partial_method(&p), DIRECT_PARTIAL_SCORE, Target::Screen);
        let sent = self.decide(if self.partial_may_send(&p, &verses, text) { candidate } else { candidate.held(Hold::StrayVerseNumber) }, now);
        // A demoted stray number mustn't destroy the range.
        if outside_range && sent == Target::Screen {
            self.clear_range(now);
        }
        self.partial_confirms_context(sent, verses[0], &p, now);
        true
    }

    fn partial_confirms_context(&mut self, sent: Target, v: VerseId, p: &Partial, now: u64) {
        // "chapter one verse thirteen" resolved in the book being read: no longer a citation
        // waiting for its book (Habakkuk 1:13 took it, Piper, dev build).
        if sent == Target::Screen && p.chapter_given {
            self.cites.bare = None;
        }
        let up_already = self.screen.recent_detections.get(&v).is_some_and(|t| now.saturating_sub(*t) < PARTIAL_CONFIRM_MAX_AGE_MS);
        if sent == Target::Screen || up_already {
            self.reading.update(p.citation.book, Some(p.citation.chapter), now);
            self.chapter_pending = None;
        }
    }

    /// At least two (or all, if fewer) of the meaningful words said are the verse's.
    pub(crate) fn shares_words_with(&self, v: VerseId, spoken: &str) -> bool {
        let said: HashSet<String> = meaningful_words(spoken).iter().map(|w| stem_lite(w)).collect();
        let need = said.len().min(2);
        if need == 0 {
            return false;
        }
        let verse: HashSet<String> = meaningful_words(&self.match_text(v)).iter().map(|w| stem_lite(w)).collect();
        verse.iter().filter(|w| said.contains(*w)).count() >= need
    }
}

fn partial_method(p: &Partial) -> Method {
    if p.chapter_given { Method::ContextCitation } else { Method::DirectPartial }
}

impl Engine {
    /// A verse called out for the chapter named and waiting for it ("go to Ezekiel twelve", then "go
    /// from verse twenty one") is a citation said in two parts, not a guess from context.
    fn partial_method(&self, p: &Partial) -> Method {
        if self.chapter_pending.as_ref().is_some_and(|c| c.book == p.citation.book && c.chapter as u32 == p.citation.chapter) {
            Method::ContextCitation
        } else {
            partial_method(p)
        }
    }
}
