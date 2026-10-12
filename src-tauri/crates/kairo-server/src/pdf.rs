//! A small PDF writer for the sermon notes: Letter pages, the standard
//! Helvetica faces (no fonts to embed), wrapped text, rules, page breaks.

use crate::pdf_metrics::WIDTHS;

pub const PAGE_W: f32 = 612.0;
pub const PAGE_H: f32 = 792.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Regular,
    Bold,
    Oblique,
}

impl Face {
    fn resource(self) -> &'static str {
        match self {
            Face::Regular => "F1",
            Face::Bold => "F2",
            Face::Oblique => "F3",
        }
    }
}

/// A Windows-1252 code for a character the standard fonts can show.
fn code(c: char) -> Option<(u8, [u16; 3])> {
    WIDTHS.iter().find(|w| w.1 == c as u32).map(|w| (w.0, [w.2, w.3, w.4]))
}

/// Characters the fonts lack, as the nearest ones they have.
fn plain(c: char) -> char {
    match c {
        '\u{2010}'..='\u{2012}' => '-',
        '\u{2032}' => '\'',
        '\u{2033}' => '"',
        '\u{00A0}' | '\u{2009}' | '\u{202F}' => ' ',
        c => c,
    }
}

pub fn text_width(s: &str, face: Face, size: f32) -> f32 {
    let i = face as usize;
    s.chars().map(|c| code(plain(c)).map_or(556, |(_, w)| w[i]) as f32).sum::<f32>() * size / 1000.0
}

fn encode(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        let b = code(plain(c)).map_or(b'?', |(b, _)| b);
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

/// Words into lines no wider than `width`; a word longer than a line is broken.
pub fn wrap(text: &str, face: Face, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&candidate, face, size) <= width || line.is_empty() && text_width(word, face, size) <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if text_width(word, face, size) <= width {
                line = word.to_string();
            } else {
                for c in word.chars() {
                    if text_width(&format!("{line}{c}"), face, size) > width && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(c);
                }
            }
        }
        lines.push(line);
    }
    lines
}

pub struct Rgb(pub f32, pub f32, pub f32);

pub fn hex(h: &str) -> Rgb {
    let v = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0);
    Rgb(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0)
}

/// A document laid out top to bottom, starting new pages as needed.
pub struct Doc {
    pages: Vec<String>,
    pub y: f32,
    pub left: f32,
    pub width: f32,
    top: f32,
    bottom: f32,
    title: String,
}

impl Doc {
    pub fn new(title: &str, margin_x: f32, margin_y: f32) -> Doc {
        Doc { pages: vec![String::new()], y: margin_y, left: margin_x, width: PAGE_W - 2.0 * margin_x, top: margin_y, bottom: margin_y, title: title.into() }
    }

    fn page(&mut self) -> &mut String {
        self.pages.last_mut().unwrap()
    }

    pub fn new_page(&mut self) {
        self.pages.push(String::new());
        self.y = self.top;
    }

    /// A new page unless `h` more points fit on this one.
    pub fn keep_room(&mut self, h: f32) {
        if self.y + h > PAGE_H - self.bottom {
            self.new_page();
        }
    }

    pub fn space(&mut self, h: f32) {
        self.y += h;
    }

    /// Height wrapped text would take.
    pub fn height_of(&self, text: &str, face: Face, size: f32, width: f32, gap: f32) -> f32 {
        wrap(text, face, size, width).len() as f32 * (size * 1.2 + gap)
    }

    /// Wrapped text at `x` (from the left margin) in `width`; lines break across pages.
    pub fn text(&mut self, text: &str, face: Face, size: f32, color: &Rgb, x: f32, width: f32, gap: f32) {
        let line_h = size * 1.2 + gap;
        for line in wrap(text, face, size, width) {
            self.keep_room(line_h);
            let baseline = PAGE_H - self.y - size;
            let cmd = format!("BT /{} {size} Tf {} {} {} rg {:.2} {:.2} Td {} Tj ET\n", face.resource(), color.0, color.1, color.2, self.left + x, baseline, encode(&line));
            self.page().push_str(&cmd);
            self.y += line_h;
        }
    }

    /// Text on one line at an exact place (no wrapping, no page break).
    pub fn text_at(&mut self, text: &str, face: Face, size: f32, color: &Rgb, x: f32, y: f32) {
        let cmd = format!("BT /{} {size} Tf {} {} {} rg {:.2} {:.2} Td {} Tj ET\n", face.resource(), color.0, color.1, color.2, x, PAGE_H - y - size, encode(text));
        self.page().push_str(&cmd);
    }

    pub fn rule(&mut self, width: f32, color: &Rgb, thickness: f32) {
        let y = PAGE_H - self.y;
        let cmd = format!("{} {} {} RG {thickness} w {:.2} {:.2} m {:.2} {:.2} l S\n", color.0, color.1, color.2, self.left, y, self.left + width, y);
        self.page().push_str(&cmd);
    }

    pub fn bottom_margin(&self) -> f32 {
        self.bottom
    }

    pub fn finish(self) -> Vec<u8> {
        let mut objs: Vec<String> = Vec::new();
        // 1 catalog, 2 pages, 3–5 fonts, 6 info, then (content, page) pairs.
        let n = self.pages.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 8 + 2 * i)).collect();
        objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
        for f in ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique"] {
            objs.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{f} /Encoding /WinAnsiEncoding >>"));
        }
        objs.push(format!("<< /Title {} /Creator (Kairo) /Producer (Kairo) >>", encode(&self.title)));
        objs.push("<< >>".into()); // 7: unused, keeps page numbering simple
        for content in &self.pages {
            objs.push(format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
            let id = objs.len();
            objs.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {id} 0 R >>"
            ));
        }
        let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objs.len());
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_wrapping() {
        assert!((text_width("Hello", Face::Regular, 10.0) - 22.78).abs() < 0.01);
        let lines = wrap("The Lord is my shepherd; I shall not want.", Face::Regular, 12.0, 120.0);
        assert!(lines.len() > 1 && lines.iter().all(|l| text_width(l, Face::Regular, 12.0) <= 120.0));
        assert_eq!(encode("“Hi” (x)"), "(\\223Hi\\224 \\(x\\))");
    }

    #[test]
    fn a_document_is_a_pdf() {
        let mut d = Doc::new("Notes", 64.0, 60.0);
        d.text("Hello", Face::Bold, 24.0, &hex("#1d1d1f"), 0.0, 400.0, 0.0);
        let b = d.finish();
        assert!(b.starts_with(b"%PDF-1.4") && b.ends_with(b"%%EOF\n"));
    }
}
