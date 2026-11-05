//! Songs from EasyWorship, for a church moving to Kairo. EasyWorship has no song export, so its
//! own files are read:
//!
//! - EasyWorship 6 and 7 song library: `Songs.db` with `SongWords.db` (SQLite), from the
//!   profile's `Databases/Data` folder; both are picked together.
//! - EasyWorship 2007 and 2009 song library: `Songs.DB` with `Songs.MB` (Paradox table and its
//!   memo file); both are picked together.
//! - EasyWorship 6 and 7 schedule: `.ewsx` (a zip holding a SQLite `main.db`).
//! - EasyWorship 2009 schedule: `.ews` (fixed-size records, lyrics zlib-compressed).
//!
//! Lyrics are RTF everywhere: a blank line (or EasyWorship's slide marker) separates slides, and
//! a slide may start with its label ("Verse 1", "Chorus"), which Kairo's text reader keeps.
//! Formats as documented by OpenLP's importer; the code here is Kairo's own.

use serde_json::{json, Value};

use crate::import::{from_text, rtf_document_text, ImportError};

/// One song as EasyWorship stored it.
#[derive(Debug, Default, PartialEq)]
pub struct Song {
    pub title: String,
    pub author: String,
    pub copyright: String,
    pub ccli: String,
    /// RTF, one entry per slide (schedules) or the whole song in one (libraries).
    pub rtf: Vec<String>,
}

fn err(msg: impl Into<String>) -> ImportError {
    ImportError(msg.into(), None)
}

/// The songs as Kairo playlist items / Song Library entries: `{ name, author, blocks }`.
pub fn items(songs: Vec<Song>) -> Vec<Value> {
    songs
        .into_iter()
        .filter_map(|s| {
            let text = s.rtf.iter().map(|r| lyrics_text(r)).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join("\n\n");
            let blocks = from_text(&text);
            if blocks.is_empty() {
                return None;
            }
            let title = if s.title.trim().is_empty() { "Untitled".to_string() } else { s.title.trim().to_string() };
            Some(json!({ "name": title, "title": title, "author": s.author.trim(), "copyright": s.copyright.trim(), "ccli": s.ccli.trim(), "blocks": blocks }))
        })
        .collect()
}

/// An EasyWorship RTF lyric as plain text, slides apart by a blank line.
fn lyrics_text(rtf: &str) -> String {
    // EasyWorship 6/7 marks a new slide with its own control word; plain RTF readers drop it.
    let rtf = rtf.replace("\\sdslidemarker", "\\par\\par ");
    // Older libraries can hold stray control characters (a NUL before a slide's label).
    let text: String = rtf_document_text(&rtf).chars().filter(|c| !c.is_control() || *c == '\n').collect();
    // Lines that are only spaces count as blank, so slides split where EasyWorship split them.
    text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

/// Bytes as text: UTF-8 when they are, else Windows-1252 (EasyWorship's older files).
fn text_of(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| cp1252(c)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&b) { HIGH[(b - 0x80) as usize] } else { b as char }
}

