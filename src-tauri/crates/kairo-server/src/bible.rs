//! Verse lookups and the verse objects the UI renders, in the wording the
//! church has chosen.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kairo_bible::{Book, Translation, VerseId, VerseRef};
use kairo_detect::knowledge::Knowledge;
use serde_json::{json, Value};

/// reference → text, for one translation or language pack (None: failed to load).
type Pack = Option<Arc<HashMap<String, String>>>;

pub struct Bible {
    pub k: Arc<Knowledge>,
    pub nlt: Translation,
    db_dir: PathBuf,
    /// Packs downloaded by the user (app data folder); looked at before the ones inside the app.
    user_packs: PathBuf,
    translations: Mutex<HashMap<String, Pack>>,
    languages: Mutex<HashMap<String, Pack>>,
}

impl Bible {
    pub fn new(k: Arc<Knowledge>, nlt: Translation, db_dir: PathBuf, user_packs: PathBuf) -> Bible {
        Bible { k, nlt, db_dir, user_packs, translations: Mutex::new(HashMap::new()), languages: Mutex::new(HashMap::new()) }
    }

    pub fn reference(&self, v: VerseId) -> String {
        let r = self.k.verse_ref(v);
        format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
    }

    /// A verse by book name, chapter and verse.
    pub fn find(&self, book: &str, chapter: u64, verse: u64) -> Option<VerseId> {
        let b = Book::from_name(book)?;
        self.k.id(VerseRef::new(b, u16::try_from(chapter).ok()?, u16::try_from(verse).ok()?))
    }

    pub fn chapter(&self, book: &str, chapter: u64) -> Vec<VerseId> {
        let Some(b) = Book::from_name(book) else { return Vec::new() };
        let Ok(c) = u16::try_from(chapter) else { return Vec::new() };
        let n = self.k.corpus.verse_count(b, c).unwrap_or(0);
        (1..=n).filter_map(|v| self.k.id(VerseRef::new(b, c, v))).collect()
    }

    pub fn range(&self, book: &str, chapter: u64, from: u64, to: u64) -> Vec<VerseId> {
        (from..=to.min(from + 200)).filter_map(|v| self.find(book, chapter, v)).collect()
    }

    /// The verse as the UI knows it: KJV text, the NLT beside it.
    pub fn verse_json(&self, v: VerseId, similarity: f32, method: &str) -> Value {
        let r = self.k.verse_ref(v);
        json!({
            "reference": self.reference(v),
            "text": self.k.corpus.kjv().text(v).unwrap_or(""),
            "nlt_text": self.nlt.text(v).unwrap_or(""),
            "book": r.book.name(),
            "chapter": r.chapter,
            "verse": r.verse,
            "similarity": similarity,
            "method": method,
        })
    }

    fn pack(&self, cache: &Mutex<HashMap<String, Pack>>, key: &str, path: PathBuf) -> Pack {
        if let Some(p) = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(key) {
            return p.clone();
        }
        let loaded = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|raw| {
            let map: HashMap<String, String> = raw["verses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| Some((v["reference"].as_str()?.to_string(), v["text"].as_str()?.to_string())))
                .collect();
            eprintln!("[Translation] Loaded {key}: {} verses", map.len());
            Arc::new(map)
        });
        if loaded.is_none() {
            eprintln!("[Translation] No pack for \"{key}\"");
        }
        cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key.to_string(), loaded.clone());
        loaded
    }

    /// The church's translation in place of the KJV text (KJV: as is; NLT: the NLT).
    pub fn apply_translation(&self, verses: &mut [Value], code: &str) {
        let code = code.to_uppercase();
        if code == "KJV" || code.is_empty() {
            return;
        }
        if code == "NLT" {
            for v in verses.iter_mut() {
                if let Some(t) = v["nlt_text"].as_str().filter(|t| !t.is_empty()).map(str::to_string) {
                    v["text"] = json!(t);
                }
            }
            return;
        }
        let Some(pack) = self.pack(&self.translations, &code, self.db_dir.join("translations").join(format!("{code}.json"))) else { return };
        replace_text(verses, &pack);
    }

    /// Where a language's Bible is: a downloaded copy if there is one, else the one inside the app.
    pub fn pack_path(&self, lang: &str) -> PathBuf {
        let downloaded = self.user_packs.join(format!("{lang}.json"));
        if downloaded.is_file() { downloaded } else { self.db_dir.join("packs").join(format!("{lang}.json")) }
    }

    pub fn user_packs_dir(&self) -> &std::path::Path {
        &self.user_packs
    }

    pub fn bundled_pack(&self, lang: &str) -> bool {
        self.db_dir.join("packs").join(format!("{lang}.json")).is_file()
    }

    /// Drops what was loaded for a language, so a pack just added or removed is read again.
    pub fn forget_language(&self, lang: &str) {
        self.languages.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(lang);
    }

    /// The service language's Bible in place of the English text.
    pub fn apply_language(&self, verses: &mut [Value], lang: &str) {
        if lang.is_empty() || lang == "en" {
            return;
        }
        let Some(pack) = self.pack(&self.languages, lang, self.pack_path(lang)) else { return };
        replace_text(verses, &pack);
    }

    /// Verses whose wording (KJV or another matched wording) contains the phrase.
    pub fn text_search(&self, query: &str, limit: usize) -> Vec<VerseId> {
        let q = kairo_detect::lexical::text::norm(query);
        if q.is_empty() {
            return Vec::new();
        }
        let lex = &self.k.lexicon;
        let has = |w: &kairo_detect::lexical::lexicon::Wording, v: VerseId| w.text(v).is_some_and(|t| kairo_detect::lexical::text::norm(t).contains(&q));
        self.k.corpus.ids().filter(|v| has(lex.kjv(), *v) || lex.others().iter().any(|o| has(o, *v))).take(limit).collect()
    }
}

fn replace_text(verses: &mut [Value], pack: &HashMap<String, String>) {
    for v in verses.iter_mut() {
        if let Some(t) = v["reference"].as_str().and_then(|r| pack.get(r)).cloned() {
            v["text"] = json!(t);
        }
    }
}
