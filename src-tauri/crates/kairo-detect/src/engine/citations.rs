//! What happens to a spoken citation once it's parsed
//! (`detection-spec/02-citations.md`).

use std::collections::HashSet;

use kairo_bible::{Book, VerseId, VerseRef};

use super::core::{Candidate, ChapterPending, Engine};
use super::event::{Effect, TimerId};
use super::evidence::{evidence_words, fits_first_at_least_as_well, preaching_common};
use super::scoring::SAME_BOOK_WINDOW_MS;
use super::twins::TWIN_SPEECH_MARGIN;
use super::types::{Method, Target};
use crate::re::re;
use crate::reference::numbers::{consume_number, spoken_to_number};
use crate::reference::{book_mentions, Citation, Verses};

/// A citation split by a pause is joined with the previous final this long after it.
pub const PREV_FINAL_JOIN_WINDOW_MS: u64 = 8_000;
/// While the offline engine's speech runs on across locks, the previous text
/// carries this many locked words.
const JOIN_CARRY_WORDS: usize = 12;
/// An unfinished interim citation acts only when parsed identically this recently.
pub const INTERIM_STABLE_WINDOW_MS: u64 = 3_000;
const CHAPTER_PENDING_TEXT_MAX_WORDS: usize = 50;
/// A verse resolved against a context or chosen within a named chapter.
pub const DIRECT_PARTIAL_SCORE: f32 = 0.90;
/// A citation in the last minute chooses among a stem's books.
const LAST_CITED_CHOOSES_MS: u64 = 60_000;
const ORDINAL_LOOKBACK_MS: u64 = 15_000;
const RESTATED_HOLD_MS: u64 = 3_000;
const RESTATED_DECIDED_TTL_MS: u64 = 20_000;
const RESTATED_REHEAR_MS: u64 = 5_000;
/// Words that always continue a citation.
const DANGLING: &[&str] = &["and", "to", "through", "verse", "verses"];
/// Spoken numbers whose sound can still grow into a longer one (four → fourteen, forty).
const GROWABLE: &[i64] = &[4, 6, 7, 8, 9, 20, 30, 40, 50, 60, 70, 80, 90, 100];
const RECENT_TEXT_WORDS: usize = 60;

/// A citation, and whether the choice among its books is still open.
#[derive(Clone, Debug)]
pub(crate) struct Cite {
    pub c: Citation,
    pub unresolved: bool,
}

pub(crate) fn cite_key(c: &Citation) -> String {
    format!("{}|{}|{:?}", c.book.index(), c.chapter, c.verses)
}

pub(crate) fn first_verse(c: &Citation) -> Option<u32> {
    match &c.verses {
        Verses::Chapter => None,
        Verses::One(v) | Verses::Span(v, _) => Some(*v),
        Verses::Spans(s) => s.first().map(|(a, _)| *a),
    }
}

fn last_verse(c: &Citation) -> Option<u32> {
    match &c.verses {
        Verses::Chapter => None,
        Verses::One(v) | Verses::Span(_, v) => Some(*v),
        Verses::Spans(s) => s.last().map(|(_, b)| *b),
    }
}

/// Lowercase words, punctuation removed.
pub(crate) fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c.is_whitespace() || c == '-' { c } else { ' ' }).collect::<String>().split_whitespace().map(str::to_string).collect()
}

fn last_raw_word(text: &str) -> &str {
    text.split_whitespace().last().unwrap_or("")
}

fn last_n_words(text: &str, n: usize) -> String {
    let w: Vec<&str> = text.split_whitespace().collect();
    w[w.len().saturating_sub(n)..].join(" ")
}

fn is_number_word(w: &str) -> bool {
    w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty() || spoken_to_number(w).is_some()
}

/// An interim still mid-citation: its last word a bare number or a word that
/// always continues one. Punctuation after it means the clause is committed.
pub(crate) fn interim_looks_incomplete(text: &str) -> bool {
    let last = last_raw_word(text);
    if last.ends_with([',', '.', ';', '!', '?']) {
        return false;
    }
    let w: String = last.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    !w.is_empty() && (is_number_word(&w) || w == "hundred" || DANGLING.contains(&w.as_str()))
}

/// A number at the end whose own sound can still grow ("seven…teen").
pub(crate) fn ends_on_growable_number(text: &str) -> bool {
    let last = last_raw_word(text);
    if last.ends_with([',', '.', ';', '!', '?']) {
        return false;
    }
    let w: String = last.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if w == "hundred" {
        return true;
    }
    let n = if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) { w.parse().ok() } else { spoken_to_number(&w) };
    n.is_some_and(|n| GROWABLE.contains(&n))
}

