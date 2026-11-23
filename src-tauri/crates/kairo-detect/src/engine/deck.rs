//! A slide deck followed as the speaker goes through it, by the same rules a cited range is read
//! on by (ranges.rs): the next slide's first words said in order, or its own opening words, or a
//! slide a little further on clearly said. The page's own follower took about 5 s a slide; the
//! owner asked for the Bible engine's way of moving on (22 Nov 2026).
//!
//! The deck's slides are its words (a picture's name for a picture). The page sets the deck when it
//! goes live (`Event::DeckSet`), moves it when the operator picks a slide (`Event::DeckAt`), and
//! hears `Effect::DeckMoved` when the speaker reaches another slide.

use std::collections::VecDeque;

use super::core::Engine;
use super::event::Effect;
use super::evidence::plain_tokens;
use super::ranges::{
    newest_words, opening_said, own_opening_said, read_run, read_to_end, shared_weight_meaningful_of, shared_weight_of, ADVANCE_COOLDOWN_MS,
    CHAPTER_KEYWORD_MIN_IDF, CHAPTER_KEYWORD_MIN_MARGIN, CONTENT_LOOKAHEAD, CONTENT_MIN_WORDS, HEARD_MAX_WORDS, OPENING_SPAN, PAUSED_SKIP_MIN_IDF,
    RESUME_RUN_OWN, RESUME_RUN_WORDS,
};
use crate::facts::SHORT_VERSE_WORDS;
use crate::lexical::text::is_stop_word;
use crate::reference::{Active, Verses};
use kairo_bible::{Book, VerseId};

/// A word the Bible never uses ("Shiloh", "WiFi", "testimony") is weighed as this rare: on a slide it
/// is the most telling word there is, where the Bible's own weights would count it as nothing.
const UNSEEN_WORD_IDF: f32 = 8.0;
/// A verse put up moves the deck to a slide this many ahead at most that carries it.
const VERSE_LOOKAHEAD: usize = 5;

/// The reference a slide is led by ("Romans 10:17 So then faith…", "– Eph. 2:6-8"): its book,
/// chapter and verses.
type SlideRef = (Book, u32, u32, u32);

#[derive(Debug, Default)]
pub struct Deck {
    /// The page's name for the deck (its playlist item), sent back with every move.
    pub id: String,
    pub slides: Vec<String>,
    /// The scripture each slide carries, when it's led by a written reference.
    pub refs: Vec<Option<SlideRef>>,
    /// Per slide: its first words are also in the slide before it, so its opening alone can't move the
    /// deck on before that slide was said to its end (the Bible's own fact, facts.rs).
    pub opening_repeats: Vec<bool>,
    pub current: Option<usize>,
    pub moved_at: u64,
    pub last_advance_at: u64,
    /// Speech since the deck last moved: finished segments, the open one.
    pub heard: VecDeque<String>,
    pub heard_open: String,
    /// Words of the open segment already said when it moved.
    pub spent: usize,
}

impl Engine {
    pub(crate) fn set_deck(&mut self, id: String, slides: Vec<String>, current: usize, now: u64) {
        let at = current.min(slides.len().saturating_sub(1));
        let refs = slides.iter().map(|t| self.slide_ref(t)).collect();
        let opening_repeats = opening_repeats(&slides);
        self.deck = Deck { id, slides, refs, opening_repeats, ..Deck::default() };
        self.deck.current = (!self.deck.slides.is_empty()).then_some(at);
        self.deck_moved(now);
    }

    /// The operator picked a slide: the deck follows on from there.
    pub(crate) fn deck_at(&mut self, index: usize, now: u64) {
        if index < self.deck.slides.len() {
            self.deck.current = Some(index);
            self.deck_moved(now);
        }
    }

    pub(crate) fn clear_deck(&mut self) {
        self.deck = Deck::default();
    }

    fn deck_moved(&mut self, now: u64) {
        self.deck.moved_at = now;
        self.deck.heard.clear();
        self.deck.heard_open.clear();
        self.deck.spent = self.t.interim.split_whitespace().count();
    }

