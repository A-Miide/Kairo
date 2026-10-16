//! The citation grammar: book → chapter → verses, read off normalized words.
//!
//! Rules, each a principle of how citations are spoken:
//! - Book, optional "chapter", a number the book can have as a chapter.
//! - An aside may sit between the book and its chapter, or the chapter and
//!   its verse ("John — 'God so loved the world' — chapter 3"), but only if
//!   the keyword ("chapter"/"verse") is said again within a short window and
//!   no other book is named first. A coincidental nearby number never counts.
//! - A chapter the book doesn't have: if a book that sounds like it has that
//!   chapter, the book was misheard; otherwise "<n> verse <m>" was said the
//!   other way round.
//! - One-chapter books: a number is the verse. Given a chapter it can't have
//!   followed by a verse, the ordinal was misheard and every same-name book
//!   with that chapter is an alternative.
//! - "N and M" with no keyword is chapter N verse M, unless that book and a
//!   chapter are already being read (then it's two verses).
//! - Homophone and ordinal number words are verse numbers only when announced
//!   ("verse", ":"), and two single digit-words after the chapter of a book
//!   with 100+ chapters are still composing a three-digit number.
//! - With both "chapter" and "verse" said, a bare number between them is a
//!   stray or the chapter restated; it is kept as an alternative.
//! - One unrelated word right where "verse" belongs, followed by "N to M",
//!   is a garbled verse marker.
//! - Ranges ("to", "through", "to the end") and lists need a connector
//!   ("and", "verse"); consecutive verses join into one range. The one
//!   exception is the very next verse said straight after ("4 20 21").
//! - A book that is also an everyday word or a person's name needs "chapter"
//!   or a verse to count as a citation.

use kairo_bible::Book;

use super::books::{numbered_prefix, Books};
use super::citation::{Citation, Shape, Verses};
use super::i18n::Lang;
use super::numbers::{consume_number, is_ambiguous_number_word, Num, ORDINALS};
use crate::re::words;

/// The book and chapter being read, if any (already past expiry checks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Active {
    pub book: Option<Book>,
    pub chapter: Option<u32>,
}

const HOLD_WINDOW: usize = 12;

fn is_verse_word(w: &str) -> bool {
    matches!(w, "verse" | "verses" | "vers")
}

fn get<'a>(w: &[&'a str], i: usize) -> &'a str {
    w.get(i).copied().unwrap_or("")
}

fn num(w: &[&str], i: usize, max: Option<u32>) -> Option<Num> {
    consume_number(w, i, max.map(i64::from))
}

fn n32(v: i64) -> u32 {
    v.clamp(0, u32::MAX as i64) as u32
}

pub struct Parser<'a> {
    pub books: &'a Books,
    pub shape: &'a Shape,
    /// The language citations are spoken in.
    pub lang: Lang,
}

