//! A Bible the church brings itself (Settings → Bible → Your Bibles). The file is read on this
//! computer, checked against the KJV's book and chapter layout, and saved as
//! `translations/<CODE>.json` in the app data folder, where the translation loader looks first.
//!
//! Formats: USFM, USX, OSIS, Zefania XML, and a zip of those. Anything else (a database,
//! a program, Logos or Accordance files) is refused with a plain reason.

use std::collections::HashMap;
use std::sync::LazyLock;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::Json;
use kairo_bible::book::BOOK_NAMES;
use kairo_bible::{Book, Corpus, VerseRef};
use regex::Regex;
use serde_json::{json, Value};

use crate::Shared;

/// Codes that belong to Bibles the app ships; an imported Bible can't take their place.
const RESERVED: [&str; 5] = ["KJV", "NLT", "NIV", "ESV", "NASB"];

const USFM_CODES: [&str; 66] = [
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH", "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO",
    "ECC", "SNG", "ISA", "JER", "LAM", "EZK", "DAN", "HOS", "JOL", "AMO", "OBA", "JON", "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL", "MAT",
    "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP", "COL", "1TH", "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAS", "1PE",
    "2PE", "1JN", "2JN", "3JN", "JUD", "REV",
];

const OSIS_CODES: [&str; 66] = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam", "1Kgs", "2Kgs", "1Chr", "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps",
    "Prov", "Eccl", "Song", "Isa", "Jer", "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos", "Obad", "Jonah", "Mic", "Nah", "Hab", "Zeph", "Hag", "Zech",
    "Mal", "Matt", "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal", "Eph", "Phil", "Col", "1Thess", "2Thess", "1Tim", "2Tim", "Titus",
    "Phlm", "Heb", "Jas", "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];

/// verse rows as read from a file: (book index, chapter, verse, text)
type Row = (usize, u16, u16, String);

#[derive(Default, Debug)]
pub struct Parsed {
    pub name: Option<String>,
    pub rows: Vec<Row>,
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("pattern")
}

/// A book by any name or code a file might use: USFM or OSIS codes, the English name ("Song of Songs",
/// "Revelation of John"), numbered books written "1 John", "I John", "First John" or "1John", and a
/// book's number (1 to 66) when the whole thing is a number.
pub(crate) fn osis_code(book: usize) -> &'static str {
    OSIS_CODES[book]
}

/// The three-letter book code (GEN, JHN) that API.Bible names its chapters by ("JHN.3").
pub(crate) fn usfm_code(book: usize) -> &'static str {
    USFM_CODES[book]
}

fn book_from_code(code: &str) -> Option<usize> {
    let c = code.trim();
    if let Ok(n) = c.parse::<usize>() {
        return (1..=66).contains(&n).then(|| n - 1);
    }
    USFM_CODES.iter().position(|b| b.eq_ignore_ascii_case(c))
        .or_else(|| OSIS_CODES.iter().position(|b| b.eq_ignore_ascii_case(c)))
        .or_else(|| book_by_name(c))
}

fn book_by_name(name: &str) -> Option<usize> {
    static NAMES: LazyLock<HashMap<String, usize>> = LazyLock::new(|| {
        let mut m: HashMap<String, usize> = BOOK_NAMES.iter().enumerate().map(|(i, n)| (squash(n), i)).collect();
        for (alias, i) in [
            ("psalm", 18), ("songofsongs", 21), ("canticles", 21), ("canticleofcanticles", 21), ("revelations", 65), ("revelationofjohn", 65),
            ("apocalypse", 65), ("qoheleth", 20), ("acts", 43), ("actsoftheapostles", 43), ("jas", 58), ("phil", 49), ("philem", 56),
        ] {
            m.insert(alias.to_string(), i);
        }
        m
    });
    NAMES.get(&squash(name)).copied()
}

/// lowercase letters and digits only, with "first/second/third" and Roman numerals in front of a book as digits
fn squash(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    let mut w = lower.split_whitespace();
    let first = w.next().unwrap_or("");
    let rest: Vec<&str> = w.collect();
    let n = match first {
        "first" | "i" => Some("1"),
        "second" | "ii" => Some("2"),
        "third" | "iii" => Some("3"),
        _ => None,
    };
    let joined = match n {
        Some(d) if !rest.is_empty() => format!("{d}{}", rest.join("")),
        _ => lower,
    };
    joined.chars().filter(char::is_ascii_alphanumeric).collect()
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn entities(s: &str) -> String {
    static E: LazyLock<Regex> = LazyLock::new(|| re(r"&(#x[0-9a-fA-F]+|#\d+|amp|lt|gt|quot|apos);"));
    E.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        match e {
            "amp" => "&".to_string(),
            "lt" => "<".to_string(),
            "gt" => ">".to_string(),
            "quot" => "\"".to_string(),
            "apos" => "'".to_string(),
            _ => {
                let n = if let Some(h) = e.strip_prefix("#x") { u32::from_str_radix(h, 16).ok() } else { e[1..].parse().ok() };
                n.and_then(char::from_u32).map(String::from).unwrap_or_default()
            }
        }
    })
    .into_owned()
}

// ── XML, just enough of it ──────────────────────────────────────────────────

