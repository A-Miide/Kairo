//! Importing slides (text, Word, PowerPoint, ProPresenter 6/7, bundles and
//! playlists) and ProPresenter themes — read without the presenting apps, from
//! the files' own structure: a zip reader, a protobuf walker and RTF text.

use crate::propresenter as pp;
use std::collections::HashMap;
use std::io::Read;
use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;
use serde_json::{json, Value};

const MAX_INFLATED_ENTRY_BYTES: u64 = 200 * 1024 * 1024;
/// All of a bundle's members together, unpacked: well above any real bundle
/// (uploads stop at ~375 MB compressed), well below a zip bomb's appetite.
const MAX_INFLATED_TOTAL_BYTES: u64 = 1536 * 1024 * 1024;
const MAX_WALK_DEPTH: usize = 200;
const ZIP64_SENTINEL: u32 = 0xFFFF_FFFF;

#[derive(Debug)]
pub struct ImportError(pub String, pub Option<&'static str>);

fn err(m: &str) -> ImportError {
    ImportError(m.to_string(), None)
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("valid pattern")
}

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn u64le(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

// ── Zip ─────────────────────────────────────────────────────────────────────

/// The members whose names match, in the archive's order.
pub fn unzip(buf: &[u8], want: impl Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>, ImportError> {
    let mut eocd = None;
    let lowest = buf.len().saturating_sub(66_000);
    let mut i = buf.len().saturating_sub(22);
    while i > lowest || i == 0 {
        if u32le(buf, i) == Some(0x0605_4b50) {
            eocd = Some(i);
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    let eocd = eocd.ok_or_else(|| err("not a zip file"))?;
    let count = u16le(buf, eocd + 10).unwrap_or(0);
    let mut p = u32le(buf, eocd + 16).unwrap_or(0) as usize;
    let mut out = Vec::new();
    let mut total: u64 = 0;
    for _ in 0..count {
        if p + 46 > buf.len() || u32le(buf, p) != Some(0x0201_4b50) {
            break;
        }
        let method = u16le(buf, p + 10).unwrap_or(0);
        let mut comp = u32le(buf, p + 20).unwrap_or(0) as u64;
        let name_len = u16le(buf, p + 28).unwrap_or(0) as usize;
        let extra_len = u16le(buf, p + 30).unwrap_or(0) as usize;
        let cmt_len = u16le(buf, p + 32).unwrap_or(0) as usize;
        let mut lho = u32le(buf, p + 42).unwrap_or(0) as u64;
        let name = String::from_utf8_lossy(buf.get(p + 46..p + 46 + name_len).unwrap_or(&[])).into_owned();
        if comp == ZIP64_SENTINEL as u64 || lho == ZIP64_SENTINEL as u64 {
            let extra = buf.get(p + 46 + name_len..p + 46 + name_len + extra_len).unwrap_or(&[]);
            let (need_comp, need_lho) = (comp == ZIP64_SENTINEL as u64, lho == ZIP64_SENTINEL as u64);
            let mut off = 0;
            while off + 4 <= extra.len() {
                let id = u16le(extra, off).unwrap_or(0);
                let len = u16le(extra, off + 2).unwrap_or(0) as usize;
                if id == 1 {
                    let (mut q, end) = (off + 4, off + 4 + len);
                    if need_comp && q + 8 <= end {
                        comp = u64le(extra, q).unwrap_or(comp);
                        q += 8;
                    }
                    if need_lho && q + 8 <= end {
                        lho = u64le(extra, q).unwrap_or(lho);
                    }
                    break;
                }
                off += 4 + len;
            }
        }
        p += 46 + name_len + extra_len + cmt_len;
        if !want(&name) {
            continue;
        }
        let lho = lho as usize;
        if u32le(buf, lho) != Some(0x0403_4b50) {
            continue;
        }
        let start = lho + 30 + u16le(buf, lho + 26).unwrap_or(0) as usize + u16le(buf, lho + 28).unwrap_or(0) as usize;
        let Some(raw) = buf.get(start..start.saturating_add(comp as usize).min(buf.len())) else { continue };
        let data = if method == 0 {
            Some(raw.to_vec())
        } else {
            let mut d = Vec::new();
            flate2::read::DeflateDecoder::new(raw).take(MAX_INFLATED_ENTRY_BYTES + 1).read_to_end(&mut d).ok().filter(|_| d.len() as u64 <= MAX_INFLATED_ENTRY_BYTES).map(|_| d)
        };
        if let Some(d) = data {
            total += d.len() as u64;
            if total > MAX_INFLATED_TOTAL_BYTES {
                return Err(err("this file is too large when unpacked"));
            }
            out.push((name, d));
        }
    }
    Ok(out)
}

// ── Protobuf ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Field<'a> {
    pub num: u64,
    pub wire: u8,
    pub value: u64,
    pub raw: &'a [u8],
}

fn varint(b: &[u8], mut p: usize) -> Option<(u64, usize)> {
    let (mut result, mut shift) = (0u64, 0u32);
    loop {
        let x = *b.get(p)?;
        p += 1;
        if shift < 64 {
            result |= ((x & 0x7f) as u64) << shift;
        }
        if x & 0x80 == 0 {
            return Some((result, p));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// The fields of a protobuf message, or None if it isn't one.
pub fn pb_fields(b: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let (tag, after) = varint(b, p)?;
        p = after;
        let (num, wire) = ((tag as u32 >> 3) as u64, (tag & 7) as u8);
        if num == 0 {
            return None;
        }
        match wire {
            0 => {
                let (v, a) = varint(b, p)?;
                out.push(Field { num, wire, value: v, raw: &[] });
                p = a;
            }
            1 => {
                out.push(Field { num, wire, value: 0, raw: b.get(p..p + 8)? });
                p += 8;
            }
            2 => {
                let (n, a) = varint(b, p)?;
                let n = usize::try_from(n).ok()?;
                out.push(Field { num, wire, value: 0, raw: b.get(a..a.checked_add(n)?)? });
                p = a + n;
            }
            5 => {
                out.push(Field { num, wire, value: 0, raw: b.get(p..p + 4)? });
                p += 4;
            }
            _ => return None,
        }
    }
    Some(out)
}

fn first<'a>(f: &[Field<'a>], n: u64) -> Option<Field<'a>> {
    f.iter().copied().find(|x| x.num == n)
}

fn bytes_field<'a>(f: &[Field<'a>], n: u64) -> Option<&'a [u8]> {
    first(f, n).filter(|x| x.wire == 2).map(|x| x.raw)
}

