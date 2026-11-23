//! What Kairo knows about the Bibles it matches, from the tables detection itself builds at load
//! (kairo-detect knowledge.rs, facts.rs, the lexicon's twins and look-alikes): one report, so the
//! analysis and the detector can't drift apart. Counts and references only, no Bible text.
//!
//! `cargo run --release -p kairo-eval --bin bible-profile -- [--bibles <folder>] [--examples 10]`
//! Builds detection as the app does for an English service: the KJV, the BSB and every flat
//! `<CODE>.json` Bible in `--bibles` (default: the Kairo app's translations folder on this Mac).

use std::collections::HashSet;
use std::path::PathBuf;

use kairo_bible::{Corpus, VerseId};
use kairo_detect::facts::tokens;
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_eval::root;

fn label(k: &Knowledge, v: VerseId) -> String {
    let r = k.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let examples: usize = opt("--examples").and_then(|s| s.parse().ok()).unwrap_or(10);
    let installed = opt("--bibles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/com.kairo.scripture/translations"));
    let dir = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).expect("kjv");
    let mut also = vec![corpus.load_flat("BSB", &dir.join("translations/BSB.json")).expect("BSB")];
    let mut files: Vec<PathBuf> = std::fs::read_dir(&installed).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    files.sort();
    for f in &files {
        let code = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_uppercase();
        if let Ok(t) = corpus.load_flat(&code, f) {
            also.push(t);
        }
    }
    let t0 = std::time::Instant::now();
    let k = Knowledge::build(Corpus::load(&dir).expect("kjv"), Wordings { own: None, also });
    println!("Built detection's Bible knowledge in {:.1}s", t0.elapsed().as_secs_f32());

    // Per version: verses present, and how much of its wording the KJV never uses.
    let ids: Vec<VerseId> = k.corpus.ids().collect();
    let kjv_words: HashSet<String> = ids.iter().filter_map(|v| k.lexicon.kjv().text(*v)).flat_map(tokens).collect();
    println!("\n{:<6} {:>7} {:>8} {:>14}", "Bible", "verses", "missing", "words not KJV");
    for w in std::iter::once(k.lexicon.kjv()).chain(k.lexicon.others().iter()) {
        let present = ids.iter().filter(|v| w.text(**v).is_some()).count();
        let (mut all, mut unseen) = (0usize, 0usize);
        for v in &ids {
            for t in w.text(*v).map(tokens).unwrap_or_default() {
                all += 1;
                unseen += usize::from(!kjv_words.contains(&t));
            }
        }
        println!("{:<6} {:>7} {:>8} {:>13.1}%", w.code, present, ids.len() - present, unseen as f64 * 100.0 / all.max(1) as f64);
    }

    // The facts each detection rule reads.
    let (lookalikes, twins) = k.lexicon.alike_counts();
    let (numbers, openings, book_words, short) = k.facts.counts();
    println!("\nAcross every Bible above:");
    println!("  look-alike pairs (a word may differ)      {lookalikes:>7}   held back while the other is read or up");
    println!("  twin pairs (KJV and church's own)          {twins:>7}   used to tell twins apart");
    println!("  verses with numbers in their words         {numbers:>7}   a number read isn't a verse called");
    println!("  verses opening like the verse before       {openings:>7}   move on only after that verse ends");
    println!("  verses with a word read as a book name     {book_words:>7}   not a citation while being read");
    println!("  verses of four words or fewer              {short:>7}   read once up");

    let pick = |f: &dyn Fn(VerseId) -> bool| ids.iter().copied().filter(|v| f(*v)).take(examples).map(|v| label(&k, v)).collect::<Vec<_>>().join(", ");
    println!("\nExamples:");
    println!("  look-alikes:  {}", ids.iter().filter_map(|v| k.lexicon.lookalikes(*v).iter().find(|o| o.0 > v.0 + 1).map(|o| format!("{} ~ {}", label(&k, *v), label(&k, *o)))).take(examples).collect::<Vec<_>>().join(", "));
    println!("  numbers:      {}", pick(&|v| !k.facts.numbers(v).is_empty()));
    println!("  openings:     {}", pick(&|v| k.facts.opening_inside_before(v)));
    println!("  book words:   {}", pick(&|v| !k.facts.book_words(v).is_empty()));
    println!("  short:        {}", pick(&|v| k.facts.short(v)));
}
