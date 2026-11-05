//! Words the speech engines are told to listen for — one list for every
//! engine: the church's own terms, the names learned from its services, the
//! books of the Bible, the names it mentions most, and the words of citing it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kairo_bible::Book;
use serde::{Deserialize, Serialize};

use crate::Shared;

const CHURCH_VOCAB_MAX: usize = 40;
const NAMES: usize = 50;
const CITING: &[&str] = &["Psalm", "Chapter", "Verse", "Scripture", "brethren", "righteous", "salvation", "covenant"];
/// Learned names used as hints: said in at least this many services, this often.
const LEARNED_MAX: usize = 20;
const LEARNED_MIN_SERVICES: u32 = 2;
const LEARNED_MIN_COUNT: u32 = 3;
/// A learned name not heard for this long is forgotten.
const LEARNED_FORGET_MS: u64 = 180 * 24 * 3_600_000;
const LEARNED_SAVE_EVERY: Duration = Duration::from_secs(30);
const LEARNED_FILE: &str = "learned-keyterms.json";
/// Capitalised for reasons other than being a name.
const NOT_NAMES: &[&str] = &[
    "I", "God", "Lord", "Jesus", "Christ", "Holy", "Spirit", "Ghost", "Amen", "Hallelujah", "Bible", "Father", "Son", "Church",
    "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday", "January", "February", "March", "April", "May",
    "June", "July", "August", "September", "October", "November", "December", "OK", "Okay", "Mr", "Mrs", "Dr", "Pastor", "Bishop",
];

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
    for t in learned(app) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
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

/// How often a name has been heard, and in how many services.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Seen {
    count: u32,
    services: u32,
    /// The service it was last heard in (its start, ms since the epoch).
    last_service: u64,
}

struct Learned {
    path: PathBuf,
    terms: HashMap<String, Seen>,
    dirty: bool,
    saved_at: Instant,
}

static LEARNED: Mutex<Option<Learned>> = Mutex::new(None);

fn with_learned<T>(app: &Shared, f: impl FnOnce(&mut Learned) -> T) -> T {
    let mut guard = LEARNED.lock().unwrap_or_else(|e| e.into_inner());
    let path = app.cfg.app_data_dir.join(LEARNED_FILE);
    if guard.as_ref().is_none_or(|l| l.path != path) {
        let terms = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        *guard = Some(Learned { path, terms, dirty: false, saved_at: Instant::now() });
    }
    f(guard.as_mut().expect("loaded above"))
}

fn save(l: &mut Learned) {
    if !l.dirty {
        return;
    }
    if let Ok(json) = serde_json::to_vec(&l.terms) {
        let tmp = l.path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &l.path).is_ok() {
            l.dirty = false;
        }
    }
    l.saved_at = Instant::now();
}

/// Names learned from past services that have earned a place as hints.
pub fn learned(app: &Shared) -> Vec<String> {
    let now = now_ms();
    with_learned(app, |l| {
        let mut ok: Vec<(&String, &Seen)> = l
            .terms
            .iter()
            .filter(|(_, s)| s.services >= LEARNED_MIN_SERVICES && s.count >= LEARNED_MIN_COUNT && now.saturating_sub(s.last_service) < LEARNED_FORGET_MS)
            .collect();
        ok.sort_by(|a, b| b.1.services.cmp(&a.1.services).then(b.1.count.cmp(&a.1.count)).then(a.0.cmp(b.0)));
        ok.into_iter().take(LEARNED_MAX).map(|(t, _)| t.clone()).collect()
    })
}

/// Every learned name with its counts, for Settings.
pub fn learned_all(app: &Shared) -> serde_json::Value {
    let in_use: std::collections::HashSet<String> = learned(app).into_iter().collect();
    with_learned(app, |l| {
        let mut all: Vec<_> = l.terms.iter().collect();
        all.sort_by(|a, b| b.1.services.cmp(&a.1.services).then(b.1.count.cmp(&a.1.count)).then(a.0.cmp(b.0)));
        serde_json::json!(all
            .into_iter()
            .filter(|(_, s)| s.services >= LEARNED_MIN_SERVICES)
            .map(|(t, s)| serde_json::json!({ "term": t, "count": s.count, "services": s.services, "inUse": in_use.contains(t) }))
            .collect::<Vec<_>>())
    })
}