enum Tok<'a> {
    Open { name: String, attrs: &'a str, selfclose: bool },
    Close(String),
    Text(&'a str),
}

fn tokens(x: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < x.len() {
        let Some(lt) = x[i..].find('<').map(|p| p + i) else {
            out.push(Tok::Text(&x[i..]));
            break;
        };
        if lt > i {
            out.push(Tok::Text(&x[i..lt]));
        }
        let rest = &x[lt..];
        let end = if rest.starts_with("<!--") {
            rest.find("-->").map(|p| p + 3)
        } else if rest.starts_with("<![CDATA[") {
            if let Some(p) = rest.find("]]>") {
                out.push(Tok::Text(&rest[9..p]));
                i = lt + p + 3;
                continue;
            }
            None
        } else {
            rest.find('>').map(|p| p + 1)
        };
        let Some(end) = end else { break };
        let tag = &rest[..end];
        i = lt + end;
        if tag.starts_with("<?") || tag.starts_with("<!") {
            continue;
        }
        let inner = tag[1..tag.len() - 1].trim();
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Tok::Close(name.trim().to_ascii_lowercase()));
        } else {
            let selfclose = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let (name, attrs) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
            out.push(Tok::Open { name: name.to_ascii_lowercase(), attrs, selfclose });
        }
    }
    out
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut from = 0;
    while let Some(p) = attrs[from..].find(name).map(|p| p + from) {
        from = p + name.len();
        let before_ok = p == 0 || bytes[p - 1].is_ascii_whitespace();
        let rest = attrs[from..].trim_start();
        if before_ok && rest.starts_with('=') {
            let v = rest[1..].trim_start();
            let q = v.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let body = &v[1..];
            return body.find(q).map(|e| entities(&body[..e]));
        }
    }
    None
}

fn leading_number(s: &str) -> Option<u16> {
    let d: String = s.trim().chars().take_while(char::is_ascii_digit).collect();
    d.parse().ok()
}

/// Collects verse text between a verse's start and end.
#[derive(Default)]
struct Verses {
    rows: Vec<Row>,
    cur: Option<(usize, u16, u16)>,
    buf: String,
}

impl Verses {
    fn start(&mut self, book: usize, chapter: u16, verse: u16) {
        self.end();
        self.cur = Some((book, chapter, verse));
    }
    fn end(&mut self) {
        if let Some((b, c, v)) = self.cur.take() {
            let t = tidy(&entities(&self.buf));
            if !t.is_empty() {
                self.rows.push((b, c, v, t));
            }
        }
        self.buf.clear();
    }
    fn text(&mut self, t: &str) {
        if self.cur.is_some() {
            self.buf.push_str(t);
            self.buf.push(' ');
        }
    }
}

/// Tags whose text is not verse text.
fn skipped(name: &str) -> bool {
    matches!(name, "note" | "title" | "rdg" | "figure" | "index")
}

fn osis(x: &str) -> Parsed {
    let mut v = Verses::default();
    let mut name = None;
    let mut skip: Vec<String> = Vec::new();
    let mut header_title = String::new();
    for t in tokens(x) {
        match t {
            Tok::Open { name: n, attrs, selfclose } => {
                if n == "verse" {
                    if attr(attrs, "eID").is_some() {
                        v.end();
                    } else if let Some(id) = attr(attrs, "osisID") {
                        let first = id.split_whitespace().next().unwrap_or("");
                        let mut p = first.split('.');
                        let (b, c, ve) = (p.next().and_then(book_from_code), p.next().and_then(leading_number), p.next().and_then(leading_number));
                        if let (Some(b), Some(c), Some(ve)) = (b, c, ve) {
                            v.start(b, c, ve);
                        } else {
                            v.end();
                        }
                    }
                } else if n == "chapter" || n == "div" {
                    if !selfclose || attr(attrs, "eID").is_some() || n == "chapter" {
                        v.end();
                    }
                } else if skipped(&n) && !selfclose {
                    skip.push(n);
                }
            }
            Tok::Close(n) => {
                if n == "verse" {
                    v.end();
                }
                if skip.last() == Some(&n) {
                    skip.pop();
                    if n == "title" && name.is_none() && v.rows.is_empty() && !header_title.trim().is_empty() {
                        name = Some(tidy(&entities(&header_title)));
                    }
                    header_title.clear();
                }
            }
            Tok::Text(s) => {
                if skip.last().is_some_and(|k| k == "title") && v.rows.is_empty() && v.cur.is_none() {
                    header_title.push_str(s);
                } else if skip.is_empty() {
                    v.text(s);
                }
            }
        }
    }
    v.end();
    Parsed { name, rows: v.rows }
}

fn zefania(x: &str) -> Parsed {
    let mut v = Verses::default();
    let mut name = None;
    let (mut book, mut chapter) = (None, 0u16);
    let mut skip = 0usize;
    for t in tokens(x) {
        match t {
            Tok::Open { name: n, attrs, selfclose } => match n.as_str() {
                "xmlbible" => name = attr(attrs, "biblename").map(|s| tidy(&s)).filter(|s| !s.is_empty()),
                "biblebook" => {
                    v.end();
                    book = attr(attrs, "bnumber").and_then(|b| leading_number(&b)).and_then(|b| (1..=66).contains(&b).then(|| b as usize - 1))
                        .or_else(|| attr(attrs, "bname").and_then(|b| book_from_code(&b)));
                }
                "chapter" => {
                    v.end();
                    chapter = attr(attrs, "cnumber").and_then(|c| leading_number(&c)).unwrap_or(0);
                }
                "vers" => {
                    let verse = attr(attrs, "vnumber").and_then(|n| leading_number(&n));
                    match (book, verse) {
                        (Some(b), Some(ve)) if chapter > 0 => v.start(b, chapter, ve),
                        _ => v.end(),
                    }
                    if selfclose {
                        v.end();
                    }
                }
                "note" if !selfclose => skip += 1,
                _ => {}
            },
            Tok::Close(n) => match n.as_str() {
                "vers" => v.end(),
                "note" => skip = skip.saturating_sub(1),
                _ => {}
            },
            Tok::Text(s) => {
                if skip == 0 {
                    v.text(s);
                }
            }
        }
    }
    v.end();
    Parsed { name, rows: v.rows }
}

