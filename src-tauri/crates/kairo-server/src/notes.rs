//! Sermon notes from a live session: the passage preached from, each point
//! the preacher made with the lines that carry it and the scriptures read,
//! cited or alluded to while making it, the sermon's themes and the people it
//! spoke of, and the prayer points raised.
//!
//! The detection engine's record of the service says how each scripture came
//! up (read aloud, cited, or alluded to), which passage was the main text,
//! and when the preacher moved between passages. The lines are the
//! preacher's own; when a local language model is available they're lightly
//! corrected for transcription and grammar, and a correction is kept only if
//! it says the same thing (measured, not assumed).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use kairo_bible::VerseId;
use kairo_detect::knowledge::Knowledge;
use regex::Regex;

use crate::pdf::{hex, text_width, Doc, Face, Rgb};

fn re(p: &str) -> Regex {
    Regex::new(p).expect("valid pattern")
}

// ── What the session recorded ───────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum How {
    /// Read aloud (word for word, or followed as it was read).
    Read,
    /// Cited by reference (or put up by the operator).
    Cited,
    /// Said in the preacher's own words.
    Alluded,
}

impl How {
    /// How a verse came up, by the method that put it on screen.
    pub fn of(method: &str) -> How {
        match method {
            "verbatim" | "stream" => How::Read,
            "paraphrase" | "semantic" | "fingerprint" => How::Alluded,
            _ => How::Cited,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Shown {
    pub verse: VerseId,
    pub at: u64,
    pub how: How,
}

#[derive(Default)]
pub struct Session {
    pub name: String,
    pub lines: Vec<Line>,
    pub shown: Vec<Shown>,
    /// Ranges cited (the passage read verse by verse).
    pub ranges: Vec<(Vec<VerseId>, u64)>,
}

// ── The notes ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Passage {
    pub verses: Vec<VerseId>,
    pub at: u64,
    pub how: How,
}

#[derive(Clone, Debug)]
pub struct Quote {
    pub text: String,
    pub at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Opening,
    Point,
    Topic,
    Whole,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub kind: Kind,
    pub heading: String,
    pub at: u64,
    pub quotes: Vec<Quote>,
    pub scriptures: Vec<Passage>,
}

pub struct Notes {
    pub title: String,
    pub started_at: Option<u64>,
    pub minutes: Option<u64>,
    pub main_text: Vec<Passage>,
    pub sections: Vec<Section>,
    pub prayer_points: Vec<Quote>,
    pub themes: Vec<String>,
    pub people: Vec<String>,
    pub passage_count: usize,
}

// ── Speech into units ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Unit {
    text: String,
    at: u64,
}

fn word_count(t: &str) -> usize {
    t.split_whitespace().count()
}

fn norm(t: &str) -> String {
    t.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '\'' || c.is_whitespace() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn content_words(t: &str) -> Vec<String> {
    norm(t).split(' ').filter(|w| w.len() >= 4).map(str::to_string).collect()
}

fn word_sets(texts: &[String]) -> Vec<HashSet<String>> {
    texts.iter().map(|t| content_words(t).into_iter().collect()).collect()
}

/// Mostly (60%+) the words of one of `sets`: a verse being read, or a point said again.
fn mostly_from(text: &str, sets: &[HashSet<String>]) -> bool {
    let w = content_words(text);
    !w.is_empty() && sets.iter().any(|s| w.iter().filter(|x| s.contains(*x)).count() as f32 / w.len() as f32 >= 0.6)
}

/// Sentences, each with the time of the line it began in.
fn sentences(lines: &[Line]) -> Vec<Unit> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut at = 0;
    for l in lines {
        let t = l.text.trim();
        if t.is_empty() {
            continue;
        }
        if buf.is_empty() {
            at = l.at;
        }
        buf = if buf.is_empty() { t.to_string() } else { format!("{buf} {t}") };
        let mut parts: Vec<String> = Vec::new();
        let mut cur = String::new();
        let words: Vec<&str> = buf.split(' ').collect();
        for (i, w) in words.iter().enumerate() {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
            if w.ends_with(['.', '!', '?']) && i + 1 < words.len() {
                parts.push(std::mem::take(&mut cur));
            }
        }
        buf = cur;
        for (i, p) in parts.iter().enumerate() {
            out.push(Unit { text: p.clone(), at: if i == 0 { at } else { l.at } });
        }
        if !parts.is_empty() {
            at = l.at;
        }
        if buf.ends_with(['.', '!', '?']) {
            out.push(Unit { text: std::mem::take(&mut buf), at });
        }
    }
    if !buf.is_empty() {
        out.push(Unit { text: buf, at });
    }
    out
}

/// Words where a long unpunctuated utterance can be split: a new clause starts.
const CLAUSE_STARTS: &[&str] = &["and", "but", "so", "because", "then", "now", "therefore", "when", "if", "that's", "which"];

/// Speech without punctuation (the offline engine, some captions): the
/// recogniser's own utterances (pauses), short ones joined, long ones cut
/// where a clause starts.
fn utterances(lines: &[Line]) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    let mut pending: Option<Unit> = None;
    for l in lines {
        let t = l.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            continue;
        }
        let u = match pending.take() {
            Some(p) => Unit { text: format!("{} {t}", p.text), at: p.at },
            None => Unit { text: t, at: l.at },
        };
        if word_count(&u.text) < 6 {
            pending = Some(u);
            continue;
        }
        let words: Vec<&str> = u.text.split(' ').collect();
        let mut start = 0;
        while words.len() - start > 32 {
            let window = start + 12..(start + 30).min(words.len());
            let cut = window.clone().rev().find(|i| CLAUSE_STARTS.contains(&words[*i].to_lowercase().as_str())).unwrap_or(start + 24);
            out.push(Unit { text: words[start..cut].join(" "), at: u.at });
            start = cut;
        }
        out.push(Unit { text: words[start..].join(" "), at: u.at });
    }
    if let Some(p) = pending {
        out.push(p);
    }
    out
}

/// Sentences when the transcript is punctuated, else utterances.
fn units(lines: &[Line]) -> (Vec<Unit>, bool) {
    let words: usize = lines.iter().map(|l| word_count(&l.text)).sum();
    let ends = lines.iter().map(|l| l.text.matches(['.', '!', '?']).count()).sum::<usize>();
    let punctuated = words > 0 && ends * 1000 / words >= 40;
    (if punctuated { sentences(lines) } else { utterances(lines) }, punctuated)
}

// ── Spoken structure: title, points, repeated lines, prayer points ──────────

static LEAD_IN: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:(?:and|so|now|but|because|then|therefore|also|again|remember|listen|very importantly|importantly|he said|she said|jesus said|paul said|the bible says|the bible said|he says)[\s,:]+)+")
});

/// A line as a note reads: spoken lead-ins dropped, first letter capitalized.
fn tidy(text: &str) -> String {
    let t = LEAD_IN.replace(text, "").trim().to_string();
    let mut c = t.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

static GARBLED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:to|the|a|an|of|for|with|by|from)\s+(?:is|are|was|were)\b|\b(?:the|a|an)\s+(?:the|a|an)\b"));
static FILLER: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:amen|praise the lord|hallelujah|thank you(?: jesus| lord)?|glory to god|are you (?:with|listening to) me|give (?:the lord|god|him) (?:a|the biggest|a big) (?:hand|shout|clap)|yes sir|somebody (?:say|shout)|can i get an amen|in (?:the|jesus) name|lift up your hands?|look at your neighbou?r|i can tell you|let me tell you|i want to tell you|i tell you|listen to me|you know what|watch this|look at this|hear me|mark this|note this)\b")
});
static CITATION_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:chapter|verses?)\s+(?:\d|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty)")
});
static DECLARATION_CUE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:say|repeat|declare|confess)\s+(?:after|with)\s+me\b[\s,:.\-–—]*"));
static TITLE_CUE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:speaking|preaching|teaching|ministering|talking|sharing)\s+(?:on|to|about|from)\s+(?:the\s+|this\s+|our\s+)?(?:subject|topic|theme|message)\s*(?:titled|entitled|called)?[\s,:]*(.{4,90}?)(?:[.!?]|$)")
});

