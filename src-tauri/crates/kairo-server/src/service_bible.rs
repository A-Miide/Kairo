//! The Bibles detection matches speech against, for the service's spoken language (Settings →
//! spoken language, `sttLanguage`).
//!
//! - English: the KJV and the BSB.
//! - Spanish, Portuguese, French, German: the KJV and that language's Bible — the church's own, when it
//!   connected one in that language through API.Bible and its chapters are saved for offline use; else
//!   the pack in that language (downloaded, or the one that ships in `databases/bibles/packs`).
//!
//! - English: every English Bible the church has, so a preacher who asks for another version mid-sermon
//!   is followed at once: the church's own (its Translation setting, matched first), the BSB, every file
//!   it added (Settings → Bible) and every one it connected through API.Bible with its chapters saved.
//!   Real NIV and NLT read aloud went from 36% and 9% on screen (KJV and BSB wordings only) to 96% and 95%
//!   (2026-11-10). Each Bible adds about 30 MB; the owner chose every Bible ready over the memory
//!   (23 Nov 2026).
//!
//! The KJV stays in every build: verse numbering, the book layout and the meaning models are its. In
//! another language only that language's Bible is held beside it (the BSB isn't built for it: Spanish
//! replays found the same with it as without).
//!
//! Built once at start, in the background, and again when the spoken language or Translation changes, a
//! Bible is added or removed, or a connected one finishes saving; the engine starts again on the new
//! knowledge, and the old one serves until then.

use std::path::Path;
use std::sync::{Arc, Mutex};

use kairo_bible::{Corpus, Translation};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::Lang;

use crate::Shared;

/// What the knowledge in use was built from ("en", "es:pack", "es:api:RVR"): a rebuild that would
/// make the same is skipped.
static BUILT_FROM: Mutex<String> = Mutex::new(String::new());

/// The two-letter code Kairo's language packs use ("es-419" → "es"), for the languages detection reads.
pub fn pack_code(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Es => Some("es"),
        Lang::Pt => Some("pt"),
        Lang::Fr => Some("fr"),
        Lang::De => Some("de"),
        Lang::En => None,
    }
}

/// The church's Translation setting, as a code ("NIV"), when it is a Bible of its own (not the KJV or the BSB).
fn church_code(translation: &str) -> Option<String> {
    let code = translation.trim().to_uppercase();
    (!code.is_empty() && code != "KJV" && code != "BSB").then_some(code)
}

/// The church's own English Bible and where it came from: an added file, else one connected through API.Bible
/// whose chapters are saved. `None` when the setting is the KJV or BSB, or the Bible isn't here (whole enough).
fn church_wording(app: &Shared, corpus: &Corpus, translation: &str) -> Option<(Translation, String)> {
    let code = church_code(translation)?;
    let data = &app.cfg.app_data_dir;
    let file = data.join("translations").join(format!("{code}.json"));
    if file.is_file() {
        return corpus.load_flat(&code, &file).ok().map(|t| (t, format!("en:file:{code}")));
    }
    crate::api_bible::connected(data, &code)?;
    crate::api_bible::whole(data, &code, corpus).map(|t| (t, format!("en:api:{code}")))
}

/// What `church_wording` would be built from, without building it (a cheap check for `rebuild`).
fn church_source(app: &Shared, corpus_chapters: &Corpus, translation: &str) -> Option<String> {
    let code = church_code(translation)?;
    let data = &app.cfg.app_data_dir;
    if data.join("translations").join(format!("{code}.json")).is_file() {
        return Some(format!("en:file:{code}"));
    }
    (crate::api_bible::connected(data, &code).is_some() && crate::api_bible::saved_enough(data, &code, corpus_chapters)).then(|| format!("en:api:{code}"))
}

/// The service language's Bible matched beside the KJV in another language than English, and where it came from.
fn own_wording(app: &Shared, corpus: &Corpus, lang: Lang) -> Option<(Translation, String)> {
    let code = pack_code(lang)?;
    let data = &app.cfg.app_data_dir;
    if let Some((t, c)) = crate::api_bible::in_language(data, code).and_then(|c| crate::api_bible::whole(data, &c, corpus).map(|t| (t, c))) {
        return Some((t, format!("{code}:api:{c}")));
    }
    let downloaded = data.join("packs").join(format!("{code}.json"));
    let path = if downloaded.is_file() { downloaded } else { app.cfg.db_dir.join("packs").join(format!("{code}.json")) };
    match corpus.load_flat(code, &path) {
        Ok(t) => Some((kairo_detect::lexical::text::for_matching(&t, lang), format!("{code}:pack"))),
        Err(e) => {
            eprintln!("[Server] No {code} Bible to match speech against ({e}); matching the English ones.");
            None
        }
    }
}