/// Forgets a learned name (the operator removed it in Settings).
pub fn forget(app: &Shared, term: &str) {
    with_learned(app, |l| {
        if l.terms.remove(term).is_some() {
            l.dirty = true;
            save(l);
        }
    });
}

/// Takes the names out of a final transcript from a service that started at
/// `service` (ms since the epoch). Only words the engine capitalised mid-
/// sentence that the Bible doesn't use count — a church's own names.
pub fn observe(app: &Shared, text: &str, service: u64) {
    let Some((bible, _)) = app.ready() else { return };
    let biblical = |w: &str| bible.k.lexicon.idf_of(&w.to_lowercase()) > 0.0 || bible.k.is_name(&w.to_lowercase());
    let names = names_in(text, &biblical);
    if names.is_empty() {
        return;
    }
    with_learned(app, |l| {
        for n in names {
            let s = l.terms.entry(n).or_default();
            s.count = s.count.saturating_add(1);
            if s.last_service != service {
                s.services = s.services.saturating_add(1);
                s.last_service = service;
            }
        }
        l.dirty = true;
        if l.saved_at.elapsed() >= LEARNED_SAVE_EVERY {
            save(l);
        }
    });
}

/// Saves what was learned (at the end of a service).
pub fn flush(app: &Shared) {
    with_learned(app, save);
}

/// Runs of capitalised words not starting a sentence, at least one of them
/// not a Bible word: "Pastor Mensah" → "Mensah", "Winners Chapel".
fn names_in(text: &str, biblical: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut run: Vec<String> = Vec::new();
    let mut sentence_start = true;
    let flush_run = |run: &mut Vec<String>, out: &mut Vec<String>| {
        if !run.is_empty() && run.len() <= 3 && run.iter().any(|w| !biblical(w)) {
            let phrase = run.join(" ");
            if !out.contains(&phrase) {
                out.push(phrase);
            }
        }
        run.clear();
    };
    for raw in text.split_whitespace() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').trim_end_matches("'s");
        let ends = raw.ends_with(['.', '!', '?']);
        let capital = word.chars().next().is_some_and(char::is_uppercase) && word.chars().skip(1).all(|c| c.is_lowercase() || c == '\'' || c == '-');
        if capital && !sentence_start && word.chars().count() >= 3 && !NOT_NAMES.contains(&word) {
            run.push(word.to_string());
        } else {
            flush_run(&mut run, &mut out);
        }
        if raw.ends_with([',', ';', ':']) {
            flush_run(&mut run, &mut out);
        }
        sentence_start = ends;
        if ends {
            flush_run(&mut run, &mut out);
        }
    }
    flush_run(&mut run, &mut out);
    out
}

use crate::util::epoch_ms as now_ms;

#[cfg(test)]
mod tests {
    use super::names_in;

    #[test]
    fn names_are_mid_sentence_capitals_the_bible_does_not_use() {
        let bible = |w: &str| ["moses", "faith", "jerusalem"].contains(&w.to_lowercase().as_str());
        assert_eq!(names_in("Welcome to Winners Chapel this morning, Pastor Mensah will lead.", &bible), ["Winners Chapel", "Mensah"]);
        assert_eq!(names_in("Moses went up. Kumasi is far from Jerusalem.", &bible), Vec::<String>::new());
        assert_eq!(names_in("We meet at Faith Tabernacle on Sunday.", &bible), ["Faith Tabernacle"]);
        assert_eq!(names_in("God bless you. I said Amen to Jesus.", &bible), Vec::<String>::new());
    }
}