const NUMBER_WORDS: [&str; 10] = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
const ORDINAL_WORDS: [&str; 10] = ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];

fn number_of(w: &str) -> Option<usize> {
    let w = w.to_lowercase();
    NUMBER_WORDS.iter().position(|x| *x == w).map(|i| i + 1).or_else(|| ORDINAL_WORDS.iter().position(|x| *x == w).map(|i| i + 1)).or_else(|| w.parse().ok())
}

static NUMBERED_CUE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(number|point)\s+(one|two|three|four|five|six|seven|eight|nine|ten|\d{1,2})\b[\s,:.\-–—]*(?:is\s+)?"));
/// Words before "number" that make it not a point ("the number one thing", "verse number 2").
const NOT_A_POINT_BEFORE: &[&str] = &["the", "a", "an", "my", "your", "our", "his", "her", "their", "its", "verse", "chapter", "psalm", "page"];
static ORDINAL_CUE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:and\s+|now\s+|so\s+)?(?:the\s+)?(first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth)(?:ly\b[\s,:.\-–—]*|\s*[,:.\-–—]+\s*|\s+(?:thing|key|point|step|reason|way|lesson|truth|principle)\b[\s,:.\-–—]*(?:is\s+)?|\s+is\s+)")
});
static BARE_NUMBER_CUE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(one|two|three|four|five|six|seven|eight|nine|ten)[,:]\s*"));

/// A point cue in this unit: its number and where the point's words begin.
fn cue(text: &str) -> Option<(usize, usize)> {
    for m in NUMBERED_CUE.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let before = text[..whole.start()].split_whitespace().last().unwrap_or("").to_lowercase();
        let before: String = before.chars().filter(|c| c.is_ascii_alphabetic()).collect();
        if NOT_A_POINT_BEFORE.contains(&before.as_str()) {
            continue;
        }
        return Some((number_of(&m[2])?, whole.end()));
    }
    if let Some(m) = ORDINAL_CUE.captures(text) {
        return Some((number_of(&m[1])?, m.get(0).unwrap().end()));
    }
    if let Some(m) = BARE_NUMBER_CUE.captures(text) {
        let rest = &text[m.get(0).unwrap().end()..];
        let counting = rest.split(|c: char| !c.is_alphabetic()).find(|w| !w.is_empty()).is_some_and(|w| NUMBER_WORDS.contains(&w.to_lowercase().as_str()));
        if !counting {
            return Some((number_of(&m[1])?, m.get(0).unwrap().end()));
        }
    }
    None
}

struct Point {
    n: usize,
    text: String,
    at: u64,
}

/// The preacher's own enumeration: a list of two points at least, a lower
/// number starting a new list, a number said again right after the same point.
fn detect_points(units: &[Unit]) -> Vec<Point> {
    static AFTER_CUE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:this|that)\s*[,:.\-–—]+\s*|^(?:of|on|in|from) (?:the|my|this|our|your|today's) [a-z' ]{2,24}?[,:]\s*"));
    let mut points: Vec<Point> = Vec::new();
    for (i, u) in units.iter().enumerate() {
        let Some((n, from)) = cue(&u.text) else { continue };
        if n > 10 {
            continue;
        }
        let mut text = u.text[from..].trim().to_string();
        let mut j = i + 1;
        // A point said across a pause continues in the next utterance
        // (speech without punctuation breaks mid-sentence).
        let unpunctuated = !u.text.trim_end().ends_with(['.', '!', '?']);
        let want = if unpunctuated { 10 } else { 3 };
        while word_count(&text) < want && j < units.len().min(i + 3) && cue(&units[j].text).is_none() {
            text = format!("{text} {}", units[j].text).trim().to_string();
            j += 1;
        }
        if word_count(&text) < 3 {
            continue;
        }
        let text = AFTER_CUE.replace(text.trim_start_matches([',', ':', '.', '-', '–', '—', ' ']), "").to_string();
        if let Some(prev) = points.last_mut() {
            if prev.n == n {
                if word_count(&prev.text) < 6 {
                    prev.text = text;
                }
                continue;
            }
        }
        points.push(Point { n, text, at: u.at });
    }
    let mut lists: Vec<Vec<Point>> = Vec::new();
    for p in points {
        match lists.last_mut() {
            Some(l) if p.n > l.last().unwrap().n => l.push(p),
            _ => lists.push(vec![p]),
        }
    }
    lists.into_iter().filter(|l| l.len() >= 2).flatten().collect()
}

/// A heading as a note: thirty words at most, cut where a clause ends when that leaves enough.
fn clip(text: &str, max: usize) -> String {
    let w: Vec<&str> = text.split_whitespace().collect();
    if w.len() <= max {
        return text.to_string();
    }
    let head = w[..max].join(" ");
    let cut = [',', ';', ':', '–', '—'].iter().filter_map(|c| head.rfind(*c)).max();
    match cut {
        Some(c) if word_count(&head[..c]) >= 8 => format!("{}…", &head[..c]),
        _ => format!("{head}…"),
    }
}

#[derive(Clone, Debug)]
struct KeyLine {
    text: String,
    count: usize,
    at: u64,
    declaration: bool,
}