/// The engine's knowledge for a spoken language (`sttLanguage`, "es-419"), and what it was built from.
pub fn build(app: &Shared, spoken: &str) -> anyhow::Result<(Knowledge, Lang, String)> {
    let db = &app.cfg.db_dir;
    let lang = Lang::from_code(spoken);
    let corpus = Corpus::load(db)?;
    let translation = tokio::runtime::Handle::current().block_on(app.settings.str("translation")).unwrap_or_default();
    let (wordings, from) = if pack_code(lang).is_some() {
        match own_wording(app, &corpus, lang) {
            Some((own, from)) => (Wordings { own: Some(own), also: Vec::new() }, from),
            None => (Wordings { own: None, also: english_modern(&corpus, db).into_iter().collect() }, "en".to_string()),
        }
    } else {
        let own = church_wording(app, &corpus, &translation).map(|(t, _)| t);
        let own_code = own.as_ref().map(|t| t.code.clone());
        let mut also: Vec<Translation> = english_modern(&corpus, db).into_iter().collect();
        for code in english_codes(app, &corpus) {
            if Some(&code) == own_code.as_ref() || code == "BSB" {
                continue;
            }
            if let Some(t) = english_bible(app, &corpus, &code) {
                also.push(t);
            }
        }
        (Wordings { own, also }, english_source(app, &corpus, &translation))
    };
    // Anchors from the language's own Bible only: its speech never says the KJV's English phrases.
    let anchors_in = if wordings.own.is_some() { lang } else { Lang::En };
    Ok((Knowledge::build_for(corpus, wordings, anchors_in), lang, from))
}

/// The BSB (public domain): the modern wording matched beside the KJV in an English service, as good as
/// the NIV and NLT together on the dev sermons, which stopped shipping 2026-10 (no licence).
pub fn english_modern(corpus: &Corpus, db: &Path) -> Option<Translation> {
    corpus.load_flat("BSB", &db.join("translations/BSB.json")).ok()
}

/// The codes of every English Bible the church has besides the KJV: the files it added, and the ones it
/// connected through API.Bible with enough chapters saved. Sorted, each once.
fn english_codes(app: &Shared, corpus: &Corpus) -> Vec<String> {
    let data = &app.cfg.app_data_dir;
    let stems = |dir: std::path::PathBuf| -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).filter_map(|p| p.file_stem()?.to_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let mut codes = stems(data.join("translations"));
    codes.extend(stems(data.join("api-bibles")).into_iter().filter(|c| {
        crate::api_bible::connected(data, c).is_some_and(|v| v["language"] == "eng") && crate::api_bible::saved_enough(data, c, corpus)
    }));
    codes.sort();
    codes.dedup();
    codes
}

/// One English Bible the church has, by code: its file, else its saved API.Bible chapters.
fn english_bible(app: &Shared, corpus: &Corpus, code: &str) -> Option<Translation> {
    let data = &app.cfg.app_data_dir;
    let file = data.join("translations").join(format!("{code}.json"));
    if file.is_file() {
        return corpus.load_flat(code, &file).ok();
    }
    crate::api_bible::whole(data, code, corpus)
}

/// What an English build is made from: the church's own Bible first, then every other one
/// ("en:NIV+BSB+NLT"), so a rebuild that would make the same is skipped.
fn english_source(app: &Shared, corpus: &Corpus, translation: &str) -> String {
    let own = church_source(app, corpus, translation).map(|_| translation.trim().to_uppercase());
    let rest: Vec<String> = english_codes(app, corpus).into_iter().filter(|c| Some(c) != own.as_ref() && c != "BSB").collect();
    let codes: Vec<String> = own.into_iter().chain(std::iter::once("BSB".to_string())).chain(rest).collect();
    format!("en:{}", codes.join("+"))
}

/// What the knowledge in use was built from ("en:NIV+BSB+NLT", "es:pack").
pub fn built_from() -> String {
    crate::util::lock(&BUILT_FROM).clone()
}

/// Notes what the knowledge in use was built from.
pub fn built(from: &str) {
    *crate::util::lock(&BUILT_FROM) = from.to_string();
}

/// Builds the knowledge again for the spoken language now in Settings, unless it would be the same as
/// the one in use, and hands it to the engine. Takes a few seconds: run off the async threads.
pub fn rebuild(app: &Shared) {
    let Some((bible, detector)) = app.ready() else { return };
    let spoken = tokio::runtime::Handle::current().block_on(app.settings.str("sttLanguage")).unwrap_or_else(|| "en-US".into());
    let lang = Lang::from_code(&spoken);
    // Cheap check first: the same language from the same source needs nothing built.
    let translation = tokio::runtime::Handle::current().block_on(app.settings.str("translation")).unwrap_or_default();
    let would_be = match pack_code(lang) {
        Some(c) => Some(match crate::api_bible::in_language(&app.cfg.app_data_dir, c) {
            Some(api) if crate::api_bible::saved_enough(&app.cfg.app_data_dir, &api, &bible.k().corpus) => format!("{c}:api:{api}"),
            _ => format!("{c}:pack"),
        }),
        None => Some(english_source(app, &bible.k().corpus, &translation)),
    };
    if *crate::util::lock(&BUILT_FROM) == would_be.unwrap_or_else(|| "en".into()) {
        return;
    }
    let t = std::time::Instant::now();
    match build(app, &spoken) {
        Ok((k, lang, from)) => {
            let k = Arc::new(k);
            bible.set_k(k.clone());
            detector.relearn(k, lang);
            eprintln!("[Server] Detection now matches {from} ({:.1}s).", t.elapsed().as_secs_f32());
            built(&from);
        }
        Err(e) => eprintln!("[Server] Couldn't build detection for {spoken}: {e}"),
    }
}

/// `rebuild` on a blocking thread.
pub fn rebuild_soon(app: &Shared) {
    let app = app.clone();
    let rt = tokio::runtime::Handle::current();
    rt.spawn_blocking(move || {
        // One rebuild at a time: two language changes in a row build in turn, the last one winning.
        static ONE: Mutex<()> = Mutex::new(());
        let _one = crate::util::lock(&ONE);
        rebuild(&app);
    });
}