/// A fixed-size, NUL-padded string field.
fn cstring(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    text_of(&b[..end]).trim().to_string()
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn i32le(b: &[u8], at: usize) -> Option<i32> {
    b.get(at..at + 4).map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

// ── SQLite (EasyWorship 6 and 7) ────────────────────────────────────────────

/// A SQLite database held in memory, opened read-only from a private temporary copy (removed
/// when dropped).
struct Db {
    conn: rusqlite::Connection,
    path: std::path::PathBuf,
}

impl Db {
    fn open(bytes: &[u8]) -> Result<Db, ImportError> {
        if !bytes.starts_with(b"SQLite format 3\0") {
            return Err(err("not an EasyWorship database"));
        }
        let path = std::env::temp_dir().join(format!("kairo-ew-{}-{}.db", std::process::id(), crate::util::uid("db", 6)));
        std::fs::write(&path, bytes).map_err(|e| err(format!("couldn't read the database: {e}")))?;
        let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| { let _ = std::fs::remove_file(&path); err(format!("couldn't open the database: {e}")) })?;
        Ok(Db { conn, path })
    }

    fn has_table(&self, name: &str) -> bool {
        self.conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(())).is_ok()
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A column as text, whatever SQLite type it holds.
fn col(row: &rusqlite::Row, i: usize) -> String {
    use rusqlite::types::ValueRef;
    match row.get_ref(i) {
        Ok(ValueRef::Text(t)) | Ok(ValueRef::Blob(t)) => text_of(t),
        Ok(ValueRef::Integer(n)) => n.to_string(),
        Ok(ValueRef::Real(f)) => f.to_string(),
        _ => String::new(),
    }
}

/// The EasyWorship 6/7 song library: `Songs.db` (table `song`) and `SongWords.db` (table `word`).
pub fn from_sqlite_library(a: &[u8], b: &[u8]) -> Result<Vec<Song>, ImportError> {
    let (x, y) = (Db::open(a)?, Db::open(b)?);
    let (songs, words) = match (x.has_table("song"), y.has_table("word"), y.has_table("song"), x.has_table("word")) {
        (true, true, _, _) => (x, y),
        (_, _, true, true) => (y, x),
        _ => return Err(err("These aren't EasyWorship's Songs.db and SongWords.db. Pick both from the profile's Databases/Data folder.")),
    };
    let sql = |e: rusqlite::Error| err(format!("the song library couldn't be read: {e}"));
    let mut list = songs.conn.prepare("SELECT rowid, title, author, copyright, vendor_id FROM song ORDER BY title").map_err(sql)?;
    let mut lyric = words.conn.prepare("SELECT words FROM word WHERE song_id = ?1 LIMIT 1").map_err(sql)?;
    let rows = list.query_map([], |r| Ok((r.get::<_, i64>(0)?, col(r, 1), col(r, 2), col(r, 3), col(r, 4)))).map_err(sql)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, author, copyright, ccli) = row.map_err(sql)?;
        let rtf = lyric.query_row([id], |r| Ok(col(r, 0))).unwrap_or_default();
        out.push(Song { title, author, copyright, ccli, rtf: vec![rtf] });
    }
    Ok(out)
}

/// An EasyWorship 6/7 schedule (`.ewsx`): its songs in schedule order, each slide's RTF.
pub fn from_ewsx(zip: &[u8]) -> Result<Vec<Song>, ImportError> {
    let members = crate::import::unzip(zip, |n| n.rsplit('/').next().is_some_and(|b| b.eq_ignore_ascii_case("main.db")))
        .map_err(|e| err(format!("The schedule couldn't be read: {}.", e.0)))?;
    let (_, main) = members.into_iter().next().ok_or_else(|| err("This isn't an EasyWorship schedule (no main.db inside)."))?;
    let db = Db::open(&main)?;
    let sql = |e: rusqlite::Error| err(format!("the schedule couldn't be read: {e}"));
    let mut list = db.conn.prepare("SELECT rowid, title, author, copyright, reference_number FROM presentation WHERE presentation_type = 6 ORDER BY rowid").map_err(sql)?;
    let mut slides = db.conn.prepare(
        "SELECT rt.rtf FROM element e JOIN slide s ON e.slide_id = s.rowid JOIN resource_text rt ON rt.resource_id = e.foreground_resource_id \
         WHERE e.element_type = 6 AND e.element_style_type = 4 AND s.presentation_id = ?1 ORDER BY s.order_index",
    ).map_err(sql)?;
    let rows = list.query_map([], |r| Ok((r.get::<_, i64>(0)?, col(r, 1), col(r, 2), col(r, 3), col(r, 4)))).map_err(sql)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, title, author, copyright, ccli) = row.map_err(sql)?;
        let rtf: Vec<String> = slides.query_map([id], |r| Ok(col(r, 0))).map_err(sql)?.filter_map(Result::ok).collect();
        out.push(Song { title, author, copyright, ccli, rtf });
    }
    Ok(out)
}