fn sub<'a>(f: Option<&[Field<'a>]>, n: u64) -> Option<Vec<Field<'a>>> {
    pb_fields(bytes_field(f?, n)?)
}

fn num(f: Option<&[Field<'_>]>, n: u64, fallback: f64) -> f64 {
    match f.and_then(|f| first(f, n)) {
        Some(x) if x.wire == 1 => f64::from_le_bytes(x.raw.try_into().unwrap_or([0; 8])),
        Some(x) if x.wire == 5 => f32::from_le_bytes(x.raw.try_into().unwrap_or([0; 4])) as f64,
        Some(x) if x.wire == 0 => x.value as f64,
        _ => fallback,
    }
}

fn text(f: Option<&[Field<'_>]>, n: u64) -> Option<String> {
    Some(String::from_utf8_lossy(bytes_field(f?, n)?).into_owned())
}

static UUID: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}$"));

fn uuid_string(raw: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes_field(&pb_fields(raw)?, 1)?).into_owned();
    UUID.is_match(&s).then_some(s)
}

// ── RTF ─────────────────────────────────────────────────────────────────────

const CP1252_HIGH: [(u8, char); 27] = [
    (0x80, '€'), (0x82, '‚'), (0x83, 'ƒ'), (0x84, '„'), (0x85, '…'), (0x86, '†'), (0x87, '‡'), (0x88, 'ˆ'), (0x89, '‰'), (0x8A, 'Š'),
    (0x8B, '‹'), (0x8C, 'Œ'), (0x8E, 'Ž'), (0x91, '‘'), (0x92, '’'), (0x93, '“'), (0x94, '”'), (0x95, '•'), (0x96, '–'), (0x97, '—'),
    (0x98, '˜'), (0x99, '™'), (0x9A, 'š'), (0x9B, '›'), (0x9C, 'œ'), (0x9E, 'ž'), (0x9F, 'Ÿ'),
];

pub fn rtf_to_text(rtf: &str) -> String {
    static FONTTBL: LazyLock<Regex> = LazyLock::new(|| re(r"\{\\fonttbl[^{}]*\}"));
    static COLORTBL: LazyLock<Regex> = LazyLock::new(|| re(r"\{\\colortbl[^{}]*\}"));
    static STAR: LazyLock<Regex> = LazyLock::new(|| re(r"\{\\\*[^}]*\}"));
    static UNI: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)\\u(-?\d+) ?."));
    static HEX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\\'([0-9a-f]{2})"));
    static PAR: LazyLock<Regex> = LazyLock::new(|| re(r"\\pard?\b"));
    static LINE: LazyLock<Regex> = LazyLock::new(|| re(r"\\line\b"));
    static ESC_NL: LazyLock<Regex> = LazyLock::new(|| re(r"\\\r?\n"));
    static WORD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\\[a-z]+-?\d* ?"));
    let s = FONTTBL.replace_all(rtf, "");
    let s = COLORTBL.replace_all(&s, "");
    let s = STAR.replace_all(&s, "");
    let s = UNI.replace_all(&s, |c: &regex::Captures| {
        let mut code: i64 = c[1].parse().unwrap_or(63);
        if code < 0 {
            code += 65536;
        }
        char::from_u32(code as u32).unwrap_or('\u{FFFD}').to_string()
    });
    let s = HEX.replace_all(&s, |c: &regex::Captures| {
        let b = u8::from_str_radix(&c[1], 16).unwrap_or(b'?');
        CP1252_HIGH.iter().find(|x| x.0 == b).map(|x| x.1).unwrap_or(b as char).to_string()
    });
    let s = PAR.replace_all(&s, "\n");
    let s = LINE.replace_all(&s, "\n");
    let s = ESC_NL.replace_all(&s, "\n");
    let s = WORD.replace_all(&s, "");
    s.replace(['{', '}'], "").trim().to_string()
}

pub fn first_rtf_color(rtf: &str) -> Option<String> {
    static TBL: LazyLock<Regex> = LazyLock::new(|| re(r"\{\\colortbl([^{}]*)\}"));
    static CF: LazyLock<Regex> = LazyLock::new(|| re(r"\\cf(\d+)"));
    static RGB: LazyLock<[Regex; 3]> = LazyLock::new(|| [re(r"\\red(\d+)"), re(r"\\green(\d+)"), re(r"\\blue(\d+)")]);
    let tbl = TBL.captures(rtf)?;
    let entries: Vec<Option<String>> = tbl[1]
        .split(';')
        .map(|e| {
            let v: Option<Vec<u32>> = RGB.iter().map(|r| r.captures(e).and_then(|c| c[1].parse().ok())).collect();
            v.map(|v| format!("#{}", v.iter().map(|x| format!("{:02x}", (*x).min(255))).collect::<String>()))
        })
        .collect();
    let i: usize = CF.captures(rtf)?[1].parse().ok()?;
    entries.get(i).cloned().flatten()
}

// ── Text, Word, PowerPoint ──────────────────────────────────────────────────

static SECTION: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^\[?\s*(verse\s*\d*|chorus|pre-?chorus|bridge|tag|outro|ending|intro|interlude|refrain|solo|vamp|breakdown)\s*\]?\s*:?\s*$"));

fn section_label(line: &str) -> String {
    let s: String = line.chars().filter(|c| !"[]:".contains(*c)).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    let mut at_word = true;
    for c in s.chars() {
        if at_word && c.is_alphanumeric() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        at_word = !(c.is_alphanumeric() || c == '_');
    }
    out
}