/// Lines said again and again (the same sentence, or a six-word phrase three
/// times), and declarations the church was led to repeat.
fn detect_key_lines(units: &[Unit], max: usize, verse_texts: &[String]) -> Vec<KeyLine> {
    let verse_words = word_sets(verse_texts);
    let mut by_sentence: Vec<(String, KeyLine)> = Vec::new();
    for u in units {
        let k = norm(&u.text);
        if word_count(&k) < 5 || FILLER.is_match(&k) || CITATION_WORDS.is_match(&k) {
            continue;
        }
        match by_sentence.iter_mut().find(|e| e.0 == k) {
            Some(e) => e.1.count += 1,
            None => by_sentence.push((k, KeyLine { text: u.text.clone(), count: 1, at: u.at, declaration: false })),
        }
    }
    let mut lines: Vec<KeyLine> = by_sentence.into_iter().map(|e| e.1).filter(|l| l.count >= 2).collect();
    let mut grams: Vec<(String, usize, usize)> = Vec::new();
    let mut gram_index: HashMap<String, usize> = HashMap::new();
    for (ui, u) in units.iter().enumerate() {
        if CITATION_WORDS.is_match(&u.text) {
            continue;
        }
        let n = norm(&u.text);
        let w: Vec<&str> = n.split(' ').collect();
        let mut seen = HashSet::new();
        for i in 0..w.len().saturating_sub(5) {
            let g = w[i..i + 6].join(" ");
            if !seen.insert(g.clone()) || FILLER.is_match(&g) {
                continue;
            }
            match gram_index.get(&g) {
                Some(&gi) => grams[gi].1 += 1,
                None => {
                    gram_index.insert(g.clone(), grams.len());
                    grams.push((g, 1, ui));
                }
            }
        }
    }
    for (g, count, ui) in grams {
        if count < 3 || lines.iter().any(|l| norm(&l.text).contains(&g)) {
            continue;
        }
        lines.push(KeyLine { text: units[ui].text.clone(), count, at: units[ui].at, declaration: false });
    }
    for u in units {
        if let Some(m) = DECLARATION_CUE.find(&u.text) {
            let text = u.text[m.end()..].trim().to_string();
            if word_count(&text) >= 3 && !lines.iter().any(|l| norm(&l.text) == norm(&text)) {
                lines.push(KeyLine { text, count: 1, at: u.at, declaration: true });
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out: Vec<KeyLine> = lines
        .into_iter()
        .map(|mut l| {
            l.text = tidy(&l.text);
            l
        })
        .filter(|l| {
            let k = norm(&l.text);
            !k.is_empty() && seen.insert(k)
        })
        .filter(|l| !GARBLED.is_match(&l.text) && !mostly_from(&l.text, &verse_words))
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.at.cmp(&b.at)));
    out.truncate(max);
    out.sort_by_key(|l| l.at);
    out
}

static RAISE_PRAYER: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:(?:(?:our|the|my|your|next|last|another|final) )?(?:first |second |third |fourth |fifth |next |last |final )?prayer points?(?:\s+(?:number\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth))?(?:\s+is)?|(?:pray|say) (?:this|these|the following) (?:prayers?|words|prayer points?)|pray (?:like this|as follows|with me|after me|along with me)|(?:begin|start|continue) (?:to pray|praying)|(?:let's|let us) pray (?:like this|this|that|for)|(?:cry|call) (?:out )?(?:to|unto) (?:god|the lord|him|heaven)|open your mouth and pray)\b[:,.\s-]*")
});
static PRAY_THAT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:now |so |and )?(?:everybody |church |everyone |all of us )?pray (?:that|for)\b"));
static PRAYER_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:(?:o |oh )?(?:lord|father|god|jesus|holy spirit|abba|daddy)\b|(?:give|help|save|forgive|wash|cleanse|heal|deliver|restore|fill|open|let|grant|teach|lead|keep|make|baptize|anoint|visit|remember|arise|stretch|show|send|use|bless|empower|strengthen|renew|revive|take|come|cause|release|destroy|break|scatter|silence|uproot|remove|turn|change|enlarge|establish|settle|lift|favou?r) (?:me|my|us|our|this|it|every|all|the|your)\b|i (?:receive|repent|confess|believe|accept|surrender|reject|refuse|come|give|thank|ask|decree|declare|command)\b|thank you\b)")
});
static PRAYER_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)[,\s]*(?:in (?:the )?(?:precious |mighty |matchless )?name of jesus|in jesus'?s? (?:precious |mighty |matchless )?name)(?: we (?:have )?prayed?)?[,.!\s]*(?:amen[.!]*)?\s*$|[,.\s]*amen[.!]*\s*$")
});

/// Prayer points raised for the church to pray, kept as the prayer itself.
fn detect_prayer_points(units: &[Unit], max: usize) -> Vec<Quote> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut i = 0;
    while i < units.len() {
        let s = units[i].text.trim();
        let body = if word_count(s) > 60 {
            None
        } else if let Some(m) = RAISE_PRAYER.find(s) {
            Some(s[m.end()..].trim().to_string())
        } else if PRAY_THAT.is_match(s) {
            Some(re(r"(?i)^(?:now |so |and )").replace(s, "").to_string())
        } else {
            None
        };
        let Some(body) = body else {
            i += 1;
            continue;
        };
        let mut parts = Vec::new();
        if word_count(&body) >= 4 && (PRAY_THAT.is_match(&body) || PRAYER_WORDS.is_match(&body)) {
            parts.push(body);
        }
        let mut j = i + 1;
        while j < units.len() && j <= i + 5 {
            let next = units[j].text.trim();
            if !PRAYER_WORDS.is_match(next) || word_count(&parts.join(" ")) >= 45 {
                break;
            }
            parts.push(next.to_string());
            j += 1;
        }
        let text = tidy(PRAYER_CLOSE.replace(&parts.join(" "), "").trim());
        if word_count(&text) >= 4 {
            i = j - 1;
            if seen.insert(norm(&text)) {
                out.push(Quote { text: if text.ends_with(['.', '!', '?']) { text } else { format!("{text}.") }, at: units[i].at });
                if out.len() >= max {
                    break;
                }
            }
        }
        i += 1;
    }
    out
}

fn detect_title(units: &[Unit]) -> Option<String> {
    units.iter().take(400).find_map(|u| {
        let m = TITLE_CUE.captures(&u.text)?;
        let t = m[1].trim_matches(|c: char| c.is_whitespace() || ",:\"'“”".contains(c)).to_string();
        (word_count(&t) >= 2).then_some(t)
    })
}

// ── Scriptures ──────────────────────────────────────────────────────────────

/// Verses into passages: a chapter's verses merged into runs of consecutive
/// verses, each run placed at its earliest verse and marked by how it came up
/// (read beats cited beats alluded).
fn group(k: &Knowledge, shown: &[Shown]) -> Vec<Passage> {
    let mut by_verse: HashMap<VerseId, (u64, How)> = HashMap::new();
    for s in shown {
        let e = by_verse.entry(s.verse).or_insert((s.at, s.how));
        e.0 = e.0.min(s.at);
        e.1 = e.1.min(s.how);
    }
    let mut verses: Vec<VerseId> = by_verse.keys().copied().collect();
    verses.sort();
    let mut out: Vec<Passage> = Vec::new();
    for v in verses {
        let (at, how) = by_verse[&v];
        if let Some(run) = out.last_mut() {
            let (last, r) = (k.verse_ref(*run.verses.last().unwrap()), k.verse_ref(v));
            if last.book == r.book && last.chapter == r.chapter && r.verse == last.verse + 1 {
                run.verses.push(v);
                run.at = run.at.min(at);
                run.how = run.how.min(how);
                continue;
            }
        }
        out.push(Passage { verses: vec![v], at, how });
    }
    out.sort_by_key(|p| p.at);
    out
}

/// Passages as compact references, a chapter's runs together: "Exodus 12:21-24, 31, 33".
pub fn compact(k: &Knowledge, passages: &[&Passage]) -> Vec<String> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for p in passages {
        let (a, b) = (k.verse_ref(p.verses[0]), k.verse_ref(*p.verses.last().unwrap()));
        let chapter = format!("{} {}", a.book.name(), a.chapter);
        let verses = if a.verse == b.verse { a.verse.to_string() } else { format!("{}-{}", a.verse, b.verse) };
        match out.iter_mut().find(|c| c.0 == chapter) {
            Some(c) => c.1.push(verses),
            None => out.push((chapter, vec![verses])),
        }
    }
    out.into_iter().map(|(c, v)| format!("{c}:{}", v.join(", "))).collect()
}

pub fn reference(k: &Knowledge, p: &Passage) -> String {
    let (a, b) = (k.verse_ref(p.verses[0]), k.verse_ref(*p.verses.last().unwrap()));
    if a.verse == b.verse { format!("{} {}:{}", a.book.name(), a.chapter, a.verse) } else { format!("{} {}:{}-{}", a.book.name(), a.chapter, a.verse, b.verse) }
}