// ── EasyWorship 2009 schedule (.ews) ────────────────────────────────────────

/// An EasyWorship 2009 schedule (`.ews`): its songs (other items, such as scripture or media,
/// are left out).
pub fn from_ews(b: &[u8]) -> Result<Vec<Song>, ImportError> {
    let not = || err("This isn't an EasyWorship schedule Kairo can read (EasyWorship 2009, versions 1.6, 3 and 5).");
    if b.len() < 892 || !b.starts_with(b"EasyWorship Schedule File Version") {
        return Err(not());
    }
    let count_at = match &b[35..38] {
        b"  5" => 56,
        b"  3" => 48,
        b"1.6" => 40,
        _ => return Err(not()),
    };
    let count = i32le(b, count_at).filter(|n| (0..=10_000).contains(n)).ok_or_else(not)? as usize;
    let entry_len = u16le(b, count_at + 4).filter(|n| *n >= 1420).ok_or_else(not)? as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let start = count_at + 6 + i * entry_len;
        let Some(e) = b.get(start..start + entry_len) else { break };
        if i32le(e, 820) != Some(1) {
            continue; // not a song
        }
        let ptr = i32le(e, 800).filter(|p| *p > 0).map(|p| p as usize);
        let rtf = ptr.and_then(|p| ews_content(b, p)).unwrap_or_default();
        out.push(Song {
            title: cstring(&e[0..50]),
            author: cstring(&e[307..357]),
            copyright: administered(cstring(&e[358..458]), cstring(&e[459..509])),
            ccli: cstring(&e[1410..1420]),
            rtf: vec![rtf],
        });
    }
    Ok(out)
}

/// The zlib-compressed RTF at `ptr`: its total length, the data, and the size it unpacks to.
fn ews_content(b: &[u8], ptr: usize) -> Option<String> {
    use std::io::Read;
    let len = u32le(b, ptr)? as usize;
    let data = b.get(ptr + 4..(ptr + len).checked_sub(6)?)?;
    let size = u32le(b, ptr + len - 2).unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(size.min(4 << 20));
    flate2::read::ZlibDecoder::new(data).take(16 << 20).read_to_end(&mut out).ok()?;
    Some(text_of(&out))
}

fn administered(copyright: String, admin: String) -> String {
    match (copyright.is_empty(), admin.is_empty()) {
        (_, true) => copyright,
        (true, false) => format!("Administered by {admin}"),
        (false, false) => format!("{copyright} Administered by {admin}"),
    }
}

// ── EasyWorship 2007/2009 song library (Paradox) ────────────────────────────

