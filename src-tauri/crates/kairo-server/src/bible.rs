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
            let hebrew = hebrew_psalms(&raw);
            let map: HashMap<String, String> = raw["verses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| {
                    let reference = v["reference"].as_str()?;
                    let text = v["text"].as_str()?.to_string();
                    if !hebrew {
                        return Some((reference.to_string(), text));
                    }
                    // Hebrew numbering: the title is verse 1 (or 1 and 2), so the KJV verse is lower.
                    Some((kjv_reference(reference, &v["book"], v["chapter"].as_u64()?, v["verse"].as_u64()?)?, text))
                })
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

    /// One verse's text in a language's Bible (a downloaded or bundled pack), if it has it.
    pub fn pack_verse(&self, lang: &str, book: &str, chapter: u64, verse: u64) -> Option<String> {
        let id = self.find(book, chapter, verse)?;
        let pack = self.pack(&self.languages, lang, self.pack_path(lang))?;
        pack.get(&self.reference(id)).cloned()
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

/// Psalms whose title counts as verse 1 (or verses 1 and 2) in the Hebrew numbering many
/// European Bibles follow; the KJV leaves the title unnumbered. (chapter, extra verses)
const PSALM_TITLE_VERSES: [(u64, u64); 62] = [
    (3, 1), (4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 1), (12, 1), (18, 1), (19, 1), (20, 1), (21, 1), (22, 1), (30, 1), (31, 1),
    (34, 1), (36, 1), (38, 1), (39, 1), (40, 1), (41, 1), (42, 1), (44, 1), (45, 1), (46, 1), (47, 1), (48, 1), (49, 1), (51, 2),
    (52, 2), (53, 1), (54, 2), (55, 1), (56, 1), (57, 1), (58, 1), (59, 1), (60, 2), (61, 1), (62, 1), (63, 1), (64, 1), (65, 1),
    (67, 1), (68, 1), (69, 1), (70, 1), (75, 1), (76, 1), (77, 1), (80, 1), (81, 1), (83, 1), (84, 1), (85, 1), (88, 1), (89, 1),
    (92, 1), (102, 1), (108, 1), (140, 1), (142, 1),
];

/// Does this Bible number the Psalms the Hebrew way? A pack can say so ("versification":
/// "hebrew" or "english"); otherwise Psalm 3's verse 1 gives it away: in the Hebrew numbering
/// it is the title, which names Absalom in every language.
fn hebrew_psalms(raw: &Value) -> bool {
    match raw["versification"].as_str() {
        Some("hebrew") => return true,
        Some("english") => return false,
        _ => {}
    }
    raw["verses"].as_array().into_iter().flatten().find(|v| {
        v["book"].as_str().is_some_and(|b| b.starts_with("Psalm")) && v["chapter"].as_u64() == Some(3) && v["verse"].as_u64() == Some(1)
    }).and_then(|v| v["text"].as_str()).is_some_and(|t| t.to_lowercase().contains("bsal"))
}

/// The KJV reference for a verse numbered the Hebrew way, or None for a title verse.
fn kjv_reference(reference: &str, book: &Value, chapter: u64, verse: u64) -> Option<String> {
    if !book.as_str().is_some_and(|b| b.starts_with("Psalm")) {
        return Some(reference.to_string());
    }
    let extra = PSALM_TITLE_VERSES.iter().find(|p| p.0 == chapter).map_or(0, |p| p.1);
    if extra == 0 {
        return Some(reference.to_string());
    }
    let kjv = verse.checked_sub(extra).filter(|v| *v > 0)?;
    let (head, _) = reference.rsplit_once(':')?;
    Some(format!("{head}:{kjv}"))
}

fn replace_text(verses: &mut [Value], pack: &HashMap<String, String>) {
    for v in verses.iter_mut() {
        if let Some(t) = v["reference"].as_str().and_then(|r| pack.get(r)).cloned() {
            v["text"] = json!(t);
        }
    }
}

#[cfg(test)]
mod psalm_numbering {
    use super::*;

    #[test]
    fn title_verses_shift_down() {
        let b = json!("Psalms");
        assert_eq!(kjv_reference("Psalms 18:29", &b, 18, 29).as_deref(), Some("Psalms 18:28"));
        assert_eq!(kjv_reference("Psalms 18:1", &b, 18, 1), None);
        assert_eq!(kjv_reference("Psalms 51:3", &b, 51, 3).as_deref(), Some("Psalms 51:1"));
        assert_eq!(kjv_reference("Psalms 23:1", &b, 23, 1).as_deref(), Some("Psalms 23:1"));
        assert_eq!(kjv_reference("John 3:16", &json!("John"), 3, 16).as_deref(), Some("John 3:16"));
    }

    #[test]
    fn detects_hebrew_numbering() {
        let he = json!({ "verses": [{ "book": "Psalms", "chapter": 3, "verse": 1, "text": "Ein Psalm Davids, da er floh vor seinem Sohn Absalom." }] });
        let en = json!({ "verses": [{ "book": "Psalms", "chapter": 3, "verse": 1, "text": "LORD, how are they increased that trouble me!" }] });
        assert!(hebrew_psalms(&he));
        assert!(!hebrew_psalms(&en));
        assert!(!hebrew_psalms(&json!({ "versification": "english", "verses": he["verses"].clone() })));
    }
}