/// The passage preached from: a range cited and read, or a chapter read verse after verse.
fn main_text(k: &Knowledge, session: &Session, passages: &[Passage]) -> Vec<Passage> {
    let mut main: Vec<Passage> = Vec::new();
    for (verses, _) in &session.ranges {
        let read = session.shown.iter().filter(|s| verses.contains(&s.verse) && s.how == How::Read).count();
        if verses.len() >= 2 && read >= 1 {
            if let Some(p) = passages.iter().find(|p| p.verses.len() >= 2 && p.verses.iter().any(|v| verses.contains(v))) {
                if !main.iter().any(|m| m.verses == p.verses) {
                    main.push(p.clone());
                }
            }
        }
    }
    for p in passages {
        if p.verses.len() >= 3 && p.how == How::Read && !main.iter().any(|m| m.verses == p.verses) {
            main.push(p.clone());
        }
    }
    main.sort_by_key(|p| std::cmp::Reverse(p.verses.len()));
    main.truncate(2);
    main.sort_by_key(|p| p.at);
    let _ = k;
    main
}

// ── Themes and people ───────────────────────────────────────────────────────

/// The words the sermon keeps returning to that identify one of its
/// scriptures: what it's about, not how preaching sounds ("today", "going").
fn themes(k: &Knowledge, units: &[Unit], shown: &[Shown]) -> Vec<String> {
    use kairo_detect::lexical::stem::heal;
    // The words that identify each scripture used (its fingerprint).
    let mut scripture: HashSet<String> = HashSet::new();
    for s in shown {
        for (w, _) in k.lexicon.signature(s.verse) {
            scripture.insert(heal(k.lexicon.vocab().word(*w)));
        }
    }
    let mut counts: HashMap<String, (usize, HashMap<String, usize>)> = HashMap::new();
    for u in units {
        for w in norm(&u.text).split(' ') {
            if w.len() >= 5 && !kairo_detect::lexical::text::is_stop_word(w) && !k.is_name(w) {
                let e = counts.entry(heal(w)).or_default();
                e.0 += 1;
                *e.1.entry(w.to_string()).or_default() += 1;
            }
        }
    }
    let mut scored: Vec<(String, f32)> = counts
        .into_iter()
        .filter(|(stem, (c, _))| *c >= 4 && scripture.contains(stem))
        .filter_map(|(stem, (c, forms))| {
            let idf = k.lexicon.idf_of(&stem);
            // The words of preaching itself and of conversation: never a theme.
            let common = [
                "jesus", "christ", "father", "spirit", "amen", "glory", "praise", "hallelujah", "church", "thing", "things", "going", "please", "thank",
                "thanks", "cannot", "today", "tonight", "really", "every", "people", "never", "always", "right", "should", "would", "could", "about",
                "there", "where", "which", "believe", "know", "think", "today's", "somebody", "everybody", "nobody", "anything", "everything",
            ];
            // Shown as it was said most.
            let said = forms.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|f| f.0).unwrap_or(stem.clone());
            (idf >= 3.0 && !common.contains(&said.as_str())).then_some((said, c as f32 * idf))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().take(6).map(|(w, _)| w).collect()
}

fn title_case(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// A person, not a place or a people: the Bible has them speak, be someone's
/// son or daughter, or have children ("Moses said", "the son of Jesse").
fn is_person(k: &Knowledge, name: &str) -> bool {
    let n = title_case(name);
    let cues = [format!("{n} said"), format!("{n} answered"), format!("{n} spake"), format!("{n} begat"), format!("son of {n}"), format!("daughter of {n}"), format!("{n} the son"), format!("{n} his son"), format!("{n} his wife")];
    let mut found = 0;
    for v in k.corpus.ids() {
        let t = k.corpus.kjv().text(v).unwrap_or("");
        if t.contains(&n) && cues.iter().any(|c| t.contains(c.as_str())) {
            found += 1;
            if found >= 2 {
                return true;
            }
        }
    }
    false
}

/// People of the Bible the sermon spoke of (named twice at least).
fn people(k: &Knowledge, units: &[Unit]) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for u in units {
        for w in u.text.split_whitespace() {
            let w: String = w.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>().to_lowercase();
            if w.len() >= 3 && k.is_name(&w) {
                *counts.entry(w).or_default() += 1;
            }
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().filter(|(w, c)| *c >= 2 && kairo_bible::Book::from_name(&title_case(w)).is_none() && is_person(k, w)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.into_iter()
        .take(8)
        .map(|(w, _)| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

// ── Quotable lines ──────────────────────────────────────────────────────────

static SERVICE_TALK: LazyLock<Regex> = LazyLock::new(|| {
    let alternatives: Vec<String> = [
        r"\b(?:stand|rise) (?:up )?(?:to|on) (?:your|his|her|their) feet\b",
        r"\b(?:please )?(?:be|remain|stay) (?:comfortably )?seated\b",
        r"\b(?:lift|raise) (?:up )?(?:(?:your|both|two|holy|right|left) )+(?:hands?|voices?)\b|\b(?:your|both|two|right|left) hands? (?:up|off|on|together|high)\b",
        r"\bgive (?:the lord|god|jesus|him) (?:a |the )?(?:(?:big|bigger|biggest|loud|louder|loudest|mighty|great|good) )*(?:hand|shout|clap|praise)\b",
        r"\b(?:bow|lift) (?:your|our) heads?\b|\bclose your eyes\b|\bheads? bowed\b",
        r"\b(?:turn|look) (?:to|at) (?:your|the person|somebody)\b|\btell (?:your neighbou?r|somebody|the person)\b",
        r"\b(?:say|pray) (?:this|these|it) (?:words? )?(?:of prayer )?after me\b|\brepeat after me\b",
        r"\bin (?:the )?(?:precious |mighty |matchless )?name of jesus\b|\bin jesus'?s? (?:precious |mighty |matchless )?name\b",
        r"\blet us pray\b|\bwe (?:have )?(?:prayed|given thanks)\b|\bfather,? (?:we|i) (?:thank|pray|ask|come)\b",
        r"\b(?:i|we) (?:decree|declare)\b|\bshall be (?:scattered|destroyed|broken|removed|consumed|silenced)\b|\bis (?:declared|broken|destroyed|averted) (?:today|now|this|over)\b",
        r"\b(?:clap|wave) your hands\b|\bopen your bibles?\b|\bturn (?:with me )?to (?:the book of )?\w+ chapter\b",
        r"\bplease (?:stand|rise|come|move|submit|sit|return|get)\b|\b(?:lift|raise) (?:up )?(?:your )?(?:right |left )?hands?\b",
        r"\b(?:let me hear|give me) (?:a |your )?(?:loud(?:est)? )?(?:amen|shout|clap)\b|\baltar\b|\b(?:dedicate|rededicate|give) (?:your|his|her|their) li(?:fe|ves) to (?:christ|jesus|him)\b",
        r"\b(?:back (?:on|to) your seats?|fill (?:out|in) (?:your|the) card|(?:submit|drop) (?:your|the) card)\b|\bthe prayer (?:i|we) pray\b",
        r"\b(?:is|are|be) (?:declared )?(?:averted|reversed|cancell?ed|broken|over) (?:today|tonight|now|this (?:morning|evening|moment))\b|\bi (?:speak|pronounce|command)\b",
        r"\b(?:is|are|be) (?:hereby )?(?:declared|pronounced|decreed)\b|\b(?:is|are) (?:broken|lifted|cancell?ed|destroyed) (?:off|over) (?:your|you)\b",
        r"\b(?:i|we|let me|let us|let's)(?: will| want to| would like to| like to|'ll|'d like to|'m| am| are)? pray(?:ing)? (?:with|for|over) (?:you|us)\b",
    ]
    .iter()
    .map(|p| format!("(?:{p})"))
    .collect::<Vec<_>>();
    re(&format!("(?i){}", alternatives.join("|")))
});

static LEANS_ON_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:it'?s?|its|they|them|their|he|him|his|she|her|this|that|these|those|there|here|which|who|whom|whose|what|where|why|how|or|nor|of|to|for|with|in|into|on|upon|at|by|from|through|under|over|without|within|about|after|before|until|while|as|like|just|maybe|well|okay|ok|yes|yeah|no|oh|uh|um|uh-uh|mm|hmm|ah|eh|now|even|also|then|than|is|are|was|were|i mean|you know|if you like|thy|thee|thou)\b|^(?:when|whenever|if|because|since|though|although|unless|once) (?:he|she|they|it|him|her|them|this|that|these|those)\b|^(?:when|whenever|if|since|though|although|unless|once)\b[^,;:–—]*$|^(?:bring|make|give|take|lead|keep|bless|help|set|cause|grant|send|put|carry|turn)\s+you\b")
});
static TELLS_THE_ROOM: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:please |just |now |come on )?(?:move|go|come|turn|look|tell|say|lift|raise|stand|sit|bow|close|open|give|clap|shout|hold|put|take|ask|pray|repeat|wave|write|mention|touch|hug|greet|check|call)\b")
});
static ABOUT_THE_ROOM: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:ushers?|offering|seats?|seated|cards?|altar|workshop|officials?|announcements?|yesterday|last (?:week|night|year|sunday)|microphone|camera|livestream|online|the team|papers?|envelopes?|naira|dollars?|pounds?)\b|\d")
});
static HAS_VERB: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:is|are|was|were|be|been|being|am|will|shall|would|should|can|could|cannot|can't|must|may|might|do|does|did|don't|doesn't|didn't|has|have|had|won't|isn't|aren't|wasn't|weren't|never|always)\b|\b\w+(?:s|ed|es)\b")
});
static ADDRESSED_TO_GOD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:o |oh )?(?:jesus|lord|father|god|holy spirit|abba),\s"));
static PRAYER_START: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:let|may)\b"));
static THIS_MOMENT: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:today|tonight|right now|from now|this (?:[a-z]+ )?(?:morning|evening|night|moment|year|week|service|meeting|gathering|encounter|conference|convention|programme|program))\b")
});
static MISHEARD: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(?:my|your|our|his|their|the|an?)\s+(?:that|which|is|are|was|were|the|an?|my|your|our|his|their)\b|\b(?:until|unless|because|although|though)\s+(?:be|is|are|was|were)\b|\b(?:and|or|but|the|an?|to|of|for|with|that|because|so)\s*[.!]?\s*$")
});
static TELLS_A_STORY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:had|went|came|said|told|saw|took|gave|happened)\b"));
static SAYS_A_TRUTH: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:is|are|will|shall|can|cannot|can't|must|never|always|don't|doesn't|won't|isn't|aren't)\b"));
static FIRST_PERSON: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:i|i'm|i've|i'll|i'd|me|my|mine|myself)\b"));
static THIRD_PERSON: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:it|it's|its|she|she's|her|they|they're|them|their)\b"));
static ORDINAL_LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:first(?:ly)?|second(?:ly)?|third(?:ly)?|fourth(?:ly)?|finally|lastly),?\s+"));