fn leading_label(lines: &[String]) -> Option<(String, Vec<String>)> {
    (lines.len() > 1 && SECTION.is_match(&lines[0])).then(|| (section_label(&lines[0]), lines[1..].to_vec()))
}

fn block(label: String, lines: Vec<String>) -> Value {
    json!({ "label": label, "lines": lines })
}

fn labelled(lines: Vec<String>, n: usize) -> Value {
    match leading_label(&lines) {
        Some((l, rest)) => block(l, rest),
        None => block(format!("Slide {n}"), lines),
    }
}

/// Pasted or plain text: a line per paragraph. Text copied from Word marks a
/// line break inside a paragraph (Shift-Enter) with a vertical tab or the
/// Unicode line separator, and may separate paragraphs with the paragraph
/// separator; those keep their meaning.
pub fn from_text(text: &str) -> Vec<Value> {
    let text = text.replace("\r\n", "\n").replace(['\r', '\u{2029}'], "\n").replace(['\u{00A0}', '\t'], " ");
    let paras: Vec<Para> = text
        .split('\n')
        .map(|p| Para { lines: p.split(['\u{000B}', '\u{2028}']).map(|l| l.trim().to_string()).collect(), heading: false })
        .collect();
    blocks_from(&paras)
}

/// A paragraph of a document: its lines (line breaks inside it), and whether
/// it's styled as a heading.
pub struct Para {
    pub lines: Vec<String>,
    pub heading: bool,
}

impl Para {
    fn blank(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }
}

/// Lines a slide holds before the next one starts (lyrics).
const LINES_PER_SLIDE: usize = 4;
/// Words a slide holds before prose is split across slides (at sentence ends).
const WORDS_PER_SLIDE: usize = 45;

fn is_prose(line: &str) -> bool {
    let n = line.split_whitespace().count();
    n > 12 || (n >= 8 && line.trim_end().ends_with(['.', '!', '?']))
}

/// Prose split at sentence ends into slides of at most `WORDS_PER_SLIDE` words.
fn split_prose(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        let ends = w.ends_with(['.', '!', '?']);
        cur = if cur.is_empty() { w.to_string() } else { format!("{cur} {w}") };
        let n = cur.split_whitespace().count();
        if (ends && n >= WORDS_PER_SLIDE * 2 / 3) || n >= WORDS_PER_SLIDE + 15 {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        // A short tail joins the slide before it.
        match out.last_mut() {
            Some(last) if cur.split_whitespace().count() < 8 && last.split_whitespace().count() + cur.split_whitespace().count() <= WORDS_PER_SLIDE + 15 => {
                last.push(' ');
                last.push_str(&cur);
            }
            _ => out.push(cur),
        }
    }
    out
}

/// Slides from paragraphs. A blank line ends a slide; a section label
/// ("Chorus") or a heading names the next. Without blank lines (most Word
/// documents), short lines are grouped a few to a slide and each paragraph
/// of prose is a slide of its own.
pub fn blocks_from(paras: &[Para]) -> Vec<Value> {
    let blank_separated = paras.iter().any(Para::blank) && paras.iter().filter(|p| !p.blank()).count() > 1;
    let mut out: Vec<Value> = Vec::new();
    let mut pending: Option<String> = None;
    let mut cur: Vec<String> = Vec::new();
    let flush = |cur: &mut Vec<String>, pending: &mut Option<String>, out: &mut Vec<Value>| {
        if cur.is_empty() {
            return;
        }
        let label = pending.take().unwrap_or_else(|| format!("Slide {}", out.len() + 1));
        out.push(block(label, std::mem::take(cur)));
    };
    for p in paras {
        if p.blank() {
            flush(&mut cur, &mut pending, &mut out);
            continue;
        }
        let lines: Vec<String> = p.lines.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        // A label on its own line, or a heading: names the slide that follows.
        if lines.len() == 1 && (SECTION.is_match(&lines[0]) || (p.heading && lines[0].split_whitespace().count() <= 8)) {
            flush(&mut cur, &mut pending, &mut out);
            pending = Some(if SECTION.is_match(&lines[0]) { section_label(&lines[0]) } else { lines[0].clone() });
            continue;
        }
        if blank_separated {
            cur.extend(lines);
            continue;
        }
        if lines.len() == 1 && is_prose(&lines[0]) {
            flush(&mut cur, &mut pending, &mut out);
            for part in split_prose(&lines[0]) {
                cur.push(part);
                flush(&mut cur, &mut pending, &mut out);
            }
            continue;
        }
        // Lines broken inside one paragraph belong together; short lines group.
        if lines.len() > 1 {
            flush(&mut cur, &mut pending, &mut out);
            for chunk in lines.chunks(LINES_PER_SLIDE) {
                cur.extend(chunk.iter().cloned());
                flush(&mut cur, &mut pending, &mut out);
            }
            continue;
        }
        cur.extend(lines);
        if cur.len() >= LINES_PER_SLIDE {
            flush(&mut cur, &mut pending, &mut out);
        }
    }
    flush(&mut cur, &mut pending, &mut out);
    // A slide that opens with its own label ("Verse 2" then its lines).
    out.into_iter()
        .map(|b| {
            let lines: Vec<String> = b["lines"].as_array().unwrap().iter().filter_map(|l| l.as_str().map(str::to_string)).collect();
            match leading_label(&lines) {
                Some((label, rest)) if b["label"].as_str().is_some_and(|l| l.starts_with("Slide ")) => block(label, rest),
                _ => b,
            }
        })
        .collect()
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'")
}

