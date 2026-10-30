//! Bibles a church connects with its own API.Bible key (Settings → Bible → Your Bibles). Kairo ships
//! none of this text: the church's key asks API.Bible for the Bibles its plan allows, each chapter is
//! fetched when a verse of it is first shown and kept for 30 days (API.Bible's caching rule), then
//! fetched again.
//!
//! Files in the app data folder: `api-bibles/<CODE>.json` (which Bible), `api-bibles/<CODE>/<Book>_<n>.json`
//! (a cached chapter). A verse shown before its chapter has come stays in the church's other Bible for
//! that moment; the chapter is asked for at once, so the next one is in it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use axum::extract::{Query, State};
use axum::Json;
use kairo_bible::book::BOOK_NAMES;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use crate::Shared;

const BASE: &str = "https://rest.api.bible/v1";
/// API.Bible's rule for cached text.
const REFRESH_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);
const KEY: &str = "apiBibleKey";

type Want = (String, usize, u16);
static QUEUE: OnceLock<UnboundedSender<Want>> = OnceLock::new();

pub fn dir(app_data: &Path) -> PathBuf {
    app_data.join("api-bibles")
}

fn chapter_file(app_data: &Path, code: &str, book: usize, chapter: u16) -> PathBuf {
    dir(app_data).join(code).join(format!("{}_{chapter}.json", BOOK_NAMES[book].replace(' ', "")))
}

/// The Bible a code stands for, if the church connected one.
pub fn connected(app_data: &Path, code: &str) -> Option<Value> {
    let raw = std::fs::read(dir(app_data).join(format!("{code}.json"))).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Chapters are kept encrypted (AES-256-GCM), as API.Bible's terms (§12) and Biblica's licence (§V.E)
/// ask: the text on disk can't be copied out as a Bible. The key is made once per computer and kept
/// beside the cache, readable only by this user.
fn cache_key(app_data: &Path) -> Option<ring::aead::LessSafeKey> {
    let path = dir(app_data).join(".key");
    let bytes = match std::fs::read(&path) {
        Ok(b) if b.len() == 32 => b,
        _ => {
            let mut b = vec![0u8; 32];
            getrandom::fill(&mut b).ok()?;
            std::fs::create_dir_all(dir(app_data)).ok()?;
            std::fs::write(&path, &b).ok()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            b
        }
    };
    let key = ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &bytes).ok()?;
    Some(ring::aead::LessSafeKey::new(key))
}

const SEALED: &[u8] = b"KAB1";

fn seal(app_data: &Path, plain: &[u8]) -> Option<Vec<u8>> {
    let key = cache_key(app_data)?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).ok()?;
    let mut data = plain.to_vec();
    key.seal_in_place_append_tag(ring::aead::Nonce::assume_unique_for_key(nonce), ring::aead::Aad::empty(), &mut data).ok()?;
    Some([SEALED, &nonce[..], &data].concat())
}

fn open(app_data: &Path, sealed: &[u8]) -> Option<Vec<u8>> {
    let rest = sealed.strip_prefix(SEALED)?;
    if rest.len() < 12 + 16 {
        return None;
    }
    let (nonce, data) = rest.split_at(12);
    let key = cache_key(app_data)?;
    let mut data = data.to_vec();
    let plain = key.open_in_place(ring::aead::Nonce::try_assume_unique_for_key(nonce).ok()?, ring::aead::Aad::empty(), &mut data).ok()?;
    Some(plain.to_vec())
}

/// One verse's text from a cached chapter. A chapter that is missing or older than 30 days is asked for
/// again (and an old one is still shown meanwhile: stale text is better than none on the screen).
pub fn verse(app_data: &Path, code: &str, book: usize, chapter: u16, verse: u16) -> Option<String> {
    let path = chapter_file(app_data, code, book, chapter);
    let stale = std::fs::metadata(&path).and_then(|m| m.modified()).ok().is_none_or(|t| SystemTime::now().duration_since(t).unwrap_or_default() > REFRESH_AFTER);
    if stale {
        if let Some(q) = QUEUE.get() {
            let _ = q.send((code.to_string(), book, chapter));
        }
    }
    let Some(raw) = std::fs::read(&path).ok().and_then(|b| open(app_data, &b)) else {
        // Saved before chapters were encrypted, or unreadable: fetch it again.
        if !stale && path.exists() {
            if let Some(q) = QUEUE.get() {
                let _ = q.send((code.to_string(), book, chapter));
            }
        }
        return None;
    };
    let v: Value = serde_json::from_slice(&raw).ok()?;
    v["verses"][verse.to_string()].as_str().map(str::to_string)
}