const LIGHT_WORDS: &str = "i i'm i've i'll i'd me my mine you you're you've you'll your yours he he's him his she she's her hers it it's its we we're us our they they're them their this that these those here there what which who whom is are was were be been being am do does did done doing don't doesn't didn't have has had will would shall should can could may might must won't can't cannot not no yes going go goes gone get gets got to a an the and or but of in on at for with so just now then out up like let make say said know see thing things one all some any";

fn stumbled(t: &str) -> bool {
    let w: Vec<String> = t.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    w.windows(2).any(|p| p[0] == p[1])
}

/// A line that stands on its own as a quote.
fn quotable(text: &str, min: usize) -> bool {
    static LIGHT: LazyLock<HashSet<&'static str>> = LazyLock::new(|| LIGHT_WORDS.split(' ').collect());
    let t = ORDINAL_LEAD.replace(text, "");
    let n = word_count(&t);
    let substantive = norm(&t).split(' ').filter(|w| !w.is_empty() && !LIGHT.contains(w)).count();
    let leans_on_pronouns = THIRD_PERSON.find_iter(&t).count() as f32 / n.max(1) as f32 >= 0.25;
    n >= min
        && n <= 24
        && substantive >= 2
        && !LEANS_ON_CONTEXT.is_match(&t)
        && !leans_on_pronouns
        && !TELLS_THE_ROOM.is_match(&t)
        && !ABOUT_THE_ROOM.is_match(&t)
        && HAS_VERB.is_match(&t)
        && !t.trim_end().ends_with('?')
        && !ADDRESSED_TO_GOD.is_match(&t)
        && !PRAYER_START.is_match(&t)
        && !THIS_MOMENT.is_match(&t)
        && !stumbled(&t)
        && FIRST_PERSON.find_iter(&t).count() < 2
        && !t.to_lowercase().starts_with("some of ")
        && !MISHEARD.is_match(&t)
        && !(TELLS_A_STORY.is_match(&t) && !SAYS_A_TRUTH.is_match(&t))
}

#[derive(Clone)]
struct Cand {
    text: String,
    at: u64,
    quotable: bool,
    vec: Vec<f32>,
    raw: Vec<f32>,
    echoes: usize,
}