pub(crate) fn opens_with_number(text: &str) -> bool {
    let t = tokens(text);
    let w: Vec<&str> = t.iter().map(String::as_str).collect();
    !w.is_empty() && consume_number(&w, 0, None).is_some()
}

/// Stops on "verse"/"and"/"to", or on a number ("…John 6").
fn ends_mid_citation(text: &str) -> bool {
    let t = text.trim().trim_end_matches(['.', ',', '!', '?']);
    let tail = t.split_whitespace().last().unwrap_or("").to_lowercase();
    DANGLING.contains(&tail.as_str()) || t.ends_with(|c: char| c.is_ascii_digit()) || consume_number(&[tail.as_str()], 0, None).is_some()
}

/// The text after the last mention of a book (or else of its chapter number).
pub(crate) fn text_after_book_mention(text: &str, book: Book, chapter: u32) -> String {
    let last = book.name().to_lowercase().split(' ').last().unwrap_or("").to_string();
    let stem = last.strip_suffix('s').unwrap_or(&last);
    let after = |pattern: String| re(&pattern).find_iter(text).last().map(|m| m.end());
    if let Some(end) = after(format!(r"(?i)\b{}s?\b", regex::escape(stem))) {
        return text[end..].to_string();
    }
    match after(format!(r"\b{chapter}\b")) {
        Some(end) => text[end..].to_string(),
        None => text.to_string(),
    }
}

/// A chapter said twice, waiting for the words read after it.
#[derive(Clone, Debug)]
pub(crate) struct Restated {
    id: u32,
    key: String,
    plain: Citation,
    alt: Citation,
    plain_words: HashSet<String>,
    alt_words: HashSet<String>,
    words: HashSet<String>,
    at: u64,
    text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RestatedDecided {
    key: String,
    plain: Citation,
    alt: Citation,
    chosen: Citation,
    at: u64,
}

#[derive(Default)]
pub(crate) struct CitationState {
    restated: Option<Restated>,
    decided: Vec<RestatedDecided>,
    seq: u32,
}

/// The same citation under either of its two chapters.
fn is_restated(r: &Citation, plain: &Citation, alt: &Citation) -> bool {
    r.book == plain.book && (r.chapter == plain.chapter || r.chapter == alt.chapter) && r.verses == plain.verses
}

impl Restated {
    fn choice(&self) -> Option<Citation> {
        let (mut plain_only, mut alt_only) = (0, 0);
        for w in &self.words {
            if preaching_common(w) {
                continue;
            }
            match (self.plain_words.contains(w), self.alt_words.contains(w)) {
                (true, false) => plain_only += 1,
                (false, true) => alt_only += 1,
                _ => {}
            }
        }
        if alt_only >= plain_only + TWIN_SPEECH_MARGIN {
            Some(self.alt.clone())
        } else if plain_only >= alt_only + TWIN_SPEECH_MARGIN {
            Some(self.plain.clone())
        } else {
            None
        }
    }
}

/// The words after a citation's "verse N": what was read after it.
fn words_after_citation(text: &str, verse: Option<u32>) -> String {
    let Some(verse) = verse else { return String::new() };
    let t = tokens(text);
    let w: Vec<&str> = t.iter().map(String::as_str).collect();
    let mut start = None;
    for i in (0..w.len()).rev() {
        if w[i] != "verse" && w[i] != "verses" {
            continue;
        }
        if let Some(n) = consume_number(&w, i + 1, None) {
            if n.value == verse as i64 {
                start = Some(i + 1 + n.consumed);
                break;
            }
        }
    }
    let Some(mut start) = start else { return String::new() };
    if matches!(w.get(start), Some(&"to") | Some(&"through") | Some(&"-")) {
        start += 1 + consume_number(&w, start + 1, None).map_or(0, |n| n.consumed);
    }
    w.get(start..).unwrap_or(&[]).join(" ")
}

impl Engine {
    pub(crate) fn citation_verses(&self, c: &Citation) -> Vec<VerseId> {
        let spans: Vec<(u32, u32)> = match &c.verses {
            Verses::Chapter => Vec::new(),
            Verses::One(v) => vec![(*v, *v)],
            Verses::Span(a, b) => vec![(*a, *b)],
            Verses::Spans(s) => s.clone(),
        };
        let ch = c.chapter.min(u16::MAX as u32) as u16;
        spans.iter().flat_map(|&(a, b)| (a..=b.min(a + 200)).filter_map(move |v| self.k.id(VerseRef::new(c.book, ch, v.min(u16::MAX as u32) as u16)))).collect()
    }

