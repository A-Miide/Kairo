//! The word matchers against Node's on the same real transcript text
//! (`parity/lexical_node.jsonl`, written by `parity/dump_lexical.js`).
//!
//!   cargo test --release -p kairo-detect --test lexical_vs_node -- --ignored --nocapture

use std::path::PathBuf;

use kairo_bible::{Book, Corpus, VerseId, VerseRef};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::{fingerprint, verbatim, Confidence, Wordings};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn label(k: &Knowledge, v: VerseId) -> String {
    let r = k.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

fn parse_ref(k: &Knowledge, s: &str) -> Option<VerseId> {
    let (book, cv) = s.rsplit_once(' ')?;
    let book = Book::from_name(book).or_else(|| Book::from_name(&format!("{book}s")))?;
    let (c, v) = cv.split_once(':')?;
    k.id(VerseRef::new(book, c.parse().ok()?, v.parse().ok()?))
}

#[test]
#[ignore]
fn word_matchers_agree_with_node() {
    let dir = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).unwrap();
    let bsb = corpus.load_flat("BSB", &dir.join("translations/BSB.json")).unwrap();
    let k = Knowledge::build(corpus, Wordings { own: None, also: vec![bsb] });
    let raw = std::fs::read_to_string(root().join("parity/lexical_node.jsonl")).expect("run parity/dump_lexical.js first");

    let (mut n, mut v_same, mut v_score, mut f_same, mut f_conf) = (0, 0, 0, 0, 0);
    let mut shown = (0, 0);
    for line in raw.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let text = row["text"].as_str().unwrap();
        n += 1;

        let node_v: Vec<(Option<VerseId>, f64)> = row["verbatim"].as_array().unwrap().iter().map(|r| (parse_ref(&k, r[0].as_str().unwrap()), r[1].as_f64().unwrap())).collect();
        let rust_v = verbatim(&k.lexicon, text, 6, 3);
        let top_same = match (node_v.first(), rust_v.first()) {
            (None, None) => true,
            (Some((a, _)), Some(b)) => *a == Some(b.verse),
            _ => false,
        };
        if top_same {
            v_same += 1;
            if node_v.first().zip(rust_v.first()).is_none_or(|((_, s), b)| (s - b.score as f64).abs() < 0.01) {
                v_score += 1;
            }
        } else if shown.0 < 25 {
            shown.0 += 1;
            println!(
                "verbatim  {:?}\n   node: {:?}\n   rust: {:?}",
                text,
                row["verbatim"],
                rust_v.iter().map(|h| format!("{} {:.4} {:?}", label(&k, h.verse), h.score, h.matched_idf)).collect::<Vec<_>>()
            );
        }

        let node_f = &row["fingerprint"];
        let node_conf = node_f["confidence"].as_str().unwrap();
        let node_hits: Vec<Option<VerseId>> = node_f["hits"].as_array().unwrap().iter().map(|r| parse_ref(&k, r[0].as_str().unwrap())).collect();
        let rf = fingerprint(&k.lexicon, &k.corpus, text, 5, &[], None);
        let conf = match rf.confidence {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
            Confidence::None => "none",
        };
        if conf == node_conf {
            f_conf += 1;
        }
        let same = node_hits.first().copied().flatten() == rf.hits.first().map(|h| h.0) || (node_hits.is_empty() && rf.hits.is_empty());
        if same && conf == node_conf {
            f_same += 1;
        } else if shown.1 < 25 {
            shown.1 += 1;
            println!(
                "fingerprint  {:?}\n   node: {} {:?}\n   rust: {} {:?}",
                text,
                node_conf,
                node_f["hits"],
                conf,
                rf.hits.iter().map(|(v, s)| format!("{} {:.4}", label(&k, *v), s)).collect::<Vec<_>>()
            );
        }
    }
    println!("\n{n} texts: verbatim top verse same {v_same} ({:.1}%), same score {v_score}; fingerprint same top+confidence {f_same} ({:.1}%), same confidence {f_conf}", v_same as f64 * 100.0 / n as f64, f_same as f64 * 100.0 / n as f64);
}