impl Parser<'_> {
    /// Normalized, titles resolved, "chapter 7 of Luke" put book-first.
    pub fn prepare(&self, text: &str) -> String {
        let t = super::normalize::normalize(text, self.books, self.lang);
        let t = self.books.apply_titles(&t);
        if t.contains(" of ") { self.book_first(&t) } else { t }
    }

    /// The first citation in `text`.
    pub fn parse_one(&self, text: &str, active: Active) -> Option<Citation> {
        let t = self.prepare(text);
        self.scan(&words(&t), active)
    }

    /// Every citation in `text`, in order.
    pub fn parse_all(&self, text: &str, active: Active) -> Vec<Citation> {
        let t = self.prepare(text);
        let w = words(&t);
        let mut out = Vec::new();
        let mut i = 0;
        while i < w.len() {
            let Some((_, consumed)) = self.books.match_at(&w, i) else {
                i += 1;
                continue;
            };
            // This citation runs until the next book is named — unless that
            // "book" is directly followed by "N to M" (a garbled verse marker).
            let mut next = w.len();
            for j in i + consumed..w.len() {
                if self.books.is_single_word(w[j]) && j > i + consumed {
                    let garbled = num(&w, j + 1, None).is_some_and(|s| matches!(get(&w, j + 1 + s.consumed), "to" | "through"));
                    if !garbled {
                        next = j;
                        break;
                    }
                }
                if let Some(n) = numbered_prefix(w[j]) {
                    if j + 1 < w.len() && self.books.numbered(n, w[j + 1]).is_some() {
                        next = j;
                        break;
                    }
                }
            }
            match self.scan(&w[i..next], active) {
                Some(mut c) => {
                    // A numbered book named without its number ("Timothy") could be either.
                    if c.alternatives.is_empty() && consumed == 1 {
                        if let Some(variants) = self.books.numbered_variants(w[i]) {
                            c.book = variants[0];
                            c.alternatives = variants[1..].to_vec();
                        }
                    }
                    out.push(c);
                    i = next;
                }
                None => i += 1,
            }
        }
        out
    }

    fn scan(&self, w: &[&str], active: Active) -> Option<Citation> {
        (0..w.len()).find_map(|i| self.citation_at(w, i, active))
    }

    fn citation_at(&self, w: &[&str], i: usize, active: Active) -> Option<Citation> {
        let (mut book, consumed) = self.books.match_at(w, i)?;
        let mut idx = i + consumed;
        let mut chapter_kw = get(w, idx) == "chapter";
        if chapter_kw {
            idx += 1;
        }

        if self.shape.chapters(book) == 1 {
            if let Some(c) = self.one_chapter_book(w, i, idx, book, chapter_kw) {
                return Some(c);
            }
        }

        let mut ch = num(w, idx, Some(self.shape.chapters(book)));
        if ch.is_none() && !chapter_kw {
            for k in idx..w.len().min(idx + HOLD_WINDOW) {
                if w[k] == "chapter" {
                    if let Some(held) = num(w, k + 1, Some(self.shape.chapters(book))) {
                        ch = Some(held);
                        idx = k + 1;
                        chapter_kw = true;
                    }
                    break;
                }
                if self.books.is_single_word(w[k]) {
                    break;
                }
            }
        }
        let mut ch = ch?;
        let mut chapter = n32(ch.value);
        let chapter_at = idx;
        idx += ch.consumed;

        let mut max = self.shape.chapters(book);
        if chapter > max {
            for alike in self.books.sound_alikes(book) {
                let alike_max = self.shape.chapters(alike);
                if let Some(a) = num(w, chapter_at, Some(alike_max)) {
                    if n32(a.value) <= alike_max && a.value > 0 {
                        book = alike;
                        max = alike_max;
                        ch = a;
                        chapter = n32(a.value);
                        idx = chapter_at + a.consumed;
                        break;
                    }
                }
            }
        }
        if chapter > max {
            // "<book> verse 1 chapter 6" order: the first number was the verse.
            if is_verse_word(get(w, idx)) {
                if let Some(v) = num(w, idx + 1, None) {
                    if n32(v.value) <= max {
                        return Some(Citation::new(book, n32(v.value), Verses::One(chapter)));
                    }
                }
            }
            return None;
        }

        if matches!(get(w, idx), "and" | "of" | "from") && is_verse_word(get(w, idx + 1)) {
            idx += 1;
        }
        // "Romans 8 from 28 to 31": "from" opens the verse range.
        if get(w, idx) == "from" && num(w, idx + 1, None).is_some_and(|n| matches!(get(w, idx + 1 + n.consumed), "to" | "through")) {
            idx += 1;
        }
        let mut verse_kw = is_verse_word(get(w, idx)) || get(w, idx) == ":";
        if verse_kw {
            idx += 1;
        } else if get(w, idx) == "and" && num(w, idx + 1, None).is_some() && !(active.book == Some(book) && active.chapter.is_some()) {
            verse_kw = true;
            idx += 1;
        }

        let mut v = num(w, idx, None);
        let mut repositioned = false;
        let mut restated = None;

        if let Some(x) = v {
            let word = get(w, idx);
            if !verse_kw && x.consumed == 1 && is_ambiguous_number_word(word) && !(ORDINALS.contains_key(word) && matches!(get(w, idx + 1), "verse" | "verses")) {
                v = None;
            }
        }
        if let Some(x) = v {
            if !verse_kw && ch.consumed == 1 && x.consumed == 1 && max >= 100 && chapter < 10 && x.value < 10 {
                v = None;
            }
        }
        if let Some(x) = v {
            if !verse_kw {
                let mut look = idx + x.consumed;
                while matches!(get(w, look), "and" | "of" | "from") {
                    look += 1;
                }
                if is_verse_word(get(w, look)) {
                    look += 1;
                    if let Some(explicit) = num(w, look, None) {
                        if chapter_kw && n32(x.value) != chapter && n32(x.value) <= max {
                            restated = Some(n32(x.value));
                        }
                        v = Some(explicit);
                        idx = look + explicit.consumed;
                        verse_kw = true;
                        repositioned = true;
                    }
                }
            }
        }
        if v.is_none() && !verse_kw {
            for k in idx..w.len().min(idx + HOLD_WINDOW) {
                if is_verse_word(w[k]) {
                    if let Some(held) = num(w, k + 1, None) {
                        v = Some(held);
                        idx = k + 1;
                        verse_kw = true;
                    }
                    break;
                }
                if self.books.is_single_word(w[k]) {
                    break;
                }
            }
        }
        if v.is_none() && !verse_kw && idx < w.len() {
            if let Some(start) = num(w, idx + 1, None) {
                if matches!(get(w, idx + 1 + start.consumed), "to" | "through") {
                    v = Some(start);
                    idx += 1;
                }
            }
        }

        let Some(v) = v else {
            if self.books.is_ambiguous(w[i]) && !chapter_kw {
                return None;
            }
            return Some(Citation::new(book, chapter, Verses::Chapter));
        };

        let start = n32(v.value);
        if !repositioned {
            idx += v.consumed;
        }
        let mut spans: Vec<(u32, u32)> = Vec::new();
        if matches!(get(w, idx), "to" | "through") {
            idx += 1;
            if matches!(get(w, idx), "verse" | "verses") {
                idx += 1;
            }
            match self.range_end(w, idx, book, chapter) {
                Some((end, used)) => {
                    spans.push((start, end));
                    idx += used;
                }
                None => spans.push((start, start)),
            }
        } else {
            spans.push((start, start));
        }

        let mut scan = idx;
        loop {
            let before = scan;
            while matches!(get(w, scan), "and" | "," | "verse" | "verses") {
                scan += 1;
            }
            if scan == before {
                // "Romans 4 20 21": the very next verse, said straight after,
                // continues the range; any other bare number is unrelated.
                let last = spans.last().map_or(start, |s| s.1);
                let next = num(w, scan, None).filter(|n| n.value == i64::from(last) + 1);
                match next {
                    Some(n) if self.shape.verses(book, chapter).is_none_or(|v| last < v) => {
                        scan += n.consumed;
                        if let Some(s) = spans.last_mut() {
                            s.1 = last + 1;
                        }
                        continue;
                    }
                    _ => break,
                }
            }
            let Some(s) = num(w, scan, None) else { break };
            // A list of verses is said in order ("verse 26 and verse 27"). A number lower than
            // the verse before it is a slip of the speaker or of the transcript ("26 and 20"
            // for 27), so it is not read as another verse.
            if n32(s.value) < spans.last().map_or(0, |p| p.1) {
                break;
            }
            scan += s.consumed;
            let mut end = n32(s.value);
            if matches!(get(w, scan), "to" | "through") {
                scan += 1;
                if matches!(get(w, scan), "verse" | "verses") {
                    scan += 1;
                }
                if let Some((e, used)) = self.range_end(w, scan, book, chapter) {
                    end = e;
                    scan += used;
                }
            }
            spans.push((n32(s.value), end));
        }

        if spans.len() == 2 && spans[0].0 == spans[0].1 && spans[1].0 == spans[0].0 && spans[1].1 > spans[1].0 {
            spans.remove(0);
        }
        let mut r = spans.len();
        while r > 1 {
            r -= 1;
            if spans[r].0 == spans[r - 1].1 + 1 {
                spans[r - 1].1 = spans[r - 1].1.max(spans[r].1);
                spans.remove(r);
            }
        }
        let verses = match spans.as_slice() {
            [(a, b)] if a == b => Verses::One(*a),
            [(a, b)] => Verses::Span(*a, *b),
            _ => Verses::Spans(spans),
        };
        let mut c = Citation::new(book, chapter, verses);
        c.alt_chapter = restated;
        Some(c)
    }

    /// A range's end: a number, or `end` — the chapter's last verse.
    fn range_end(&self, w: &[&str], idx: usize, book: Book, chapter: u32) -> Option<(u32, usize)> {
        if get(w, idx) == "end" {
            return self.shape.verses(book, chapter).map(|last| (last, 1));
        }
        num(w, idx, None).map(|e| (n32(e.value), e.consumed))
    }

    fn one_chapter_book(&self, w: &[&str], i: usize, idx: usize, book: Book, chapter_kw: bool) -> Option<Citation> {
        let mut scan = idx;
        if chapter_kw {
            if let Some(c) = num(w, scan, None) {
                scan += c.consumed;
            }
        }
        let mut verse_at = None;
        for k in scan..w.len().min(scan + 4) {
            if is_verse_word(w[k]) || w[k] == ":" {
                verse_at = Some(k);
                break;
            }
            if self.books.is_single_word(w[k]) {
                break;
            }
        }
        if !chapter_kw {
            if let Some(k) = verse_at.filter(|k| *k > scan) {
                if let (Some(c), Some(v)) = (num(w, scan, None), num(w, k + 1, None)) {
                    if c.value > 1 && scan + c.consumed == k {
                        let chapter = n32(c.value);
                        let base = book.name().trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == ' ');
                        let alts: Vec<Book> = ["1 ", "2 ", "3 ", ""]
                            .iter()
                            .filter_map(|p| Book::from_name(&format!("{p}{base}")))
                            .filter(|b| *b != book && self.shape.chapters(*b) >= chapter)
                            .collect();
                        if let Some((first, rest)) = alts.split_first() {
                            let mut cit = Citation::new(*first, chapter, Verses::One(n32(v.value)));
                            cit.alternatives = rest.to_vec();
                            return Some(cit);
                        }
                    }
                }
            }
        }
        let (start, mut after) = match verse_at {
            Some(k) => {
                let r = num(w, k + 1, None)?;
                (n32(r.value), k + 1 + r.consumed)
            }
            None if !chapter_kw && !self.books.is_ambiguous(w[i]) => {
                let r = num(w, scan, None)?;
                (n32(r.value), scan + r.consumed)
            }
            None => return None,
        };
        let mut end = start;
        if matches!(get(w, after), "to" | "through") {
            after += 1;
            if matches!(get(w, after), "verse" | "verses") {
                after += 1;
            }
            if let Some((e, _)) = self.range_end(w, after, book, 1) {
                if e >= start {
                    end = e;
                }
            }
        }
        let verses = if end != start { Verses::Span(start, end) } else { Verses::One(start) };
        Some(Citation::new(book, 1, verses))
    }

    /// "chapter 7 of Luke" → "luke chapter 7"; "twenty four of Genesis verse
    /// 12" → "genesis chapter twenty four verse 12" (only with a verse after
    /// the book, so "one of John's disciples" stays English); "verse 28 of
    /// Romans 8" → "romans chapter 8 verse 28".
    fn book_first(&self, text: &str) -> String {
        let w = words(text);
        let book_tokens = |b: usize| -> Option<usize> {
            if let Some(n) = numbered_prefix(get(&w, b)) {
                if self.books.numbered(n, get(&w, b + 1)).is_some() {
                    return Some(2);
                }
            }
            self.books.is_single_word(get(&w, b)).then_some(1)
        };
        let mut out: Vec<&str> = Vec::with_capacity(w.len());
        let mut i = 0;
        while i < w.len() {
            if w[i] == "chapter" {
                let mut j = i + 1;
                while j < w.len() && j - (i + 1) < 4 && w[j] != "of" {
                    j += 1;
                }
                if j > i + 1 && get(&w, j) == "of" {
                    if let Some(bt) = book_tokens(j + 1) {
                        out.extend_from_slice(&w[j + 1..j + 1 + bt]);
                        out.push("chapter");
                        out.extend_from_slice(&w[i + 1..j]);
                        i = j + 1 + bt;
                        continue;
                    }
                }
            }
            if w[i] != "chapter" && !is_ambiguous_number_word(w[i]) && w[i] != "one" {
                if let Some(n) = num(&w, i, None) {
                    let of = i + n.consumed;
                    if get(&w, of) == "of" {
                        if let Some(bt) = book_tokens(of + 1) {
                            if is_verse_word(get(&w, of + 1 + bt)) {
                                out.extend_from_slice(&w[of + 1..of + 1 + bt]);
                                out.push("chapter");
                                out.extend_from_slice(&w[i..of]);
                                i = of + 1 + bt;
                                continue;
                            }
                        }
                    }
                }
            }
            out.push(w[i]);
            i += 1;
        }
        self.verse_of_book(&out)
    }

    fn verse_of_book(&self, w: &[&str]) -> String {
        let mut out: Vec<&str> = Vec::with_capacity(w.len());
        let mut i = 0;
        while i < w.len() {
            if matches!(w[i], "verse" | "verses") {
                if let Some(v) = num(w, i + 1, None) {
                    let of = i + 1 + v.consumed;
                    if get(w, of) == "of" {
                        // "verse eight of that Isaiah chapter 58": a pointing word may sit before the book.
                        let b = of + 1 + usize::from(matches!(get(w, of + 1), "that" | "the" | "this"));
                        let bt = numbered_prefix(get(w, b))
                            .filter(|n| self.books.numbered(*n, get(w, b + 1)).is_some())
                            .map(|_| 2)
                            .or_else(|| self.books.is_single_word(get(w, b)).then_some(1));
                        if let Some(bt) = bt {
                            let mut c = b + bt;
                            if get(w, c) == "chapter" {
                                c += 1;
                            }
                            if let Some(chn) = num(w, c, None) {
                                out.extend_from_slice(&w[b..b + bt]);
                                out.push("chapter");
                                out.extend_from_slice(&w[c..c + chn.consumed]);
                                out.push("verse");
                                out.extend_from_slice(&w[i + 1..i + 1 + v.consumed]);
                                i = c + chn.consumed;
                                continue;
                            }
                        }
                    }
                }
            }
            out.push(w[i]);
            i += 1;
        }
        out.join(" ")
    }
}
