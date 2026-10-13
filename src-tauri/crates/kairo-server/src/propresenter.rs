//! ProPresenter 7 slides, read exactly. Field numbers are ProPresenter's own
//! (rv.data.* from the protobuf schema embedded in ProPresenter 21's ProCore
//! framework): every element keeps its bounds, rotation, opacity, shape path,
//! fill (colour, gradient, picture or video, background effect), stroke,
//! shadow, feather, flip, and its text as ProPresenter stores it — the RTF
//! runs (font, size, colour, letter spacing, underline, strike, baseline) and
//! paragraphs (alignment, line height, spacing, indents) plus the element's
//! text attributes (capitalisation, fills, stroke, shadow, margins, vertical
//! alignment, scale behaviour). The result is a `pp` layer that
//! src/pp_render.js draws.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::import::{pb_fields, Field};

// ── protobuf access ─────────────────────────────────────────────────────────

fn first<'a>(f: &[Field<'a>], n: u64) -> Option<Field<'a>> {
    f.iter().copied().find(|x| x.num == n)
}
fn msg<'a>(f: &[Field<'a>], n: u64) -> Option<Vec<Field<'a>>> {
    first(f, n).filter(|x| x.wire == 2).and_then(|x| pb_fields(x.raw))
}
fn msgs<'a>(f: &[Field<'a>], n: u64) -> Vec<Vec<Field<'a>>> {
    f.iter().filter(|x| x.num == n && x.wire == 2).filter_map(|x| pb_fields(x.raw)).collect()
}
fn dbl(f: &[Field<'_>], n: u64) -> f64 {
    match first(f, n) {
        Some(x) if x.wire == 1 => f64::from_le_bytes(x.raw.try_into().unwrap_or([0; 8])),
        Some(x) if x.wire == 5 => f32::from_le_bytes(x.raw.try_into().unwrap_or([0; 4])) as f64,
        Some(x) if x.wire == 0 => x.value as f64,
        _ => 0.0,
    }
}
fn has(f: &[Field<'_>], n: u64) -> bool {
    first(f, n).is_some()
}
fn boolean(f: &[Field<'_>], n: u64) -> bool {
    first(f, n).is_some_and(|x| x.wire == 0 && x.value != 0)
}
fn uint(f: &[Field<'_>], n: u64) -> u64 {
    first(f, n).filter(|x| x.wire == 0).map_or(0, |x| x.value)
}
fn string(f: &[Field<'_>], n: u64) -> Option<String> {
    first(f, n).filter(|x| x.wire == 2).map(|x| String::from_utf8_lossy(x.raw).into_owned())
}
fn uuid(f: &[Field<'_>], n: u64) -> Option<String> {
    msg(f, n).and_then(|u| string(&u, 1))
}
fn packed_doubles(f: &[Field<'_>], n: u64) -> Vec<f64> {
    let mut out = Vec::new();
    for x in f.iter().filter(|x| x.num == n) {
        match x.wire {
            1 => out.push(f64::from_le_bytes(x.raw.try_into().unwrap_or([0; 8]))),
            2 => out.extend(x.raw.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap()))),
            _ => {}
        }
    }
    out
}

/// rv.data.Color → [r, g, b, a] (0–1). Proto3: an absent component is 0.
fn color(c: &[Field<'_>]) -> Value {
    json!([dbl(c, 1), dbl(c, 2), dbl(c, 3), dbl(c, 4)])
}
fn color_at(f: &[Field<'_>], n: u64) -> Value {
    msg(f, n).map_or(Value::Null, |c| color(&c))
}

// ── where files come from, and where imported ones go ──────────────────────

/// The files inside a .protheme / .probundle / .proplaylist.
pub struct Archive<'a> {
    files: Vec<(String, &'a [u8])>,
}

impl<'a> Archive<'a> {
    pub fn new(files: &'a [(String, Vec<u8>)]) -> Self {
        Self { files: files.iter().map(|(n, b)| (n.trim_start_matches('/').to_string(), b.as_slice())).collect() }
    }
    pub fn empty() -> Self {
        Self { files: Vec::new() }
    }
    /// The file a rv.data.URL points at: by its relative path, its absolute
    /// path, or (last) its file name.
    fn find(&self, url: &[Field<'_>]) -> Option<(String, &'a [u8])> {
        let abs = string(url, 1).map(|a| crate::percent_decode(a.trim_start_matches("file://")).trim_start_matches('/').to_string());
        let rel = msg(url, 4).and_then(|l| string(&l, 2)).map(|p| p.trim_start_matches('/').to_string());
        let lower = |s: &str| s.to_lowercase();
        for (name, bytes) in &self.files {
            let n = lower(name);
            if abs.as_deref().is_some_and(|a| n == lower(a)) || rel.as_deref().is_some_and(|r| !r.is_empty() && n.ends_with(&lower(r))) {
                return Some((name.clone(), bytes));
            }
        }
        let base = abs.as_deref().or(rel.as_deref())?.rsplit('/').next()?.to_lowercase();
        self.files.iter().find(|(n, _)| n.rsplit('/').next().is_some_and(|b| b.to_lowercase() == base)).map(|(n, b)| (n.clone(), *b))
    }
}

/// Imported pictures and videos are kept as files in the app's data folder
/// (one copy per distinct file) and served at /api/pp-assets/…, so a theme or
/// a playlist with large media doesn't have to carry it inline.
pub struct Assets {
    dir: PathBuf,
}

impl Assets {
    pub fn new(app_data_dir: &std::path::Path) -> Self {
        let dir = app_data_dir.join("pp-assets");
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
    fn put(&self, name: &str, bytes: &[u8]) -> Option<String> {
        let ext = name.rsplit('.').next().filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("bin").to_lowercase();
        let hash: String = Sha256::digest(bytes).iter().take(16).map(|b| format!("{b:02x}")).collect();
        let file = format!("{hash}.{ext}");
        let path = self.dir.join(&file);
        if !path.is_file() {
            std::fs::write(&path, bytes).ok()?;
        }
        Some(format!("/api/pp-assets/{file}"))
    }
}

fn is_video_name(n: &str) -> bool {
    let n = n.to_lowercase();
    [".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi", ".wmv", ".mpg", ".mpeg", ".ts", ".mts", ".m2ts", ".3gp"].iter().any(|e| n.ends_with(e))
}

// ── RTF (as Cocoa writes it) ────────────────────────────────────────────────

const CP1252: [(u8, char); 27] = [
    (0x80, '€'), (0x82, '‚'), (0x83, 'ƒ'), (0x84, '„'), (0x85, '…'), (0x86, '†'), (0x87, '‡'), (0x88, 'ˆ'), (0x89, '‰'), (0x8A, 'Š'),
    (0x8B, '‹'), (0x8C, 'Œ'), (0x8E, 'Ž'), (0x91, '‘'), (0x92, '’'), (0x93, '“'), (0x94, '”'), (0x95, '•'), (0x96, '–'), (0x97, '—'),
    (0x98, '˜'), (0x99, '™'), (0x9A, 'š'), (0x9B, '›'), (0x9C, 'œ'), (0x9E, 'ž'), (0x9F, 'Ÿ'),
];
fn cp1252(b: u8) -> char {
    CP1252.iter().find(|(k, _)| *k == b).map_or(b as char, |(_, c)| *c)
}

#[derive(Clone, PartialEq)]
struct CharFmt {
    font: i32,
    half_points: i32,
    bold: bool,
    italic: bool,
    underline: i32, // 0 none, 1 single, 2 thick, 3 double, 4 dotted, 5 dashed
    underline_color: i32,
    strike: i32, // 0 none, 1 single, 2 double
    strike_color: i32,
    color: i32,
    background: i32,
    expand_twips: i32,
    baseline_half_points: i32,
    script: i32, // 1 super, -1 sub
    caps: bool,
    small_caps: bool,
    outline: bool,
    stroke_width: f64,
    stroke_color: i32,
    ligatures: bool,
}
impl Default for CharFmt {
    fn default() -> Self {
        Self {
            font: 0, half_points: 24, bold: false, italic: false, underline: 0, underline_color: 0, strike: 0, strike_color: 0,
            color: 0, background: 0, expand_twips: 0, baseline_half_points: 0, script: 0, caps: false, small_caps: false,
            outline: false, stroke_width: 0.0, stroke_color: 0, ligatures: true,
        }
    }
}

#[derive(Clone, Default)]
struct ParaFmt {
    align: &'static str,
    sl: i32,
    slmult: bool,
    sa: i32,
    sb: i32,
    fi: i32,
    li: i32,
    ri: i32,
}

#[derive(Clone)]
struct State {
    c: CharFmt,
    skip: bool,
    dest: &'static str,
    uc: i32,
}

struct Rtf {
    fonts: HashMap<i32, String>,
    colors: Vec<Option<[f64; 4]>>,
    expanded: Vec<Option<[f64; 4]>>,
    paras: Vec<Value>,
    runs: Vec<Value>,
    run_text: String,
    run_fmt: CharFmt,
    para: ParaFmt,
}

impl Rtf {
    fn col(&self, i: i32) -> Value {
        if i <= 0 {
            return Value::Null;
        }
        let i = i as usize;
        let c = self.expanded.get(i).copied().flatten().or_else(|| self.colors.get(i).copied().flatten());
        c.map_or(Value::Null, |c| json!(c))
    }
    fn flush_run(&mut self) {
        if self.run_text.is_empty() {
            return;
        }
        let c = &self.run_fmt;
        let mut r = json!({
            "t": std::mem::take(&mut self.run_text),
            "font": self.fonts.get(&c.font).cloned().unwrap_or_default(),
            "size": c.half_points as f64 / 2.0,
        });
        let o = r.as_object_mut().unwrap();
        let col = self.col(c.color);
        if !col.is_null() { o.insert("color".into(), col); }
        if c.bold { o.insert("b".into(), json!(true)); }
        if c.italic { o.insert("i".into(), json!(true)); }
        if c.underline != 0 {
            o.insert("ul".into(), json!(c.underline));
            let uc = self.col(c.underline_color);
            if !uc.is_null() { o.insert("ulColor".into(), uc); }
        }
        if c.strike != 0 {
            o.insert("st".into(), json!(c.strike));
            let sc = self.col(c.strike_color);
            if !sc.is_null() { o.insert("stColor".into(), sc); }
        }
        let bg = self.col(c.background);
        if !bg.is_null() { o.insert("bg".into(), bg); }
        if c.expand_twips != 0 { o.insert("kern".into(), json!(c.expand_twips as f64 / 20.0)); }
        if c.baseline_half_points != 0 { o.insert("base".into(), json!(c.baseline_half_points as f64 / 2.0)); }
        if c.script != 0 { o.insert("script".into(), json!(c.script)); }
        if c.caps { o.insert("caps".into(), json!(true)); }
        if c.small_caps { o.insert("smallCaps".into(), json!(true)); }
        if c.stroke_width != 0.0 || c.outline {
            o.insert("strokeW".into(), json!(if c.stroke_width != 0.0 { c.stroke_width } else { 3.0 }));
            let sc = self.col(c.stroke_color);
            if !sc.is_null() { o.insert("strokeColor".into(), sc); }
        }
        if !c.ligatures { o.insert("noLig".into(), json!(true)); }
        self.runs.push(r);
    }
    fn push_char(&mut self, ch: char, fmt: &CharFmt) {
        if *fmt != self.run_fmt {
            self.flush_run();
            self.run_fmt = fmt.clone();
        }
        self.run_text.push(ch);
    }
    fn end_para(&mut self) {
        self.flush_run();
        let p = &self.para;
        let mut v = json!({ "align": if p.align.is_empty() { "natural" } else { p.align }, "runs": std::mem::take(&mut self.runs) });
        let o = v.as_object_mut().unwrap();
        if p.sl != 0 {
            if p.slmult { o.insert("lineMult".into(), json!(p.sl as f64 / 240.0)); } else { o.insert("lineExact".into(), json!(p.sl as f64 / 20.0)); }
        }
        if p.sb != 0 { o.insert("before".into(), json!(p.sb as f64 / 20.0)); }
        if p.sa != 0 { o.insert("after".into(), json!(p.sa as f64 / 20.0)); }
        if p.fi != 0 { o.insert("firstIndent".into(), json!(p.fi as f64 / 20.0)); }
        if p.li != 0 { o.insert("indent".into(), json!(p.li as f64 / 20.0)); }
        if p.ri != 0 { o.insert("tailIndent".into(), json!(p.ri as f64 / 20.0)); }
        self.paras.push(v);
    }
}

/// Cocoa RTF → paragraphs of styled runs.
pub fn parse_rtf(src: &str) -> Value {
    let b = src.as_bytes();
    let mut r = Rtf { fonts: HashMap::new(), colors: Vec::new(), expanded: Vec::new(), paras: Vec::new(), runs: Vec::new(), run_text: String::new(), run_fmt: CharFmt::default(), para: ParaFmt::default() };
    let mut st = State { c: CharFmt::default(), skip: false, dest: "", uc: 1 };
    let mut stack: Vec<State> = Vec::new();
    // colour-table scratch
    let mut ct: [i32; 3] = [-1, -1, -1];
    let mut ect: Vec<f64> = Vec::new();
    let mut ect_space = "";
    let mut font_idx = 0;
    let mut font_name = String::new();
    let mut group_start = false;
    let mut skip_chars = 0i32;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'{' => {
                stack.push(st.clone());
                group_start = true;
                i += 1;
                continue;
            }
            b'}' => {
                if st.dest == "fonttbl" && !font_name.trim().is_empty() {
                    r.fonts.insert(font_idx, font_name.trim().trim_end_matches(';').to_string());
                    font_name.clear();
                }
                if let Some(prev) = stack.pop() {
                    st = prev;
                }
                i += 1;
                continue;
            }
            b'\\' => {
                i += 1;
                let Some(&n) = b.get(i) else { break };
                if n.is_ascii_alphabetic() {
                    let s = i;
                    while i < b.len() && b[i].is_ascii_alphabetic() { i += 1; }
                    let word = &src[s..i];
                    let mut param: Option<i32> = None;
                    let ps = i;
                    if i < b.len() && (b[i] == b'-' || b[i].is_ascii_digit()) {
                        i += 1;
                        while i < b.len() && b[i].is_ascii_digit() { i += 1; }
                        param = src[ps..i].parse().ok();
                    }
                    if i < b.len() && b[i] == b' ' { i += 1; }
                    let p = param.unwrap_or(1);
                    let was_start = group_start;
                    group_start = false;
                    // destinations
                    match word {
                        "fonttbl" => { st.dest = "fonttbl"; continue; }
                        "colortbl" => { st.dest = "colortbl"; continue; }
                        "expandedcolortbl" => { st.dest = "expandedcolortbl"; st.skip = false; continue; }
                        "info" | "stylesheet" | "listtable" | "listoverridetable" | "pict" | "header" | "footer" | "NeXTGraphic" => { st.skip = true; continue; }
                        _ => {}
                    }
                    if st.skip { continue; }
                    match st.dest {
                        "fonttbl" => { if word == "f" { font_idx = p; font_name.clear(); } continue; }
                        "colortbl" => {
                            match word { "red" => ct[0] = p, "green" => ct[1] = p, "blue" => ct[2] = p, _ => {} }
                            continue;
                        }
                        "expandedcolortbl" => {
                            match word {
                                "cssrgb" | "csgenericrgb" | "csdevicergb" | "csgray" | "csgenericgray" | "csdevicegray" => { ect_space = if word.contains("gray") { "gray" } else { "rgb" }; ect.clear(); }
                                "c" => ect.push(p as f64 / 100000.0),
                                _ => {}
                            }
                            continue;
                        }
                        _ => {}
                    }
                    let _ = was_start;
                    let cf = &mut st.c;
                    match word {
                        "par" => { r.end_para(); }
                        "line" => { let f = st.c.clone(); r.push_char('\u{2028}', &f); }
                        "tab" => { let f = st.c.clone(); r.push_char('\t', &f); }
                        "pard" => { r.para = ParaFmt::default(); }
                        "plain" => { *cf = CharFmt { font: cf.font, ..CharFmt::default() }; }
                        "ql" => r.para.align = "left",
                        "qr" => r.para.align = "right",
                        "qc" => r.para.align = "center",
                        "qj" => r.para.align = "justify",
                        "qn" | "pardirnatural" => { if word == "qn" { r.para.align = "natural"; } }
                        "sl" => r.para.sl = p,
                        "slmult" => r.para.slmult = p != 0,
                        "sa" => r.para.sa = p,
                        "sb" => r.para.sb = p,
                        "fi" => r.para.fi = p,
                        "li" => r.para.li = p,
                        "ri" => r.para.ri = p,
                        "f" => cf.font = p,
                        "fs" => cf.half_points = p,
                        "b" => cf.bold = p != 0,
                        "i" => cf.italic = p != 0,
                        "ul" => cf.underline = if param == Some(0) { 0 } else { 1 },
                        "ulth" => cf.underline = 2,
                        "uldb" => cf.underline = 3,
                        "uld" => cf.underline = 4,
                        "uldash" => cf.underline = 5,
                        "ulnone" => cf.underline = 0,
                        "ulc" => cf.underline_color = p,
                        "strike" => cf.strike = if param == Some(0) { 0 } else { 1 },
                        "striked" => cf.strike = if param == Some(0) { 0 } else { 2 },
                        "strikec" => cf.strike_color = p,
                        "cf" => cf.color = p,
                        "cb" | "highlight" | "chcbpat" => cf.background = p,
                        "expnd" => { if param.is_some() && cf.expand_twips == 0 { cf.expand_twips = p * 5; } }
                        "expndtw" => cf.expand_twips = p,
                        "up" => cf.baseline_half_points = p,
                        "dn" => cf.baseline_half_points = -p,
                        "super" => cf.script = 1,
                        "sub" => cf.script = -1,
                        "nosupersub" => cf.script = 0,
                        "caps" => cf.caps = p != 0,
                        "scaps" => cf.small_caps = p != 0,
                        "outl" => cf.outline = p != 0,
                        "strokewidth" => cf.stroke_width = p as f64,
                        "strokec" => cf.stroke_color = p,
                        "CocoaLigature" => cf.ligatures = p != 0,
                        "uc" => st.uc = p,
                        "u" => {
                            let code = if p < 0 { p + 65536 } else { p };
                            let f = st.c.clone();
                            if let Some(ch) = char::from_u32(code as u32) { r.push_char(ch, &f); }
                            skip_chars = st.uc;
                        }
                        _ => {}
                    }
                    continue;
                }
                // control symbols
                i += 1;
                group_start = false;
                if st.skip { continue; }
                match n {
                    b'*' => { st.skip = true; }
                    b'\'' => {
                        let hex = src.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
                        i += 2;
                        if let Some(h) = hex {
                            if skip_chars > 0 { skip_chars -= 1; continue; }
                            let ch = cp1252(h);
                            if st.dest == "fonttbl" { font_name.push(ch); } else if st.dest.is_empty() { let f = st.c.clone(); r.push_char(ch, &f); }
                        }
                    }
                    b'\n' | b'\r' => { if st.dest.is_empty() { r.end_para(); } }
                    b'~' => { let f = st.c.clone(); r.push_char('\u{a0}', &f); }
                    b'-' => {}
                    b'_' => { let f = st.c.clone(); r.push_char('\u{2011}', &f); }
                    b'\\' | b'{' | b'}' => {
                        if st.dest == "fonttbl" { font_name.push(n as char); } else if st.dest.is_empty() { let f = st.c.clone(); r.push_char(n as char, &f); }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => { i += 1; }
            _ => {
                // text: one UTF-8 character
                let ch = src[i..].chars().next().unwrap_or(' ');
                i += ch.len_utf8();
                group_start = false;
                if st.skip { continue; }
                if skip_chars > 0 { skip_chars -= 1; continue; }
                match st.dest {
                    "fonttbl" => {
                        if ch == ';' {
                            r.fonts.insert(font_idx, font_name.trim().to_string());
                            font_name.clear();
                        } else {
                            font_name.push(ch);
                        }
                    }
                    "colortbl" => {
                        if ch == ';' {
                            r.colors.push(if ct[0] < 0 { None } else { Some([ct[0].max(0) as f64 / 255.0, ct[1].max(0) as f64 / 255.0, ct[2].max(0) as f64 / 255.0, 1.0]) });
                            ct = [-1, -1, -1];
                        }
                    }
                    "expandedcolortbl" => {
                        if ch == ';' {
                            r.expanded.push(match (ect_space, ect.len()) {
                                ("rgb", 3) => Some([ect[0], ect[1], ect[2], 1.0]),
                                ("rgb", n) if n >= 4 => Some([ect[0], ect[1], ect[2], ect[3]]),
                                ("gray", 1) => Some([ect[0], ect[0], ect[0], 1.0]),
                                ("gray", n) if n >= 2 => Some([ect[0], ect[0], ect[0], ect[1]]),
                                _ => None,
                            });
                            ect.clear();
                            ect_space = "";
                        }
                    }
                    _ => { let f = st.c.clone(); r.push_char(ch, &f); }
                }
            }
        }
    }
    // the last paragraph has no \par
    r.flush_run();
    if !r.runs.is_empty() || r.paras.is_empty() {
        r.end_para();
    }
    Value::Array(r.paras)
}

/// The plain words of parsed RTF.
pub fn plain_text(paras: &Value) -> String {
    paras.as_array().into_iter().flatten()
        .map(|p| p["runs"].as_array().into_iter().flatten().filter_map(|r| r["t"].as_str()).collect::<String>().replace('\u{2028}', "\n"))
        .collect::<Vec<_>>().join("\n")
}

// ── elements ────────────────────────────────────────────────────────────────

const ENUM_SHAPE: [&str; 12] = ["unknown", "rectangle", "ellipse", "isoscelesTriangle", "rightTriangle", "rhombus", "star", "polygon", "custom", "rightArrow", "doubleArrow", "roundedRectangle"];
const ENUM_ALIGN: [&str; 5] = ["left", "right", "center", "justify", "natural"];
const ENUM_CAPS: [&str; 5] = ["none", "all", "small", "title", "start"];
const ENUM_VALIGN: [&str; 3] = ["top", "middle", "bottom"];
const ENUM_TEXT_SCALE: [&str; 5] = ["none", "adjustHeight", "down", "up", "upDown"];
const ENUM_TEXT_TRANSFORM: [&str; 5] = ["none", "singleLine", "wordPerLine", "charPerLine", "replaceLineReturns"];
const ENUM_MEDIA_SCALE: [&str; 4] = ["fit", "fill", "stretch", "custom"];
const ENUM_MEDIA_ALIGN: [&str; 9] = ["middleCenter", "topLeft", "topCenter", "topRight", "middleRight", "bottomRight", "bottomCenter", "bottomLeft", "middleLeft"];
const ENUM_GRADIENT: [&str; 3] = ["linear", "radial", "angle"];
const ENUM_FLIP: [&str; 4] = ["none", "vertical", "horizontal", "both"];
const ENUM_STROKE: [&str; 4] = ["solid", "squareDash", "shortDash", "longDash"];
const ENUM_FEATHER: [&str; 3] = ["inside", "center", "outside"];
const ENUM_UNDERLINE: [i64; 4] = [0, 1, 2, 3];

fn e<'a>(table: &[&'a str], v: u64) -> &'a str {
    table.get(v as usize).copied().unwrap_or(table[0])
}

fn point(f: &[Field<'_>], n: u64) -> Value {
    msg(f, n).map_or(json!([0.0, 0.0]), |p| json!([dbl(&p, 1), dbl(&p, 2)]))
}

fn gradient(g: &[Field<'_>]) -> Value {
    let stops: Vec<Value> = msgs(g, 4).iter().map(|s| json!({ "pos": dbl(s, 2), "blend": if has(s, 3) { dbl(s, 3) } else { 0.5 }, "color": color_at(s, 1) })).collect();
    json!({ "type": e(&ENUM_GRADIENT, uint(g, 1)), "angle": dbl(g, 2), "length": if has(g, 3) { dbl(g, 3) } else { 1.0 }, "stops": stops })
}

pub struct Ctx<'a, 'b> {
    pub archive: &'b Archive<'a>,
    pub assets: &'b Assets,
    pub warnings: &'b mut Vec<String>,
}

/// rv.data.Media → { src, kind, scale, align, natural, flipH, flipV, crop, custom }.
fn media(m: &[Field<'_>], ctx: &mut Ctx<'_, '_>) -> Option<Value> {
    let url = msg(m, 2).or_else(|| msg(m, 5).and_then(|im| msg(&im, 2)).and_then(|fp| msg(&fp, 1)));
    let video = msg(m, 6);
    let props = msg(m, 5).or_else(|| video.clone());
    let drawing = props.as_ref().and_then(|p| msg(p, 1)).unwrap_or_default();
    let name_hint = url.as_ref().and_then(|u| string(u, 1)).unwrap_or_default();
    let found = url.as_ref().and_then(|u| ctx.archive.find(u));
    let Some((name, bytes)) = found else {
        ctx.warnings.push(format!("Media \"{}\" isn't inside the file — the element shows its fill without it.", name_hint.rsplit('/').next().unwrap_or("?")));
        return None;
    };
    let src = ctx.assets.put(&name, bytes)?;
    let nat = msg(&drawing, 5).map_or(Value::Null, |s| json!({ "w": dbl(&s, 1), "h": dbl(&s, 2) }));
    let mut out = json!({
        "src": src,
        "kind": if video.is_some() || is_video_name(&name) { "video" } else { "image" },
        "scale": e(&ENUM_MEDIA_SCALE, uint(&drawing, 1)),
        "align": e(&ENUM_MEDIA_ALIGN, uint(&drawing, 2)),
        "natural": nat,
    });
    let o = out.as_object_mut().unwrap();
    if boolean(&drawing, 3) { o.insert("flipH".into(), json!(true)); }
    if boolean(&drawing, 4) { o.insert("flipV".into(), json!(true)); }
    if has(&drawing, 6) { o.insert("rotation".into(), json!(dbl(&drawing, 6))); }
    if let Some(cb) = msg(&drawing, 7) {
        let (o2, s2) = (msg(&cb, 1).unwrap_or_default(), msg(&cb, 2).unwrap_or_default());
        if dbl(&s2, 1) > 0.0 { o.insert("customBounds".into(), json!({ "x": dbl(&o2, 1), "y": dbl(&o2, 2), "w": dbl(&s2, 1), "h": dbl(&s2, 2) })); }
    }
    if boolean(&drawing, 13) {
        if let Some(ci) = msg(&drawing, 14) { o.insert("crop".into(), json!({ "l": dbl(&ci, 1), "r": dbl(&ci, 2), "t": dbl(&ci, 3), "b": dbl(&ci, 4) })); }
    }
    if boolean(&drawing, 16) { o.insert("blurred".into(), json!(true)); }
    if !msgs(&drawing, 12).is_empty() {
        ctx.warnings.push("A picture uses ProPresenter media effects; the picture is imported without them.".into());
    }
    Some(out)
}

fn shadow(s: &[Field<'_>]) -> Value {
    json!({ "enable": boolean(s, 7), "angle": dbl(s, 2), "offset": dbl(s, 3), "radius": dbl(s, 4), "color": color_at(s, 5), "opacity": dbl(s, 6) })
}

fn text(t: &[Field<'_>], ctx: &mut Ctx<'_, '_>) -> Value {
    let attrs = msg(t, 3).unwrap_or_default();
    let font = msg(&attrs, 1).unwrap_or_default();
    let para = msg(&attrs, 6).unwrap_or_default();
    let rtf = first(t, 5).filter(|x| x.wire == 2).map(|x| String::from_utf8_lossy(x.raw).into_owned()).unwrap_or_default();
    let paras = if rtf.is_empty() { json!([]) } else { parse_rtf(&rtf) };
    let mut out = json!({
        "paragraphs": paras,
        "font": { "name": string(&font, 1).unwrap_or_default(), "family": string(&font, 9).unwrap_or_default(), "face": string(&font, 10).unwrap_or_default(),
                  "size": dbl(&font, 2), "bold": boolean(&font, 8), "italic": boolean(&font, 4) },
        "caps": e(&ENUM_CAPS, uint(&attrs, 2)),
        "fill": color_at(&attrs, 3),
        "paragraph": {
            "align": e(&ENUM_ALIGN, uint(&para, 1)), "firstIndent": dbl(&para, 2), "indent": dbl(&para, 3), "tailIndent": dbl(&para, 4),
            "lineMult": dbl(&para, 5), "maxLine": dbl(&para, 6), "minLine": dbl(&para, 7), "lineSpacing": dbl(&para, 8),
            "after": dbl(&para, 9), "before": dbl(&para, 10),
        },
        "kerning": dbl(&attrs, 7),
        "superscript": first(&attrs, 8).map_or(0, |x| x.value as i64),
        "strokeWidth": dbl(&attrs, 11),
        "strokeColor": color_at(&attrs, 12),
        "valign": e(&ENUM_VALIGN, uint(t, 6)),
        "scale": e(&ENUM_TEXT_SCALE, uint(t, 7)),
        "transform": e(&ENUM_TEXT_TRANSFORM, uint(t, 10)),
        "delimiter": string(t, 11).unwrap_or_default(),
        "noLigatures": uint(&attrs, 19) == 1,
    });
    let o = out.as_object_mut().unwrap();
    if let Some(m) = msg(t, 8) { o.insert("margins".into(), json!({ "l": dbl(&m, 1), "r": dbl(&m, 2), "t": dbl(&m, 3), "b": dbl(&m, 4) })); }
    if let Some(s) = msg(t, 4) { o.insert("shadow".into(), shadow(&s)); }
    if let Some(u) = msg(&attrs, 4) { if uint(&u, 1) != 0 { o.insert("underline".into(), json!({ "style": ENUM_UNDERLINE[(uint(&u, 1) as usize).min(3)], "pattern": uint(&u, 2), "byWord": boolean(&u, 3), "color": color_at(&attrs, 5) })); } }
    if let Some(u) = msg(&attrs, 9) { if uint(&u, 1) != 0 { o.insert("strike".into(), json!({ "style": ENUM_UNDERLINE[(uint(&u, 1) as usize).min(3)], "pattern": uint(&u, 2), "color": color_at(&attrs, 10) })); } }
    if let Some(g) = msg(&attrs, 14) { if let Some(gr) = msg(&g, 1) { o.insert("gradientFill".into(), json!({ "gradient": gradient(&gr), "stretch": boolean(&g, 2) })); } }
    if has(&attrs, 15) { o.insert("background".into(), color_at(&attrs, 15)); }
    if let Some(mf) = msg(&attrs, 17) { if let Some(m) = msg(&mf, 1) { if let Some(v) = media(&m, ctx) { o.insert("mediaFill".into(), v); } } }
    if has(&attrs, 16) { o.insert("cutOut".into(), json!(true)); }
    if let Some(be) = msg(&attrs, 18) { o.insert("backgroundEffect".into(), background_effect(&be)); }
    if let Some(lm) = msg(t, 14) { o.insert("lineMask".into(), json!(lm.len())); }
    if let Some(cp) = msg(t, 12) { if boolean(&cp, 1) { o.insert("chordPro".into(), json!({ "color": color_at(&cp, 3) })); } }
    out
}

fn background_effect(be: &[Field<'_>]) -> Value {
    if let Some(b) = msg(be, 2) {
        return json!({ "blur": dbl(&b, 2), "saturation": dbl(&b, 1) });
    }
    if has(be, 3) {
        return json!({ "invert": true });
    }
    Value::Null
}

/// One rv.data.Graphics.Element (inside a rv.data.Slide.Element) → a `pp` layer.
pub fn element(wrapper: &[Field<'_>], slide_w: f64, slide_h: f64, static_text: bool, ctx: &mut Ctx<'_, '_>) -> Option<Value> {
    let el = msg(wrapper, 1)?;
    let name = string(&el, 2).unwrap_or_default();
    let rect = msg(&el, 3).unwrap_or_default();
    let (o, s) = (msg(&rect, 1).unwrap_or_default(), msg(&rect, 2).unwrap_or_default());
    let (sx, sy) = (1920.0 / slide_w, 1080.0 / slide_h);
    let pos = json!({ "x": dbl(&o, 1) * sx, "y": dbl(&o, 2) * sy, "w": dbl(&s, 1) * sx, "h": dbl(&s, 2) * sy });

    let path = msg(&el, 8).unwrap_or_default();
    let shape = msg(&path, 3).unwrap_or_default();
    let points: Vec<Value> = msgs(&path, 2).iter().map(|bp| json!({ "p": point(bp, 1), "q0": point(bp, 2), "q1": point(bp, 3), "curved": boolean(bp, 4) })).collect();
    let mut shape_v = json!({ "type": e(&ENUM_SHAPE, uint(&shape, 1)), "closed": boolean(&path, 1), "points": points });
    if let Some(rr) = msg(&shape, 2) { shape_v["roundness"] = json!(dbl(&rr, 1)); }
    if let Some(pg) = msg(&shape, 3) { shape_v["sides"] = json!(uint(&pg, 1)); }
    if let Some(stp) = msg(&shape, 4) { shape_v["star"] = json!({ "inner": dbl(&stp, 1), "points": uint(&stp, 2) }); }
    if let Some(ar) = msg(&shape, 5) { shape_v["arrowCorner"] = point(&ar, 1); }

    let fill = msg(&el, 9).unwrap_or_default();
    let mut fill_v = json!({ "enable": boolean(&fill, 4) });
    if has(&fill, 1) { fill_v["color"] = color_at(&fill, 1); }
    if let Some(g) = msg(&fill, 2) { fill_v["gradient"] = gradient(&g); }
    if let Some(m) = msg(&fill, 3) {
        if boolean(&fill, 4) {
            if let Some(v) = media(&m, ctx) { fill_v["media"] = v; }
        }
    }
    if let Some(be) = msg(&fill, 5) { fill_v["backgroundEffect"] = background_effect(&be); }

    let stroke = msg(&el, 10).unwrap_or_default();
    let stroke_v = json!({ "enable": boolean(&stroke, 5), "style": e(&ENUM_STROKE, uint(&stroke, 1)), "width": dbl(&stroke, 2), "color": color_at(&stroke, 3), "pattern": packed_doubles(&stroke, 4) });
    let feather = msg(&el, 12).unwrap_or_default();
    let feather_v = json!({ "enable": boolean(&feather, 3), "style": e(&ENUM_FEATHER, uint(&feather, 1)), "radius": dbl(&feather, 2) });

    let text_v = msg(&el, 13).map(|t| text(&t, ctx));
    let plain = text_v.as_ref().map(|t| plain_text(&t["paragraphs"])).unwrap_or_default();
    let has_words = !plain.trim().is_empty();
    let binding = if static_text { "custom" } else {
        match name.trim().to_lowercase().as_str() {
            "verse" | "text" | "lyrics" | "scripture" => "verse",
            "reference" | "ref" | "verse reference" => "reference",
            _ => "custom",
        }
    };

    let mut layer = json!({
        "id": uid("pp"),
        "type": "pp",
        "name": if name.is_empty() { if has_words { "Text".to_string() } else if fill_v.get("media").is_some() { "Media".to_string() } else { "Shape".to_string() } } else { name.clone() },
        "visible": !boolean(&el, 16),
        "binding": if text_v.is_some() && (has_words || binding != "custom") { binding } else { "none" },
        "pos": pos,
        "rotation": dbl(&el, 4),
        "opacity": dbl(&el, 5),
        "flip": e(&ENUM_FLIP, uint(&el, 15)),
        "shape": shape_v,
        "fill": fill_v,
        "stroke": stroke_v,
        "shadow": msg(&el, 11).map_or(json!({ "enable": false }), |s| shadow(&s)),
        "feather": feather_v,
        "slideSize": { "w": slide_w, "h": slide_h },
    });
    if let Some(t) = text_v { layer["text"] = t; }
    if let Some(ts) = msg(wrapper, 9) {
        if boolean(&ts, 1) {
            layer["scroller"] = json!({ "rate": dbl(&ts, 2), "repeat": boolean(&ts, 3), "distance": dbl(&ts, 4), "direction": uint(&ts, 5), "startsOffScreen": boolean(&ts, 6), "fadeLeft": dbl(&ts, 7), "fadeRight": dbl(&ts, 8) });
        }
    }
    if has(wrapper, 2) || has(wrapper, 3) {
        ctx.warnings.push(format!("\"{}\" has build animations; they aren't played yet.", layer["name"].as_str().unwrap_or("An element")));
    }
    Some(layer)
}

fn uid(prefix: &str) -> String {
    let mut b = [0u8; 6];
    getrandom::fill(&mut b).ok();
    format!("{prefix}-{}-{}", crate::detect::epoch_ms(), b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

/// A rv.data.Slide → Kairo layers (bottom first), its canvas size, and its words.
pub fn slide(slide: &[Field<'_>], static_text: bool, ctx: &mut Ctx<'_, '_>) -> (Vec<Value>, Value, String) {
    let size = msg(slide, 6).unwrap_or_default();
    let w = Some(dbl(&size, 1)).filter(|v| *v > 0.0).unwrap_or(1920.0);
    let h = Some(dbl(&size, 2)).filter(|v| *v > 0.0).unwrap_or(1080.0);
    // The canvas: the slide's own background colour when it draws one, else clear.
    let draws = boolean(slide, 4);
    let bg = if draws { msg(slide, 5).map(|c| color(&c)) } else { None };
    let canvas = match &bg {
        Some(c) => {
            let a = c[3].as_f64().unwrap_or(1.0);
            json!({ "id": uid("bg"), "type": "background", "name": "Canvas", "visible": true, "fill": "solid", "color": hex(c), "opacity": (a * 100.0).round() })
        }
        None => json!({ "id": uid("bg"), "type": "background", "name": "Canvas", "visible": true, "fill": "transparent", "color": "#000000", "opacity": 100 }),
    };
    // ProPresenter lists elements top first; Kairo paints bottom first.
    let mut layers: Vec<Value> = msgs(slide, 1).iter().filter_map(|wr| element(wr, w, h, static_text, ctx)).collect();
    layers.reverse();
    let words = layers.iter().filter(|l| l["text"].is_object()).map(|l| plain_text(&l["text"]["paragraphs"])).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join("\n");
    let mut all = vec![canvas];
    all.extend(layers);
    (all, json!({ "w": w, "h": h }), words)
}

fn hex(c: &Value) -> String {
    let ch = |i: usize| (c[i].as_f64().unwrap_or(0.0) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(1), ch(2))
}

// ── documents ───────────────────────────────────────────────────────────────

/// rv.data.Template.Document (a .protheme's "Theme") → one look per slide.
pub fn theme_document(buf: &[u8], group_name: &str, ctx: &mut Ctx<'_, '_>) -> Option<Vec<Value>> {
    let root = pb_fields(buf)?;
    let mut looks = Vec::new();
    for (i, ts) in msgs(&root, 3).iter().enumerate() {
        let Some(base) = msg(ts, 1) else { continue };
        let name = string(ts, 2).unwrap_or_else(|| format!("{group_name} {}", i + 1));
        let (layers, canvas, _) = slide(&base, false, ctx);
        looks.push(json!({ "id": uid("protheme"), "name": name, "layout": "fullscreen", "animation": "fade", "canvasSize": canvas, "layers": layers, "source": "propresenter" }));
    }
    Some(looks)
}

/// rv.data.Presentation (.pro) → slides in arrangement order, each with its
/// own layers, its group's name and colour, and its words.
pub fn presentation(buf: &[u8], ctx: &mut Ctx<'_, '_>) -> Option<Vec<Value>> {
    let root = pb_fields(buf)?;
    // Groups: name + colour + their cues.
    let groups: Vec<(String, String, Value, Vec<String>)> = msgs(&root, 12).iter().filter_map(|cg| {
        let g = msg(cg, 1)?;
        let ids: Vec<String> = msgs(cg, 2).iter().filter_map(|u| string(u, 1)).collect();
        Some((uuid(&g, 1).unwrap_or_default(), string(&g, 2).unwrap_or_default(), color_at(&g, 3), ids))
    }).collect();
    // Order: the selected arrangement's groups, else the groups in order, else the cues as stored.
    let selected = uuid(&root, 10);
    let arrangement = msgs(&root, 11).into_iter().find(|a| selected.is_some() && uuid(a, 1) == selected);
    let mut order: Vec<(String, Option<usize>)> = Vec::new();
    if let Some(a) = arrangement {
        for gid in msgs(&a, 3).iter().filter_map(|u| string(u, 1)) {
            if let Some((gi, g)) = groups.iter().enumerate().find(|(_, g)| g.0 == gid) {
                order.extend(g.3.iter().map(|c| (c.clone(), Some(gi))));
            }
        }
    } else {
        for (gi, g) in groups.iter().enumerate() {
            order.extend(g.3.iter().map(|c| (c.clone(), Some(gi))));
        }
    }
    let cues: Vec<Vec<Field<'_>>> = msgs(&root, 13);
    let by_id: HashMap<String, usize> = cues.iter().enumerate().filter_map(|(i, c)| uuid(c, 1).map(|u| (u, i))).collect();
    if order.is_empty() {
        order = cues.iter().filter_map(|c| uuid(c, 1)).map(|u| (u, None)).collect();
    }
    let mut out = Vec::new();
    for (cid, gi) in order {
        let Some(&ci) = by_id.get(&cid) else { continue };
        let cue = &cues[ci];
        let group = gi.map(|g| &groups[g]);
        let label = group.map(|g| g.1.clone()).filter(|n| !n.is_empty()).or_else(|| string(cue, 2).filter(|n| !n.trim().is_empty())).unwrap_or_default();
        let mut slide_v: Option<Value> = None;
        let mut media_v: Option<Value> = None;
        for action in msgs(cue, 10) {
            if let Some(base) = msg(&action, 23).and_then(|st| msg(&st, 2)).and_then(|ps| msg(&ps, 1)) {
                let (layers, canvas, words) = slide(&base, true, ctx);
                slide_v = Some(json!({ "layers": layers, "canvasSize": canvas, "text": words }));
            } else if let Some(m) = msg(&action, 20).and_then(|mt| msg(&mt, 5)) {
                media_v = media(&m, ctx);
            }
        }
        let label = if label.is_empty() { format!("Slide {}", out.len() + 1) } else { label };
        let mut block = match (slide_v, media_v) {
            (Some(s), _) => json!({ "label": label, "layers": s["layers"], "canvasSize": s["canvasSize"], "text": s["text"] }),
            (None, Some(m)) => json!({ "label": label, "lines": [], "image": m["src"] }),
            _ => continue,
        };
        if let Some(g) = group {
            if !g.2.is_null() { block["groupColor"] = g.2.clone(); }
        }
        out.push(block);
    }
    Some(out)
}

/// rv.data.PlaylistDocument → (name, document file name) of every presentation, in order.
pub fn playlist_items(buf: &[u8]) -> Vec<(String, String)> {
    fn walk(p: &[Field<'_>], out: &mut Vec<(String, String)>, depth: usize) {
        if depth > 16 { return; }
        if let Some(items) = msg(p, 13) {
            for it in msgs(&items, 1) {
                let name = string(&it, 2).unwrap_or_default();
                if let Some(pres) = msg(&it, 4) {
                    let path = msg(&pres, 1).and_then(|u| msg(&u, 4).and_then(|l| string(&l, 2)).or_else(|| string(&u, 1))).unwrap_or_default();
                    out.push((name, crate::percent_decode(path.rsplit('/').next().unwrap_or(""))));
                }
            }
        }
        if let Some(arr) = msg(p, 12) {
            for child in msgs(&arr, 1) { walk(&child, out, depth + 1); }
        }
        for child in msgs(p, 9) { walk(&child, out, depth + 1); }
    }
    let mut out = Vec::new();
    if let Some(root) = pb_fields(buf) {
        if let Some(node) = msg(&root, 3) { walk(&node, &mut out, 0); }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cocoa_rtf_runs_and_paragraphs() {
        let rtf = r"{\rtf1\ansi\ansicpg1252\cocoartf2907
\cocoatextscaling0\cocoaplatform0{\fonttbl\f0\fnil\fcharset0 AstridRegular;\f1\fswiss\fcharset0 Helvetica;}
{\colortbl;\red255\green255\blue255;\red255\green0\blue0;}
{\*\expandedcolortbl;;\cssrgb\c100000\c0\c0\c50000;}
\deftab1680
\pard\pardeftab1680\sl192\slmult1\pardirnatural\qc\partightenfactor0

\f0\fs204 \cf2 \kerning1\expnd16\expndtw80
\CocoaLigature0 Glory to \b God\b0 \'96 it\'92s\
second line}";
        let p = parse_rtf(rtf);
        let paras = p.as_array().unwrap();
        assert_eq!(paras.len(), 2, "{p}");
        assert_eq!(paras[0]["align"], "center");
        assert!((paras[0]["lineMult"].as_f64().unwrap() - 0.8).abs() < 1e-9);
        let r0 = &paras[0]["runs"][0];
        assert_eq!(r0["font"], "AstridRegular");
        assert_eq!(r0["size"], 102.0);
        assert_eq!(r0["kern"], 4.0);
        assert_eq!(r0["color"], json!([1.0, 0.0, 0.0, 0.5]));
        assert_eq!(r0["noLig"], true);
        assert_eq!(paras[0]["runs"][1]["t"], "God");
        assert_eq!(paras[0]["runs"][1]["b"], true);
        assert_eq!(plain_text(&p), "Glory to God– it’s\nsecond line"); // the space after \b0 is its delimiter
    }

    #[test]
    fn unicode_escapes() {
        let bs = '\\';
        let src = format!("{{{bs}rtf1{{{bs}fonttbl{bs}f0 Arial;}}{bs}f0{bs}uc0{bs}u8220 Hi{bs}u8221 }}");
        assert_eq!(plain_text(&parse_rtf(&src)), "\u{201c}Hi\u{201d}");
    }
}