    pub(crate) fn process_references(&mut self, text: &str, is_final: bool, continues_live: bool, now: u64) -> bool {
        self.references(text, is_final, &HashSet::new(), continues_live, now)
    }

    pub(crate) fn process_references_final(&mut self, text: &str, sent_from_interim: &HashSet<VerseId>, continues_live: bool, now: u64) -> bool {
        self.references(text, true, sent_from_interim, continues_live, now)
    }

    fn parse(&self, text: &str, now: u64) -> Vec<Citation> {
        self.k.parser(self.cfg.lang).parse_all(text, self.active(now))
    }

    fn references(&mut self, text: &str, is_final: bool, sent_from_interim: &HashSet<VerseId>, continues_live: bool, now: u64) -> bool {
        let parsed = self.parse(text, now);
        let mut refs = self.resolve_ambiguous(parsed, now);
        let tail = last_raw_word(text).trim_end_matches(['.', ',', '!', '?']).to_lowercase();
        if !refs.is_empty() && is_final && DANGLING.contains(&tail.as_str()) {
            refs.clear(); // the next segment's join finishes it
        }

        // A citation split by a pause: the previous final joined with this one.
        let mut joined_text: Option<String> = None;
        if is_final {
            if let Some((prev, at)) = self.t.prev_final.clone() {
                let split = !refs.is_empty() && opens_with_number(text) && ends_mid_citation(&prev);
                if (refs.is_empty() || split) && now.saturating_sub(at) < PREV_FINAL_JOIN_WINDOW_MS {
                    // Only citations the join creates or extends.
                    let prev_keys: HashSet<String> = self.parse(&prev, now).iter().map(cite_key).collect();
                    let joined = format!("{prev} {text}");
                    let fresh: Vec<Citation> = self.parse(&joined, now).into_iter().filter(|c| !prev_keys.contains(&cite_key(c))).collect();
                    let joined_refs = if fresh.is_empty() { Vec::new() } else { self.resolve_ambiguous(fresh, now) };
                    if !joined_refs.is_empty() && split {
                        let own: HashSet<String> = refs.iter().map(|r| cite_key(&r.c)).collect();
                        let added: Vec<Cite> = joined_refs.into_iter().filter(|r| !own.contains(&cite_key(&r.c))).collect();
                        if !added.is_empty() {
                            refs = added.into_iter().chain(refs).collect();
                        }
                    } else if !joined_refs.is_empty() {
                        refs = joined_refs;
                        joined_text = Some(joined);
                    }
                }
            }
            let carried = match (&self.t.prev_final, self.t.prev_continues) {
                (Some((p, _)), true) => last_n_words(&format!("{p} {text}"), JOIN_CARRY_WORDS),
                _ => text.to_string(),
            };
            self.t.prev_final = Some((carried, now));
            self.t.prev_continues = continues_live;
        }

        let (settled, held) = self.settle_restated(refs, text, now);
        let refs = settled;
        if refs.is_empty() {
            return held || self.no_citation(text, is_final, now);
        }

        let cited_now: HashSet<VerseId> = refs
            .iter()
            .filter_map(|r| first_verse(&r.c).and_then(|v| self.k.id(VerseRef::new(r.c.book, r.c.chapter as u16, v as u16))))
            .collect();
        let count = refs.len();
        for (i, Cite { c, mut unresolved }) in refs.into_iter().enumerate() {
            let (book, chapter) = (c.book, c.chapter);
            let ch = chapter as u16;
            // Even a chapter alone is context for what's said next.
            if !unresolved {
                self.reading.update(book, Some(chapter), now);
                if self.chapter_resolved_for != Some((book, ch)) {
                    self.chapter_resolved_for = None;
                }
            }

            if matches!(c.verses, Verses::Chapter) {
                self.chapter_named(book, chapter, unresolved, joined_text.as_deref().unwrap_or(text), is_final, now);
                continue;
            }
            self.chapter_pending = None;

            let mut verses = self.citation_verses(&c);
            if verses.is_empty() {
                if let Verses::One(v) = c.verses {
                    // A verse the chapter doesn't have: the chapter's verse that fits
                    // what was said (not for one-chapter books — there the chapter
                    // was never said, so the citation itself is unreliable).
                    if self.k.shape.chapters(book) != 1 {
                        if let Some(found) = self.pick_chapter_verse_by_idf(book, chapter, &self.t.last_words(RECENT_TEXT_WORDS)) {
                            verses = vec![found];
                        }
                    }
                    // Else a book that sounds like it has that verse: offered only.
                    if verses.is_empty() {
                        if let Some(id) = self.k.books.sound_alikes(book).into_iter().find_map(|b| self.k.id(VerseRef::new(b, ch, v as u16))) {
                            verses = vec![id];
                            unresolved = true;
                        }
                    }
                }
            }
            if verses.is_empty() {
                continue;
            }

            // A citation at the end of a growing interim may be half-heard.
            if !is_final && !unresolved && i + 1 == count && interim_looks_incomplete(text) {
                if ends_on_growable_number(text) {
                    continue;
                }
                let key = cite_key(&c);
                let confirmed = self.t.interim_seen.get(&key).is_some_and(|t| now.saturating_sub(*t) < INTERIM_STABLE_WINDOW_MS);
                self.t.interim_seen.retain(|_, t| now.saturating_sub(*t) < INTERIM_STABLE_WINDOW_MS);
                self.t.interim_seen.insert(key, now);
                if !confirmed {
                    continue;
                }
            }

            self.screen.last_citation_at = now;
            if !unresolved {
                let v = self.r(verses[0]).verse;
                self.note_cited(book, Some(ch), Some(v), true, now);
                self.reading.update(book, Some(chapter), now);
                if self.try_range_advance_by_ref(&verses, now) {
                    return true;
                }
            } else {
                // Never sent: every reading is offered for the operator to pick.
                self.decide(Candidate::new(verses, Method::Direct, 1.0, Target::Suggestions).capped(), now);
                continue;
            }

            if is_final {
                if verses.len() > 1 {
                    self.set_range(verses.clone(), now);
                } else {
                    self.clear_range(now);
                }
                // Already put up from this utterance's interim.
                if verses.len() == 1 && sent_from_interim.contains(&verses[0]) {
                    continue;
                }
                // The final re-hears the chapter of a citation already up from this
                // sentence's live text: the words read decide.
                if verses.len() == 1 {
                    if let Some(up) = self.screen.up_verse() {
                        let (u, v) = (self.r(up), self.r(verses[0]));
                        if u.book == v.book
                            && u.chapter != v.chapter
                            && sent_from_interim.contains(&up)
                            && !cited_now.contains(&up)
                            && fits_first_at_least_as_well(self.verse_text(up), self.verse_text(verses[0]), text)
                        {
                            continue;
                        }
                    }
                }
                let sent = self.decide(Candidate::new(verses.clone(), Method::Direct, 1.0, Target::Screen), now);
                if sent == Target::Screen && verses.len() == 1 {
                    self.start_citation_check(verses[0], text, true, None, now);
                }
            } else {
                let top = verses[0];
                if self.t.interim_sent.contains(&top) {
                    continue;
                }
                // Another chapter of a book already up from this sentence: the
                // recogniser re-hearing it waits for the final. A second citation
                // of the same book (the first still in the sentence) goes up.
                let tr = self.r(top);
                let rehears = self.t.interim_sent.iter().any(|k| {
                    let kr = self.r(*k);
                    kr.book == tr.book && kr.chapter != tr.chapter && !cited_now.contains(k)
                });
                if rehears {
                    continue;
                }
                self.t.interim_sent.insert(top);
                let single = verses.len() == 1;
                let sent = self.decide(Candidate::new(verses, Method::Direct, 1.0, Target::Screen), now);
                if sent == Target::Screen && single {
                    self.start_citation_check(top, text, false, None, now);
                }
            }
        }
        true
    }