/// XML with its own tag names: whatever elements stand for a book, a chapter and a verse, numbered by
/// a "number", "n", "id" or similar attribute, or named by "name" / "code" for a book.
fn loose_xml(x: &str) -> Parsed {
    const BOOK: [&str; 4] = ["book", "b", "biblebook", "bk"];
    const CHAPTER: [&str; 4] = ["chapter", "c", "ch", "chap"];
    const VERSE: [&str; 5] = ["verse", "v", "vers", "vs", "ver"];
    const KEYS: [&str; 8] = ["number", "n", "num", "id", "no", "bnumber", "cnumber", "vnumber"];
    let num = |attrs: &str| KEYS.iter().find_map(|k| attr(attrs, k)).and_then(|v| leading_number(&v));
    let mut v = Verses::default();
    let mut name = None;
    let (mut book, mut chapter) = (None, 0u16);
    for t in tokens(x) {
        match t {
            Tok::Open { name: n, attrs, selfclose } => {
                let n = n.as_str();
                if BOOK.contains(&n) {
                    v.end();
                    book = attr(attrs, "name").or(attr(attrs, "code")).or(attr(attrs, "osisid")).and_then(|b| book_from_code(&b))
                        .or_else(|| num(attrs).and_then(|b| (1..=66).contains(&b).then(|| b as usize - 1)));
                    chapter = 0;
                } else if CHAPTER.contains(&n) {
                    v.end();
                    chapter = num(attrs).unwrap_or(0);
                } else if VERSE.contains(&n) {
                    match (book, num(attrs)) {
                        (Some(b), Some(ve)) if chapter > 0 => v.start(b, chapter, ve),
                        _ => v.end(),
                    }
                    if selfclose {
                        v.end();
                    }
                } else if name.is_none() && v.rows.is_empty() {
                    name = attr(attrs, "translation").or(attr(attrs, "biblename")).map(|s| tidy(&s)).filter(|s| !s.is_empty());
                }
            }
            Tok::Close(n) => {
                if VERSE.contains(&n.as_str()) {
                    v.end();
                }
            }
            Tok::Text(s) => v.text(s),
        }
    }
    v.end();
    Parsed { name, rows: v.rows }
}

/// `<bible translation="…"><testament><book number="1"><chapter number="1"><verse number="1">text</verse>`:
/// the plain layout many free Bible downloads use (the book by its number, 1 to 66, or its name).
fn plain_xml(x: &str) -> Parsed {
    let mut v = Verses::default();
    let mut name = None;
    let (mut book, mut chapter) = (None, 0u16);
    let mut skip = 0usize;
    for t in tokens(x) {
        match t {
            Tok::Open { name: n, attrs, selfclose } => match n.as_str() {
                "bible" => name = attr(attrs, "translation").or(attr(attrs, "name")).map(|s| tidy(&s)).filter(|s| !s.is_empty()),
                "book" => {
                    v.end();
                    book = attr(attrs, "number").and_then(|b| leading_number(&b)).and_then(|b| (1..=66).contains(&b).then(|| b as usize - 1))
                        .or_else(|| attr(attrs, "name").or(attr(attrs, "code")).or(attr(attrs, "id")).and_then(|b| book_from_code(&b)));
                    chapter = 0;
                }
                "chapter" => {
                    v.end();
                    chapter = attr(attrs, "number").and_then(|c| leading_number(&c)).unwrap_or(0);
                }
                "verse" => {
                    match (book, attr(attrs, "number").and_then(|n| leading_number(&n))) {
                        (Some(b), Some(ve)) if chapter > 0 => v.start(b, chapter, ve),
                        _ => v.end(),
                    }
                    if selfclose {
                        v.end();
                    }
                }
                "note" if !selfclose => skip += 1,
                _ => {}
            },
            Tok::Close(n) => match n.as_str() {
                "verse" => v.end(),
                "note" => skip = skip.saturating_sub(1),
                _ => {}
            },
            Tok::Text(s) => {
                if skip == 0 {
                    v.text(s);
                }
            }
        }
    }
    v.end();
    Parsed { name, rows: v.rows }
}

fn usx(x: &str) -> Parsed {
    let mut v = Verses::default();
    let (mut book, mut chapter) = (None, 0u16);
    let mut skip: Vec<String> = Vec::new();
    let heading = |style: &str| {
        let s = style.trim_end_matches(|c: char| c.is_ascii_digit());
        matches!(s, "s" | "r" | "mt" | "mte" | "ms" | "mr" | "h" | "toc" | "sr" | "d" | "sp" | "cl" | "cp" | "cd" | "rem" | "ide" | "b" | "periph" | "imt" | "is" | "ip")
    };
    for t in tokens(x) {
        match t {
            Tok::Open { name: n, attrs, selfclose } => match n.as_str() {
                "book" => {
                    v.end();
                    book = attr(attrs, "code").and_then(|c| book_from_code(&c));
                    chapter = 0;
                    if !selfclose {
                        skip.push("book".into());
                    }
                }
                "chapter" => {
                    v.end();
                    if attr(attrs, "eid").is_none() {
                        chapter = attr(attrs, "number").and_then(|c| leading_number(&c)).unwrap_or(0);
                    }
                }
                "verse" => {
                    if attr(attrs, "eid").is_some() {
                        v.end();
                    } else if let (Some(b), Some(ve)) = (book, attr(attrs, "number").and_then(|n| leading_number(&n))) {
                        if chapter > 0 {
                            v.start(b, chapter, ve);
                        }
                    }
                }
                "para" if !selfclose => {
                    if attr(attrs, "style").is_some_and(|s| heading(&s)) {
                        skip.push("para".into());
                    }
                }
                "note" if !selfclose => skip.push("note".into()),
                _ => {}
            },
            Tok::Close(n) => {
                if matches!(n.as_str(), "note" | "book") && skip.last() == Some(&n) {
                    skip.pop();
                } else if n == "para" && skip.last().is_some_and(|k| k == "para") {
                    skip.pop();
                }
            }
            Tok::Text(s) => {
                if skip.is_empty() {
                    v.text(s);
                }
            }
        }
    }
    v.end();
    Parsed { name: None, rows: v.rows }
}

// ── USFM ────────────────────────────────────────────────────────────────────