/// The EasyWorship 2007/2009 song library: `Songs.DB` (a Paradox table) and `Songs.MB` (its
/// memo file, which holds the lyrics).
pub fn from_paradox_library(db: &[u8], mb: &[u8]) -> Result<Vec<Song>, ImportError> {
    let not = || err("These aren't EasyWorship's Songs.DB and Songs.MB. Pick both from EasyWorship's Data folder.");
    if db.len() < 0x800 {
        return Err(not());
    }
    let record_size = u16le(db, 0).filter(|n| *n > 0).ok_or_else(not)? as usize;
    let header_size = u16le(db, 2).filter(|n| *n == 0x800).ok_or_else(not)? as usize;
    let factor = *db.get(5).filter(|f| (1..=4).contains(*f)).ok_or_else(not)? as usize;
    let first_block = u16le(db, 14).ok_or_else(not)? as usize;
    let field_count = u16le(db, 33).filter(|n| (1..=255).contains(n)).ok_or_else(not)? as usize;
    // Field types and sizes, then (after a gap) their names.
    let defs: Vec<(u8, usize)> = (0..field_count).map(|i| (db[120 + 2 * i], db[121 + 2 * i] as usize)).collect();
    let mut at = 120 + 2 * field_count + 4 + 4 * field_count + 261;
    let mut names = Vec::new();
    for _ in 0..field_count {
        let end = db.get(at..).and_then(|s| s.iter().position(|&c| c == 0)).ok_or_else(not)?;
        names.push(text_of(&db[at..at + end]));
        at += end + 1;
    }
    let field = |name: &str| names.iter().position(|n| n.eq_ignore_ascii_case(name));
    let (Some(title), Some(author), Some(copyright), Some(admin), Some(words)) =
        (field("Title"), field("Author"), field("Copyright"), field("Administrator"), field("Words")) else { return Err(not()) };
    let number = field("Song Number");
    let offsets: Vec<usize> = defs.iter().scan(0, |o, (_, size)| { let at = *o; *o += size; Some(at) }).collect();
    let text = |rec: &[u8], f: usize| rec.get(offsets[f]..offsets[f] + defs[f].1).map(cstring).unwrap_or_default();

    let block_size = 1024 * factor;
    let mut out = Vec::new();
    let mut block = first_block;
    let mut seen = std::collections::HashSet::new();
    while block != 0 && seen.insert(block) {
        let start = header_size + (block - 1) * block_size;
        let Some(head) = db.get(start..start + 6) else { break };
        let next = u16le(head, 0).unwrap_or(0) as usize;
        // The last record's offset from the first; negative when the block is empty.
        let last = i16::from_le_bytes([head[4], head[5]]);
        let records = if last < 0 { 0 } else { last as usize / record_size + 1 };
        for r in 0..records {
            let at = start + 6 + r * record_size;
            let Some(rec) = db.get(at..at + record_size) else { break };
            let lyric = rec.get(offsets[words]..offsets[words] + defs[words].1).and_then(|f| memo(mb, f)).unwrap_or_default();
            out.push(Song {
                title: text(rec, title),
                author: text(rec, author),
                copyright: administered(text(rec, copyright), text(rec, admin)),
                ccli: number.map(|n| text(rec, n)).unwrap_or_default(),
                rtf: vec![text_of(&lyric)],
            });
        }
        block = next;
    }
    Ok(out)
}

/// A memo field's bytes from the `.MB` file: its last 10 bytes point at a block there (the low
/// byte a sub-block index) and give the size.
fn memo(mb: &[u8], field: &[u8]) -> Option<Vec<u8>> {
    let n = field.len();
    if n < 10 {
        return None;
    }
    let pointer = u32le(field, n - 10)? as usize;
    let size = u32le(field, n - 6)? as usize;
    let (index, block) = (pointer & 0xFF, pointer & !0xFF);
    if size == 0 || block == 0 {
        return None;
    }
    match mb.get(block)? {
        2 => mb.get(block + 8..block + 8 + size).map(<[u8]>::to_vec),
        3 if index <= 63 => {
            let at = *mb.get(block + 11 + 5 * index)? as usize * 16;
            mb.get(block + at..block + at + size).map(<[u8]>::to_vec)
        }
        _ => None,
    }
}

// ── Which format ────────────────────────────────────────────────────────────