/// "[1] In the beginning… [2] Now the earth…" → verse number → text. A Bible that runs verses together
/// ("[19-21] Wherever the spirit went…", as the Message does) gives that text to each verse it covers,
/// so any of them shows the whole passage.
pub fn split_verses(content: &str) -> HashMap<u16, String> {
    let mut out: HashMap<u16, String> = HashMap::new();
    let mut cur: Option<(u16, u16)> = None;
    let mut buf = String::new();
    let flush = |cur: &mut Option<(u16, u16)>, buf: &mut String, out: &mut HashMap<u16, String>| {
        if let Some((a, z)) = cur.take() {
            let t = buf.split_whitespace().collect::<Vec<_>>().join(" ");
            if !t.is_empty() {
                for n in a..=z.max(a).min(a.saturating_add(40)) {
                    out.insert(n, t.clone());
                }
            }
        }
        buf.clear();
    };
    let mut rest = content;
    while let Some(open) = rest.find('[') {
        let (before, after) = rest.split_at(open);
        buf.push_str(before);
        let Some(close) = after.find(']') else { break };
        let marker = after[1..close].trim();
        let range = match marker.split_once(['-', '\u{2013}']) {
            Some((x, y)) => x.trim().parse::<u16>().ok().zip(y.trim().parse::<u16>().ok()),
            None => marker.parse::<u16>().ok().map(|n| (n, n)),
        };
        match range {
            Some(r) => {
                flush(&mut cur, &mut buf, &mut out);
                cur = Some(r);
            }
            None => buf.push_str(&after[..=close]),
        }
        rest = &after[close + 1..];
    }
    buf.push_str(rest);
    flush(&mut cur, &mut buf, &mut out);
    out
}

/// A copyright notice as API.Bible sends it (sometimes with HTML), as one plain line.
fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => { in_tag = false; out.push(' '); }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&nbsp;", " ").replace("&copy;", "©").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())
}

async fn get(key: &str, path: &str) -> Result<Value, String> {
    let resp = client()?.get(format!("{BASE}{path}")).header("api-key", key).send().await.map_err(|_| "Couldn't reach API.Bible. Check the internet connection.".to_string())?;
    match resp.status().as_u16() {
        200 => resp.json::<Value>().await.map_err(|_| "API.Bible's answer couldn't be read.".to_string()),
        401 | 403 => Err("API.Bible didn't accept that key.".into()),
        429 => Err("API.Bible's call limit for this key has been reached.".into()),
        s => Err(format!("API.Bible answered with an error (HTTP {s}).")),
    }
}