fn usfm(text: &str) -> Parsed {
    static NOTES: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)\\(f|fe|ef|x|ex|va|vp|fig)\s.*?\\(?:f|fe|ef|x|ex|va|vp|fig)\*"));
    static WORD: LazyLock<Regex> = LazyLock::new(|| re(r"\\\+?(?:w|wg|wh|wa)\s+([^|\\]*)(?:\|[^\\]*)?\\\+?(?:w|wg|wh|wa)\*"));
    static HEADING: LazyLock<Regex> = LazyLock::new(|| {
        re(r"^\\(?:s\d?|r|mt\d?|mte\d?|ms\d?|mr|h|toc\d|sr|sp|d|cl|cp|cd|rem|ide|imt\d?|is\d?|ip|ili\d?|iot|io\d?|ie|b|periph|lit|sts|usfm|cat)(?:\s|$)")
    });
    static VERSE: LazyLock<Regex> = LazyLock::new(|| re(r"\\v\s+(\d+)[a-z]?(?:\s*[-\u{2013},]\s*\d+[a-z]?)*\s*"));
    static MARKS: LazyLock<Regex> = LazyLock::new(|| re(r"\\\*|\\\+?[A-Za-z]+\d*\*?"));
    let text = NOTES.replace_all(text, "");
    let text = WORD.replace_all(&text, "$1");
    let mut v = Verses::default();
    let (mut book, mut chapter) = (None, 0u16);
    let mut name = None;
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(r) = line.strip_prefix("\\id ").or_else(|| line.strip_prefix("\\id\t")) {
            v.end();
            book = r.split_whitespace().next().and_then(book_from_code);
            chapter = 0;
            continue;
        }
        if let Some(r) = line.strip_prefix("\\c ") {
            v.end();
            chapter = leading_number(r).unwrap_or(0);
            continue;
        }
        if name.is_none() && (line.starts_with("\\h ") || line.starts_with("\\toc3 ")) && book.is_none() {
            name = Some(tidy(&line[line.find(' ').unwrap_or(0)..])).filter(|s| !s.is_empty());
        }
        if HEADING.is_match(line) || line.starts_with("\\c") {
            continue;
        }
        let mut last = 0;
        let mut segs: Vec<(Option<u16>, &str)> = Vec::new();
        let mut open: Option<u16> = None;
        for m in VERSE.captures_iter(line) {
            let whole = m.get(0).unwrap();
            segs.push((open, &line[last..whole.start()]));
            open = m[1].parse().ok();
            last = whole.end();
        }
        segs.push((open, &line[last..]));
        for (verse, seg) in segs {
            if let Some(ve) = verse {
                match (book, chapter) {
                    (Some(b), c) if c > 0 => v.start(b, c, ve),
                    _ => v.end(),
                }
            }
            v.text(&MARKS.replace_all(seg, " "));
        }
    }
    v.end();
    Parsed { name, rows: v.rows }
}

// ── JSON ────────────────────────────────────────────────────────────────────

fn json(text: &str) -> Result<Parsed, String> {
    let raw: Value = serde_json::from_str(text).map_err(|_| "That JSON file couldn't be read.".to_string())?;
    let mut rows = Vec::new();
    if let Some(list) = raw["verses"].as_array() {
        for v in list {
            let (Some(b), Some(c), Some(ve), Some(t)) = (
                v["book"].as_str().and_then(book_from_code),
                v["chapter"].as_u64().and_then(|c| u16::try_from(c).ok()),
                v["verse"].as_u64().and_then(|c| u16::try_from(c).ok()),
                v["text"].as_str(),
            ) else { continue };
            rows.push((b, c, ve, t.to_string()));
        }
        let name = raw["translation"].as_str().or(raw["name"].as_str()).map(tidy).filter(|s| !s.is_empty());
        return Ok(Parsed { name, rows });
    }
    // [{ "chapters": [[verse, ...], ...] }, ...] in canonical order, like kjv.json
    if let Some(books) = raw.as_array().filter(|b| b.len() == Book::COUNT && b.iter().all(|x| x["chapters"].is_array())) {
        for (b, book) in books.iter().enumerate() {
            for (c, ch) in book["chapters"].as_array().into_iter().flatten().enumerate() {
                for (ve, t) in ch.as_array().into_iter().flatten().enumerate() {
                    if let Some(t) = t.as_str() {
                        rows.push((b, c as u16 + 1, ve as u16 + 1, t.to_string()));
                    }
                }
            }
        }
        return Ok(Parsed { name: None, rows });
    }
    Err("That JSON file isn't laid out like a Kairo Bible (a \"verses\" list with book, chapter, verse and text).".into())
}

// ── Any file ────────────────────────────────────────────────────────────────

fn decode(bytes: &[u8]) -> String {
    if let Some(b) = bytes.strip_prefix(&[0xFF, 0xFE]).or_else(|| bytes.strip_prefix(&[0xFE, 0xFF])) {
        let le = bytes.starts_with(&[0xFF, 0xFE]);
        let units: Vec<u16> = b.chunks_exact(2).map(|p| if le { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) }).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).into_owned()
}

/// The readers, each with a cheap look at the text to say whether it could be that format.
const READERS: [(&str, fn(&str) -> bool, fn(&str) -> Parsed); 6] = [
    ("USFM", |t| t.contains("\\id ") || t.contains("\\c "), usfm),
    ("OSIS", |t| t.contains('<') && t.to_ascii_lowercase().contains("osisid"), osis),
    ("Zefania", |t| t.contains('<') && t.to_ascii_lowercase().contains("<vers"), zefania),
    ("USX", |t| t.contains('<') && t.to_ascii_lowercase().contains("<usx"), usx),
    ("XML", |t| t.contains('<') && t.to_ascii_lowercase().contains("<verse"), plain_xml),
    ("XML", |t| t.contains('<'), loose_xml),
];

