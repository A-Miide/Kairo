//! Prints what the parser reads in each argument: `cargo run --example parse -- "John 3 16"`.
use std::path::PathBuf;

use kairo_bible::Corpus;
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::{Active, Lang};

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    let k = Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] });
    let p = k.parser(Lang::En);
    for text in std::env::args().skip(1) {
        println!("{text:?}\n  prepared: {:?}\n  {:?}", p.prepare(&text), p.parse_all(&text, Active::default()));
    }
}