    /// No citation: a bare book mention is context; a chapter waiting for its
    /// verse tries again with what's been said since.
    fn no_citation(&mut self, text: &str, is_final: bool, now: u64) -> bool {
        let books = book_mentions(text, self.k.books, self.cfg.lang);
        if let Some(&book) = books.last() {
            let a = self.active(now);
            // A book mentioned in passing doesn't wipe an active chapter; an announced one does.
            if a.book == Some(book) || a.chapter.is_none() || book_is_announced(&tokens(text), book) {
                self.reading.update(book, None, now);
            }
        }
        let Some(pending) = self.chapter_pending.clone() else { return false };
        let a = self.active(now);
        if a.book != Some(pending.book) || a.chapter != Some(pending.chapter as u32) {
            return false;
        }
        if !is_final {
            let found = self.chapter_callout(pending.book, pending.chapter as u32, &format!("{} {text}", pending.text));
            return found.is_some_and(|v| self.send_chapter_keyword(pending.book, pending.chapter as u32, v, now));
        }
        let accumulated = last_n_words(&format!("{} {text}", pending.text), CHAPTER_PENDING_TEXT_MAX_WORDS);
        if let Some(p) = self.chapter_pending.as_mut() {
            p.text = accumulated.clone();
        }
        match self.chapter_by_keywords(pending.book, pending.chapter as u32, &accumulated) {
            Some(v) => self.send_chapter_keyword(pending.book, pending.chapter as u32, v, now),
            None => false,
        }
    }