/// Tries every reader that could apply and keeps the one that found the most verses, so a file doesn't
/// have to announce what it is (or announce it truthfully).
fn parse_text(text: &str) -> Result<Parsed, String> {
    let t = text.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        return json(t);
    }
    let best = READERS.iter().filter(|(_, could, _)| could(text)).map(|(_, _, read)| read(text)).rev().max_by_key(|p| p.rows.len()); // a tie goes to the more specific reader
    match best {
        Some(p) if !p.rows.is_empty() => Ok(p),
        _ => Err("Kairo couldn't find Bible verses in this file. It reads USFM, USX, OSIS, Zefania XML and other XML that numbers its books, chapters and verses, or a zip of them.".into()),
    }
}

/// Why a file is not accepted, if it is a database or a program (by its name or by what it starts with).
fn refuse(lower_name: &str, bytes: &[u8]) -> Option<String> {
    const DATABASE: [&str; 5] = [".sql", ".sqlite", ".sqlite3", ".db", ".mdb"];
    const PROGRAM: [&str; 9] = [".exe", ".dll", ".app", ".dmg", ".pkg", ".msi", ".sh", ".bat", ".command"];
    if DATABASE.iter().any(|e| lower_name.ends_with(e)) || bytes.starts_with(b"SQLite format 3") {
        return Some("Database files aren't accepted. Export the Bible as USFM, OSIS, Zefania XML or USX instead.".into());
    }
    if PROGRAM.iter().any(|e| lower_name.ends_with(e)) || bytes.starts_with(b"MZ") || bytes.starts_with(&[0x7F, b'E', b'L', b'F']) || bytes.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) {
        return Some("Programs aren't accepted.".into());
    }
    None
}

pub fn parse(filename: &str, bytes: &[u8]) -> Result<Parsed, String> {
    let lower = filename.to_ascii_lowercase();
    if let Some(why) = refuse(&lower, bytes) {
        return Err(why);
    }
    if bytes.starts_with(b"PK") {
        let members = crate::import::unzip(bytes, |n| !n.ends_with('/')).map_err(|e| format!("The zip couldn't be read: {}.", e.0))?;
        // Anything in the zip that isn't Bible text refuses the whole zip, so nothing unexpected is carried in with it.
        for (name, data) in &members {
            let l = name.to_ascii_lowercase();
            let base = l.rsplit('/').next().unwrap_or("");
            if l.contains("__macosx") || base.starts_with("._") || base == ".ds_store" {
                continue;
            }
            if let Some(why) = refuse(&l, data) {
                return Err(format!("The zip holds {name}: {why}"));
            }
        }
        let files: Vec<_> = members
            .into_iter()
            .filter(|(n, _)| {
                let l = n.to_ascii_lowercase();
                !l.contains("__macosx") && !l.rsplit('/').next().unwrap_or("").starts_with("._")
                    && [".usfm", ".sfm", ".usx", ".xml", ".osis", ".json", ".txt"].iter().any(|e| l.ends_with(e))
            })
            .collect();
        if files.is_empty() {
            return Err("The zip holds none of the formats Kairo reads.".into());
        }
        let mut all = Parsed::default();
        for (_, data) in files {
            // a licence or metadata file in the zip is not a Bible; skip what doesn't read as one
            if let Ok(p) = parse_text(&decode(&data)) {
                all.name = all.name.or(p.name);
                all.rows.extend(p.rows);
            }
        }
        if all.rows.is_empty() {
            return Err("Nothing in the zip read as Bible text.".into());
        }
        return Ok(all);
    }
    parse_text(&decode(bytes))
}

// ── Checking and saving ─────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Report {
    pub name: String,
    pub verses: usize,
    pub books: usize,
    pub off_layout: usize,
}

/// The saved file, in the flat shape the translation loader reads, once the Bible has been checked
/// against the KJV's layout. Every verse is kept (a Psalm numbered the Hebrew way needs its title
/// verses for the loader to see that); the check counts the ones that line up.
pub fn build(parsed: Parsed, fallback_name: &str, corpus: &Corpus) -> Result<(Value, Report), String> {
    let mut by_ref: HashMap<(usize, u16, u16), String> = HashMap::new();
    for (b, c, v, t) in parsed.rows {
        let t = kairo_bible::text::clean(&t);
        if !t.is_empty() {
            by_ref.insert((b, c, v), t);
        }
    }
    if by_ref.is_empty() {
        return Err("No verses could be read from this file.".into());
    }
    let mut keys: Vec<_> = by_ref.keys().copied().collect();
    keys.sort_unstable();
    let fits = |&(b, c, v): &(usize, u16, u16)| Book::from_index(b).is_some_and(|bk| corpus.exists(VerseRef::new(bk, c, v)));
    let matched = keys.iter().filter(|k| fits(k)).count();
    let mut books: Vec<usize> = keys.iter().map(|k| k.0).collect();
    books.dedup();
    if matched < 1000 {
        return Err(format!("Only {matched} verses lined up with the Bible's chapters and verses. Kairo needs at least 1,000 (a New Testament has about 8,000)."));
    }
    if matched * 100 < keys.len() * 85 {
        return Err(format!("This Bible doesn't follow the usual chapter and verse layout ({} of {} verses fit). Nothing was added.", matched, keys.len()));
    }
    let name = parsed.name.map(|n| tidy(&n)).filter(|n| !n.is_empty()).unwrap_or_else(|| fallback_name.to_string());
    let verses: Vec<Value> = keys
        .iter()
        .map(|k| {
            json!({
                "book": BOOK_NAMES[k.0], "chapter": k.1, "verse": k.2,
                "reference": format!("{} {}:{}", BOOK_NAMES[k.0], k.1, k.2),
                "text": by_ref[k],
            })
        })
        .collect();
    let report = Report { name: name.clone(), verses: matched, books: books.len(), off_layout: keys.len() - matched };
    Ok((json!({ "translation": name, "license": "Added by the church", "imported": true, "verseCount": verses.len(), "verses": verses }), report))
}