    /// Notes a segment for the deck; returns its unspent words (as ranges.rs note_range_speech).
    pub(crate) fn note_deck_speech(&mut self, text: &str, is_final: bool, continues_live: bool) -> String {
        if self.deck.current.is_none() {
            return String::new();
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        let fresh = words.get(self.deck.spent.min(words.len())..).unwrap_or(&[]).join(" ");
        if !is_final {
            self.deck.heard_open = fresh.clone();
            return fresh;
        }
        self.deck.spent = if continues_live { self.deck.spent.saturating_sub(words.len()) } else { 0 };
        self.deck.heard_open.clear();
        if !fresh.is_empty() {
            self.deck.heard.push_back(fresh.clone());
            let mut total: usize = self.deck.heard.iter().map(|t| t.split_whitespace().count()).sum();
            while self.deck.heard.len() > 1 && total > HEARD_MAX_WORDS {
                total -= self.deck.heard.pop_front().map_or(0, |t| t.split_whitespace().count());
            }
        }
        fresh
    }

    fn deck_heard_words(&self) -> Vec<String> {
        self.deck.heard.iter().map(String::as_str).chain(std::iter::once(self.deck.heard_open.as_str())).flat_map(|t| t.split_whitespace()).map(str::to_string).collect()
    }

    /// The reference written at the start of a slide, read by the engine's own parser.
    fn slide_ref(&self, text: &str) -> Option<SlideRef> {
        static WRITTEN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?:\b[1-3]\s?)?\b[A-Za-z]{2,}\.?\s*\d{1,3}\s*:\s*\d{1,3}(?:\s*[-–]\s*\d{1,3})?").expect("reference pattern")
        });
        let m = WRITTEN.find(text).filter(|m| m.start() <= 6)?;
        let q: String = m.as_str().replace('–', "-").to_lowercase().chars().map(|c| if ".,".contains(c) { ' ' } else { c }).collect();
        let c = self.k.parser(self.cfg.lang).parse_one(&q, Active::default())?;
        let (a, b) = match c.verses {
            Verses::One(v) => (v, v),
            Verses::Span(a, b) => (a, b),
            Verses::Spans(s) => (s.first()?.0, s.last()?.1),
            Verses::Chapter => return None,
        };
        Some((c.book, c.chapter, a, b))
    }

    /// A verse went up on the screen: the deck moves to the slide just ahead that carries it (the
    /// preacher cited the scripture the next slide shows). The slide up carrying it keeps it there.
    pub(crate) fn deck_on_verse(&mut self, v: VerseId) {
        let Some(current) = self.deck.current else { return };
        let r = self.r(v);
        let carries = |x: &Option<SlideRef>| x.is_some_and(|(b, c, a, e)| b == r.book && c == u32::from(r.chapter) && (a..=e).contains(&u32::from(r.verse)));
        let last = (current + VERSE_LOOKAHEAD).min(self.deck.slides.len().saturating_sub(1));
        if let Some(i) = (current..=last).find(|i| carries(&self.deck.refs[*i])) {
            if i > current {
                let now = self.last_now;
                self.advance_deck(i, "its scripture went up", now);
            }
        }
    }

    fn deck_idf(&self, w: &str) -> f32 {
        let known = self.k.lexicon.idf_of(w);
        if known > 0.0 || w.len() < 4 || is_stop_word(w) { known } else { UNSEEN_WORD_IDF }
    }

    /// The speaker moved on: the next slide's opening, or its own words, heard just now; or a slide
    /// a little further on said nearly word for word. Every segment, interims too.
    pub(crate) fn follow_deck(&mut self, fresh: &str, now: u64) {
        let Some(current) = self.deck.current else { return };
        if current + 1 >= self.deck.slides.len() || now.saturating_sub(self.deck.last_advance_at) < ADVANCE_COOLDOWN_MS {
            return;
        }
        let (cur, next) = (self.deck.slides[current].clone(), self.deck.slides[current + 1].clone());
        let cur_tokens = plain_tokens(&cur);
        let heard = self.deck_heard_words();
        let ended = cur_tokens.len() <= SHORT_VERSE_WORDS || read_to_end(&plain_tokens(&heard.join(" ")), &cur_tokens);
        // The next slide's first words in order, as a range's next verse: two once the slide up was
        // read to its end, three before. Its own telling words need two, not one (a range held after
        // a quote, the stricter setting of the same rule): a preacher says "faith" or "the church" all
        // through a sermon on faith. Measured on five sermons with their decks (slide-follow-eval.js
        // --mode engine): one word put a slide up early 26 times; two, 5.
        if fresh.is_empty() {
            return self.deck_by_content(current, now);
        }
        let recent = newest_words(&heard, fresh, OPENING_SPAN);
        let repeats = self.deck.opening_repeats.get(current + 1).copied().unwrap_or(false);
        if (ended || !repeats) && opening_said([next.as_str()], &recent, if ended { 2 } else { 3 }) {
            return self.advance_deck(current + 1, "the next slide's opening words", now);
        }
        let idf = |w: &str| self.deck_idf(w);
        if own_opening_said(&cur, &next, &heard.join(" "), &recent, 2, &idf) {
            return self.advance_deck(current + 1, "the next slide's own words", now);
        }
        // As the words come in, not only at the pause: what it takes (below) is a stretch of the
        // slide said nearly word for word, which a stray word can't give.
        self.deck_by_content(current, now);
    }

    /// One of the next few slides fits the last stretch of speech clearly better than the slide up.
    fn deck_by_content(&mut self, current: usize, now: u64) {
        let ahead: Vec<usize> = (current + 1..self.deck.slides.len()).take(CONTENT_LOOKAHEAD).collect();
        let mut taken: Vec<&str> = Vec::new();
        let mut words = 0;
        for seg in self.deck.heard.iter().rev() {
            if taken.len() >= 2 && words >= CONTENT_MIN_WORDS {
                break;
            }
            words += seg.split_whitespace().count();
            taken.push(seg);
        }
        taken.reverse();
        // The sentence still being said counts too.
        taken.push(self.deck.heard_open.as_str());
        let text = taken.join(" ").trim().to_string();
        if text.is_empty() || ahead.is_empty() {
            return;
        }
        let idf = |w: &str| self.deck_idf(w);
        let weight = |i: usize| shared_weight_of(&self.deck.slides[i], &text, &idf).0;
        let best = ahead.iter().copied().fold(ahead[0], |a, b| if weight(b) > weight(a) { b } else { a });
        let (wb, wc) = (weight(best), weight(current));
        if wb < CHAPTER_KEYWORD_MIN_IDF || wb - wc < CHAPTER_KEYWORD_MIN_MARGIN {
            return;
        }
        if shared_weight_meaningful_of(&self.deck.slides[best], &text, &idf) < PAUSED_SKIP_MIN_IDF {
            return;
        }
        // Skipping slides takes a stretch of the slide said nearly word for word, always (a paused
        // reading's rule for a range): shared words alone are the sermon's theme.
        let said = plain_tokens(&text);
        let (run, own) = read_run(&said, &plain_tokens(&self.deck.slides[best]));
        if run < RESUME_RUN_WORDS || own < RESUME_RUN_OWN {
            return;
        }
        // A slide that repeats the one up ("Faith is…" twice) needs more of its own run than the one up
        // has: the words said may be the slide up's.
        if read_run(&said, &plain_tokens(&self.deck.slides[current])).0 >= run {
            return;
        }
        self.advance_deck(best, "a slide further on said", now);
    }

    fn advance_deck(&mut self, to: usize, why: &str, now: u64) {
        self.deck.current = Some(to);
        self.deck.last_advance_at = now;
        self.deck_moved(now);
        self.note(|_| format!("deck moves to slide {}: {why}", to + 1));
        self.effect(Effect::DeckMoved { deck: self.deck.id.clone(), index: to });
    }
}

/// Per slide: its first OPENING_WORDS words come in the slide before it, anywhere.
fn opening_repeats(slides: &[String]) -> Vec<bool> {
    const OPENING_WORDS: usize = 3;
    let tokens: Vec<Vec<String>> = slides.iter().map(|s| plain_tokens(s)).collect();
    (0..slides.len())
        .map(|i| i > 0 && tokens[i].len() >= OPENING_WORDS && tokens[i - 1].windows(OPENING_WORDS).any(|w| w == &tokens[i][..OPENING_WORDS]))
        .collect()
}
