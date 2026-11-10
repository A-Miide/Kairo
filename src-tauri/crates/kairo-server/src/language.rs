//! The detector follows the service's spoken language: the reference grammar of that language, and the
//! wording of that language's Bible beside the KJV, so a verse quoted in Spanish is found by Spanish words
//! (read aloud in the pack's own wording the detector puts it on screen 96-99% of the time; with only the
//! English wordings it puts up none: kairo-eval `read-aloud`, 2026-11-10). In an English service the
//! church's own Bible (a file it added) is built in the same way, since a version the detector has never
//! seen is found far less often and later (WEB 66%, ASV 82%, Darby 73%, YLT 40% on screen, against 98% for the KJV).

use std::sync::{Arc, Mutex};

use kairo_bible::Corpus;
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::Lang;

use crate::bible::Bible;
use crate::detect::Detector;

/// What the detector matches now: "en" (the KJV and BSB), a language pack's code ("es"), or an English
/// service's own Bible ("en+NIV").
static APPLIED: Mutex<String> = Mutex::new(String::new());
/// The last knowledge built with a church wording, kept so listening again doesn't build it again.
/// What the latest call asked for, so a build that finishes after the service moved on is dropped.
static WANTED: Mutex<String> = Mutex::new(String::new());
static BUILT: Mutex<Option<(String, Arc<Knowledge>)>> = Mutex::new(None);

/// The pack code for a spoken language setting ("es-ES" -> "es", "multi" -> none), when a Bible of that language
/// is on this computer (bundled, or downloaded into the app's folder).
fn pack_code(bible: &Bible, spoken: &str) -> Option<String> {
    let code = spoken.get(..2)?.to_ascii_lowercase();
    (code != "en" && bible.pack_path(&code).is_file()).then_some(code)
}

/// The church's own English Bible: a file it added, or one it connected with its API.Bible key (the chapters
/// kept on this computer). The bundled BSB is matched for everyone already, and the KJV is the base, so
/// neither needs building again. A connected Bible's key carries how much of it is kept, so it is built
/// again when more has come.
fn church_code(bible: &Bible, translation: &str) -> Option<String> {
    let code = translation.trim().to_uppercase();
    if code.is_empty() || code == "KJV" || code == "BSB" {
        return None;
    }
    if bible.user_translations_dir().join(format!("{code}.json")).is_file() {
        return Some(code);
    }
    let app_data = bible.user_translations_dir().parent()?;
    crate::api_bible::connected(app_data, &code)?;
    Some(format!("{code}@{}", crate::api_bible::kept_hundreds(app_data, &code)))
}

/// Sets the detector's language and wording for this service. The reference grammar changes at once; the Bible
/// (the service language's, or the church's own in an English service) is built into the detector's words in
/// the background (a few seconds, below the speech engine's priority) and swapped in when ready, so listening
/// starts without waiting for it. Call again when the language or the church's translation changes.
pub fn follow_spoken_language(bible: &Arc<Bible>, detector: &Detector, spoken: &str, translation: &str) {
    let lang = Lang::from_code(spoken);
    detector.configure(move |c| c.lang = lang);
    let pack = pack_code(bible, spoken);
    let want = match (&pack, church_code(bible, translation)) {
        (Some(code), _) => code.clone(),
        (None, Some(church)) if Lang::from_code(spoken) == Lang::En => format!("en+{church}"),
        _ => "en".to_string(),
    };
    *crate::util::lock(&WANTED) = want.clone();
    let mut applied = crate::util::lock(&APPLIED);
    if applied.is_empty() {
        *applied = "en".to_string();
    }
    if *applied == want {
        return;
    }
    if want == "en" {
        detector.use_knowledge(bible.k.clone());
        // Nothing keeps the church wording's words now, so its memory goes back.
        *crate::util::lock(&BUILT) = None;
        *applied = want;
        return;
    }
    if let Some((code, k)) = crate::util::lock(&BUILT).clone().filter(|(code, _)| *code == want) {
        detector.use_knowledge(k);
        *applied = code;
        return;
    }
    drop(applied);
    let (bible, detector) = (bible.clone(), detector.clone());
    let _ = std::thread::Builder::new().name("wording".into()).spawn(move || {
        let _below = crate::util::below_speech();
        let church = want.strip_prefix("en+").map(|c| c.split('@').next().unwrap_or(c).to_string());
        let code = church.clone().unwrap_or_else(|| want.clone());
        let built = Corpus::load(&bible.db_dir).ok().and_then(|corpus| {
            let own = match &church {
                Some(c) => {
                    let file = bible.user_translations_dir().join(format!("{c}.json"));
                    if file.is_file() {
                        corpus.load_flat(c, &file).ok()?
                    } else {
                        let (rows, _) = crate::api_bible::kept_verses(bible.user_translations_dir().parent()?, c);
                        (!rows.is_empty()).then(|| corpus.align(c, rows.into_iter()))?
                    }
                }
                None => corpus.load_flat(&code, &bible.pack_path(&want)).ok()?,
            };
            Some(Arc::new(Knowledge::build(corpus, Wordings { own: Some(own), also: vec![] })))
        });
        match built {
            Some(k) => {
                *crate::util::lock(&BUILT) = Some((want.clone(), k.clone()));
                if *crate::util::lock(&WANTED) != want {
                    return;
                }
                detector.use_knowledge(k);
                eprintln!("[Language] the detector now matches the {code} Bible's wording and reads {lang:?}");
                *crate::util::lock(&APPLIED) = want;
            }
            None => eprintln!("[Language] could not read the {code} Bible for detection: quotes will be matched in the KJV and BSB wording"),
        }
    });
}