    /// "Book chapter N" with no verse: the words that follow choose the verse.
    fn chapter_named(&mut self, book: Book, chapter: u32, unresolved: bool, text: &str, is_final: bool, now: u64) {
        let ch = chapter as u16;
        // Re-parsed after it was resolved a moment ago.
        if self.chapter_resolved_for == Some((book, ch)) {
            return;
        }
        // The chapter on screen named again: the preacher pointing at what's up.
        if let Some((up, at)) = self.screen.up {
            let u = self.r(up);
            if u.book == book && u.chapter == ch && now.saturating_sub(at) < SAME_BOOK_WINDOW_MS {
                self.chapter_pending = None;
                return;
            }
        }
        if unresolved {
            return;
        }
        if !is_final {
            // Speech in progress: only a verse number called out on its own.
            if let Some(v) = self.chapter_callout(book, chapter, text) {
                self.send_chapter_keyword(book, chapter, v, now);
            }
            return;
        }
        if let Some(v) = self.chapter_by_keywords(book, chapter, text) {
            if self.send_chapter_keyword(book, chapter, v, now) {
                return;
            }
        }
        if self.chapter_resolved_for != Some((book, ch)) {
            self.chapter_pending = Some(ChapterPending { book, chapter: ch, text: text.to_string(), at: now });
        }
    }

    fn send_chapter_keyword(&mut self, book: Book, chapter: u32, v: VerseId, now: u64) -> bool {
        if self.decide(Candidate::new(vec![v], Method::ChapterKeyword, DIRECT_PARTIAL_SCORE, Target::Screen), now) != Target::Screen {
            return false;
        }
        let ch = chapter as u16;
        self.chapter_resolved_for = Some((book, ch));
        self.chapter_pending = None;
        self.screen.last_citation_at = now;
        let verse = self.r(v).verse;
        self.note_cited(book, Some(ch), Some(verse), true, now);
        true
    }

    /// "In Matthew 11, say, come to me. 28, all you…": a verse number called
    /// out on its own after the chapter.
    pub(crate) fn chapter_callout(&self, book: Book, chapter: u32, text: &str) -> Option<VerseId> {
        let after = text_after_book_mention(text, book, chapter);
        let scan = after.split_whitespace().take(40).collect::<Vec<_>>().join(" ");
        static CALLOUT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| re(r"(?:^|[.!?;,]\s+)(\d{1,3})\s*[,.](?:\s|$)"));
        for cap in CALLOUT.captures_iter(&scan) {
            let n: u32 = cap[1].parse().ok()?;
            if n == chapter {
                continue;
            }
            if let Some(id) = self.k.id(VerseRef::new(book, chapter as u16, n as u16)) {
                return Some(id);
            }
        }
        None
    }

    pub(crate) fn chapter_by_keywords(&self, book: Book, chapter: u32, text: &str) -> Option<VerseId> {
        self.chapter_callout(book, chapter, text).or_else(|| self.pick_chapter_verse_by_idf(book, chapter, text))
    }

    /// The chapter's verse sharing clearly the most identifying words with `text`.
    pub(crate) fn pick_chapter_verse_by_idf(&self, book: Book, chapter: u32, text: &str) -> Option<VerseId> {
        use super::ranges::{CHAPTER_KEYWORD_MIN_IDF, CHAPTER_KEYWORD_MIN_MARGIN};
        let rows = self.score_chapter(book, chapter as u16, text);
        let (best, second) = (rows.first()?, rows.get(1).map_or(0.0, |r| r.1));
        (best.1 >= CHAPTER_KEYWORD_MIN_IDF && best.1 - second >= CHAPTER_KEYWORD_MIN_MARGIN).then_some(best.0)
    }

