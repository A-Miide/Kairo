//! Where the Rust parser reads real sermon transcripts differently from the
//! Node parser. Not an equality test — the rebuild changes some readings on
//! purpose — but every difference is printed so each one can be judged.
//! Development sermons only (docs/RUST_REWRITE_PLAN.md, "Evaluation split").
//!
//!   node parity/dump_parser.js > parity/parser_node.jsonl
//!   cargo test -p kairo-detect --test reference_vs_node -- --ignored --nocapture

use std::path::PathBuf;

use kairo_bible::Corpus;
use kairo_detect::reference::{Lang, Active, Books, Citation, Parser, Shape, Verses};
use serde::Deserialize;

#[derive(Deserialize)]
struct Line {
    src: String,
    text: String,
    refs: Vec<String>,
}

fn fmt(c: &Citation) -> String {
    let v = match &c.verses {
        Verses::Chapter => String::new(),
        Verses::One(v) => v.to_string(),
        Verses::Span(a, z) => format!("{a}-{z}"),
        Verses::Spans(s) => s.iter().map(|(a, z)| if a == z { a.to_string() } else { format!("{a}-{z}") }).collect::<Vec<_>>().join("+"),
    };
    let alt = c.alt_chapter.map(|a| format!(" or {a}")).unwrap_or_default();
    format!("{} {}:{v}{alt}", c.book, c.chapter)
}

#[test]
#[ignore = "needs parity/parser_node.jsonl"]
fn differences_from_node() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let shape = Shape::new(&Corpus::load(&root.join("databases/bibles")).unwrap());
    let parser = Parser { books: Books::get(), shape: &shape, lang: Lang::En };
    let dump = std::fs::read_to_string(root.join("parity/parser_node.jsonl")).expect("run parity/dump_parser.js first");
    let (mut same, mut differ) = (0, 0);
    for line in dump.lines() {
        let l: Line = serde_json::from_str(line).unwrap();
        let ours: Vec<String> = parser.parse_all(&l.text, Active::default()).iter().map(fmt).collect();
        if ours == l.refs {
            same += 1;
        } else {
            differ += 1;
            println!("[{}] {:?}\n    node: {:?}\n    rust: {:?}", l.src, l.text, l.refs, ours);
        }
    }
    println!("\n{same} lines read the same, {differ} differently");
}