/// Songs from the file(s) picked: a schedule on its own, or a library's two files together.
/// `companions` are the other files picked with `filename`.
pub fn read(filename: &str, buf: &[u8], companions: &[(String, Vec<u8>)]) -> Result<Vec<Song>, ImportError> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".ewsx") {
        return from_ewsx(buf);
    }
    if lower.ends_with(".ews") {
        return from_ews(buf);
    }
    let all: Vec<(&str, &[u8])> = std::iter::once((filename, buf)).chain(companions.iter().map(|(n, b)| (n.as_str(), b.as_slice()))).collect();
    let find = |want: &str| all.iter().find(|(n, _)| n.rsplit(['/', '\\']).next().is_some_and(|b| b.eq_ignore_ascii_case(want))).map(|(_, b)| *b);
    if all.iter().any(|(_, b)| b.starts_with(b"SQLite format 3\0")) {
        return match (find("Songs.db"), find("SongWords.db")) {
            (Some(a), Some(b)) => from_sqlite_library(a, b),
            _ => Err(err("Pick both Songs.db and SongWords.db (EasyWorship 6/7 keeps a song library in the two files, in the profile's Databases/Data folder).")),
        };
    }
    let db = all.iter().find(|(n, _)| n.to_ascii_lowercase().ends_with(".db")).map(|(_, b)| *b);
    let mb = all.iter().find(|(n, _)| n.to_ascii_lowercase().ends_with(".mb")).map(|(_, b)| *b);
    match (db, mb) {
        (Some(db), Some(mb)) => from_paradox_library(db, mb),
        _ => Err(err("Pick both Songs.DB and Songs.MB (EasyWorship 2007/2009 keeps a song library in the two files).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite(sql: &str) -> Vec<u8> {
        let path = std::env::temp_dir().join(format!("kairo-ew-test-{}-{}.db", std::process::id(), crate::util::uid("t", 6)));
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(sql).unwrap();
        drop(c);
        let b = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        b
    }

    /// A zip with the files stored as they are (no compression; the reader doesn't check CRCs).
    fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let at = out.len() as u32;
            let (n, len) = (name.len() as u16, data.len() as u32);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version, flags, method 0, time, date, crc
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // made by, needed, flags, method, time, date, crc
            central.extend_from_slice(&len.to_le_bytes());
            central.extend_from_slice(&len.to_le_bytes());
            central.extend_from_slice(&n.to_le_bytes());
            central.extend_from_slice(&[0u8; 12]); // extra, comment, disk, attributes
            central.extend_from_slice(&at.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let (cd_at, cd_len) = (out.len() as u32, central.len() as u32);
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_len.to_le_bytes());
        out.extend_from_slice(&cd_at.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    const RTF: &str = r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\f0 Verse 1\par Amazing grace how sweet the sound\par That saved a wretch like me\par\par Chorus\par I once was lost\par}";

    #[test]
    fn a_v6_library_reads_titles_authors_and_slides() {
        let songs = sqlite(&format!(
            "CREATE TABLE song (title TEXT, author TEXT, copyright TEXT, vendor_id INTEGER);
             INSERT INTO song VALUES ('Amazing Grace', 'John Newton', 'Public Domain', 22025);"
        ));
        let words = sqlite(&format!("CREATE TABLE word (song_id INTEGER, words TEXT); INSERT INTO word VALUES (1, '{RTF}');"));
        let got = read("SongWords.db", &words, &[("Songs.db".into(), songs)]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].title.as_str(), got[0].author.as_str(), got[0].ccli.as_str()), ("Amazing Grace", "John Newton", "22025"));
        let it = items(got);
        let blocks = it[0]["blocks"].as_array().unwrap();
        assert!(blocks.len() >= 2, "verse and chorus are separate: {blocks:?}");
        assert!(it[0]["blocks"].to_string().contains("Amazing grace how sweet the sound"));
        assert!(!it[0]["blocks"].to_string().contains("Arial"), "the font table isn't lyrics: {blocks:?}");
    }

    #[test]
    fn one_library_file_alone_says_which_is_missing() {
        let songs = sqlite("CREATE TABLE song (title TEXT, author TEXT, copyright TEXT, vendor_id INTEGER);");
        let e = read("Songs.db", &songs, &[]).unwrap_err();
        assert!(e.0.contains("SongWords.db"), "{}", e.0);
    }

    #[test]
    fn an_ewsx_schedule_reads_its_songs_slide_by_slide_in_order() {
        let db = sqlite(
            "CREATE TABLE presentation (title TEXT, author TEXT, copyright TEXT, reference_number TEXT, presentation_type INTEGER);
             INSERT INTO presentation VALUES ('Blessed Assurance', 'Fanny Crosby', '', '', 6);
             INSERT INTO presentation VALUES ('Welcome slide', '', '', '', 1);
             CREATE TABLE slide (presentation_id INTEGER, order_index INTEGER);
             INSERT INTO slide VALUES (1, 2); INSERT INTO slide VALUES (1, 1);
             CREATE TABLE element (slide_id INTEGER, foreground_resource_id INTEGER, element_type INTEGER, element_style_type INTEGER);
             INSERT INTO element VALUES (1, 10, 6, 4); INSERT INTO element VALUES (2, 11, 6, 4);
             CREATE TABLE resource_text (resource_id INTEGER, rtf TEXT);
             INSERT INTO resource_text VALUES (10, '{\\rtf1 Second slide words}');
             INSERT INTO resource_text VALUES (11, '{\\rtf1 First slide words}');",
        );
        let zip = stored_zip(&[("main.db", &db)]);
        let got = read("Sunday.ewsx", &zip, &[]).unwrap();
        assert_eq!(got.len(), 1, "only songs (presentation_type 6)");
        assert_eq!(got[0].title, "Blessed Assurance");
        let text = items(got)[0]["blocks"].to_string();
        assert!(text.find("First slide").unwrap() < text.find("Second slide").unwrap(), "slides in order: {text}");
    }

    #[test]
    fn an_ews_2009_schedule_reads_its_songs() {
        use std::io::Write;
        let entry_len = 1816usize;
        let mut b = vec![0u8; 62 + 2 * entry_len];
        b[..38].copy_from_slice(b"EasyWorship Schedule File Version  5\0\0");
        b[35..38].copy_from_slice(b"  5");
        b[56..60].copy_from_slice(&2i32.to_le_bytes());
        b[60..62].copy_from_slice(&(entry_len as u16).to_le_bytes());
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(RTF.as_bytes()).unwrap();
        let packed = z.finish().unwrap();
        let ptr = b.len();
        let total = packed.len() + 10;
        b.extend_from_slice(&(total as u32).to_le_bytes());
        b.extend_from_slice(&packed);
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&(RTF.len() as u32).to_le_bytes());
        let e = 62;
        b[e..e + 13].copy_from_slice(b"Amazing Grace");
        b[e + 307..e + 318].copy_from_slice(b"John Newton");
        b[e + 800..e + 804].copy_from_slice(&(ptr as i32).to_le_bytes());
        b[e + 820..e + 824].copy_from_slice(&1i32.to_le_bytes());
        let e2 = 62 + entry_len; // a non-song item
        b[e2..e2 + 5].copy_from_slice(b"Media");
        b[e2 + 820..e2 + 824].copy_from_slice(&3i32.to_le_bytes());
        let got = read("Sunday.ews", &b, &[]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].title.as_str(), got[0].author.as_str()), ("Amazing Grace", "John Newton"));
        assert!(items(got)[0]["blocks"].to_string().contains("That saved a wretch like me"));
    }

    #[test]
    fn a_paradox_library_reads_records_and_memo_lyrics() {
        // Fields: Title(50) Author(50) Copyright(50) Administrator(50) Song Number(10) Words(memo, 20).
        let fields: [(&str, u8, usize); 6] = [("Title", 1, 50), ("Author", 1, 50), ("Copyright", 1, 50), ("Administrator", 1, 50), ("Song Number", 1, 10), ("Words", 0x0C, 20)];
        let record_size: usize = fields.iter().map(|f| f.2).sum();
        let mut db = vec![0u8; 0x800];
        db[0..2].copy_from_slice(&(record_size as u16).to_le_bytes());
        db[2..4].copy_from_slice(&0x800u16.to_le_bytes());
        db[5] = 1;
        db[14..16].copy_from_slice(&1u16.to_le_bytes());
        db[33..35].copy_from_slice(&(fields.len() as u16).to_le_bytes());
        for (i, f) in fields.iter().enumerate() {
            db[120 + 2 * i] = f.1;
            db[121 + 2 * i] = f.2 as u8;
        }
        let mut at = 120 + 2 * fields.len() + 4 + 4 * fields.len() + 261;
        for f in &fields {
            db[at..at + f.0.len()].copy_from_slice(f.0.as_bytes());
            at += f.0.len() + 1;
        }
        // One data block (block 1) with one record.
        let mut block = vec![0u8; 1024];
        block[4..6].copy_from_slice(&0i16.to_le_bytes()); // last record at offset 0: one record
        let mut rec = vec![0u8; record_size];
        rec[0..13].copy_from_slice(b"Amazing Grace");
        rec[50..61].copy_from_slice(b"John Newton");
        rec[200..205].copy_from_slice(b"22025");
        // Memo pointer: a type-2 block at 4096 in the .MB, the lyric's size.
        let words_at = 210;
        rec[words_at + 10..words_at + 14].copy_from_slice(&4096u32.to_le_bytes());
        rec[words_at + 14..words_at + 18].copy_from_slice(&(RTF.len() as u32).to_le_bytes());
        block[6..6 + record_size].copy_from_slice(&rec);
        db.extend_from_slice(&block);
        let mut mb = vec![0u8; 4096];
        mb.push(2);
        mb.extend_from_slice(&[0u8; 7]);
        mb.extend_from_slice(RTF.as_bytes());
        let got = read("Songs.DB", &db, &[("Songs.MB".into(), mb)]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].title.as_str(), got[0].author.as_str(), got[0].ccli.as_str()), ("Amazing Grace", "John Newton", "22025"));
        assert!(items(got)[0]["blocks"].to_string().contains("I once was lost"));
    }

    /// Real files saved by EasyWorship, kept outside the repo (their licences differ from Kairo's):
    /// `KAIRO_EW_SAMPLES=<folder> cargo test -p kairo-server real_easyworship -- --ignored --nocapture`.
    /// The folder holds any of: `*.ews`, `*.ewsx`, `Songs.DB` + `Songs.MB`, `Songs.db` + `SongWords.db`
    /// (each library pair in a folder of its own).
    #[test]
    #[ignore]
    fn real_easyworship_files_import() {
        let Ok(root) = std::env::var("KAIRO_EW_SAMPLES") else { return };
        let mut dirs = vec![std::path::PathBuf::from(root)];
        let mut checked = 0;
        while let Some(dir) = dirs.pop() {
            let mut files = Vec::new();
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() { dirs.push(p) } else { files.push(p) }
            }
            let name = |p: &std::path::PathBuf| p.file_name().unwrap().to_string_lossy().to_string();
            let mut runs: Vec<(String, Vec<u8>, Vec<(String, Vec<u8>)>)> = Vec::new();
            let lib: Vec<_> = files.iter().filter(|p| matches!(p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("db" | "mb"))).collect();
            if let Some((first, rest)) = lib.split_first() {
                runs.push((name(first), std::fs::read(first).unwrap(), rest.iter().map(|p| (name(p), std::fs::read(p).unwrap())).collect()));
            }
            for p in files.iter().filter(|p| matches!(p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("ews" | "ewsx"))) {
                runs.push((name(p), std::fs::read(p).unwrap(), Vec::new()));
            }
            for (file, buf, companions) in runs {
                let songs = read(&file, &buf, &companions).unwrap_or_else(|e| panic!("{}: {}", dir.join(&file).display(), e.0));
                let n = songs.len();
                let it = items(songs);
                println!("{} — {n} songs, {} with lyrics", dir.join(&file).display(), it.len());
                for s in &it {
                    let blocks = s["blocks"].as_array().unwrap();
                    let first = blocks.first().map(|b| b.to_string()).unwrap_or_default();
                    println!("  {:?} by {:?} ({}) — {} slides; first: {}", s["title"].as_str().unwrap(), s["author"].as_str().unwrap(), s["copyright"].as_str().unwrap(), blocks.len(), first.chars().take(140).collect::<String>());
                }
                assert!(!it.is_empty(), "{file}: no songs with lyrics");
                checked += 1;
            }
        }
        assert!(checked > 0, "no EasyWorship files found");
    }
}
