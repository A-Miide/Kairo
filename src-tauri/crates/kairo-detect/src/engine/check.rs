//! A citation goes up as heard; what's read around it then decides whether a
//! sibling was meant (`detection-spec/05-citation-check.md`).

use std::collections::HashSet;

use kairo_bible::{Book, VerseId, VerseRef};

use super::citations::text_after_book_mention;
use super::core::{Candidate, Engine};
use super::event::Request;
use super::evidence::{meaningful_words, stem_lite};
use super::ranges::CHAPTER_KEYWORD_MIN_IDF;
use super::state::CorrectedAway;
use super::twins::TWIN_SPEECH_MARGIN;
use super::types::{Method, Target};
use crate::re::re;

const CHECK_WORDS: usize = 35;
const CHECK_MS: u64 = 25_000;
const MEANING_MIN_WORDS: usize = 6;
const MEANING_EVERY_WORDS: usize = 8;
const MAX_SIBLINGS: usize = 4;
/// The cross-encoder says "this is the verse" / "this is not".
const RR_FITS: f32 = 0.5;
const RR_NOT: f32 = 0.1;
const COS_LEAD: f32 = 0.1;
/// The cross-encoder this sure needs the embedding only to agree.
const RR_SURE: f32 = 0.9;
/// "world" 4.3, "greater" 5.5, "knee" 8.5; "God" 1.9, "Lord" 1.5.
const WORD_MIN_IDF: f32 = 3.5;
const BEFORE_MS: u64 = 8_000;
const BEFORE_WORDS: usize = 30;
const READING_FINALS_MS: u64 = 6_000;
const MEANING_WINDOW_WORDS: usize = 40;

#[derive(Clone, Debug)]
struct Sibling {
    verse: VerseId,
    words: HashSet<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CitationCheck {
    verse: VerseId,
    words: HashSet<String>,
    siblings: Vec<Sibling>,
    /// Finished speech since the citation.
    finals: String,
    /// The citation's own utterance is still being spoken.
    in_utterance: bool,
    at: u64,
    /// Words spoken when meaning was last checked.
    meaning_at: usize,
    /// The meaning comparison in flight, and the word-based pick when it was asked.
    asked: Option<(u32, Option<(VerseId, usize, usize)>, String)>,
    /// Speech before the citation (preachers paraphrase, then cite).
    before: String,
    /// Started for a reading match (looser siblings), not a citation.
    reading: bool,
}

/// Verse numbers that sound like `v`: tens off by one, teen ↔ -ty, a dropped or extra "1".
fn sound_alike_numbers(v: u16) -> Vec<u16> {
    let mut out: Vec<i32> = vec![v as i32 - 10, v as i32 + 10];
    let v = v as i32;
    if v < 10 {
        out.push(v + 10);
    }
    if v > 10 && v < 20 {
        out.push(v - 10);
        out.push((v - 10) * 10);
    }
    if (20..=90).contains(&v) && v % 10 == 0 {
        out.push(v / 10 + 10);
    }
    let mut seen = HashSet::new();
    out.into_iter().filter(|n| *n >= 1 && *n != v && seen.insert(*n)).map(|n| n as u16).collect()
}

impl Engine {
    /// Books a citation of `book` could have been: one that sounds like it, and
    /// the same name with another number.
    fn sibling_books(&self, book: Book) -> Vec<Book> {
        let mut out: Vec<Book> = self.k.books.sound_alikes(book).into_iter().take(1).collect();
        let base = book.name().trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ');
        for name in [base.to_string(), format!("1 {base}"), format!("2 {base}"), format!("3 {base}")] {
            if let Some(b) = Book::from_name(&name) {
                if b != book && !out.contains(&b) {
                    out.push(b);
                }
            }
        }
        out
    }

    fn rare_words(&self, v: VerseId) -> HashSet<String> {
        meaningful_words(&self.match_text(v)).into_iter().filter(|w| self.k.lexicon.idf_of(w) >= WORD_MIN_IDF).map(|w| stem_lite(&w)).collect()
    }

