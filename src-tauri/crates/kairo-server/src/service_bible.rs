//! The Bibles detection matches speech against, for the service's spoken language (Settings →
//! spoken language, `sttLanguage`).
//!
//! - English: the KJV and the BSB.
//! - Spanish, Portuguese, French, German: the KJV and that language's Bible — the church's own, when it
//!   connected one in that language through API.Bible and its chapters are saved for offline use; else
//!   the pack in that language (downloaded, or the one that ships in `databases/bibles/packs`).
//!
//! The KJV stays in every build: verse numbering, the book layout and the meaning models are its. Only
//! one wording is held beside it, so a service in another language costs no more memory than an English
//! one (the BSB isn't built for it: Spanish replays found the same with it as without).
//!
//! Built once at start, and again when the spoken language changes or the church's Bible in the service
//! language finishes saving; the engine starts again on the new knowledge.

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

/// The wording matched beside the KJV, and where it came from.
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
    let (wordings, from) = match own_wording(app, &corpus, lang) {
        Some((own, from)) => (Wordings { own: Some(own), also: Vec::new() }, from),
        None => (Wordings { own: None, also: english_modern(&corpus, db).into_iter().collect() }, "en".to_string()),
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
    let would_be = pack_code(lang).map(|c| match crate::api_bible::in_language(&app.cfg.app_data_dir, c) {
        Some(api) if crate::api_bible::saved_enough(&app.cfg.app_data_dir, &api, &bible.k().corpus) => format!("{c}:api:{api}"),
        _ => format!("{c}:pack"),
    });
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