/// Starts the worker that fetches chapters, once.
pub fn start(app: &Shared) {
    if QUEUE.get().is_some() {
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Want>();
    if QUEUE.set(tx).is_err() {
        return;
    }
    let outer = app.clone();
    let app = app.clone();
    tokio::spawn(async move {
        let mut asked: std::collections::HashSet<Want> = Default::default();
        while let Some(want) = rx.recv().await {
            if !asked.insert(want.clone()) {
                continue;
            }
            let (code, book, chapter) = want.clone();
            let Some(info) = connected(&app.cfg.app_data_dir, &code) else { continue };
            let (Some(id), Some(key)) = (info["id"].as_str().map(str::to_string), app.settings.str(KEY).await) else { continue };
            let osis = crate::bible_import::usfm_code(book);
            let url = format!("/bibles/{id}/chapters/{osis}.{chapter}?content-type=text&include-notes=false&include-titles=false&include-chapter-numbers=false&include-verse-numbers=true");
            match get(&key, &url).await {
                Ok(v) => {
                    let verses = split_verses(v["data"]["content"].as_str().unwrap_or(""));
                    if !verses.is_empty() {
                        let path = chapter_file(&app.cfg.app_data_dir, &code, book, chapter);
                        if let Some(p) = path.parent() {
                            let _ = std::fs::create_dir_all(p);
                        }
                        let map: HashMap<String, &String> = verses.iter().map(|(n, t)| (n.to_string(), t)).collect();
                        if let Some(sealed) = seal(&app.cfg.app_data_dir, &serde_json::to_vec(&json!({ "verses": map })).unwrap_or_default()) {
                            let _ = std::fs::write(&path, sealed);
                        }
                        if let Some(b) = app.bible.get() {
                            b.forget_translation(&code);
                        }
                    }
                }
                Err(e) => eprintln!("[ApiBible] {code} {} {chapter}: {e}", BOOK_NAMES[book]),
            }
            // forgotten, so it can be asked for again after a failure or once it has gone stale
            asked.remove(&want);
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    // Bibles already connected: what's missing or older than 30 days is fetched again in the background.
    if let Ok(rd) = std::fs::read_dir(dir(&outer.cfg.app_data_dir)) {
        for e in rd.flatten() {
            let p = e.path();
            let Some(code) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if p.extension().is_some_and(|x| x == "json") && connected(&outer.cfg.app_data_dir, &code).is_some_and(|v| v["id"].is_string()) {
                keep_offline(&outer, &code);
            }
        }
    }
}

fn fail(msg: impl Into<String>) -> Json<Value> {
    Json(json!({ "ok": false, "error": msg.into() }))
}

/// POST /api/api-bible/connect {key}: checks the key and keeps it; answers with the English Bibles it can use.
pub async fn connect(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    start(&app);
    let key = b["key"].as_str().map(str::trim).filter(|k| !k.is_empty() && k.len() < 200 && k.is_ascii());
    let Some(key) = key else { return fail("Paste the API key from your API.Bible account.") };
    match get(key, "/bibles?language=eng").await {
        Ok(v) => {
            let mut m = serde_json::Map::new();
            m.insert(KEY.into(), json!(key));
            app.settings.merge(m).await;
            Json(json!({ "ok": true, "bibles": listing(&app, key, &v, true).await }))
        }
        Err(e) => fail(e),
    }
}

/// GET /api/api-bible/bibles: the English Bibles the saved key can use.
pub async fn bibles(State(app): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    start(&app);
    let Some(key) = app.settings.str(KEY).await else { return Json(json!({ "ok": true, "connected": false, "bibles": [] })) };
    match get(&key, "/bibles?language=eng").await {
        Ok(v) => Json(json!({ "ok": true, "connected": true, "bibles": listing(&app, &key, &v, q.contains_key("refresh")).await })),
        Err(e) => fail(e),
    }
}

/// What a Bible in the account can be used for: its whole text (`full`), the Old Testament only (`ot`,
/// a Septuagint or Tanakh with no John), or nothing (`none`: listed, but the plan doesn't include it).
/// Asked once per Bible (one chapter, John 3) and kept for 30 days in `api-bibles/access.json`.
async fn coverage(app: &Shared, key: &str, ids: &[String], refresh: bool) -> HashMap<String, String> {
    let path = dir(&app.cfg.app_data_dir).join("access.json");
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
    let mut known: HashMap<String, (String, u64)> = if refresh {
        HashMap::new()
    } else {
        std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    };
    let missing: Vec<String> = ids.iter().filter(|id| known.get(*id).is_none_or(|(_, at)| now.saturating_sub(*at) > REFRESH_AFTER.as_secs())).cloned().collect();
    for chunk in missing.chunks(8) {
        let asked = futures_util::future::join_all(chunk.iter().map(|id| async move {
            let r = client().ok()?.get(format!("{BASE}/bibles/{id}/chapters/JHN.3?content-type=text")).header("api-key", key).send().await.ok()?;
            let c = match r.status().as_u16() {
                200 => "full",
                404 => "ot",
                401 | 403 => "none",
                _ => return None,
            };
            Some((id.clone(), c.to_string()))
        }))
        .await;
        for (id, c) in asked.into_iter().flatten() {
            known.insert(id, (c, now));
        }
    }
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(&path, serde_json::to_vec(&known).unwrap_or_default());
    known.into_iter().map(|(id, (c, _))| (id, c)).collect()
}

/// The English Bibles the key can read, each with its edition ("Catholic", "Protestant") and coverage.
async fn listing(app: &Shared, key: &str, v: &Value, refresh: bool) -> Vec<Value> {
    let all: Vec<&Value> = v["data"].as_array().into_iter().flatten().collect();
    let ids: Vec<String> = all.iter().filter_map(|b| b["id"].as_str().map(str::to_string)).collect();
    let cov = coverage(app, key, &ids, refresh).await;
    all.into_iter()
        .filter_map(|b| {
            let id = b["id"].as_str()?;
            let c = cov.get(id).map(String::as_str).unwrap_or("full");
            (c != "none").then(|| json!({
                "id": id,
                "abbreviation": b["abbreviationLocal"].as_str().or(b["abbreviation"].as_str()),
                "name": b["nameLocal"].as_str().or(b["name"].as_str()),
                // Only the tradition an edition is for ("Catholic", "Protestant"); other descriptions just repeat the name.
                "edition": b["description"].as_str().filter(|d| ["catholic", "protestant", "orthodox", "ecumenical", "jewish", "septuagint"].iter().any(|t| d.eq_ignore_ascii_case(t))),
                "coverage": c,
            }))
        })
        .collect()
}

/// POST /api/api-bible/add {id, code, name}
pub async fn add(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    start(&app);
    let Some(id) = b["id"].as_str().filter(|i| i.len() < 64 && i.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')) else { return fail("Choose a Bible from the list.") };
    // The short name is the Bible's own abbreviation, made unique ("NIV2" beside the NIV that ships).
    let code = match b["code"].as_str().filter(|c| !c.trim().is_empty()) {
        Some(c) => match crate::bible_import::clean_code(c) {
            Some(c) => c,
            None => return fail(crate::bible_import::code_error(c)),
        },
        None => crate::bible_import::unique_code(&app.cfg.app_data_dir, b["abbreviation"].as_str().or(b["name"].as_str()).unwrap_or("")),
    };
    let name = b["name"].as_str().unwrap_or(&code);
    let coverage = if b["coverage"] == "ot" { "ot" } else { "full" };
    // The Bible's own abbreviation and copyright line, which every verse shown from it must carry
    // (API.Bible terms §7, Appendix A).
    let about = match app.settings.str(KEY).await {
        Some(key) => get(&key, &format!("/bibles/{id}")).await.ok().map(|v| v["data"].clone()).unwrap_or(Value::Null),
        None => Value::Null,
    };
    let abbreviation = about["abbreviationLocal"].as_str().or(about["abbreviation"].as_str()).or(b["abbreviation"].as_str()).unwrap_or(&code).to_string();
    let copyright = about["copyright"].as_str().map(plain_text).unwrap_or_default();
    let d = dir(&app.cfg.app_data_dir);
    let info = json!({ "id": id, "name": name, "coverage": coverage, "abbreviation": abbreviation, "copyright": copyright });
    if std::fs::create_dir_all(&d).is_err() || std::fs::write(d.join(format!("{code}.json")), info.to_string()).is_err() {
        return fail("Kairo couldn't save that.");
    }
    if let Some(bible) = app.bible.get() {
        bible.forget_translation(&code);
    }
    let total = keep_offline(&app, &code);
    Json(json!({ "ok": true, "code": code, "name": name, "total": total }))
}

/// The chapters this Bible has, in the KJV's layout (the Old Testament's for one without the New).
fn chapters(app: &Shared, code: &str) -> Vec<(usize, u16)> {
    let Some(bible) = app.bible.get() else { return Vec::new() };
    let ot_only = connected(&app.cfg.app_data_dir, code).is_some_and(|v| v["coverage"] == "ot");
    kairo_bible::Book::all()
        .filter(|b| !(ot_only && b.is_new_testament()))
        .flat_map(|b| (1..=bible.k.corpus.chapter_count(b)).map(move |c| (b.index(), c)))
        .collect()
}

/// Asks for every chapter not already saved (or gone stale), so the Bible works with no internet in the
/// service. About 1,200 API calls for a whole Bible, at four a second; again after 30 days.
fn keep_offline(app: &Shared, code: &str) -> usize {
    let all = chapters(app, code);
    if let Some(q) = QUEUE.get() {
        for &(b, c) in &all {
            let file = chapter_file(&app.cfg.app_data_dir, code, b, c);
            let fresh = std::fs::metadata(&file).and_then(|m| m.modified()).ok()
                .is_some_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < REFRESH_AFTER)
                && std::fs::read(&file).is_ok_and(|b| b.starts_with(SEALED));
            if !fresh {
                let _ = q.send((code.to_string(), b, c));
            }
        }
    }
    all.len()
}

/// GET /api/api-bible/mine
pub async fn mine(State(app): State<Shared>, Query(_): Query<HashMap<String, String>>) -> Json<Value> {
    start(&app);
    let mut list = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir(&app.cfg.app_data_dir)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                if let (Some(code), Some(v)) = (p.file_stem().and_then(|s| s.to_str()), std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())) {
                    if v["id"].is_string() {
                        let saved = std::fs::read_dir(p.with_extension("")).map(|d| d.count()).unwrap_or(0);
                        list.push(json!({ "code": code, "name": v["name"], "abbreviation": v["abbreviation"], "copyright": v["copyright"], "chapters": saved, "total": chapters(&app, code).len() }));
                    }
                }
            }
        }
    }
    list.sort_by(|a, b| a["code"].as_str().cmp(&b["code"].as_str()));
    // The key's last four characters, so the church can tell which account is connected.
    let key = app.settings.str(KEY).await;
    let hint = key.as_deref().map(|k| k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<String>());
    Json(json!({ "connected": key.is_some(), "keyHint": hint, "bibles": list }))
}