    /// A stem with several books ("Timothy"): the ordinal said just before it,
    /// else the book being read, else the last citation's book; otherwise every
    /// reading stays open.
    fn resolve_ambiguous(&self, cites: Vec<Citation>, now: u64) -> Vec<Cite> {
        let mut out = Vec::new();
        for c in cites {
            let mut c = self.prefer_numbered_variant(c, now);
            if c.alternatives.is_empty() {
                out.push(Cite { c, unresolved: false });
                continue;
            }
            let group: Vec<Book> = std::iter::once(c.book).chain(c.alternatives.iter().copied()).collect();
            let word = c.book.name().to_lowercase().split(' ').last().unwrap_or("").to_string();
            let ordinal = self.ordinal_said_before(&word).and_then(|n| group.iter().copied().find(|b| b.name().starts_with(&format!("{n} "))));
            let active = self.active(now).book.filter(|b| group.contains(b));
            let last_cited = self.sermon.iter().rev().find(|s| s.named && now.saturating_sub(s.at) < LAST_CITED_CHOOSES_MS).map(|s| s.book).filter(|b| group.contains(b));
            match ordinal.or(active).or(last_cited) {
                Some(book) => {
                    c.book = book;
                    c.alternatives.clear();
                    out.push(Cite { c, unresolved: false });
                }
                None => {
                    for b in group {
                        let mut v = c.clone();
                        v.book = b;
                        v.alternatives.clear();
                        out.push(Cite { c: v, unresolved: true });
                    }
                }
            }
        }
        out
    }

    /// A book named without its number continues the numbered book being read
    /// ("First John 4… John 5:19"), when that verse exists there.
    fn prefer_numbered_variant(&self, mut c: Citation, now: u64) -> Citation {
        if !c.alternatives.is_empty() {
            return c;
        }
        let Some(active) = self.active(now).book else { return c };
        if active == c.book {
            return c;
        }
        let Some(variants) = self.k.books.numbered_variants(&c.book.name().to_lowercase()) else { return c };
        if !variants.contains(&active) || c.chapter > self.k.shape.chapters(active) {
            return c;
        }
        if let Some(v) = first_verse(&c) {
            if self.k.id(VerseRef::new(active, c.chapter as u16, v as u16)).is_none() {
                return c;
            }
        }
        c.book = active;
        c
    }

    /// "first"/"second"/"third" said right before the last mention of `word`
    /// in the last few seconds, across segment breaks.
    fn ordinal_said_before(&self, word: &str) -> Option<u8> {
        let now = self.last_now;
        let recent: Vec<&str> = self.t.finals.iter().filter(|f| now.saturating_sub(f.at) < ORDINAL_LOOKBACK_MS).map(|f| f.text.as_str()).collect();
        let recent = &recent[recent.len().saturating_sub(3)..];
        let all = tokens(&format!("{} {}", recent.join(" "), self.t.interim));
        let at = all.iter().rposition(|w| w == word)?;
        match all.get(at.checked_sub(1)?)?.as_str() {
            "first" | "1st" => Some(1),
            "second" | "2nd" => Some(2),
            "third" | "3rd" => Some(3),
            _ => None,
        }
    }

    /// Is a different book being announced (followed by a chapter number, or
    /// introduced: "the book of", "turn to")? Includes the unfinished tail of
    /// the previous final ("…in the book of Acts" | "one verse eight").
    pub(crate) fn announces_different_book(&self, text: &str, prev: Option<(String, u64)>, now: u64) -> bool {
        let mut prev_tail = match prev {
            Some((p, at)) if now.saturating_sub(at) < PREV_FINAL_JOIN_WINDOW_MS => last_n_words(&p, 8),
            _ => String::new(),
        };
        if !prev_tail.is_empty() && self.parse(&prev_tail, now).iter().any(|c| !matches!(c.verses, Verses::Chapter)) {
            prev_tail.clear(); // a complete citation was handled with its own segment
        }
        let combined = format!("{prev_tail} {text}");
        let toks = tokens(&combined);
        let current = self.active(now).book;
        book_mentions(&combined, self.k.books, self.cfg.lang).into_iter().any(|b| Some(b) != current && book_is_announced(&toks, b))
    }