fn from_docx(buf: &[u8]) -> Result<Vec<Value>, ImportError> {
    // A paragraph's text runs, its line breaks and tabs, in order; its style.
    static P: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<w:p\b[^>]*?(?:/>|>(.*?)</w:p>)"));
    static PIECE: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<w:t\b[^>]*>(.*?)</w:t>|<w:br\b[^>]*/>|<w:cr\b[^>]*/>|<w:tab\b[^>]*/>"));
    static STYLE: LazyLock<Regex> = LazyLock::new(|| re(r#"<w:pStyle\b[^>]*w:val="([^"]+)""#));
    let files = unzip(buf, |n| n == "word/document.xml")?;
    let xml = files.first().map(|f| String::from_utf8_lossy(&f.1).into_owned()).ok_or_else(|| err("no document.xml — is this really a .docx?"))?;
    let paras: Vec<Para> = P
        .captures_iter(&xml)
        .map(|m| {
            let body = m.get(1).map_or("", |b| b.as_str());
            let mut text = String::new();
            for piece in PIECE.captures_iter(body) {
                match piece.get(1) {
                    Some(t) => text.push_str(&decode_entities(t.as_str())),
                    None if piece[0].starts_with("<w:tab") => text.push(' '),
                    None => text.push('\n'),
                }
            }
            let style = STYLE.captures(body).map(|s| s[1].to_lowercase()).unwrap_or_default();
            Para { lines: text.split('\n').map(|l| l.trim().to_string()).collect(), heading: style.starts_with("heading") || style == "title" }
        })
        .collect();
    Ok(blocks_from(&paras))
}

fn from_pptx(buf: &[u8]) -> Result<Vec<Value>, ImportError> {
    static SLIDE: LazyLock<Regex> = LazyLock::new(|| re(r"^ppt/slides/slide(\d+)\.xml$"));
    static P: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<a:p\b[^>]*>(.*?)</a:p>"));
    static T: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<a:t\b[^>]*>(.*?)</a:t>"));
    let mut files = unzip(buf, |n| SLIDE.is_match(n))?;
    if files.is_empty() {
        return Err(err("no slides found — is this really a .pptx?"));
    }
    let n = |s: &str| SLIDE.captures(s).and_then(|c| c[1].parse::<u32>().ok()).unwrap_or(0);
    files.sort_by_key(|f| n(&f.0));
    let mut out = Vec::new();
    for (i, (_, data)) in files.iter().enumerate() {
        let xml = String::from_utf8_lossy(data);
        let lines: Vec<String> = P
            .captures_iter(&xml)
            .map(|p| T.captures_iter(&p[1]).map(|t| decode_entities(&t[1])).collect::<String>().trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() {
            out.push(labelled(lines, i + 1));
        }
    }
    Ok(out)
}

// ── ProPresenter ────────────────────────────────────────────────────────────

fn from_pro6(buf: &[u8]) -> Result<Vec<Value>, ImportError> {
    static RTF: LazyLock<Regex> = LazyLock::new(|| re(r#"RTFData="([^"]+)""#));
    let xml = String::from_utf8_lossy(buf);
    let chunks: Vec<String> = RTF.captures_iter(&xml).map(|c| c[1].to_string()).collect();
    if chunks.is_empty() {
        return Err(err("no slide text found in this ProPresenter file"));
    }
    let mut out = Vec::new();
    for (i, b64) in chunks.iter().enumerate() {
        let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64) else { continue };
        let lines: Vec<String> = rtf_to_text(&String::from_utf8_lossy(&raw)).split('\n').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            out.push(labelled(lines, i + 1));
        }
    }
    if out.is_empty() {
        return Err(err("slide text could not be decoded"));
    }
    Ok(out)
}

fn collect_rtf_lines(buf: &[u8], out: &mut Vec<String>, depth: usize) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Some(fields) = pb_fields(buf) else { return };
    for f in fields.iter().filter(|f| f.wire == 2) {
        if f.raw.len() >= 5 && &f.raw[..5] == b"{\\rtf" {
            let t = rtf_to_text(&String::from_utf8_lossy(f.raw));
            out.extend(t.split('\n').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
            continue;
        }
        collect_rtf_lines(f.raw, out, depth + 1);
    }
}

static IMAGE_PATH: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(?:jpe?g|png|gif|bmp|tiff?)$"));

fn looks_like_image_path(raw: &[u8]) -> bool {
    if raw.len() < 5 || raw.len() > 1024 {
        return false;
    }
    let Ok(s) = std::str::from_utf8(raw) else { return false };
    IMAGE_PATH.is_match(s) && !s.chars().any(|c| ('\u{0}'..='\u{8}').contains(&c) || ('\u{e}'..='\u{1f}').contains(&c))
}

fn collect_media_paths(buf: &[u8], out: &mut Vec<String>, depth: usize) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Some(fields) = pb_fields(buf) else { return };
    for f in fields.iter().filter(|f| f.wire == 2) {
        if looks_like_image_path(f.raw) {
            out.push(String::from_utf8_lossy(f.raw).into_owned());
            continue;
        }
        collect_media_paths(f.raw, out, depth + 1);
    }
}

fn mime_for(base: &str) -> &'static str {
    match base.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
}

fn data_url(base: &str, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", mime_for(base), base64::engine::general_purpose::STANDARD.encode(bytes))
}


fn resolve(media: &HashMap<String, Vec<u8>>, path: &str) -> Option<String> {
    let base = path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
    media.get(&base).map(|b| data_url(&base, b))
}

fn decode_cue_scene(cue: &[Field<'_>], media: &HashMap<String, Vec<u8>>, warnings: &mut Vec<String>) -> Option<Value> {
    for action in cue.iter().filter(|f| f.num == 10 && f.wire == 2) {
        let af = pb_fields(action.raw)?;
        let Some(st) = sub(Some(&af), 23) else { continue };
        let Some(pres) = sub(Some(&st), 2) else { continue };
        let Some(slide) = sub(Some(&pres), 1) else { continue };
        let s = decode_slide(&slide, "", media, warnings, true);
        return Some(json!({ "layers": s["layers"], "canvasSize": s["canvasSize"] }));
    }
    None
}

fn from_pro7(buf: &[u8], media: Option<&HashMap<String, Vec<u8>>>) -> Result<Vec<Value>, ImportError> {
    let root = pb_fields(buf).ok_or_else(|| err("could not parse this ProPresenter 7 file"))?;
    // The arrangement (field 12) orders the cues by id; else file order.
    let order: Option<Vec<String>> = sub(Some(&root), 12).map(|a| a.iter().filter(|f| f.wire == 2 && f.num == 2).filter_map(|f| uuid_string(f.raw)).collect::<Vec<_>>()).filter(|v| !v.is_empty());
    let empty = HashMap::new();
    let media_ref = media.unwrap_or(&empty);
    let mut warnings = Vec::new();
    let mut by_uuid: HashMap<String, usize> = HashMap::new();
    let mut cues: Vec<(String, Vec<String>, Option<String>, Option<Value>)> = Vec::new();
    for f in root.iter().filter(|f| f.num == 13 && f.wire == 2) {
        let Some(cue) = pb_fields(f.raw) else { continue };
        let uuid = bytes_field(&cue, 1).and_then(uuid_string);
        let label = text(Some(&cue), 8).map(|l| l.trim().to_string()).unwrap_or_default();
        let mut lines = Vec::new();
        collect_rtf_lines(f.raw, &mut lines, 0);
        let mut image = None;
        if let Some(m) = media {
            let mut paths = Vec::new();
            collect_media_paths(f.raw, &mut paths, 0);
            image = paths.iter().find_map(|p| resolve(m, p));
        }
        let scene = decode_cue_scene(&cue, media_ref, &mut warnings).filter(|s| s["layers"].as_array().is_some_and(|l| l.iter().any(|x| x["type"] != "background")));
        if let Some(u) = uuid {
            by_uuid.insert(u, cues.len());
        }
        cues.push((label, lines, image, scene));
    }
    let ordered: Vec<usize> = match order {
        Some(o) => o.iter().filter_map(|u| by_uuid.get(u).copied()).collect(),
        None => (0..cues.len()).collect(),
    };
    let mut out = Vec::new();
    for i in ordered {
        let (label, lines, image, scene) = &cues[i];
        if lines.is_empty() && image.is_none() && scene.is_none() {
            continue;
        }
        let label = if label.is_empty() { format!("Slide {}", out.len() + 1) } else { label.clone() };
        out.push(match (scene, image) {
            (Some(s), _) => json!({ "label": label, "layers": s["layers"], "canvasSize": s["canvasSize"], "text": lines.join("\n") }),
            (None, Some(img)) if lines.is_empty() => json!({ "label": label, "lines": [], "image": img }),
            _ => block(label, lines.clone()),
        });
    }
    if out.is_empty() {
        return Err(err("no slide text found in this ProPresenter 7 file"));
    }
    Ok(out)
}

fn looks_like_pro6_xml(buf: &[u8]) -> bool {
    let head: String = buf.iter().take(200).map(|b| *b as char).collect();
    head.trim_start().starts_with("<?xml") || head.contains("RVPresentationDocument")
}

fn presentation(buf: &[u8], archive: &pp::Archive<'_>, assets: &pp::Assets, warnings: &mut Vec<String>) -> Result<Vec<Value>, ImportError> {
    if looks_like_pro6_xml(buf) {
        return from_pro6(buf);
    }
    let mut ctx = pp::Ctx { archive, assets, warnings };
    let blocks = pp::presentation(buf, &mut ctx).ok_or_else(|| err("could not parse this ProPresenter 7 file"))?;
    if blocks.is_empty() {
        // Nothing drawable: fall back to the words alone.
        return from_pro7(buf, None);
    }
    Ok(blocks)
}

static PRO: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.pro6?$"));

fn from_pro_bundle(buf: &[u8], assets: &pp::Assets, warnings: &mut Vec<String>) -> Result<Vec<Value>, ImportError> {
    let files = unzip(buf, |_| true)?;
    let mut pres: Vec<&(String, Vec<u8>)> = files.iter().filter(|f| PRO.is_match(&f.0)).collect();
    if pres.is_empty() {
        return Err(err("no presentation found in this ProPresenter bundle"));
    }
    pres.sort_by_key(|f| f.0.len());
    let archive = pp::Archive::new(&files);
    presentation(&pres[0].1, &archive, assets, warnings)
}

fn file_ref_rel_path(buf: &[u8]) -> Option<String> {
    for f in pb_fields(buf)?.iter().filter(|f| f.wire == 2) {
        let Some(inner) = pb_fields(f.raw) else { continue };
        let Some(rel) = sub(Some(&inner), 4) else { continue };
        if let Some(p) = text(Some(&rel), 2) {
            return Some(p);
        }
    }
    None
}

fn walk_playlist(fields: &[Field<'_>], out: &mut Vec<(String, String)>, depth: usize) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    for f in fields.iter().filter(|f| f.wire == 2) {
        let Some(s) = pb_fields(f.raw) else { continue };
        if let (Some(name), Some(fr)) = (bytes_field(&s, 2), bytes_field(&s, 4)) {
            if let Some(rel) = file_ref_rel_path(fr) {
                out.push((String::from_utf8_lossy(name).into_owned(), rel));
                continue;
            }
        }
        walk_playlist(&s, out, depth + 1);
    }
}

fn from_pro_playlist(buf: &[u8], assets: &pp::Assets, warnings: &mut Vec<String>) -> Result<Vec<Value>, ImportError> {
    static DATA: LazyLock<Regex> = LazyLock::new(|| re(r"(^|/)data$"));
    let files = unzip(buf, |_| true)?;
    let manifest = files.iter().find(|f| DATA.is_match(&f.0)).ok_or_else(|| err("no playlist data found in this ProPresenter playlist"))?;
    let mut items = pp::playlist_items(&manifest.1);
    if items.is_empty() {
        // An older layout: walk it the old way.
        if let Some(fields) = pb_fields(&manifest.1) {
            walk_playlist(&fields, &mut items, 0);
        }
    }
    if items.is_empty() {
        return Err(err("this ProPresenter playlist has no items"));
    }
    let by_base: HashMap<String, &Vec<u8>> = files.iter().filter(|f| PRO.is_match(&f.0)).map(|f| (f.0.rsplit('/').next().unwrap_or("").to_lowercase(), &f.1)).collect();
    let archive = pp::Archive::new(&files);
    let mut out = Vec::new();
    for (name, rel) in items {
        let base = if rel.is_empty() { format!("{name}.pro") } else { rel.rsplit('/').next().unwrap_or("").to_string() }.to_lowercase();
        let Some(pro) = by_base.get(&base).or_else(|| by_base.get(&format!("{name}.pro").to_lowercase())) else {
            warnings.push(format!("\"{name}\" isn't inside the playlist file."));
            continue;
        };
        match presentation(pro, &archive, assets, warnings) {
            Ok(blocks) if !blocks.is_empty() => out.push(json!({ "name": name, "blocks": blocks })),
            _ => warnings.push(format!("\"{name}\" has no slides Kairo could read.")),
        }
    }
    if out.is_empty() {
        return Err(err("could not read any presentations from this playlist"));
    }
    Ok(out)
}

/// Images shared by many slides are sent once: each slide refers to them by index.
pub fn dedupe_image_layers(list: &mut [Value]) -> Vec<String> {
    let mut media: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut take = |src: String, media: &mut Vec<String>| -> usize {
        *index.entry(src.clone()).or_insert_with(|| {
            media.push(src);
            media.len() - 1
        })
    };
    let mut dedupe = |blocks: &mut Value, media: &mut Vec<String>| {
        for b in blocks.as_array_mut().into_iter().flatten() {
            if let Some(img) = b.get("image").and_then(Value::as_str).map(str::to_string) {
                b["imageRef"] = json!(take(img, media));
                b.as_object_mut().unwrap().remove("image");
            }
            for l in b.get_mut("layers").and_then(Value::as_array_mut).into_iter().flatten() {
                if l["type"] == "image" {
                    if let Some(src) = l.get("src").and_then(Value::as_str).map(str::to_string) {
                        l["srcRef"] = json!(take(src, media));
                        l.as_object_mut().unwrap().remove("src");
                    }
                }
            }
        }
    };
    if list.first().is_some_and(|i| i.get("blocks").is_some()) {
        for item in list.iter_mut() {
            dedupe(&mut item["blocks"], &mut media);
        }
    } else {
        let mut all = Value::Array(list.to_vec());
        dedupe(&mut all, &mut media);
        if let Value::Array(a) = all {
            for (dst, src) in list.iter_mut().zip(a) {
                *dst = src;
            }
        }
    }
    media
}

/// What an import found: slides, or a playlist's presentations.
pub enum Imported {
    Blocks { format: &'static str, blocks: Vec<Value>, note: Option<String>, warnings: Vec<String> },
    Items { format: &'static str, items: Vec<Value>, warnings: Vec<String> },
}

pub fn import_slides(filename: &str, buf: &[u8], assets: &pp::Assets) -> Result<Imported, ImportError> {
    let ext = filename.to_lowercase().rsplit('.').next().unwrap_or("").to_string();
    let mut warnings = Vec::new();
    let blocks = |format, blocks| Ok(Imported::Blocks { format, blocks, note: None, warnings: Vec::new() });
    match ext.as_str() {
        "txt" | "md" | "json" => blocks("text", from_text(&String::from_utf8_lossy(buf))),
        "docx" => blocks("docx", from_docx(buf)?),
        "pptx" => blocks("pptx", from_pptx(buf)?),
        "pro6" | "pro" | "pro7" => {
            let b = presentation(buf, &pp::Archive::empty(), assets, &mut warnings)?;
            Ok(Imported::Blocks { format: if looks_like_pro6_xml(buf) { "pro6" } else { "pro7" }, blocks: b, note: None, warnings })
        }
        "probundle" => {
            let b = from_pro_bundle(buf, assets, &mut warnings)?;
            Ok(Imported::Blocks { format: "probundle", blocks: b, note: None, warnings })
        }
        "proplaylist" => {
            let items = from_pro_playlist(buf, assets, &mut warnings)?;
            Ok(Imported::Items { format: "proplaylist", items, warnings })
        }
        "pdf" => Err(ImportError("PDF import isn't supported yet".into(), Some("PDF_UNSUPPORTED"))),
        _ => Ok(Imported::Blocks { format: "text", blocks: from_text(&String::from_utf8_lossy(buf)), note: Some(format!("treated .{ext} as plain text")), warnings }),
    }
}

// ── Themes ──────────────────────────────────────────────────────────────────

const BOILERPLATE_FILL_HEX: &str = "#2196f2";

fn uid(prefix: &str) -> String {
    crate::util::uid(prefix, 4)
}

fn rgba(fields: &[Field<'_>]) -> (String, i64) {
    let c = |n| (num(Some(fields), n, 0.0) * 255.0).round().clamp(0.0, 255.0) as u8;
    let a = if first(fields, 4).is_some() { num(Some(fields), 4, 1.0) } else { 1.0 };
    (format!("#{:02x}{:02x}{:02x}", c(1), c(2), c(3)), (a.clamp(0.0, 1.0) * 100.0).round() as i64)
}

fn first_color_like(fields: Option<&[Field<'_>]>) -> Option<(String, i64)> {
    for f in fields?.iter().filter(|f| f.wire == 2) {
        if let Some(inner) = pb_fields(f.raw) {
            if !inner.is_empty() && inner.iter().all(|x| x.wire == 5 && (1..=4).contains(&x.num)) {
                return Some(rgba(&inner));
            }
        }
    }
    None
}

fn bounds(fields: &[Field<'_>], sx: f64, sy: f64) -> Value {
    let (o, s) = (sub(Some(fields), 1), sub(Some(fields), 2));
    json!({
        "x": (num(o.as_deref(), 1, 0.0) * sx).round(), "y": (num(o.as_deref(), 2, 0.0) * sy).round(),
        "w": (num(s.as_deref(), 1, 0.0) * sx).round(), "h": (num(s.as_deref(), 2, 0.0) * sy).round(),
    })
}

fn align(rtf: &str) -> &'static str {
    static R: LazyLock<[Regex; 3]> = LazyLock::new(|| [re(r"\\qr\b"), re(r"\\qc\b"), re(r"\\qj\b")]);
    if R[0].is_match(rtf) {
        "right"
    } else if R[1].is_match(rtf) {
        "center"
    } else if R[2].is_match(rtf) {
        "justify"
    } else {
        "left"
    }
}

fn looks_like_reference(t: &str) -> bool {
    static REF: LazyLock<Regex> = LazyLock::new(|| re(r#"^[A-Z0-9][A-Z0-9 .:\-'"()]{1,48}$"#));
    let t = t.trim();
    !t.is_empty() && t.chars().count() <= 50 && !t.chars().any(|c| c.is_ascii_lowercase()) && REF.is_match(t)
}

enum Element {
    Layer(Value),
    Fill { bounds: Value, color: String, opacity: i64, rotation: f64 },
}

fn decode_element(el: &[Field<'_>], media: &HashMap<String, Vec<u8>>, sx: f64, sy: f64, warnings: &mut Vec<String>, static_text: bool) -> Option<Element> {
    let rotation = num(Some(el), 4, 0.0);
    let pos = bounds(&sub(Some(el), 3).unwrap_or_default(), sx, sy);
    let fill = sub(Some(el), 9);
    let image = sub(fill.as_deref(), 3);
    let text_desc = sub(Some(el), 13);
    let font_sub = sub(text_desc.as_deref(), 3);
    let rtf = text_desc.as_deref().and_then(|t| bytes_field(t, 5)).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let txt = if rtf.is_empty() { String::new() } else { rtf_to_text(&rtf) };
    if let Some(m) = image {
        let rel_path = sub(sub(Some(&m), 2).as_deref(), 4).and_then(|r| text(Some(&r), 2));
        let base = rel_path.as_ref().map(|p| p.rsplit('/').next().unwrap_or("").to_lowercase());
        let Some(bytes) = base.as_ref().and_then(|b| media.get(b)) else {
            warnings.push(format!("Couldn't find the image asset \"{}\" inside the .protheme bundle — that element was skipped.", rel_path.as_deref().unwrap_or("(unknown)")));
            return None;
        };
        return Some(Element::Layer(json!({
            "id": uid("img"), "type": "image", "name": "Image", "visible": true,
            "src": data_url(base.as_deref().unwrap_or(""), bytes), "fit": "cover", "opacity": 100, "radius": 0,
            "rotation": rotation, "pos": pos,
        })));
    }
    let color = first_color_like(fill.as_deref());
    if !txt.is_empty() {
        let font = sub(font_sub.as_deref(), 1);
        let size = Some(num(font.as_deref(), 2, 36.0)).filter(|s| *s != 0.0).unwrap_or(36.0);
        let family = text(font.as_deref(), 9).or_else(|| text(font.as_deref(), 1)).filter(|f| !f.is_empty()).unwrap_or_else(|| "Manrope".into());
        let paragraph = sub(font_sub.as_deref(), 6);
        let leading = num(paragraph.as_deref(), 12, 0.0);
        let line_height = if leading > 0.0 { ((leading / size) * 100.0).round() / 100.0 } else { 1.3 };
        let role = text(Some(el), 2);
        let binding = if static_text {
            "custom"
        } else {
            match role.as_deref() {
                Some("verse") => "verse",
                Some("reference") => "reference",
                Some("number") => "custom",
                _ if looks_like_reference(&txt) => "reference",
                _ => "custom",
            }
        };
        let fill_hex = color.as_ref().map(|c| c.0.clone()).filter(|h| h != BOILERPLATE_FILL_HEX);
        return Some(Element::Layer(json!({
            "id": uid("text"), "type": "text",
            "name": match binding { "reference" => "Reference", "verse" => "Verse", _ => "Text" }, "visible": true,
            "binding": binding, "customText": if binding == "custom" { txt.clone() } else { String::new() },
            "font": { "family": family, "size": (size * sy).round(), "weight": 500, "italic": false, "lineHeight": line_height, "letterSpacing": 0, "transform": "none" },
            "color": first_rtf_color(&rtf).or(fill_hex).unwrap_or_else(|| "#ffffff".into()),
            "opacity": color.as_ref().map_or(100, |c| c.1),
            "align": align(&rtf),
            "shadow": { "enabled": false, "color": "#000000", "opacity": 70, "blur": 8, "x": 0, "y": 2 },
            "outline": { "enabled": false, "color": "#000000", "width": 2 },
            "pos": pos,
        })));
    }
    let (hex, opacity) = color.filter(|c| c.0 != BOILERPLATE_FILL_HEX)?;
    Some(Element::Fill { bounds: pos, color: hex, opacity, rotation })
}

pub fn decode_slide(slide: &[Field<'_>], name: &str, media: &HashMap<String, Vec<u8>>, warnings: &mut Vec<String>, static_text: bool) -> Value {
    let size = sub(Some(slide), 6);
    let w = Some(num(size.as_deref(), 1, 1920.0).round()).filter(|v| *v != 0.0).unwrap_or(1920.0);
    let h = Some(num(size.as_deref(), 2, 1080.0).round()).filter(|v| *v != 0.0).unwrap_or(1080.0);
    let (sx, sy) = (1920.0 / w, 1080.0 / h);
    let mut bg: Option<Value> = None;
    let mut bg_area = -1.0;
    let (mut images, mut others) = (Vec::new(), Vec::new());
    for wrapper in slide.iter().filter(|f| f.num == 1 && f.wire == 2) {
        let Some(el) = pb_fields(wrapper.raw).and_then(|outer| sub(Some(&outer), 1)) else { continue };
        match decode_element(&el, media, sx, sy, warnings, static_text) {
            Some(Element::Fill { bounds, color, opacity, rotation }) => {
                let (bw, bh) = (bounds["w"].as_f64().unwrap_or(0.0), bounds["h"].as_f64().unwrap_or(0.0));
                let area = bw.max(0.0) * bh.max(0.0);
                if bw >= 1920.0 * 0.9 && bh >= 1080.0 * 0.9 && area > bg_area {
                    if let Some(prev) = bg.take() {
                        others.push(prev);
                    }
                    bg_area = area;
                    bg = Some(json!({ "id": uid("bg"), "type": "background", "name": "Canvas", "visible": true, "fill": "solid", "color": color, "opacity": opacity }));
                } else {
                    others.push(json!({ "id": uid("shape"), "type": "background", "name": "Shape", "visible": true, "fill": "solid", "color": color, "opacity": opacity, "radius": 0, "rotation": rotation, "pos": bounds }));
                }
            }
            Some(Element::Layer(l)) if l["type"] == "image" => images.push(l),
            Some(Element::Layer(l)) => others.push(l),
            None => {}
        }
    }
    let bg = bg.unwrap_or_else(|| {
        warnings.push(format!("\"{name}\" had no full-canvas fill in the original theme — imported with a plain black background."));
        json!({ "id": uid("bg"), "type": "background", "name": "Canvas", "visible": true, "fill": "solid", "color": "#000000", "opacity": 100 })
    });
    let mut layers = vec![bg];
    layers.extend(images);
    layers.extend(others);
    json!({ "id": uid("protheme"), "name": name, "layout": "fullscreen", "animation": "fade", "canvasSize": { "w": w, "h": h }, "layers": layers })
}

pub fn from_pro_theme(buf: &[u8], assets: &pp::Assets) -> Result<(Vec<Value>, Vec<String>), ImportError> {
    static THEME: LazyLock<Regex> = LazyLock::new(|| re(r"(^|/)Theme$"));
    let files = unzip(buf, |_| true)?;
    let theme = files.iter().find(|f| THEME.is_match(&f.0)).ok_or_else(|| err("no Theme document found in this .protheme bundle"))?;
    let group = theme.0.rsplit('/').nth(1).unwrap_or("Theme").to_string();
    let archive = pp::Archive::new(&files);
    let mut warnings = Vec::new();
    let mut ctx = pp::Ctx { archive: &archive, assets, warnings: &mut warnings };
    let themes = pp::theme_document(&theme.1, &group, &mut ctx).ok_or_else(|| err("could not parse this ProPresenter theme (unrecognized format)"))?;
    if themes.is_empty() {
        return Err(err("this ProPresenter theme has no slides"));
    }
    warnings.sort();
    warnings.dedup();
    Ok((themes, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_document_without_blank_lines() {
        let p = |t: &str| Para { lines: vec![t.to_string()], heading: false };
        let b = blocks_from(&[
            p("Verse 1"),
            p("Amazing grace how sweet the sound"),
            p("That saved a wretch like me"),
            p("Chorus"),
            p("My chains are gone, I've been set free"),
            p("Welcome to our service this morning. We are glad you are here with us today."),
            p("Announcements: the youth meeting is on Friday at 6pm in the main hall."),
        ]);
        assert_eq!(b.len(), 4, "{b:?}");
        assert_eq!(b[0]["label"], "Verse 1");
        assert_eq!(b[0]["lines"], json!(["Amazing grace how sweet the sound", "That saved a wretch like me"]));
        assert_eq!(b[1]["label"], "Chorus");
        assert_eq!(b[1]["lines"], json!(["My chains are gone, I've been set free"]));
        assert_eq!(b[2]["lines"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn long_prose_is_split_at_sentence_ends() {
        let long = "This is a sentence of exactly ten words in it here. ".repeat(8);
        let parts = split_prose(&long);
        assert!(parts.len() >= 2 && parts.iter().all(|s| s.split_whitespace().count() <= 60 && s.ends_with('.')));
    }

    #[test]
    fn plain_text_becomes_labelled_slides() {
        let b = from_text("Verse 1\nAmazing grace\nhow sweet\n\nChorus\n\nMy chains are gone\n\nplain");
        assert_eq!(b[0]["label"], "Verse 1");
        assert_eq!(b[0]["lines"], json!(["Amazing grace", "how sweet"]));
        assert_eq!(b[1]["label"], "Chorus");
        assert_eq!(b[2]["label"], "Slide 3");
    }

    #[test]
    fn protobuf_edges() {
        assert!(pb_fields(&[]).is_some_and(|f| f.is_empty()), "an empty message is valid");
        let f = pb_fields(&[0x08, 0x05]).unwrap();
        assert_eq!((f[0].num, f[0].wire, f[0].value), (1, 0, 5));
        assert!(pb_fields(&[0x08]).is_none(), "a truncated varint isn't a message");
    }

    #[test]
    fn rtf_escapes() {
        assert_eq!(rtf_to_text(r"{\rtf1\ansi\ansicpg1252 It\'92s a \'93great\'94 day \'96 don\'92t miss it\'85}"), "It’s a “great” day – don’t miss it…");
        assert_eq!(rtf_to_text(r"{\rtf1 Caf\u233? na\u239?ve}"), "Café naïve");
        assert_eq!(rtf_to_text(r"{\rtf1 Hello World, no special chars here.}"), "Hello World, no special chars here.");
    }

    #[test]
    fn rtf_text() {
        let t = rtf_to_text(r"{\rtf1\ansi{\fonttbl\f0 Arial;}\f0 Hello\par World \'93quoted\'94}");
        assert_eq!(t.split('\n').map(str::trim).collect::<Vec<_>>(), ["Hello", "World “quoted”"]);
        assert_eq!(first_rtf_color(r"{\rtf1{\colortbl;\red255\green0\blue0;}\cf1 hi}").as_deref(), Some("#ff0000"));
    }
}
