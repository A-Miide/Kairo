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
/// A pack's own name ("Yoruba", "New International Version"), kept in its map under a key no
/// reference can be.
const PACK_NAME: &str = "\u{1}name";

pub struct Bible {
    pub k: Arc<Knowledge>,
    pub nlt: Translation,
    pub(crate) db_dir: PathBuf,
    /// Packs downloaded by the user (app data folder); looked at before the ones inside the app.
    user_packs: PathBuf,
    /// Bibles the church added itself (Settings → Bible → Your Bibles), looked at before the bundled ones.
    user_translations: PathBuf,
    translations: Mutex<HashMap<String, Pack>>,
    languages: Mutex<HashMap<String, Pack>>,
}

impl Bible {
    pub fn new(k: Arc<Knowledge>, nlt: Translation, db_dir: PathBuf, user_packs: PathBuf) -> Bible {
        let user_translations = user_packs.parent().unwrap_or(&user_packs).join("translations");
        Bible { k, nlt, db_dir, user_packs, user_translations, translations: Mutex::new(HashMap::new()), languages: Mutex::new(HashMap::new()) }
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
        if let Some(p) = crate::util::lock(&cache).get(key) {
            return p.clone();
        }
        let loaded = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|raw| {
            let numbering = psalm_numbering(&raw);
            let mut map: HashMap<String, String> = raw["verses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| {
                    let reference = v["reference"].as_str()?;
                    let text = v["text"].as_str()?.to_string();
                    let title_psalm = is_psalm(&v["book"]) && v["chapter"].as_u64().is_some_and(|c| PSALM_TITLE_VERSES.iter().any(|p| p.0 == c));
                    match numbering {
                        PsalmNumbering::English => Some((reference.to_string(), text)),
                        // Hebrew numbering: the title is verse 1 (or 1 and 2), so the KJV verse is lower.
                        PsalmNumbering::Hebrew => Some((kjv_reference(reference, &v["book"], v["chapter"].as_u64()?, v["verse"].as_u64()?)?, text)),
                        // Titles numbered but verses missing: those Psalms can't be lined up, so they
                        // stay in English rather than show the wrong verse.
                        PsalmNumbering::Unreliable => (!title_psalm).then(|| (reference.to_string(), text)),
                    }
                })
                .collect();
            if let Some(name) = raw["language"].as_str().or(raw["translation"].as_str()) {
                map.insert(PACK_NAME.into(), name.to_string());
            }
            if numbering == PsalmNumbering::Hebrew {
                // Psalm 13: KJV 13:5 and 13:6 are one Hebrew verse; both show it.
                let book = raw["verses"].as_array().into_iter().flatten()
                    .find(|v| is_psalm(&v["book"])).and_then(|v| v["reference"].as_str()).and_then(|r| r.rsplit_once(' ')).map(|(b, _)| b.to_string());
                if let Some(b) = book {
                    if let Some(t) = map.get(&format!("{b} 13:5")).cloned() { map.entry(format!("{b} 13:6")).or_insert(t); }
                }
            }
            eprintln!("[Translation] Loaded {key}: {} verses", map.len());
            Arc::new(map)
        });
        if loaded.is_none() {
            eprintln!("[Translation] No pack for \"{key}\"");
        }
        crate::util::lock(&cache).insert(key.to_string(), loaded.clone());
        loaded
    }

    /// The church's translation in place of the KJV text (KJV: as is; NLT: the NLT).
    /// Each verse in `code`'s wording where it has one, labelled with the version it's in
    /// ("translation": what a theme's version layer shows; a verse the translation lacks stays KJV).
    pub fn apply_translation(&self, verses: &mut [Value], code: &str) {
        let code = code.to_uppercase();
        for v in verses.iter_mut() {
            v["translation"] = json!("KJV");
        }
        if code == "KJV" || code.is_empty() {
            return;
        }
        if code == "NLT" {
            for v in verses.iter_mut() {
                if let Some(t) = v["nlt_text"].as_str().filter(|t| !t.is_empty()).map(str::to_string) {
                    v["text"] = json!(t);
                    v["translation"] = json!("NLT");
                }
            }
            return;
        }
        // A Bible the church connected with its own API.Bible key: the chapters kept on this computer.
        let app_data = self.user_translations.parent().unwrap_or(&self.user_translations);
        if let Some(info) = crate::api_bible::connected(app_data, &code) {
            // Labelled with the Bible's own abbreviation ("NIV"), as API.Bible requires, not the short name.
            let label = info["abbreviation"].as_str().filter(|a| !a.is_empty()).unwrap_or(&code).to_string();
            for v in verses.iter_mut() {
                let (Some(b), Some(c), Some(n)) = (v["book"].as_str().and_then(Book::from_name), v["chapter"].as_u64(), v["verse"].as_u64()) else { continue };
                if let Some(t) = crate::api_bible::verse(app_data, &code, b.index(), c as u16, n as u16) {
                    v["text"] = json!(t);
                    v["translation"] = json!(label);
                    if let Some(c) = info["copyright"].as_str().filter(|c| !c.is_empty()) {
                        v["copyright"] = json!(c);
                    }
                }
            }
            return;
        }
        let Some(pack) = self.pack(&self.translations, &code, self.translation_path(&code)) else { return };
        replace_text(verses, &pack, &code);
    }

    /// Where a translation's file is: one the church added, else one inside the app.
    pub fn translation_path(&self, code: &str) -> PathBuf {
        let added = self.user_translations.join(format!("{code}.json"));
        if added.is_file() { added } else { self.db_dir.join("translations").join(format!("{code}.json")) }
    }

    pub fn user_translations_dir(&self) -> &std::path::Path {
        &self.user_translations
    }

    /// Drops what was loaded for a translation, so one just added or removed is read again.
    pub fn forget_translation(&self, code: &str) {
        crate::util::lock(&self.translations).remove(code);
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
        crate::util::lock(&self.languages).remove(lang);
    }

    /// The service language's Bible in place of the English text.
    pub fn apply_language(&self, verses: &mut [Value], lang: &str) {
        if lang.is_empty() || lang == "en" {
            return;
        }
        let Some(pack) = self.pack(&self.languages, lang, self.pack_path(lang)) else { return };
        // A language's Bible goes by the language's name ("Yoruba").
        let name = pack.get(PACK_NAME).cloned().unwrap_or_else(|| lang.to_uppercase());
        replace_text(verses, &pack, &name);
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
const PSALM_TITLE_VERSES: [(u64, u64); 63] = [
    (3, 1), (4, 1), (5, 1), (6, 1), (7, 1), (8, 1), (9, 1), (12, 1), (13, 1), (18, 1), (19, 1), (20, 1), (21, 1), (22, 1), (30, 1), (31, 1),
    (34, 1), (36, 1), (38, 1), (39, 1), (40, 1), (41, 1), (42, 1), (44, 1), (45, 1), (46, 1), (47, 1), (48, 1), (49, 1), (51, 2),
    (52, 2), (53, 1), (54, 2), (55, 1), (56, 1), (57, 1), (58, 1), (59, 1), (60, 2), (61, 1), (62, 1), (63, 1), (64, 1), (65, 1),
    (67, 1), (68, 1), (69, 1), (70, 1), (75, 1), (76, 1), (77, 1), (80, 1), (81, 1), (83, 1), (84, 1), (85, 1), (88, 1), (89, 1),
    (92, 1), (102, 1), (108, 1), (140, 1), (142, 1),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum PsalmNumbering {
    /// The KJV's numbering (titles unnumbered).
    English,
    /// Titles numbered as verse 1 (or 1 and 2), every verse present.
    Hebrew,
    /// Titles numbered, but the Psalms cut to the KJV's verse counts, so verses are missing.
    Unreliable,
}

fn is_psalm(book: &Value) -> bool {
    book.as_str().is_some_and(|b| b.starts_with("Psalm"))
}

/// How this Bible numbers the Psalms. A pack can say so ("versification": "hebrew" or
/// "english"). Otherwise from its shape: in the Hebrew numbering Psalm 3 has 9 verses (the
/// KJV's 8 and the title); a Psalm 3 of 8 verses whose verse 1 is the title (it names Absalom
/// in every language) lost a verse somewhere.
fn psalm_numbering(raw: &Value) -> PsalmNumbering {
    match raw["versification"].as_str() {
        Some("hebrew") => return PsalmNumbering::Hebrew,
        Some("english") => return PsalmNumbering::English,
        _ => {}
    }
    let ps3: Vec<&Value> = raw["verses"].as_array().into_iter().flatten()
        .filter(|v| is_psalm(&v["book"]) && v["chapter"].as_u64() == Some(3)).collect();
    if ps3.iter().any(|v| v["verse"].as_u64() == Some(9)) {
        return PsalmNumbering::Hebrew;
    }
    let titled = ps3.iter().find(|v| v["verse"].as_u64() == Some(1))
        .and_then(|v| v["text"].as_str()).is_some_and(|t| t.to_lowercase().contains("bsal"));
    if titled { PsalmNumbering::Unreliable } else { PsalmNumbering::English }
}

/// The KJV reference for a verse numbered the Hebrew way, or None for a title verse.
fn kjv_reference(reference: &str, book: &Value, chapter: u64, verse: u64) -> Option<String> {
    if !is_psalm(book) {
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

fn replace_text(verses: &mut [Value], pack: &HashMap<String, String>, label: &str) {
    for v in verses.iter_mut() {
        if let Some(t) = v["reference"].as_str().and_then(|r| pack.get(r)).cloned() {
            v["text"] = json!(t);
            v["translation"] = json!(label);
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
        let v = |c: u64, n: u64, t: &str| json!({ "book": "Psalms", "chapter": c, "verse": n, "reference": format!("Psalms {c}:{n}"), "text": t });
        let full = json!({ "verses": [v(3, 1, "Ein Psalm Davids, da er floh vor Absalom."), v(3, 9, "x")] });
        let cut = json!({ "verses": [v(3, 1, "Ein Psalm Davids, da er floh vor Absalom."), v(3, 8, "x")] });
        let en = json!({ "verses": [v(3, 1, "LORD, how are they increased that trouble me!"), v(3, 8, "x")] });
        assert_eq!(psalm_numbering(&full), PsalmNumbering::Hebrew);
        assert_eq!(psalm_numbering(&cut), PsalmNumbering::Unreliable);
        assert_eq!(psalm_numbering(&en), PsalmNumbering::English);
        assert_eq!(psalm_numbering(&json!({ "versification": "english", "verses": full["verses"].clone() })), PsalmNumbering::English);
        assert_eq!(PSALM_TITLE_VERSES.iter().filter(|p| p.0 == 13).count(), 1);
    }
}