/// A code that is safe to use as a file name and doesn't take a shipped Bible's place.
pub fn clean_code(raw: &str) -> Option<String> {
    let c: String = raw.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    (c.len() >= 2 && c.len() <= 8 && !RESERVED.contains(&c.as_str())).then_some(c)
}

/// A short name made from a Bible's own name, not taken by a shipped Bible or one already added:
/// "English NIV" → NIV2 (NIV ships), "World English Bible" → WEB, "New Living Translation" → NLT.
pub fn unique_code(app_data: &std::path::Path, name: &str) -> String {
    let name = name.trim();
    let name = name.strip_prefix("English:").or_else(|| name.strip_prefix("English ")).unwrap_or(name);
    let words: Vec<&str> = name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let base = words.iter().find(|w| (2..=8).contains(&w.len()) && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_uppercase()))
        .map(|w| w.to_string())
        .unwrap_or_else(|| words.iter().filter(|w| !["the", "of", "and", "a", "in"].contains(&w.to_ascii_lowercase().as_str())).filter_map(|w| w.chars().next()).collect::<String>().to_ascii_uppercase());
    let base: String = base.chars().filter(char::is_ascii_alphanumeric).take(7).collect::<String>().to_ascii_uppercase();
    // One significant word ("The Message"): the word itself, not a lone initial.
    let base = if base.len() >= 2 {
        base
    } else {
        words.iter().find(|w| !["the", "of", "and", "a", "in"].contains(&w.to_ascii_lowercase().as_str())).map(|w| w.chars().take(7).collect::<String>().to_ascii_uppercase()).filter(|w| w.len() >= 2).unwrap_or_else(|| "BIBLE".into())
    };
    let taken = |c: &str| RESERVED.contains(&c) || app_data.join("translations").join(format!("{c}.json")).exists() || app_data.join("api-bibles").join(format!("{c}.json")).exists();
    if !taken(&base) {
        return base;
    }
    (2..100).map(|n| format!("{}{n}", &base[..base.len().min(8 - n.to_string().len())])).find(|c| !taken(c)).unwrap_or(base)
}

/// What is wrong with a short name, in words for the person typing it.
pub fn code_error(raw: &str) -> String {
    let c: String = raw.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    if RESERVED.contains(&c.as_str()) {
        format!("{c} is already a Bible that comes with Kairo. Choose another short name, such as {c}2.")
    } else {
        "Use 2 to 8 letters or numbers for the short name.".into()
    }
}

fn fail(msg: impl Into<String>) -> Json<Value> {
    Json(json!({ "ok": false, "error": msg.into() }))
}

/// POST /api/bibles/import?code=XYZ&filename=name.usfm  (the file as the body)
pub async fn import(State(app): State<Shared>, Query(q): Query<HashMap<String, String>>, body: Bytes) -> Json<Value> {
    let Some(bible) = app.bible.get().cloned() else { return fail("Kairo is still starting. Try again in a moment.") };
    let filename = q.get("filename").cloned().unwrap_or_default();
    let stem = filename.rsplit(['/', '\\']).next().unwrap_or("").split('.').next().unwrap_or("").to_string();
    // A short name asked for is checked now; without one, it is made from the Bible's own name once read.
    let asked = q.get("code").filter(|c| !c.trim().is_empty()).cloned();
    if let Some(c) = &asked {
        if clean_code(c).is_none() {
            return fail(code_error(c));
        }
    }
    if body.is_empty() {
        return fail("The file is empty.");
    }
    let app_data = app.cfg.app_data_dir.clone();
    let built = tokio::task::spawn_blocking(move || {
        let parsed = parse(&filename, &body)?;
        let fallback = parsed.name.clone().unwrap_or_else(|| stem.clone());
        let code = asked.as_deref().and_then(clean_code).unwrap_or_else(|| unique_code(&app_data, &fallback));
        let (file, report) = build(parsed, &code, &bible.k.corpus)?;
        let dir = bible.user_translations_dir().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("{code}.json.part"));
        std::fs::write(&tmp, serde_json::to_vec(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join(format!("{code}.json"))).map_err(|e| e.to_string())?;
        bible.forget_translation(&code);
        Ok::<_, String>((code, report))
    })
    .await;
    match built {
        Ok(Ok((code, r))) => Json(json!({ "ok": true, "code": code, "name": r.name, "verses": r.verses, "books": r.books, "skipped": r.off_layout })),
        Ok(Err(e)) => fail(e),
        Err(_) => fail("Kairo couldn't read this file."),
    }
}

/// GET /api/bibles/mine
pub async fn mine(State(app): State<Shared>) -> Json<Value> {
    let Some(bible) = app.bible.get() else { return Json(json!({ "bibles": [] })) };
    let mut list = Vec::new();
    if let Ok(dir) = std::fs::read_dir(bible.user_translations_dir()) {
        for e in dir.flatten() {
            let p = e.path();
            let (Some(code), true) = (p.file_stem().and_then(|s| s.to_str()), p.extension().is_some_and(|x| x == "json")) else { continue };
            let head = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            list.push(json!({
                "code": code,
                "name": head.as_ref().and_then(|h| h["translation"].as_str()).unwrap_or(code),
                "verses": head.as_ref().and_then(|h| h["verseCount"].as_u64()).unwrap_or(0),
            }));
        }
    }
    list.sort_by(|a, b| a["code"].as_str().cmp(&b["code"].as_str()));
    Json(json!({ "bibles": list }))
}