/// POST /api/api-bible/remove {code} (one Bible), or {all: true} (forget the key and every connected Bible).
pub async fn remove(State(app): State<Shared>, Json(b): Json<Value>) -> Json<Value> {
    let d = dir(&app.cfg.app_data_dir);
    if b["all"] == true {
        let _ = std::fs::remove_dir_all(&d);
        let mut m = serde_json::Map::new();
        m.insert(KEY.into(), Value::Null);
        app.settings.merge(m).await;
        return Json(json!({ "ok": true }));
    }
    let Some(code) = b["code"].as_str().and_then(crate::bible_import::clean_code) else { return Json(json!({ "ok": false })) };
    let _ = std::fs::remove_dir_all(d.join(&code));
    let ok = std::fs::remove_file(d.join(format!("{code}.json"))).is_ok();
    if let Some(bible) = app.bible.get() {
        bible.forget_translation(&code);
    }
    Json(json!({ "ok": ok }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copyright_lines_lose_their_markup() {
        assert_eq!(plain_text("<p>Scripture taken from the NIV&copy;, Copyright &amp; 2011 Biblica</p>"), "Scripture taken from the NIV©, Copyright & 2011 Biblica");
    }

    #[test]
    fn verses_split_on_their_numbers() {
        let v = split_verses("[1] In the beginning God created the heavens and the earth. [2] Now the earth was formless\n and empty. ");
        assert_eq!(v[&1], "In the beginning God created the heavens and the earth.");
        assert_eq!(v[&2], "Now the earth was formless and empty.");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn verses_run_together_show_the_whole_passage_in_each() {
        let v = split_verses("[18] A. [19-21] When the creatures went, the wheels went. [22-24] Over the heads.");
        assert_eq!(v[&18], "A.");
        assert_eq!((v[&19].as_str(), v[&20].as_str(), v[&21].as_str()), ("When the creatures went, the wheels went.", "When the creatures went, the wheels went.", "When the creatures went, the wheels went."));
        assert_eq!(v[&22], "Over the heads.");
        assert_eq!(v.len(), 7);
    }

    #[test]
    fn text_before_the_first_number_and_other_brackets_are_not_lost_or_misread() {
        let v = split_verses("intro [1] He said [aside] hello. [2] Next.");
        assert_eq!(v[&1], "He said [aside] hello.");
        assert_eq!(v[&2], "Next.");
        assert!(split_verses("no numbers here").is_empty());
    }

    #[test]
    fn a_cached_chapter_gives_its_verse() {
        let d = std::env::temp_dir().join("kairo_api_bible_test");
        let _ = std::fs::remove_dir_all(&d);
        let f = chapter_file(&d, "TST", 42, 3);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, seal(&d, br#"{"verses":{"16":"For God so loved"}}"#).unwrap()).unwrap();
        // what is on disk is not readable text
        assert!(!String::from_utf8_lossy(&std::fs::read(&f).unwrap()).contains("God so loved"));
        assert_eq!(verse(&d, "TST", 42, 3, 16).as_deref(), Some("For God so loved"));
        assert_eq!(verse(&d, "TST", 42, 3, 17), None);
        assert_eq!(verse(&d, "TST", 0, 1, 1), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