    pub(crate) fn start_citation_check(&mut self, v: VerseId, text: &str, is_final: bool, given: Option<Vec<VerseId>>, now: u64) {
        if self.check.as_ref().is_some_and(|c| c.verse == v && now.saturating_sub(c.at) < CHECK_MS) {
            return; // the same citation re-parsed
        }
        self.check = None;
        let r = self.r(v);
        let reading = given.is_some();
        let mut siblings: Vec<VerseId> = match given {
            Some(g) => g,
            None => {
                let books = self.sibling_books(r.book).into_iter().filter_map(|b| self.k.id(VerseRef::new(b, r.chapter, r.verse)));
                let numbers = sound_alike_numbers(r.verse).into_iter().filter_map(|n| self.k.id(VerseRef::new(r.book, r.chapter, n)));
                books.chain(numbers).collect()
            }
        };
        if siblings.is_empty() {
            return;
        }
        siblings.truncate(MAX_SIBLINGS);
        let mut check = CitationCheck {
            verse: v,
            words: self.rare_words(v),
            siblings: siblings.iter().map(|s| Sibling { verse: *s, words: self.rare_words(*s) }).collect(),
            finals: String::new(),
            in_utterance: !is_final,
            at: now,
            meaning_at: 0,
            asked: None,
            before: String::new(),
            reading,
        };
        if reading {
            let finals: Vec<&str> = self.t.finals.iter().filter(|f| now.saturating_sub(f.at) < READING_FINALS_MS).map(|f| f.text.as_str()).collect();
            check.finals = finals.join(" ");
            check.in_utterance = false;
            self.check = Some(check);
            return;
        }
        check.before = self.speech_before_citation(text, r.book, is_final, now);
        if is_final {
            check.finals = text_after_book_mention(text, r.book, r.chapter as u32);
        }
        self.check = Some(check);
        self.check_citation_siblings(text, is_final, true, now);
    }

    fn speech_before_citation(&self, text: &str, book: Book, is_final: bool, now: u64) -> String {
        let last = book.name().to_lowercase().split(' ').next_back().unwrap_or("").to_string();
        let stem = last.strip_suffix('s').unwrap_or(&last).to_string();
        let cut = re(&format!(r"(?i)\b{}s?\b", regex::escape(&stem))).find_iter(text).last().map_or(0, |m| m.start());
        let here = if cut > 0 { &text[..cut] } else { "" };
        let mut earlier: Vec<&str> = self.t.finals.iter().filter(|f| now.saturating_sub(f.at) < BEFORE_MS).map(|f| f.text.as_str()).collect();
        if is_final && earlier.last() == Some(&text) {
            earlier.pop();
        }
        let all = format!("{} {here}", earlier.join(" "));
        let words: Vec<&str> = all.split_whitespace().collect();
        words[words.len().saturating_sub(BEFORE_WORDS)..].join(" ")
    }

    /// Every segment while a check is open: the rare words, then meaning, decide.
    pub(crate) fn check_citation_siblings(&mut self, text: &str, is_final: bool, just_started: bool, now: u64) {
        let Some(mut c) = self.check.take() else { return };
        if now.saturating_sub(c.at) > CHECK_MS || self.screen.last_detected.map(|(v, _, _)| v) != Some(c.verse) {
            self.note(|e| format!("citation check of {} closed", e.label(c.verse)));
            return;
        }
        let r = self.r(c.verse);
        let mut after = c.finals.clone();
        if !(just_started && is_final) {
            let part = if c.in_utterance { text_after_book_mention(text, r.book, r.chapter as u32) } else { text.to_string() };
            after = format!("{} {part}", c.finals);
            if is_final {
                c.finals = after.clone();
                c.in_utterance = false;
            }
        }
        let judged_text = format!("{} {after}", c.before).trim().to_string();
        let spoken = after.split_whitespace().count();
        let judged = judged_text.split_whitespace().count();

        let speech: HashSet<String> = meaningful_words(&judged_text).iter().map(|w| stem_lite(w)).collect();
        let mut by_words: Option<(VerseId, usize, usize)> = None;
        for s in &c.siblings {
            let heard_only = c.words.iter().filter(|w| !s.words.contains(*w) && speech.contains(*w)).count();
            let sibling_only = s.words.iter().filter(|w| !c.words.contains(*w) && speech.contains(*w)).count();
            if heard_only >= sibling_only + TWIN_SPEECH_MARGIN {
                return; // the cited verse is being read: settled
            }
            if sibling_only >= heard_only + TWIN_SPEECH_MARGIN && by_words.is_none_or(|(_, hits, _)| sibling_only > hits) {
                by_words = Some((s.verse, sibling_only, heard_only));
            }
        }

        if judged >= MEANING_MIN_WORDS && c.asked.is_none() && (is_final || just_started || spoken.saturating_sub(c.meaning_at) >= MEANING_EVERY_WORDS) {
            c.meaning_at = spoken;
            // The rare words already say which verse is being read: switch now, in the same
            // breath, with or without the models. Asking the cross-encoder first left the
            // misspoken citation on screen while it answered (measured: "Ephesians two verse
            // ten … every knee" showed Ephesians 2:10 before Philippians 2:10). The models
            // are asked only when the words don't settle it.
            if let Some((v, _, _)) = by_words {
                self.check = Some(c);
                self.switch_citation_to_sibling(v, &judged_text, now);
                return;
            }
            if self.cfg.meaning {
                let id = self.next_request_id();
                let words: Vec<&str> = judged_text.split_whitespace().collect();
                let window = words[words.len().saturating_sub(MEANING_WINDOW_WORDS)..].join(" ");
                let mut texts = vec![window];
                for w in self.paraphrase_windows(&judged_text).into_iter().take(2) {
                    if !texts.contains(&w) {
                        texts.push(w);
                    }
                }
                let verses = std::iter::once(c.verse).chain(c.siblings.iter().map(|s| s.verse)).collect();
                self.out.requests.push(Request::Compare { id, texts, verses });
                c.asked = Some((id, by_words, judged_text));
            }
        }
        if spoken < CHECK_WORDS {
            self.check = Some(c);
        }
    }