/// POST /api/bibles/remove {code}
pub async fn remove(State(app): State<Shared>, b: Option<Json<Value>>) -> Json<Value> {
    let b = b.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    let Some(code) = b["code"].as_str().and_then(clean_code) else { return Json(json!({ "ok": false })) };
    let Some(bible) = app.bible.get() else { return Json(json!({ "ok": false })) };
    let path = bible.user_translations_dir().join(format!("{code}.json"));
    let ok = path.is_file() && std::fs::remove_file(&path).is_ok();
    if ok {
        bible.forget_translation(&code);
    }
    Json(json!({ "ok": ok }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(p: &Parsed) -> Vec<(String, String)> {
        p.rows.iter().map(|(b, c, v, t)| (format!("{} {c}:{v}", BOOK_NAMES[*b]), t.clone())).collect()
    }

    #[test]
    fn usfm_reads_verses_footnotes_and_poetry() {
        let p = parse("x.usfm", b"\\id JHN\n\\h John\n\\c 3\n\\s1 Heading\n\\p\n\\v 16 For God so loved\\f + \\ft a note\\f* the \\w world|strong=\"G2889\"\\w*,\n\\q1 that he gave\n\\v 17 Not to condemn.\n\\v 18-19 Bridged.\n").unwrap();
        assert_eq!(
            rows(&p),
            vec![
                ("John 3:16".into(), "For God so loved the world, that he gave".into()),
                ("John 3:17".into(), "Not to condemn.".into()),
                ("John 3:18".into(), "Bridged.".into()),
            ]
        );
    }

    #[test]
    fn osis_reads_milestone_and_container_verses() {
        let x = r#"<osis><osisText><header><work><title>My Bible</title></work></header>
            <div type="book" osisID="Gen"><chapter osisID="Gen.1">
            <verse osisID="Gen.1.1">In the <w>beginning</w><note>skip me</note> God.</verse>
            <verse sID="Gen.1.2" osisID="Gen.1.2"/>And the earth.<verse eID="Gen.1.2"/>
            </chapter></div></osisText></osis>"#;
        let p = parse("x.xml", x.as_bytes()).unwrap();
        assert_eq!(p.name.as_deref(), Some("My Bible"));
        assert_eq!(rows(&p), vec![("Genesis 1:1".into(), "In the beginning God.".into()), ("Genesis 1:2".into(), "And the earth.".into())]);
    }

    #[test]
    fn zefania_and_usx_read() {
        let z = r#"<XMLBIBLE biblename="Zef"><BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">For God <NOTE>n</NOTE>so loved.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#;
        let p = parse("z.xml", z.as_bytes()).unwrap();
        assert_eq!(p.name.as_deref(), Some("Zef"));
        assert_eq!(rows(&p), vec![("John 3:16".into(), "For God so loved.".into())]);
        let u = r#"<usx version="3.0"><book code="JHN" style="id">John</book><chapter number="3" style="c" sid="JHN 3"/>
            <para style="s1">Heading</para><para style="p"><verse number="16" style="v" sid="JHN 3:16"/>For God <char style="nd">so</char> loved.<verse eid="JHN 3:16"/></para></usx>"#;
        assert_eq!(rows(&parse("u.usx", u.as_bytes()).unwrap()), vec![("John 3:16".into(), "For God so loved.".into())]);
    }

    #[test]
    fn plain_xml_reads_numbered_books() {
        let x = r#"<?xml version="1.0"?><bible translation="English NIV"><testament name="Old"><book number="1"><chapter number="1">
            <verse number="1">In the beginning God created.</verse><verse number="2">Now the earth.</verse></chapter></book></testament></bible>"#;
        let p = parse("b.xml", x.as_bytes()).unwrap();
        assert_eq!(p.name.as_deref(), Some("English NIV"));
        assert_eq!(rows(&p), vec![("Genesis 1:1".into(), "In the beginning God created.".into()), ("Genesis 1:2".into(), "Now the earth.".into())]);
    }

    /// `KAIRO_TRY_BIBLE=/path/to/file cargo test -p kairo-server try_a_file -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn try_a_file() {
        let Ok(path) = std::env::var("KAIRO_TRY_BIBLE") else { return };
        let bytes = std::fs::read(&path).unwrap();
        match import(&path, &bytes) {
            Ok((_, r)) => println!("ACCEPTED: {:?}", r),
            Err(e) => println!("REFUSED: {e}"),
        }
    }

    #[test]
    fn json_and_refusals() {
        let j = br#"{"translation":"T","verses":[{"book":"John","chapter":3,"verse":16,"text":"x"}]}"#;
        assert_eq!(rows(&parse("a.json", j).unwrap()), vec![("John 3:16".into(), "x".into())]);
        assert!(parse("a.sql", b"INSERT INTO verses").unwrap_err().contains("Database"));
        assert!(parse("a.txt", b"hello there").unwrap_err().contains("find Bible verses"));
    }

    #[test]
    fn codes_are_file_names_and_never_a_shipped_bible() {
        assert_eq!(clean_code("my-bible"), Some("MYBIBLE".into()));
        assert_eq!(clean_code("../x"), None);
        assert_eq!(clean_code("niv"), None);
        assert_eq!(clean_code("msg"), Some("MSG".into()));
        let d = std::env::temp_dir().join("kairo_code_test");
        assert_eq!(unique_code(&d, "English NIV"), "NIV2");
        assert_eq!(unique_code(&d, "World English Bible"), "WEB");
        assert_eq!(unique_code(&d, "New Living Translation"), "NLT2");
        assert_eq!(unique_code(&d, "The Message"), "MESSAGE");
        assert_eq!(unique_code(&d, "MSG"), "MSG");
        assert!(code_error("niv").contains("already a Bible that comes with Kairo"));
        assert!(code_error("a").contains("2 to 8"));
        assert_eq!(clean_code("a"), None);
    }

    // ── Acceptance: a file is only accepted when its shape fits the Bible's ─────────────────────

    static CORPUS: LazyLock<Corpus> =
        LazyLock::new(|| Corpus::load(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles")).expect("kjv.json"));

    /// The whole Bible as USFM, one book per string, from the books `keep` lets through. `shift` moves
    /// the verse number of every verse for which it returns Some.
    fn usfm_books(keep: impl Fn(usize) -> bool, shift: impl Fn(usize, u16, u16) -> Option<u16>) -> Vec<(String, String)> {
        let c = &*CORPUS;
        let mut out = Vec::new();
        for b in Book::all().filter(|b| keep(b.index())) {
            let mut f = format!("\\id {}\n", USFM_CODES[b.index()]);
            for ch in 1..=c.chapter_count(b) {
                f.push_str(&format!("\\c {ch}\n\\p\n"));
                for v in 1..=c.verse_count(b, ch).unwrap() {
                    let id = c.id(VerseRef::new(b, ch, v)).unwrap();
                    let n = shift(b.index(), ch, v).unwrap_or(v);
                    f.push_str(&format!("\\v {n} {}\n", c.kjv().text(id).unwrap()));
                }
            }
            out.push((format!("{}.usfm", USFM_CODES[b.index()]), f));
        }
        out
    }

    /// A zip that stores its files uncompressed.
    fn stored_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let (mut body, mut dir) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let off = body.len() as u32;
            let (n, len) = (name.as_bytes(), data.len() as u32);
            body.extend([0x50, 0x4b, 3, 4, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            body.extend(len.to_le_bytes());
            body.extend(len.to_le_bytes());
            body.extend((n.len() as u16).to_le_bytes());
            body.extend([0, 0]);
            body.extend(n);
            body.extend(data);
            dir.extend([0x50, 0x4b, 1, 2, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            dir.extend(len.to_le_bytes());
            dir.extend(len.to_le_bytes());
            dir.extend((n.len() as u16).to_le_bytes());
            dir.extend([0u8; 12]);
            dir.extend(off.to_le_bytes());
            dir.extend(n);
        }
        let (count, dir_at, dir_len) = (files.len() as u16, body.len() as u32, dir.len() as u32);
        body.extend(dir);
        body.extend([0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
        body.extend(count.to_le_bytes());
        body.extend(count.to_le_bytes());
        body.extend(dir_len.to_le_bytes());
        body.extend(dir_at.to_le_bytes());
        body.extend([0, 0]);
        body
    }

    fn import(filename: &str, bytes: &[u8]) -> Result<(Value, Report), String> {
        build(parse(filename, bytes)?, "TEST", &CORPUS)
    }

    fn one_file(books: Vec<(String, String)>) -> Vec<u8> {
        books.into_iter().map(|(_, f)| f).collect::<Vec<_>>().join("\n").into_bytes()
    }

    #[test]
    fn a_whole_bible_is_accepted_and_loads_back_complete() {
        let (file, r) = import("whole.usfm", &one_file(usfm_books(|_| true, |_, _, _| None))).unwrap();
        assert_eq!((r.books, r.verses, r.off_layout), (66, CORPUS.len(), 0));
        let path = std::env::temp_dir().join("kairo_accept_whole.json");
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        let t = CORPUS.load_flat("TEST", &path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(t.len(), CORPUS.len());
    }

    #[test]
    fn a_new_testament_alone_is_accepted() {
        let (_, r) = import("nt.usfm", &one_file(usfm_books(|b| b >= 39, |_, _, _| None))).unwrap();
        assert_eq!((r.books, r.off_layout), (27, 0));
        assert!(r.verses > 7000);
    }

    #[test]
    fn a_zip_of_books_is_accepted_and_a_zip_of_nothing_useful_is_not() {
        let files: Vec<_> = usfm_books(|_| true, |_, _, _| None).into_iter().map(|(n, f)| (n, f.into_bytes())).collect();
        let (_, r) = import("bible.zip", &stored_zip(&files)).unwrap();
        assert_eq!(r.books, 66);
        // one database inside an otherwise good zip refuses the whole zip, whatever it is called or hides as
        let mut with_sql = files.clone();
        with_sql.push(("verses.sql".into(), b"INSERT INTO verses VALUES (1)".to_vec()));
        let e = import("bible.zip", &stored_zip(&with_sql)).unwrap_err();
        assert!(e.contains("verses.sql") && e.contains("Database"), "{e}");
        let mut disguised = files.clone();
        disguised.push(("notes.txt".into(), b"SQLite format 3\0".to_vec()));
        assert!(import("bible.zip", &stored_zip(&disguised)).unwrap_err().contains("Database"));
        let mut program = files.clone();
        program.push(("setup.exe".into(), b"MZ".to_vec()));
        assert!(import("bible.zip", &stored_zip(&program)).unwrap_err().contains("Programs"));
        let junk = stored_zip(&[("LICENSE.txt".into(), b"All rights reserved".to_vec()), ("cover.png".into(), vec![1, 2, 3])]);
        assert!(import("bible.zip", &junk).is_err());
    }

    #[test]
    fn a_few_verses_is_not_a_bible() {
        let e = import("john.usfm", &one_file(usfm_books(|b| b == 42, |_, _, _| None))).unwrap_err();
        assert!(e.contains("Only"), "{e}");
    }

    #[test]
    fn verses_that_do_not_fit_the_chapters_are_refused() {
        // every verse number pushed past the end of its chapter
        let e = import("odd.usfm", &one_file(usfm_books(|_| true, |_, _, v| Some(v + 400)))).unwrap_err();
        assert!(e.contains("Only") || e.contains("layout"), "{e}");
        // a third of the verses off the page: enough lines up to look like a Bible, but not a Bible
        let e = import("third.usfm", &one_file(usfm_books(|_| true, |_, c, v| ((c + v) % 3 == 0).then_some(v + 400)))).unwrap_err();
        assert!(e.contains("layout"), "{e}");
    }

    #[test]
    fn things_that_are_not_bibles_are_refused_with_a_reason() {
        assert!(import("verses.sql", b"INSERT INTO verses VALUES (1)").unwrap_err().contains("Database"));
        assert!(import("a.db", b"SQLite format 3\0stuff").unwrap_err().contains("Database"));
        assert!(import("app.exe", b"MZ\x90\x00").unwrap_err().contains("Programs"));
        assert!(import("notes.txt", b"For God so loved the world").unwrap_err().contains("find Bible verses"));
        assert!(import("empty.usfm", b"\\id JHN\n\\c 3\n").unwrap_err().contains("find Bible verses"));
    }
}