    /// Settles citations whose chapter was said twice ("Luke chapter six four
    /// and verse 14"): chosen already, chosen now by the words after it, or
    /// held until the words read next (or the hold) decide.
    fn settle_restated(&mut self, refs: Vec<Cite>, text: &str, now: u64) -> (Vec<Cite>, bool) {
        if let Some(mut p) = self.cites.restated.take() {
            if !refs.iter().any(|r| is_restated(&r.c, &p.plain, &p.alt)) {
                p.words.extend(evidence_words(text));
                if let Some(chosen) = p.choice() {
                    self.send_restated_choice(p, chosen, true, now);
                } else {
                    self.cites.restated = Some(p);
                }
            } else {
                self.cites.restated = Some(p);
            }
        }
        self.cites.decided.retain(|d| now.saturating_sub(d.at) <= RESTATED_DECIDED_TTL_MS);
        if !refs.iter().any(|r| r.c.alt_chapter.is_some()) && self.cites.restated.is_none() && self.cites.decided.is_empty() {
            return (refs, false);
        }
        let mut out = Vec::new();
        let mut held = false;
        for r in refs {
            if let Some(alt_chapter) = r.c.alt_chapter {
                let mut plain = r.c.clone();
                plain.alt_chapter = None;
                let key = format!("{}|{alt_chapter}", cite_key(&plain));
                if let Some(d) = self.cites.decided.iter().find(|d| d.key == key) {
                    out.push(Cite { c: d.chosen.clone(), unresolved: r.unresolved });
                    continue;
                }
                if self.cites.restated.as_ref().is_none_or(|p| p.key != key) {
                    let mut alt = plain.clone();
                    alt.chapter = alt_chapter;
                    let (pv, av) = (self.citation_verses(&plain), self.citation_verses(&alt));
                    if av.is_empty() {
                        out.push(Cite { c: plain, unresolved: r.unresolved });
                        continue;
                    }
                    if pv.is_empty() {
                        out.push(Cite { c: alt, unresolved: r.unresolved });
                        continue;
                    }
                    self.cites.seq += 1;
                    let id = self.cites.seq;
                    self.cites.restated = Some(Restated {
                        id,
                        key,
                        plain,
                        alt,
                        plain_words: evidence_words(&self.match_text(pv[0])),
                        alt_words: evidence_words(&self.match_text(av[0])),
                        words: HashSet::new(),
                        at: now,
                        text: text.to_string(),
                    });
                    self.effect(Effect::WakeAt { id: TimerId::RestatedChapter(id), at_ms: now + RESTATED_HOLD_MS });
                }
            } else if let Some(p) = self.cites.restated.clone().filter(|p| is_restated(&r.c, &p.plain, &p.alt)) {
                // Re-heard without the extra number: the second chapter alone is the one meant.
                if r.c.chapter == p.alt.chapter {
                    self.settle(p.clone(), p.alt.clone(), now);
                    out.push(Cite { c: p.alt, unresolved: r.unresolved });
                    continue;
                }
            } else {
                // Decided for the second chapter, then re-heard with only the first.
                let decided = self.cites.decided.iter().find(|d| {
                    now.saturating_sub(d.at) < RESTATED_REHEAR_MS && d.chosen == d.alt && r.c.chapter == d.plain.chapter && is_restated(&r.c, &d.plain, &d.alt)
                });
                out.push(Cite { c: decided.map_or(r.c.clone(), |d| d.chosen.clone()), unresolved: r.unresolved });
                continue;
            }
            let Some(mut p) = self.cites.restated.take() else { continue };
            p.words.extend(evidence_words(&words_after_citation(text, first_verse(&p.plain))));
            match p.choice() {
                Some(chosen) => {
                    self.settle(p, chosen.clone(), now);
                    out.push(Cite { c: chosen, unresolved: r.unresolved });
                }
                None => {
                    self.cites.restated = Some(p);
                    held = true;
                }
            }
        }
        (out, held)
    }

    fn settle(&mut self, p: Restated, chosen: Citation, now: u64) {
        self.cites.decided.push(RestatedDecided { key: p.key.clone(), plain: p.plain.clone(), alt: p.alt.clone(), chosen, at: now });
        if self.cites.restated.as_ref().is_some_and(|r| r.id == p.id) {
            self.cites.restated = None;
        }
    }

    /// The hold for a chapter said twice is over.
    pub(crate) fn on_restated_timer(&mut self, id: u32, now: u64) {
        let Some(p) = self.cites.restated.take() else { return };
        if p.id != id {
            self.cites.restated = Some(p);
            return;
        }
        match p.choice() {
            Some(chosen) => self.send_restated_choice(p, chosen, true, now),
            None => {
                let plain = p.plain.clone();
                self.send_restated_choice(p, plain, false, now)
            }
        }
    }