    /// The meaning comparison for an open check: (cosine, cross-encoder) per
    /// verse, the cited verse first.
    pub(crate) fn on_compare(&mut self, id: u32, scores: &[(Option<f32>, Option<f32>)], now: u64) {
        let Some(c) = self.check.as_mut() else { return };
        let Some((asked, by_words, text)) = c.asked.take().filter(|(a, _, _)| *a == id) else { return };
        let _ = asked;
        let Some(&(heard_cos, heard_rr)) = scores.first() else { return };
        let siblings = c.siblings.clone();
        let reading = c.reading;
        let cited = c.verse;
        self.note(|e| {
            let all: Vec<String> = std::iter::once(cited)
                .chain(siblings.iter().map(|s| s.verse))
                .zip(scores)
                .map(|(v, (cos, rr))| format!("{} cos {:.2} rr {:.2}", e.label(v), cos.unwrap_or(0.0), rr.unwrap_or(-1.0)))
                .collect();
            format!("citation check \"{text}\": {}; words pick {:?}", all.join(", "), by_words.map(|(v, s, h)| (e.label(v), s, h)))
        });
        match heard_rr {
            Some(heard_rr) => {
                let mut pick: Option<(VerseId, f32)> = None;
                for (i, s) in siblings.iter().enumerate() {
                    let Some(&(cos, rr)) = scores.get(i + 1) else { continue };
                    let Some(rr) = rr else { continue };
                    let lead = cos.unwrap_or(0.0) - heard_cos.unwrap_or(0.0);
                    let agrees = lead >= COS_LEAD || (rr >= RR_SURE && lead > 0.0);
                    if rr >= RR_FITS && heard_rr <= RR_NOT && agrees && pick.is_none_or(|(_, p)| rr > p) {
                        pick = Some((s.verse, rr));
                    }
                }
                if let Some((v, _)) = pick {
                    self.switch_citation_to_sibling(v, &text, now);
                    return;
                }
                if let Some((v, _, _)) = by_words {
                    let i = siblings.iter().position(|s| s.verse == v).unwrap_or(0);
                    let (cos, rr) = scores.get(i + 1).copied().unwrap_or((None, None));
                    // Both read as said (a phrase they share): the rare words decide.
                    let both_fit = !reading && heard_rr >= RR_FITS && rr.is_some_and(|rr| rr >= RR_FITS);
                    // The cited verse plainly isn't what's said, and the sibling's
                    // words and meaning both are.
                    let only_sibling = heard_rr <= RR_NOT && cos.unwrap_or(0.0) >= heard_cos.unwrap_or(0.0) + COS_LEAD;
                    if both_fit || only_sibling {
                        self.switch_citation_to_sibling(v, &text, now);
                    }
                }
            }
            None => {
                if let Some((v, _, _)) = by_words {
                    self.switch_citation_to_sibling(v, &text, now);
                }
            }
        }
    }

    fn switch_citation_to_sibling(&mut self, v: VerseId, spoken: &str, now: u64) {
        let Some(c) = self.check.clone() else { return };
        let (from, to) = (self.r(c.verse), self.r(v));
        // Another verse number of the same chapter must also be the chapter's
        // best match for what was said.
        if from.book == to.book && from.chapter == to.chapter {
            let text = if spoken.is_empty() { format!("{} {}", c.before, c.finals) } else { spoken.to_string() };
            if let Some(&(best, idf)) = self.score_chapter(to.book, to.chapter, &text).first() {
                if best != v && idf >= CHAPTER_KEYWORD_MIN_IDF {
                    return; // keep listening
                }
            }
        }
        self.check = None;
        self.screen.corrected_away = Some(CorrectedAway { verse: c.verse, replaced_by: v, at: now });
        let mut cand = Candidate::new(vec![v], Method::Direct, 1.0, Target::Screen);
        cand.corrected_from = Some(c.verse);
        self.decide(cand, now);
        self.note_cited(to.book, Some(to.chapter), Some(to.verse), true, now);
        self.reading.update(to.book, Some(to.chapter as u32), now);
    }
}

#[cfg(test)]
mod tests {
    use super::sound_alike_numbers;

    #[test]
    fn verse_numbers_that_sound_alike() {
        assert!(sound_alike_numbers(13).contains(&30));
        assert!(sound_alike_numbers(30).contains(&13));
        assert!(sound_alike_numbers(8).contains(&18));
        assert!(sound_alike_numbers(53).contains(&63));
        assert!(!sound_alike_numbers(5).contains(&5));
    }
}
