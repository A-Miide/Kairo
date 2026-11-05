//! What Kairo learns from this church's services (kairo_detect::engine::learned), kept between
//! services in app data (learned.json), on this computer only. Nothing to set up: it learns from
//! what the operator does anyway, putting up the verse the preacher meant or pressing Back on a
//! wrong one. Settings → Detection → Forget starts it over.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kairo_bible::{Book, VerseId};
use serde::{Deserialize, Serialize};

use crate::bible::Bible;
use crate::detect::Detector;

/// A verse up on this many different days is one the church comes back to.
const FAMILIAR_DAYS: usize = 3;
/// Taken back down this many times after going up on wording alone: offered, not sent.
const TAKEN_DOWN_TIMES: u32 = 2;
/// Speech this recent may hold the words for the verse the operator put up.
const RECENT_MS: u64 = 25_000;
/// Words that are part of how a citation is said, never a book.
const CITATION_WORDS: &[&str] = &["verse", "verses", "chapter", "chapters", "to", "through", "and", "from", "number"];

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    /// Heard word → book ("san" → "Psalms").
    book_words: BTreeMap<String, String>,
    /// "Book|chapter|verse" → the days it was up (YYYY-MM-DD), newest kept.
    shown: BTreeMap<String, Vec<String>>,
    /// "Book|chapter|verse" → times taken back down after going up on wording alone.
    taken_down: BTreeMap<String, u32>,
}

struct Store {
    path: PathBuf,
    data: Data,
    bible: Arc<Bible>,
    detector: Detector,
    heard: VecDeque<(u64, String)>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn key(bible: &Bible, v: VerseId) -> String {
    let r = bible.k.verse_ref(v);
    format!("{}|{}|{}", r.book.name(), r.chapter, r.verse)
}

fn verse(bible: &Bible, key: &str) -> Option<VerseId> {
    let mut p = key.split('|');
    bible.find(p.next()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?)
}

/// Loads what was learned and hands it to the detector.
pub fn start(app_data: &std::path::Path, bible: Arc<Bible>, detector: Detector) {
    let path = app_data.join("learned.json");
    let data = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let store = Store { path, data, bible, detector, heard: VecDeque::new() };
    store.apply();
    *crate::util::lock(&STORE) = Some(store);
}

impl Store {
    fn apply(&self) {
        let b = &self.bible;
        let book_words: HashMap<String, Book> = self.data.book_words.iter().filter_map(|(w, n)| Some((w.clone(), Book::from_name(n)?))).collect();
        let familiar: HashSet<VerseId> = self.data.shown.iter().filter(|(_, days)| days.len() >= FAMILIAR_DAYS).filter_map(|(k, _)| verse(b, k)).collect();
        let taken_down: HashSet<VerseId> = self.data.taken_down.iter().filter(|(_, n)| **n >= TAKEN_DOWN_TIMES).filter_map(|(k, _)| verse(b, k)).collect();
        let learned = Arc::new(kairo_detect::engine::Learned::new(book_words, familiar, taken_down));
        self.detector.configure(move |c| c.learned = learned);
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_vec_pretty(&self.data) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }
}

fn with(f: impl FnOnce(&mut Store)) {
    if let Some(s) = crate::util::lock(&STORE).as_mut() {
        f(s);
    }
}

/// A finished line of speech.
pub fn heard(text: &str) {
    let now = crate::util::epoch_ms();
    with(|s| {
        s.heard.push_back((now, text.to_string()));
        while s.heard.front().is_some_and(|(t, _)| now.saturating_sub(*t) > RECENT_MS) {
            s.heard.pop_front();
        }
    });
}

/// The operator put `v` up (search, a click): if the speech just before said its chapter after
/// a word that isn't one Kairo knows, that word is this preacher's book ("San one ten").
pub fn operator_put_up(v: VerseId) {
    with(|s| {
        let r = s.bible.k.verse_ref(v);
        let text: String = s.heard.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(" ");
        let vocab = s.bible.k.lexicon.vocab();
        let known = |w: &str| CITATION_WORDS.contains(&w) || vocab.id(w).is_some() || Book::from_name(w).is_some();
        let Some(word) = kairo_detect::engine::learned::book_word(&text, r.chapter as u32, known) else { return };
        let book = r.book.name().to_string();
        if s.data.book_words.get(&word) != Some(&book) {
            eprintln!("[Learned] \"{word}\" is this preacher's {book}");
            s.data.book_words.insert(word, book);
            s.save();
            s.apply();
        }
    });
}

/// `v` went up on screen today.
pub fn shown(v: VerseId) {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    with(|s| {
        let days = s.data.shown.entry(key(&s.bible, v)).or_default();
        if days.last() == Some(&today) {
            return;
        }
        days.push(today);
        if days.len() > 12 {
            days.remove(0);
        }
        let became_familiar = days.len() == FAMILIAR_DAYS;
        s.save();
        if became_familiar {
            s.apply();
        }
    });
}

/// The operator took `v` back down soon after it went up by itself, brought up by `method`.
pub fn taken_down(v: VerseId, method: &str) {
    // Cited by reference: the preacher named it, and that's never held back.
    if matches!(method, "direct" | "direct-partial" | "context-citation" | "chapter-keyword" | "continuation" | "named-passage" | "reference" | "range") {
        return;
    }
    with(|s| {
        let n = s.data.taken_down.entry(key(&s.bible, v)).or_default();
        *n += 1;
        let now_held = *n == TAKEN_DOWN_TIMES;
        s.save();
        if now_held {
            s.apply();
        }
    });
}

/// Settings → Forget what Kairo learned.
pub fn forget() {
    with(|s| {
        s.data = Data::default();
        s.save();
        s.apply();
    });
}

/// For Settings: how much has been learned.
pub fn summary() -> serde_json::Value {
    let mut out = serde_json::json!({ "bookWords": 0, "familiar": 0, "heldBack": 0 });
    with(|s| {
        out = serde_json::json!({
            "bookWords": s.data.book_words.len(),
            "familiar": s.data.shown.values().filter(|d| d.len() >= FAMILIAR_DAYS).count(),
            "heldBack": s.data.taken_down.values().filter(|n| **n >= TAKEN_DOWN_TIMES).count(),
        });
    });
    out
}