    /// Decided after the citation's own segment: up as the citation, unless
    /// the screen has moved on.
    fn send_restated_choice(&mut self, p: Restated, chosen: Citation, by_words: bool, now: u64) {
        self.settle(p.clone(), chosen.clone(), now);
        let shows = |e: &Engine, c: &Citation| {
            e.screen.up.is_some_and(|(up, at)| {
                let u = e.r(up);
                at >= p.at && u.book == c.book && u.chapter as u32 == c.chapter && first_verse(c).is_some_and(|f| u.verse as u32 >= f) && last_verse(c).is_some_and(|l| u.verse as u32 <= l)
            })
        };
        if shows(self, &p.plain) || shows(self, &p.alt) || self.screen.last_citation_at > p.at {
            return;
        }
        let verses = self.citation_verses(&chosen);
        if verses.is_empty() {
            return;
        }
        self.screen.last_citation_at = now;
        let first = self.r(verses[0]).verse;
        self.note_cited(chosen.book, Some(chosen.chapter as u16), Some(first), true, now);
        self.reading.update(chosen.book, Some(chosen.chapter), now);
        if verses.len() > 1 {
            self.set_range(verses.clone(), now);
        } else {
            self.clear_range(now);
        }
        let sent = self.decide(Candidate::new(verses.clone(), Method::Direct, 1.0, Target::Screen), now);
        if sent == Target::Screen && verses.len() == 1 {
            self.start_citation_check(verses[0], &p.text, true, None, now);
        }
        // Undecided: the other reading stays a click away.
        if sent == Target::Screen && !by_words {
            let other = if chosen == p.plain { &p.alt } else { &p.plain };
            if let Some(&v) = self.citation_verses(other).first() {
                self.decide(Candidate::new(vec![v], Method::Direct, 1.0, Target::Suggestions).capped(), now);
            }
        }
    }
}

/// Is `book` announced in `tokens` — followed by a chapter number, or after
/// "the book of", "turn to"…? A book found only through an alias this scan
/// can't see counts as announced.
pub(crate) fn book_is_announced(tokens: &[String], book: Book) -> bool {
    const BIGRAMS: &[&str] = &["book of", "gospel of", "according to", "epistle of", "epistle to", "letter of", "letter to"];
    const VERBS: &[&str] = &["turn", "turning", "open", "go", "look"];
    let last = book.name().to_lowercase().split(' ').last().unwrap_or("").to_string();
    let stem = last.strip_suffix('s').unwrap_or(&last).to_string();
    let plural = format!("{stem}s");
    let mut found = false;
    for (i, t) in tokens.iter().enumerate() {
        if *t != last && *t != stem && *t != plural {
            continue;
        }
        found = true;
        if let Some(next) = tokens.get(i + 1) {
            if is_number_word(next) || next == "chapter" {
                return true;
            }
        }
        let mut j = i as isize - 1;
        if j >= 0 && tokens[j as usize] == "the" {
            j -= 1;
        }
        if j >= 1 && BIGRAMS.contains(&format!("{} {}", tokens[j as usize - 1], tokens[j as usize]).as_str()) {
            return true;
        }
        if j >= 0 && matches!(tokens[j as usize].as_str(), "to" | "at") {
            let from = (j - 3).max(0) as usize;
            if tokens[from..j as usize].iter().any(|w| VERBS.contains(&w.as_str())) {
                return true;
            }
        }
    }
    !found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfinished_interims() {
        assert!(interim_looks_incomplete("Romans chapter eight verse"));
        assert!(interim_looks_incomplete("Isaiah 41 from verse seven"));
        assert!(!interim_looks_incomplete("Isaiah 41 verse seven,"));
        assert!(ends_on_growable_number("from verse seven"));
        assert!(!ends_on_growable_number("from verse three"));
        assert!(ends_on_growable_number("verse 20"));
    }

    #[test]
    fn announced_books() {
        let b = Book::from_name("Acts").unwrap();
        assert!(book_is_announced(&tokens("in the book of Acts"), b));
        assert!(book_is_announced(&tokens("turn with me to Acts"), b));
        assert!(!book_is_announced(&tokens("the acts of the people verse 6"), b));
    }

    #[test]
    fn words_read_after_the_verse() {
        assert_eq!(words_after_citation("Luke chapter six four and verse 14 and Jesus returned", Some(14)), "and jesus returned");
        assert_eq!(words_after_citation("verses 3 to 5 blessed is the man", Some(3)), "blessed is the man");
    }
}
