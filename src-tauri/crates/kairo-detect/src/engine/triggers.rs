//! Spoken triggers: "next verse", a verse number called on its own, "verse N"
//! against the passage being read (`detection-spec/07-ranges-triggers-context.md`).

use std::collections::HashSet;
use std::sync::LazyLock;

use kairo_bible::{VerseId, VerseRef};

use super::citations::{ends_on_growable_number, ends_on_tens_and_unit, interim_looks_incomplete, opens_with_number, tokens, DIRECT_PARTIAL_SCORE, PREV_FINAL_JOIN_WINDOW_MS};
use super::core::{Candidate, ChapterPending, Engine};
use super::evidence::{meaningful_words, stem_lite};
use super::types::{Hold, Method, Target};
use crate::re::re;
use crate::reference::i18n::localize;
use crate::reference::numbers::{consume_number, is_ambiguous_number_word};
use crate::reference::partial::BARE_NUMBER_FILLER;
use crate::reference::{Active, Citation, Partial, Verses};

const NEXT_VERSE_COOLDOWN_MS: u64 = 4_000;
const NEXT_VERSE_MAX_GAP_MS: u64 = 180_000;
const BARE_NUMBER_COOLDOWN_MS: u64 = 2_000;
const BARE_NUMBER_TAIL_WORDS: usize = 4;
const BARE_NUMBER_MAX_GAP_MS: u64 = 15_000;
const VERSE_END_NUMBER_WINDOW_MS: u64 = 8_000;
const VERSE_JUMP_MAX_WORDS: usize = 4;
const VERSE_JUMP_COOLDOWN_MS: u64 = 2_000;
/// How soon after a named chapter its first number still counts as the verse, in time and in words
/// (owner, 2026-10-30: a time limit between book, chapter and verse).
const NEXT_NUMBER_WITHIN_MS: u64 = 15_000;
const NEXT_NUMBER_WITHIN_WORDS: usize = 15;
/// A number counting something is not a verse: "I have three points", "forty days", "the two
/// things". Words before it that make it a count, and words after it that it counts.
const COUNTING: &[&str] = &[
    "have", "has", "had", "got", "are", "were", "about", "over", "than", "only", "just", "all", "those", "these", "the", "my", "our",
    "your", "his", "her", "their", "every", "some", "nearly", "almost", "around",
];
const COUNTED: &[&str] = &["day", "year", "people", "men", "women", "thousand", "million", "billion", "o'clock", "percent", "times", "a", "of"];
/// Words ending in "s" that aren't a plural being counted.
const NOT_PLURAL: &[&str] = &["says", "is", "was", "this", "us", "yes", "thus", "has", "does", "goes", "jesus", "his", "its", "lets", "let's", "as"];

fn counted(w: &str) -> bool {
    COUNTED.contains(&w) || (w.len() > 3 && w.ends_with('s') && !NOT_PLURAL.contains(&w))
}
/// A word this identifying, heard, lets a bare "verse N" in a chapter never named go up.
const PARTIAL_OWN_WORD_MIN_IDF: f32 = 4.0;
/// A verse of the same chapter on screen this recently also lets a bare "verse N" go up.
const PARTIAL_CHAPTER_UP_MS: u64 = 60_000;
/// A "verse N" resolution renews the context only if it reached the screen,
/// or that verse is already up from this recently.
const PARTIAL_CONFIRM_MAX_AGE_MS: u64 = 15_000;
/// After the reading context lapses, "verse N" still means a verse of the
/// passage on screen for this long after it went up.
/// The verse up stays context for a verse number this long; past the announced passage, without
/// the verse's own words, it is only offered.
const ON_SCREEN_CONTEXT_MS: u64 = 600_000;

static NEXT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"\b(next verse|following verse|verse after (that|this|it))\b"));
static BACK: LazyLock<regex::Regex> = LazyLock::new(|| re(r"\b(previous verse|verse before|preceding verse)\b"));
/// A preacher taking back the number just said: "Romans 8:28, sorry, verse 31".
static CORRECTION_WORD: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?i)\b(sorry|i mean|i meant|rather|correction|pardon me|excuse me|my mistake)\b[\s,.;:!-]*"));
static VERSE_OR_CHAPTER_NEXT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?i)^(verses?|chapter)\b"));

fn filler(w: &str) -> bool {
    BARE_NUMBER_FILLER.contains(&w)
}