fn candidates(units: &[Unit], verse_texts: &[String]) -> Vec<Cand> {
    let verse_words = word_sets(verse_texts);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for u in units {
        let text = tidy(&u.text);
        let n = word_count(&text);
        if !(6..=45).contains(&n) {
            continue;
        }
        let k = norm(&text);
        if seen.contains(&k) || FILLER.is_match(&k) || CITATION_WORDS.is_match(&k) || GARBLED.is_match(&text) || DECLARATION_CUE.is_match(&text) || SERVICE_TALK.is_match(&text) {
            continue;
        }
        if cue(&u.text).is_some() || (text.trim_end().ends_with('?') && n < 12) || mostly_from(&text, &verse_words) {
            continue;
        }
        seen.insert(k);
        let q = quotable(&text, 6);
        out.push(Cand { text, at: u.at, quotable: q, vec: Vec::new(), raw: Vec::new(), echoes: 0 });
    }
    out
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn centroid(vecs: &[&Vec<f32>]) -> Vec<f32> {
    let mut c = vec![0f32; vecs[0].len()];
    for v in vecs {
        for (a, b) in c.iter_mut().zip(v.iter()) {
            *a += b;
        }
    }
    let n = c.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    c.iter_mut().for_each(|x| *x /= n);
    c
}

/// Up to `k` lines near `centre`, each unlike those already picked
/// (maximal marginal relevance), kept in the order said.
fn pick_central(items: &[&Cand], centre: &[f32], k: usize, avoid: Option<&[f32]>, floor: f32) -> Vec<Cand> {
    let mut pool: Vec<(&Cand, f32)> = items
        .iter()
        .map(|it| {
            let mut rel = dot(&it.vec, centre) - avoid.map_or(0.0, |a| 0.5 * dot(&it.vec, a).max(0.0)) + 0.06 * (it.echoes as f32).ln_1p();
            rel *= (0.88 + 0.12 * (content_words(&it.text).len() as f32 / 10.0).min(1.0)) * if it.text.starts_with("I ") { 0.9 } else { 1.0 };
            (*it, rel)
        })
        .filter(|p| p.1 >= floor)
        .collect();
    let mut picked: Vec<Cand> = Vec::new();
    while picked.len() < k && !pool.is_empty() {
        let mut best: Option<(usize, f32)> = None;
        for (i, (c, rel)) in pool.iter().enumerate() {
            let redundancy = picked.iter().map(|p| dot(&p.vec, &c.vec)).fold(0.0, f32::max);
            if redundancy >= 0.9 {
                continue;
            }
            let s = 0.72 * rel - 0.28 * redundancy;
            if best.is_none_or(|b| s > b.1) {
                best = Some((i, s));
            }
        }
        let Some((i, _)) = best else { break };
        picked.push(pool.remove(i).0.clone());
    }
    picked.sort_by_key(|c| c.at);
    picked
}

/// Where the subject shifts (TextTiling over the meaning vectors): about one
/// section per eight minutes, two to six of them.
fn topic_sections(cands: &[Cand], minutes: Option<u64>) -> Vec<Vec<Cand>> {
    let n = cands.len();
    let want = (((minutes.unwrap_or(n as u64 / 10)) as f32 / 8.0).round() as usize).clamp(2, 6);
    let w = 5;
    let min_len = (n / (want * 2)).max(6);
    if n < want * min_len || n < 2 * w {
        return vec![cands.to_vec()];
    }
    let mut gaps: Vec<(usize, f32, f32)> = (w..=n - w)
        .map(|g| {
            let a: Vec<&Vec<f32>> = cands[g - w..g].iter().map(|c| &c.vec).collect();
            let b: Vec<&Vec<f32>> = cands[g..g + w].iter().map(|c| &c.vec).collect();
            (g, dot(&centroid(&a), &centroid(&b)), 0.0)
        })
        .collect();
    for i in 0..gaps.len() {
        let (mut l, mut r) = (gaps[i].1, gaps[i].1);
        let mut j = i;
        while j > 0 && gaps[j - 1].1 >= l {
            j -= 1;
            l = gaps[j].1;
        }
        let mut j = i;
        while j + 1 < gaps.len() && gaps[j + 1].1 >= r {
            j += 1;
            r = gaps[j].1;
        }
        gaps[i].2 = (l - gaps[i].1) + (r - gaps[i].1);
    }
    let mut by_depth = gaps.clone();
    by_depth.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut cuts: Vec<usize> = Vec::new();
    for (g, _, depth) in by_depth {
        if cuts.len() >= want - 1 || depth <= 0.0 {
            break;
        }
        if g >= min_len && n - g >= min_len && cuts.iter().all(|c| c.abs_diff(g) >= min_len) {
            cuts.push(g);
        }
    }
    cuts.sort();
    let mut out = Vec::new();
    let mut from = 0;
    for c in cuts.into_iter().chain(std::iter::once(n)) {
        out.push(cands[from..c].to_vec());
        from = c;
    }
    out
}

// ── Building ────────────────────────────────────────────────────────────────

/// What the notes need from the meaning model, when it's loaded.
pub trait Meaning {
    /// A unit vector per text.
    fn embed(&self, texts: &[String]) -> Option<Vec<Vec<f32>>>;
}

impl Meaning for kairo_semantic::MeaningService {
    fn embed(&self, texts: &[String]) -> Option<Vec<Vec<f32>>> {
        kairo_semantic::MeaningService::embed(self, texts).ok()
    }
}

/// Almost every word a real word (the Bible's vocabulary across its
/// translations covers everyday English): a line the recogniser garbled
/// ("Financial is Tien why is it Kos") isn't fit to quote.
fn fluent(k: &Knowledge, text: &str) -> bool {
    let words: Vec<String> = text.split_whitespace().map(|w| w.to_lowercase().chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>()).filter(|w| !w.is_empty()).collect();
    let unknown = words.iter().filter(|w| k.lexicon.vocab().id(w).is_none() && !k.is_name(w)).count();
    !words.is_empty() && unknown * 12 <= words.len()
}

/// Is a line a verse being read (most of it a verse's own wording)?
fn is_scripture(k: &Knowledge, text: &str) -> bool {
    kairo_detect::lexical::verbatim(&k.lexicon, text, 6, 1).first().is_some_and(|h| h.score >= 0.6)
}

pub fn build(k: &Knowledge, session: &Session, meaning: Option<&dyn Meaning>) -> Notes {
    let (units, _punctuated) = units(&session.lines);
    let first = session.lines.iter().map(|l| l.at).filter(|a| *a > 0).min();
    let last = session.lines.iter().map(|l| l.at).max();
    let minutes = first.zip(last).map(|(a, b)| (b.saturating_sub(a) / 60_000).max(1));
    let passages = group(k, &session.shown);
    let main = main_text(k, session, &passages);
    let verse_texts: Vec<String> = session.shown.iter().map(|s| k.corpus.kjv().text(s.verse).unwrap_or("").to_string()).collect();

    let points: Vec<Section> = detect_points(&units).into_iter().map(|p| Section { kind: Kind::Point, heading: clip(&tidy(&p.text), 30), at: p.at, quotes: Vec::new(), scriptures: Vec::new() }).collect();
    let prayer_points = detect_prayer_points(&units, 12);
    let prayers: Vec<String> = prayer_points.iter().map(|p| norm(&p.text)).collect();
    let headings = word_sets(&points.iter().map(|p| p.heading.clone()).collect::<Vec<_>>());
    let mut key_lines: Vec<KeyLine> = detect_key_lines(&units, 12, &verse_texts)
        .into_iter()
        .filter(|l| quotable(&l.text, 5) && !mostly_from(&l.text, &headings) && !FILLER.is_match(&norm(&l.text)) && !prayers.iter().any(|p| p.contains(&norm(&l.text))) && (l.declaration || !SERVICE_TALK.is_match(&l.text)))
        .filter(|l| !is_scripture(k, &l.text) && fluent(k, &l.text))
        .collect();

    let mut sections: Vec<Section> = if points.is_empty() {
        vec![Section { kind: Kind::Whole, heading: String::new(), at: 0, quotes: Vec::new(), scriptures: Vec::new() }]
    } else {
        std::iter::once(Section { kind: Kind::Opening, heading: "Opening".into(), at: 0, quotes: Vec::new(), scriptures: Vec::new() }).chain(points).collect()
    };

    // Meaning: each part's lines are the quotable sentences nearest what the
    // part is about; unnumbered sermons are split where the subject shifts.
    if let Some(m) = meaning {
        let mut cands: Vec<Cand> = candidates(&units, &verse_texts).into_iter().filter(|c| !is_scripture(k, &c.text)).collect();
        for c in cands.iter_mut() {
            c.quotable = c.quotable && fluent(k, &c.text);
        }
        if cands.len() >= 8 {
            let service: Vec<String> = units.iter().filter(|u| word_count(&u.text) >= 5 && (SERVICE_TALK.is_match(&u.text) || FILLER.is_match(&norm(&u.text)))).take(80).map(|u| u.text.clone()).collect();
            let texts: Vec<String> = cands.iter().map(|c| c.text.clone()).chain(key_lines.iter().map(|l| l.text.clone())).chain(service.iter().cloned()).collect();
            if let Some(vecs) = m.embed(&texts).filter(|v| v.len() == texts.len()) {
                let nc = cands.len();
                for (c, v) in cands.iter_mut().zip(&vecs) {
                    c.raw = v.clone();
                }
                let raws: Vec<Vec<f32>> = cands.iter().map(|c| c.raw.clone()).collect();
                for (i, c) in cands.iter_mut().enumerate() {
                    c.echoes = raws.iter().enumerate().filter(|(j, o)| *j != i && dot(o, &raws[i]) >= 0.88).count();
                }
                // Every sentence of a sermon sounds alike to the model: take the sermon's average away.
                let dims = vecs[0].len();
                let mut mean = vec![0f32; dims];
                for c in &cands {
                    for (a, b) in mean.iter_mut().zip(&c.raw) {
                        *a += b / nc as f32;
                    }
                }
                let centre = |v: &[f32]| -> Vec<f32> {
                    let mut o: Vec<f32> = v.iter().zip(&mean).map(|(a, b)| a - b).collect();
                    let n = o.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
                    o.iter_mut().for_each(|x| *x /= n);
                    o
                };
                for c in cands.iter_mut() {
                    c.vec = centre(&c.raw);
                }
                let service_vecs: Vec<Vec<f32>> = vecs[nc + key_lines.len()..].iter().map(|v| centre(v)).collect();
                let avoid = (service_vecs.len() >= 3).then(|| centroid(&service_vecs.iter().collect::<Vec<_>>()));
                let key_raw: Vec<Vec<f32>> = vecs[nc..nc + key_lines.len()].to_vec();
                let mut used: Vec<Vec<f32>> = key_raw.clone();
                let unused = |list: &[Cand], used: &[Vec<f32>]| -> Vec<Cand> { list.iter().filter(|c| !used.iter().any(|u| dot(u, &c.raw) >= 0.88)).cloned().collect() };
                let pick = |from: &[Cand], k: usize, all: &[Cand], avoid: Option<&[f32]>| -> Vec<Cand> {
                    if all.len() < 3 || from.is_empty() || k == 0 {
                        return Vec::new();
                    }
                    let quot: Vec<&Cand> = from.iter().filter(|c| c.quotable).collect();
                    pick_central(&quot, &centroid(&all.iter().map(|c| &c.vec).collect::<Vec<_>>()), k, avoid, 0.12)
                };
                let parts_said: Vec<Vec<Cand>> = if sections.len() > 1 {
                    (0..sections.len())
                        .map(|i| {
                            let (from, to) = (sections[i].at, sections.get(i + 1).map_or(u64::MAX, |s| s.at));
                            cands.iter().filter(|c| c.at >= from && c.at < to).cloned().collect()
                        })
                        .collect()
                } else {
                    // Unnumbered: where the subject shifts, each part headed by its most telling sentence.
                    let mut topics: Vec<(Section, Vec<Cand>)> = Vec::new();
                    let mut carry: Vec<Cand> = Vec::new();
                    for list in topic_sections(&cands, minutes) {
                        let head = pick(&unused(&list, &used), 1, &list, avoid.as_deref()).into_iter().next();
                        match head {
                            None => match topics.last_mut() {
                                Some(t) => t.1.extend(list),
                                None => carry.extend(list),
                            },
                            Some(h) => {
                                used.push(h.raw.clone());
                                let at = carry.first().or(list.first()).map_or(0, |c| c.at);
                                let mut all = std::mem::take(&mut carry);
                                all.extend(list);
                                topics.push((Section { kind: Kind::Topic, heading: h.text.clone(), at, quotes: Vec::new(), scriptures: Vec::new() }, all));
                            }
                        }
                    }
                    if topics.len() >= 2 {
                        sections = topics.iter().map(|t| t.0.clone()).collect();
                        topics.into_iter().map(|t| t.1).collect()
                    } else {
                        used.truncate(key_raw.len());
                        vec![cands.clone()]
                    }
                };
                for (i, s) in sections.iter_mut().enumerate() {
                    let list = &parts_said[i];
                    let k = match s.kind {
                        Kind::Opening => if list.len() >= 4 { 2 } else { 0 },
                        Kind::Whole => 6,
                        _ => if list.len() >= 30 { 4 } else { 3 },
                    };
                    let heading = if matches!(s.kind, Kind::Point | Kind::Topic) { word_sets(std::slice::from_ref(&s.heading)) } else { Vec::new() };
                    let from: Vec<Cand> = unused(list, &used).into_iter().filter(|c| !mostly_from(&c.text, &heading)).collect();
                    let lines = pick(&from, k, list, avoid.as_deref());
                    used.extend(lines.iter().map(|c| c.raw.clone()));
                    s.quotes = lines.into_iter().map(|c| Quote { text: c.text, at: c.at }).collect();
                }
                // A repeated line that's mostly a verse's wording is the verse.
                key_lines.retain(|l| !is_scripture(k, &l.text));
            }
        }
    }

    // Each part: its scriptures (when they came up), its repeated lines first.
    let starts: Vec<u64> = sections.iter().map(|s| s.at).collect();
    let main_verses: HashSet<VerseId> = main.iter().flat_map(|p| p.verses.iter().copied()).collect();
    for (i, s) in sections.iter_mut().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(u64::MAX);
        let mine: Vec<Shown> = session.shown.iter().filter(|x| (x.at >= s.at || i == 0) && x.at < end && !main_verses.contains(&x.verse)).copied().collect();
        s.scriptures = group(k, &mine);
        let mut repeated: Vec<Quote> = key_lines.iter().filter(|l| starts.iter().rposition(|st| *st <= l.at).unwrap_or(0) == i).map(|l| Quote { text: l.text.clone(), at: l.at }).collect();
        let cap = if s.kind == Kind::Whole { 6 } else { 3 };
        repeated.truncate(cap);
        let mut all = repeated;
        for q in std::mem::take(&mut s.quotes) {
            if all.len() > cap {
                break;
            }
            if !all.iter().any(|a| norm(&a.text) == norm(&q.text)) {
                all.push(q);
            }
        }
        all.sort_by_key(|q| q.at);
        s.quotes = all;
    }
    sections.retain(|s| !matches!(s.kind, Kind::Opening | Kind::Whole) || !s.quotes.is_empty() || !s.scriptures.is_empty());

    Notes {
        title: Some(session.name.trim().to_string()).filter(|n| !n.is_empty()).or_else(|| detect_title(&units)).unwrap_or_else(|| "Sermon Notes".into()),
        started_at: first,
        minutes,
        passage_count: passages.len(),
        main_text: main,
        sections,
        prayer_points,
        themes: themes(k, &units, &session.shown),
        people: people(k, &units),
    }
}

