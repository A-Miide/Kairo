//! Words the speech engines are told to listen for — one list for every
//! engine: the church's own terms, the books of the Bible, the names it
//! mentions most, and the words of citing it.

use kairo_bible::Book;

use crate::Shared;

const CHURCH_VOCAB_MAX: usize = 40;
const NAMES: usize = 50;
const CITING: &[&str] = &["Psalm", "Chapter", "Verse", "Scripture", "brethren", "righteous", "salvation", "covenant"];

/// The church's own terms from settings: up to 40, each at most 4 words.
pub async fn church(app: &Shared) -> Vec<String> {
    let raw = app.settings.str("customKeyterms").await.unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for t in raw.split(['\n', ',']) {
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() && t.chars().count() <= 40 && t.split(' ').count() <= 4 && !out.contains(&t) {
            out.push(t);
        }
    }
    out.truncate(CHURCH_VOCAB_MAX);
    out
}

/// Everything, for English services (other languages get the church's terms only from the engine itself).
pub async fn terms(app: &Shared, lang: &str) -> Vec<String> {
    if !lang.starts_with("en") && lang != "multi" {
        return Vec::new();
    }
    let mut out = church(app).await;
    let mut push = |t: String| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    for b in Book::all() {
        push(b.name().trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ').to_string());
    }
    for w in CITING {
        push(w.to_string());
    }
    if let Some((bible, _)) = app.ready() {
        for n in bible.k.frequent_names(NAMES) {
            let mut c = n.chars();
            push(c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default());
        }
    }
    out
}