/// A verse number this close after a verse cited in passing reads on from that verse.
const PASSING_READS_ON: u32 = 3;
/// Words that make a number after them a verse called ("goes to twenty three").
const LEAD_IN: &[&str] = &["to", "at", "from"];
/// …as far as this from the verse being read.
const LED_NUMBER_REACH: i64 = 5;

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
        // The range moves only while its verse is the one up: "compare that with John 3:16 and the
        // next verse" during a Romans 8 range is John 3:17.
        if !self.range.queue.is_empty() && !back && self.range_is_up() {
            self.request_range_advance("'the next verse'", now);
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
        // The number closes what was said, after a pause word or a lead-in ("…okay twenty five now",
        // "…goes to twenty three"): the recogniser writes no commas, so the clause is the whole
        // stretch (live 2026-10-28).
        let at = (0..words.len()).rev().find(|&i| !filler(&words[i])).map_or(0, |i| i + 1 - num.consumed);
        let before = at.checked_sub(1).map(|i| words[i].as_str());
        // …but "seven to eight" is a range, not "to eight".
        let range_end = at >= 2 && consume_number(&words.iter().map(String::as_str).collect::<Vec<_>>(), at - 2, None).is_some_and(|n| n.consumed == 1);
        let led = before.is_some_and(|b| LEAD_IN.contains(&b)) && !range_end;
        let clause = text.split(['.', '!', '?', ';', ',']).map(str::trim).rfind(|c| !c.is_empty()).unwrap_or("");
        let alone = tokens(clause).iter().filter(|w| !filler(w)).count() == num.consumed || before.is_none_or(filler);
        if !alone && !led {
            return false;
        }
        let Some(base) = (if self.range_is_up() { self.range.current } else { None }).or(self.screen.up_verse()) else { return false };
        // A lead-in ("go to") may name any verse near the reading; a number on its own only the next.
        let step = num.value - self.r(base).verse as i64;
        if !(step == 1 || led && step != 0 && step.abs() <= LED_NUMBER_REACH) {
            return false;
        }
        if self.range.queue.is_empty() && self.screen.up.is_none_or(|(_, at)| now.saturating_sub(at) > BARE_NUMBER_MAX_GAP_MS) {
            return false;
        }
        self.throttle.bare_number_at = now;
        if !self.range.queue.is_empty() && self.range_is_up() && step == 1 {
            self.request_range_advance("the next verse number", now);
            return true;
        }
        if let Some(v) = self.verse_in_chapter(base, num.value) {
            if step != 1 && !self.jump_range_to(v, now) {
                self.clear_range(now);
            }
            self.decide(Candidate::new(vec![v], Method::Continuation, 1.0, Target::Screen), now);
        }
        true
    }

    /// The range's verse is the one on screen (or nothing else is).
    fn range_is_up(&self) -> bool {
        self.screen.up_verse().is_none_or(|u| self.range.current == Some(u))
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
        // "thirty verse one" is chapter 30 verse 1, not "thirty one" with "verse" left out.
        if all.iter().position(|w| w == "verse").is_some_and(|i| i > 0 && consume_number(&all.iter().map(String::as_str).collect::<Vec<_>>(), i + 1, None).is_some()) {
            return false;
        }
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
        // A complete number ("verse twenty four") needn't be heard twice: the offline engine's next
        // interim is half a second away, and its final over a second (live 2026-10-28).
        // …and so needn't one that can't grow ("fifteen", "three"); "four" or "twenty" may still be
        // "fourteen" or "twenty four", so those wait for the next word.
        if interim_looks_incomplete(text, self.cfg.lang) && ends_on_growable_number(text, self.cfg.lang) && !ends_on_tens_and_unit(text) {
            return;
        }
        let Some(v) = self.k.id(VerseRef::new(p.citation.book, p.citation.chapter as u16, verse as u16)) else { return };
        let candidate = Candidate::new(vec![v], self.partial_method(&p), DIRECT_PARTIAL_SCORE, Target::Screen);
        let sent = self.decide(if self.partial_may_send(&p, &[v], text) { candidate } else { candidate.held(Hold::StrayVerseNumber) }, now);
        // The range follows only a call that went up: one only offered mustn't make the verse look
        // up already, so its reading can't send it (Joel 2:11, "…before his army. Verse eleven").
        if sent == Target::Screen {
            self.jump_range_to(v, now);
        }
        self.partial_confirms_context(sent, v, &p, now);
    }

    /// A chapter named without a verse, then its verses said as bare numbers in a sentence of their own:
    /// "Psalm twenty two. We'll read twenty seven and twenty eight." Looked for in each sentence finished
    /// since the chapter was named, as the words come in: the offline engine locks text only at a pause,
    /// and the preacher went on for half a minute, so waiting for the final put the verses up late
    /// (live, 2026-10-29). A sentence still being said may grow, so it waits for its full stop.
    /// A chapter named in the words just heard, waiting for its verse: its numbers may be in the same
    /// words ("Psalm twenty two will read twenty seven and twenty eight." in one final).
    pub(crate) fn chapter_named_now(&self, now: u64) -> bool {
        self.chapter_pending.as_ref().is_some_and(|p| p.at == now)
    }

    pub(crate) fn pending_chapter_numbers(&mut self, now: u64) -> bool {
        let Some(p) = self.chapter_pending.clone() else { return false };
        if now.saturating_sub(p.at) > super::core::CHAPTER_NAMED_WAITS_MS {
            return false;
        }
        let since = self.t.words.text(p.from..self.t.words.end());
        let active = Active { book: Some(p.book), chapter: Some(p.chapter as u32) };
        // The first number said after the chapter is its verse, whatever words come between (owner,
        // 2026-10-30: "Psalm twenty two will read twenty seven and twenty eight", one sentence, went
        // up only when the reading was matched). Then, as before, a later sentence's own bare numbers.
        let next = (now.saturating_sub(p.at) <= NEXT_NUMBER_WITHIN_MS).then(|| self.next_numbers_after_chapter(&since, &p)).flatten();
        let later = since.split_inclusive(['.', '?', '!']).skip(1).filter(|s| s.trim_end().ends_with(['.', '?', '!']));
        for part in next.into_iter().chain(later.filter_map(|s| self.k.partials(self.cfg.lang).resolve(s, active, true))) {
            if part.citation.book != p.book || part.citation.chapter != p.chapter as u32 {
                continue;
            }
            let verses = self.citation_verses(&part.citation);
            let Some(&first) = verses.first() else { continue };
            if verses.len() > 1 {
                self.set_range(verses.clone(), now);
            }
            let sent = self.decide(Candidate::new(verses, self.partial_method(&part), DIRECT_PARTIAL_SCORE, Target::Screen), now);
            if sent == Target::Screen {
                self.jump_range_to(first, now);
                self.chapter_resolved_for = Some((p.book, p.chapter));
                self.chapter_pending = None;
            }
            self.partial_confirms_context(sent, first, &part, now);
            return true;
        }
        false
    }

    /// The first number said after the waiting chapter's own number, as its verse, or "N and/to M" as
    /// its verses, when the chapter has them. Not when another book, "chapter" or "verse" comes first
    /// (the parser's), when the number counts something ("forty days"), or while it may still grow
    /// ("twenty" of "twenty seven"): a word or a full stop must follow.
    fn next_numbers_after_chapter(&self, since: &str, p: &ChapterPending) -> Option<Partial> {
        let raw: Vec<&str> = since.split_whitespace().collect();
        let owned: Vec<String> = raw.iter().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').to_lowercase()).collect();
        let words: Vec<&str> = owned.iter().map(String::as_str).collect();
        let number = |i: usize| (!is_ambiguous_number_word(words.get(i)?)).then(|| consume_number(&words, i, None)).flatten();
        // The chapter's own number, said with the book.
        let at = (0..words.len().min(6)).find(|&i| number(i).is_some())?;
        let chapter = number(at)?;
        if chapter.value != p.chapter as i64 {
            return None;
        }
        let count = self.k.shape.verses(p.book, p.chapter as u32)? as i64;
        let from = at + chapter.consumed;
        for i in from..words.len().min(from + NEXT_NUMBER_WITHIN_WORDS) {
            let w = words[i];
            if matches!(w, "verse" | "verses" | "chapter" | "chapters") || self.k.books.alias(w).is_some() {
                return None;
            }
            let Some(a) = number(i) else { continue };
            let mut end = i + a.consumed;
            let mut b = None;
            if matches!(words.get(end).copied(), Some("and" | "to" | "through" | "thru" | "till" | "until")) {
                if let Some(n) = number(end + 1) {
                    b = Some(n.value);
                    end += 1 + n.consumed;
                }
            }
            let closed = end < words.len() || raw[end - 1].ends_with(['.', '?', '!', ',']);
            let counts = i.checked_sub(1).is_some_and(|j| COUNTING.contains(&words[j])) || words.get(end).is_some_and(|w| counted(w));
            if !closed || counts || !(1..=count).contains(&a.value) {
                return None;
            }
            let verses = match b {
                Some(b) if b > a.value && b <= count => Verses::Span(a.value as u32, b as u32),
                Some(_) => return None,
                None => Verses::One(a.value as u32),
            };
            return Some(Partial { citation: Citation::new(p.book, p.chapter as u32, verses), chapter_given: false });
        }
        None
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
        // "the next chapter verse one" taken already, on an earlier part of the same speech: the chapter
        // it moved to is now the one in play, so it mustn't move again (30:1, then 31:1).
        let lower = text.to_lowercase();
        let step: i64 = if lower.contains("next chapter") || lower.contains("following chapter") { 1 } else if lower.contains("previous chapter") || lower.contains("last chapter") { -1 } else { 0 };
        if step != 0 && p.chapter_given {
            let up = self.screen.up_verse().map(|u| self.r(u));
            if up.is_some_and(|u| u.book == p.citation.book && u.verse as u32 == n && u.chapter as i64 == p.citation.chapter as i64 - step) {
                p.citation.chapter = (p.citation.chapter as i64 - step) as u32;
                return p;
            }
        }
        // Just after the verse cited in passing, the number reads on from it ("John 3:16 … verse 17").
        if let Some((book, chapter, v, _)) = self.passing.filter(|_| self.chapter_pending.is_some() && !p.chapter_given) {
            if (v as u32 + 1..=v as u32 + PASSING_READS_ON).contains(&n) {
                p.citation.book = book;
                p.citation.chapter = chapter as u32;
                return p;
            }
        }
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
        if !is_final && interim_looks_incomplete(text, self.cfg.lang) && !ends_on_tens_and_unit(text) {
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