// ── Polish ──────────────────────────────────────────────────────────────────

/// A corrected line is kept when it means what was said: close in meaning
/// (the same model's similarity), much the same length, and no new
/// numbers or names.
pub fn same_meaning(original: &str, corrected: &str, sim: f32) -> bool {
    let (a, b) = (word_count(original) as f32, word_count(corrected) as f32);
    let digits = |t: &str| t.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    let caps = |t: &str| t.split_whitespace().skip(1).filter(|w| w.chars().next().is_some_and(char::is_uppercase)).map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).collect::<HashSet<_>>();
    sim >= 0.9 && b >= a * 0.6 && b <= a * 1.3 && digits(original) == digits(corrected) && caps(corrected).is_subset(&caps(original).union(&caps(&original.to_lowercase())).cloned().collect())
}

// ── PDF ─────────────────────────────────────────────────────────────────────

const BRAND: &str = "#F5301A";
const INK: &str = "#1d1d1f";
const MUTED: &str = "#6e6e73";

fn how_label(h: How) -> &'static str {
    match h {
        How::Read => "Read",
        How::Cited => "Cited",
        How::Alluded => "Alluded to",
    }
}

pub fn render_pdf(k: &Knowledge, notes: &Notes, when: &str) -> Vec<u8> {
    let (brand, ink, muted) = (hex(BRAND), hex(INK), hex(MUTED));
    let mut d = Doc::new(&notes.title, 64.0, 60.0);
    let w = d.width;
    let heading = |d: &mut Doc, t: &str, brand: &Rgb| {
        d.space(14.0);
        d.keep_room(70.0);
        d.text(&t.to_uppercase(), Face::Bold, 12.0, brand, 0.0, w, 0.0);
        d.space(5.0);
    };
    d.text(&notes.title, Face::Bold, 24.0, &ink, 0.0, w, 0.0);
    d.space(3.0);
    let meta: Vec<String> = [
        when.to_string(),
        notes.minutes.map(|m| format!("{m} min")).unwrap_or_default(),
        format!("{} passage{}", notes.passage_count, if notes.passage_count == 1 { "" } else { "s" }),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    d.text(&meta.join("  ·  "), Face::Regular, 10.5, &muted, 0.0, w, 0.0);
    d.space(5.0);
    d.rule(w, &brand, 1.5);
    d.space(6.0);
    // Themes and people, one line each.
    for (label, list) in [("Themes", &notes.themes), ("People", &notes.people)] {
        if !list.is_empty() {
            let t = format!("{label}   {}", list.join("  ·  "));
            d.text(&t, Face::Regular, 10.0, &muted, 0.0, w, 1.5);
        }
    }

    if !notes.main_text.is_empty() {
        heading(&mut d, if notes.main_text.len() > 1 { "The texts" } else { "The text" }, &brand);
        for p in &notes.main_text {
            d.keep_room(60.0);
            d.text(&reference(k, p), Face::Bold, 12.0, &ink, 0.0, w, 0.0);
            d.space(3.0);
            for v in p.verses.iter().take(12) {
                let r = k.verse_ref(*v);
                let t = format!("{}  {}", r.verse, k.corpus.kjv().text(*v).unwrap_or(""));
                d.text(&t, Face::Regular, 10.5, &ink, 22.0, w - 22.0, 1.5);
            }
            if p.verses.len() > 12 {
                d.text("…", Face::Regular, 10.5, &muted, 22.0, w - 22.0, 1.5);
            }
            d.space(6.0);
        }
    }

    if !notes.sections.is_empty() {
        let has_points = notes.sections.iter().any(|s| matches!(s.kind, Kind::Point | Kind::Topic));
        heading(&mut d, if has_points { "The points" } else { "The message" }, &brand);
        let mut n = 0;
        for s in &notes.sections {
            match s.kind {
                Kind::Opening => {
                    d.keep_room(55.0);
                    d.text(&s.heading, Face::Bold, 12.0, &ink, 22.0, w - 22.0, 0.0);
                }
                Kind::Point | Kind::Topic => {
                    n += 1;
                    let h = d.height_of(&s.heading, Face::Bold, 12.0, w - 22.0, 1.5);
                    d.keep_room(h + 40.0);
                    let y = d.y;
                    let num = format!("{n}.");
                    d.text_at(&num, Face::Bold, 12.0, &brand, d.left, y);
                    d.text(&s.heading, Face::Bold, 12.0, &ink, 22.0, w - 22.0, 1.5);
                }
                Kind::Whole => {}
            }
            for q in &s.quotes {
                let line = if q.text.ends_with(['.', '!', '?']) { q.text.clone() } else { format!("{}.", q.text.trim_end_matches([' ', ',', ';', ':', '–', '—', '-'])) };
                d.space(3.0);
                d.text(&format!("“{line}”"), Face::Oblique, 11.5, &ink, 22.0, w - 22.0, 1.5);
            }
            // Scriptures by how they came up.
            for how in [How::Read, How::Cited, How::Alluded] {
                let of_kind: Vec<&Passage> = s.scriptures.iter().filter(|p| p.how == how).collect();
                let refs: Vec<String> = compact(k, &of_kind).into_iter().map(|r| r.replace(' ', "\u{00A0}")).collect();
                if refs.is_empty() {
                    continue;
                }
                d.space(3.0);
                let label = how_label(how);
                let lw = text_width(label, Face::Bold, 10.0) + 8.0;
                let y = d.y;
                d.text_at(label, Face::Bold, 10.0, &muted, d.left + 22.0, y);
                d.text(&refs.join("  ·  "), Face::Regular, 10.0, &muted, 22.0 + lw, w - 22.0 - lw, 1.5);
            }
            d.space(10.0);
        }
    }

    if !notes.prayer_points.is_empty() {
        heading(&mut d, "Prayer points", &brand);
        for (i, p) in notes.prayer_points.iter().enumerate() {
            let h = d.height_of(&p.text, Face::Regular, 12.0, w - 22.0, 1.5);
            d.keep_room(h);
            let y = d.y;
            d.text_at(&format!("{}.", i + 1), Face::Bold, 12.0, &brand, d.left, y);
            d.text(&p.text, Face::Regular, 12.0, &ink, 22.0, w - 22.0, 1.5);
            d.space(5.0);
        }
    }

    // The footer under the notes, or at the foot of the last page.
    let footer = "Notes generated by Kairo from the live session.";
    let y = (d.y + 14.0).min(crate::pdf::PAGE_H - 44.0);
    let x = d.left + (w - text_width(footer, Face::Regular, 8.5)) / 2.0;
    d.text_at(footer, Face::Regular, 8.5, &muted, x, y);
    let _ = d.bottom_margin();
    d.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<Line> {
        texts.iter().enumerate().map(|(i, t)| Line { text: t.to_string(), at: 1_000 + i as u64 * 5_000 }).collect()
    }

    #[test]
    fn points_in_unpunctuated_speech() {
        let l = lines(&[
            "today we are looking at the triggers of favor",
            "number one trigger of favor is humility before God and men",
            "he that humbles himself shall be exalted",
            "number two trigger of favor is prayer and fasting",
            "the king could not resist",
            "number three trigger of favor is serving the interest of God",
        ]);
        let (u, punctuated) = units(&l);
        assert!(!punctuated);
        let p = detect_points(&u);
        assert_eq!(p.len(), 3);
        assert!(p[1].text.starts_with("trigger of favor is prayer"));
    }

    #[test]
    fn a_lone_number_one_isnt_a_list() {
        let (u, _) = units(&lines(&["The number one thing is love.", "Number one, we must pray."]));
        assert!(detect_points(&u).is_empty());
    }

    #[test]
    fn a_correction_must_mean_the_same() {
        assert!(same_meaning("God cannot confirm what you will not declare", "God cannot confirm what you won't declare.", 0.95));
        assert!(!same_meaning("God cannot confirm what you will not declare", "God cannot confirm what you will not declare in 2025.", 0.95));
        assert!(!same_meaning("Your faith must be kept charged", "Faith.", 0.95));
        assert!(!same_meaning("Your faith must be kept charged", "Your faith must be kept charged", 0.5));
    }
}
