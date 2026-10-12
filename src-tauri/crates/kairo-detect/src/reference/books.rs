//! Book names as they are spoken: canonical names, standard abbreviations,
//! ordinal-numbered books ("first Corinthians", "1 Cor"), each book's own
//! alternative titles, and sound-based matching for names a speech engine
//! got slightly wrong.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use kairo_bible::Book;

/// Single tokens and "<n> <stem>" pairs → book. Names and the standard typed
/// abbreviations.
const ALIASES: &[(&str, &str)] = &[
    ("genesis", "Genesis"), ("gen", "Genesis"), ("exodus", "Exodus"), ("exod", "Exodus"), ("exo", "Exodus"),
    ("leviticus", "Leviticus"), ("lev", "Leviticus"), ("numbers", "Numbers"), ("num", "Numbers"),
    ("deuteronomy", "Deuteronomy"), ("deut", "Deuteronomy"), ("deu", "Deuteronomy"),
    ("joshua", "Joshua"), ("josh", "Joshua"), ("jos", "Joshua"), ("judges", "Judges"), ("judg", "Judges"), ("jdg", "Judges"),
    ("ruth", "Ruth"),
    ("1 samuel", "1 Samuel"), ("2 samuel", "2 Samuel"), ("1 kings", "1 Kings"), ("2 kings", "2 Kings"),
    ("1 chronicles", "1 Chronicles"), ("2 chronicles", "2 Chronicles"),
    ("ezra", "Ezra"), ("nehemiah", "Nehemiah"), ("esther", "Esther"), ("job", "Job"),
    ("psalms", "Psalms"), ("psalm", "Psalms"), ("ps", "Psalms"), ("psa", "Psalms"), ("pss", "Psalms"), ("psal", "Psalms"), ("sal", "Psalms"),
    ("proverbs", "Proverbs"), ("prov", "Proverbs"), ("pro", "Proverbs"), ("prv", "Proverbs"),
    ("ecclesiastes", "Ecclesiastes"), ("eccl", "Ecclesiastes"), ("ecc", "Ecclesiastes"), ("qoh", "Ecclesiastes"),
    ("songofsolomon", "Song of Solomon"), ("sos", "Song of Solomon"), ("ss", "Song of Solomon"), ("cant", "Song of Solomon"),
    ("isaiah", "Isaiah"), ("isa", "Isaiah"), ("jeremiah", "Jeremiah"), ("jer", "Jeremiah"),
    ("lamentations", "Lamentations"), ("lam", "Lamentations"), ("ezekiel", "Ezekiel"), ("ezek", "Ezekiel"), ("eze", "Ezekiel"),
    ("daniel", "Daniel"), ("dan", "Daniel"), ("hosea", "Hosea"), ("hos", "Hosea"), ("joel", "Joel"), ("amos", "Amos"),
    ("obadiah", "Obadiah"), ("obad", "Obadiah"), ("oba", "Obadiah"), ("jonah", "Jonah"), ("jon", "Jonah"),
    ("micah", "Micah"), ("mic", "Micah"), ("nahum", "Nahum"), ("nah", "Nahum"), ("habakkuk", "Habakkuk"), ("hab", "Habakkuk"),
    ("zephaniah", "Zephaniah"), ("zeph", "Zephaniah"), ("zep", "Zephaniah"), ("haggai", "Haggai"), ("hag", "Haggai"),
    ("zechariah", "Zechariah"), ("zachariah", "Zechariah"), ("zacharias", "Zechariah"), ("zech", "Zechariah"), ("zec", "Zechariah"),
    ("malachi", "Malachi"), ("mal", "Malachi"),
    ("matthew", "Matthew"), ("matt", "Matthew"), ("mt", "Matthew"), ("mark", "Mark"), ("mk", "Mark"), ("mrk", "Mark"),
    ("luke", "Luke"), ("lk", "Luke"), ("luk", "Luke"), ("john", "John"), ("jn", "John"), ("joh", "John"), ("acts", "Acts"),
    ("romans", "Romans"), ("rom", "Romans"),
    ("1 corinthians", "1 Corinthians"), ("1 cor", "1 Corinthians"), ("1cor", "1 Corinthians"),
    ("2 corinthians", "2 Corinthians"), ("2 cor", "2 Corinthians"), ("2cor", "2 Corinthians"),
    ("galatians", "Galatians"), ("gal", "Galatians"), ("ephesians", "Ephesians"), ("eph", "Ephesians"),
    ("philippians", "Philippians"), ("phil", "Philippians"), ("php", "Philippians"), ("colossians", "Colossians"), ("col", "Colossians"),
    ("1 thessalonians", "1 Thessalonians"), ("1 thess", "1 Thessalonians"), ("1thess", "1 Thessalonians"), ("1 th", "1 Thessalonians"),
    ("2 thessalonians", "2 Thessalonians"), ("2 thess", "2 Thessalonians"), ("2thess", "2 Thessalonians"), ("2 th", "2 Thessalonians"),
    ("1 timothy", "1 Timothy"), ("1 tim", "1 Timothy"), ("1tim", "1 Timothy"), ("2 timothy", "2 Timothy"), ("2 tim", "2 Timothy"), ("2tim", "2 Timothy"),
    ("titus", "Titus"), ("tit", "Titus"), ("philemon", "Philemon"), ("phlm", "Philemon"), ("phm", "Philemon"),
    ("hebrews", "Hebrews"), ("heb", "Hebrews"), ("james", "James"), ("jas", "James"), ("jam", "James"),
    ("1 peter", "1 Peter"), ("1 pet", "1 Peter"), ("1pet", "1 Peter"), ("1 pe", "1 Peter"),
    ("2 peter", "2 Peter"), ("2 pet", "2 Peter"), ("2pet", "2 Peter"), ("2 pe", "2 Peter"),
    ("1 john", "1 John"), ("1 jn", "1 John"), ("1jn", "1 John"), ("1 jo", "1 John"),
    ("2 john", "2 John"), ("2 jn", "2 John"), ("2jn", "2 John"), ("3 john", "3 John"), ("3 jn", "3 John"), ("3jn", "3 John"),
    ("jude", "Jude"), ("jud", "Jude"), ("revelation", "Revelation"), ("revelations", "Revelation"), ("rev", "Revelation"), ("re", "Revelation"),
];

/// Books that share a name and differ by number, by their spoken stem.
const NUMBERED: &[(&str, &[&str])] = &[
    ("corinthians", &["1 Corinthians", "2 Corinthians"]),
    ("thessalonians", &["1 Thessalonians", "2 Thessalonians"]),
    ("timothy", &["1 Timothy", "2 Timothy"]),
    ("peter", &["1 Peter", "2 Peter"]),
    ("samuel", &["1 Samuel", "2 Samuel"]),
    ("kings", &["1 Kings", "2 Kings"]),
    ("chronicles", &["1 Chronicles", "2 Chronicles"]),
];

/// Each book's other spoken titles → the token the grammar reads. Book-of
/// and ordinal forms for every book are generated, not listed.
const TITLES: &[(&str, &str)] = &[
    ("acts of the apostles", "acts"), ("acts of the apostle", "acts"), ("acts of apostles", "acts"), ("acts of apostle", "acts"),
    ("book of the apostles", "acts"), ("book of the apostle", "acts"), ("book of apostles", "acts"), ("book of apostle", "acts"),
    ("lamentations of jeremiah", "lamentations"),
    ("songs of solomon", "songofsolomon"), ("songs of songs", "songofsolomon"), ("song of songs", "songofsolomon"),
    ("song of solomon", "songofsolomon"), ("solomon songs", "songofsolomon"), ("solomons song", "songofsolomon"),
];

/// Book names that are also everyday English words, or the names of people
/// preachers talk about as people. A bare mention of one doesn't establish
/// the book; it needs a citation shape ("chapter", a verse) or an
/// introduction ("turn to", "the book of").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbiguityRule {
    /// The list the Node engine used.
    Legacy,
    /// Every everyday-word book and every book named after a person.
    Principled,
}

const AMBIGUOUS_LEGACY: &[&str] = &[
    "numbers", "ruth", "mark", "john", "james", "acts", "judges", "job", "joel", "amos", "micah", "nahum", "titus", "jude",
    "luke", "hebrews", "esther", "hosea", "jonah", "genesis", "exodus", "philemon", "obadiah", "haggai", "revelation",
];

const EVERYDAY_WORD_BOOKS: &[&str] = &[
    "genesis", "exodus", "numbers", "judges", "job", "acts", "mark", "revelation", "revelations", "lamentations",
    // peoples, said as peoples ("the Romans", "the Hebrews")
    "romans", "hebrews",
];
const PERSON_NAMED_BOOKS: &[&str] = &[
    "joshua", "ruth", "ezra", "nehemiah", "esther", "job", "isaiah", "jeremiah", "ezekiel", "daniel", "hosea", "joel", "amos",
    "obadiah", "jonah", "micah", "nahum", "habakkuk", "zephaniah", "haggai", "zechariah", "malachi", "matthew", "mark",
    "luke", "john", "titus", "philemon", "james", "jude",
];

pub struct Books {
    aliases: HashMap<String, Book>,
    /// In first-listed order: fuzzy matching keeps the first best match.
    single_words: Vec<String>,
    single_set: HashSet<String>,
    numbered: Vec<(&'static str, Vec<Book>)>,
    /// (phrase, replacement), longest phrases first.
    titles: Vec<(String, String)>,
    ambiguous: HashSet<&'static str>,
}

static BOOKS: LazyLock<Books> = LazyLock::new(|| Books::build(AmbiguityRule::Legacy));
static BOOKS_PRINCIPLED: LazyLock<Books> = LazyLock::new(|| Books::build(AmbiguityRule::Principled));

fn book(name: &str) -> Book {
    Book::from_name(name).unwrap_or_else(|| panic!("unknown book {name}"))
}

impl Books {
    pub fn get() -> &'static Books {
        &BOOKS
    }

    pub fn with_rule(rule: AmbiguityRule) -> &'static Books {
        match rule {
            AmbiguityRule::Legacy => &BOOKS,
            AmbiguityRule::Principled => &BOOKS_PRINCIPLED,
        }
    }

    fn build(rule: AmbiguityRule) -> Books {
        let mut aliases: HashMap<String, Book> = ALIASES.iter().map(|(k, v)| (k.to_string(), book(v))).collect();
        let mut single_words: Vec<String> = Vec::new();
        let mut single_set = HashSet::new();
        let mut add_single = |w: &str, list: &mut Vec<String>| {
            if single_set.insert(w.to_string()) {
                list.push(w.to_string());
            }
        };
        for (alias, _) in ALIASES {
            if !alias.contains(' ') && !alias.starts_with(|c: char| c.is_ascii_digit()) {
                add_single(alias, &mut single_words);
            }
        }
        let numbered: Vec<(&'static str, Vec<Book>)> =
            NUMBERED.iter().map(|(stem, names)| (*stem, names.iter().map(|n| book(n)).collect())).collect();
        for (stem, books) in &numbered {
            add_single(stem, &mut single_words);
            aliases.entry(stem.to_string()).or_insert(books[0]);
        }

        // Spoken titles: listed alternative titles, then generated forms for
        // every book — "book of <name>", "first/second/third <stem>".
        let mut titles: Vec<(String, String)> = TITLES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        for w in &single_words {
            titles.push((format!("book of {w}"), w.clone()));
        }
        for (ord, n) in [("first", "1"), ("second", "2"), ("third", "3")] {
            for key in aliases.keys().filter(|k| k.starts_with(&format!("{n} "))).cloned().collect::<Vec<_>>() {
                let stem = &key[2..];
                titles.push((format!("{ord} {stem}"), key.clone()));
            }
        }
        titles.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        titles.dedup_by(|a, b| a.0 == b.0);

        let ambiguous: HashSet<&'static str> = match rule {
            AmbiguityRule::Legacy => AMBIGUOUS_LEGACY.iter().copied().collect(),
            AmbiguityRule::Principled => EVERYDAY_WORD_BOOKS.iter().chain(PERSON_NAMED_BOOKS).copied().collect(),
        };
        Books { aliases, single_words, single_set, numbered, titles, ambiguous }
    }

    pub fn alias(&self, key: &str) -> Option<Book> {
        self.aliases.get(key).copied()
    }

    pub fn is_single_word(&self, w: &str) -> bool {
        self.single_set.contains(w)
    }

    /// Every one-word name and abbreviation, in listed order.
    pub fn single_words(&self) -> impl Iterator<Item = &str> {
        self.single_words.iter().map(String::as_str)
    }

    pub fn is_ambiguous(&self, w: &str) -> bool {
        self.ambiguous.contains(w)
    }

    /// The variants of a numbered stem ("timothy" → 1 and 2 Timothy).
    pub fn numbered_variants(&self, stem: &str) -> Option<&[Book]> {
        self.numbered.iter().find(|(s, _)| *s == stem).map(|(_, v)| v.as_slice())
    }

    /// "<n> <stem>" → book.
    pub fn numbered(&self, n: u8, stem: &str) -> Option<Book> {
        self.alias(&format!("{n} {stem}"))
    }

    /// Rewrites spoken titles to the tokens the grammar reads. A title only
    /// matches as a whole word.
    pub fn apply_titles(&self, text: &str) -> String {
        let mut t = text.to_string();
        for (find, replace) in &self.titles {
            if t.contains(find.as_str()) {
                t = replace_whole(&t, find, replace);
            }
        }
        t
    }

    /// The book named at `words[i]`, and how many words it took.
    pub fn match_at(&self, words: &[&str], i: usize) -> Option<(Book, usize)> {
        let w = *words.get(i)?;
        let next = words.get(i + 1).copied();
        if let (Some(n), Some(stem)) = (numbered_prefix(w), next) {
            if let Some(b) = self.numbered(n, stem) {
                return Some((b, 2));
            }
            // The number prefix is deliberate; a near miss on the stem is still that book.
            let mut best: Option<(usize, &[Book])> = None;
            for (s, books) in &self.numbered {
                let max = if s.len() >= 9 { 2 } else { 1 };
                if s.len().abs_diff(stem.len()) > max || stem.chars().next() != s.chars().next() {
                    continue;
                }
                let d = levenshtein(stem, s);
                if d <= max && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, books));
                }
            }
            if let Some((_, books)) = best {
                return Some((books.get(n as usize - 1).copied().unwrap_or(books[0]), 2));
            }
        }
        if self.single_set.contains(w) {
            return self.alias(w).map(|b| (b, 1));
        }
        self.fuzzy_single(words, i).map(|b| (b, 1))
    }

    /// A near miss of a book name, only where a citation follows: "chapter"
    /// next allows a looser match; a bare number next only for longer words
    /// (sermons are full of "<word> four"), and short names need "chapter".
    fn fuzzy_single(&self, words: &[&str], i: usize) -> Option<Book> {
        let w = words[i];
        let len = w.chars().count();
        if len < 4 {
            return None;
        }
        let next = words.get(i + 1).copied().unwrap_or("");
        let chapter_next = next == "chapter";
        let number_next = is_digits(next) || super::numbers::word_value(next).is_some();
        let short = len < 6;
        if !(chapter_next || (number_next && !short)) {
            return None;
        }
        let max = if short { 1 } else if chapter_next && len >= 8 { 2 } else { 1 };
        let key = sound_key(w);
        // (spelling distance, matched by sound only) — a spelling match always
        // ranks above one that only sounds alike.
        let mut best: Option<((usize, bool), &str)> = None;
        for alias in &self.single_words {
            if alias.chars().next() != w.chars().next() {
                continue;
            }
            let spelled = if alias.chars().count().abs_diff(len) <= max { levenshtein(w, alias) } else { usize::MAX };
            let rank = if spelled <= max {
                (spelled, false)
            } else if !short && sounds_alike(&key, &sound_key(alias)) {
                // Spelled differently, sounding the same ("philippine", "proverbate").
                (max, true)
            } else {
                continue;
            };
            if best.is_none_or(|(br, _)| rank < br) {
                best = Some((rank, alias));
            }
        }
        let (_, alias) = best?;
        // "number" is an everyday word one letter from "numbers": only a verse
        // after the chapter makes it the book.
        if alias == "numbers" && w == "number" {
            let after = words.get(i + 2).copied().unwrap_or("");
            let verse_follows = after == "verse" || after == "verses" || is_digits(after) || super::numbers::word_value(after).is_some();
            if !verse_follows {
                return None;
            }
        }
        self.alias(alias)
    }

    /// Books that sound like `heard`, closest first — for a citation whose
    /// chapter the book as heard doesn't have. Names a speech engine confuses
    /// open with the same sound and have a similar shape; ties go to the
    /// closer spelling.
    pub fn sound_alikes(&self, heard: Book) -> Vec<Book> {
        let name = heard.name().to_lowercase();
        let key = sound_key(&name);
        let mut scored: Vec<(f32, usize, Book)> = Book::all()
            .filter(|b| *b != heard && b.name().contains(' ') == name.contains(' '))
            .filter_map(|b| {
                let other = b.name().to_lowercase();
                let k = sound_key(&other);
                if k.chars().next() != key.chars().next() {
                    return None;
                }
                let sim = 1.0 - levenshtein(&key, &k) as f32 / key.len().max(k.len()).max(1) as f32;
                (sim >= 0.5).then(|| (sim, levenshtein(&name, &other), b))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, _, b)| b).collect()
    }
}

pub fn numbered_prefix(w: &str) -> Option<u8> {
    match w {
        "first" | "1" | "1st" => Some(1),
        "second" | "2" | "2nd" => Some(2),
        "third" | "3" | "3rd" => Some(3),
        _ => None,
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Replaces whole-word occurrences of `find` (not followed by a letter).
fn replace_whole(text: &str, find: &str, replace: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(find) {
        let end = pos + find.len();
        let before_ok = pos == 0 || !rest[..pos].ends_with(|c: char| c.is_ascii_alphanumeric());
        let after_ok = !rest[end..].starts_with(|c: char| c.is_ascii_lowercase());
        out.push_str(&rest[..pos]);
        if before_ok && after_ok {
            out.push_str(replace);
        } else {
            out.push_str(find);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Edit distance; anything more than 3 apart in length is simply "far".
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 3 {
        return 99;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] { prev[j - 1] } else { 1 + prev[j].min(cur[j - 1]).min(prev[j - 1]) };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// How a word sounds, roughly: spellings of the same consonant collapse,
/// vowels and doubled letters drop ("philippians" and "philippine" → "flpns"
/// / "flpn"; "john" and "jonah" → "jn").
pub fn sound_key(w: &str) -> String {
    let w: Vec<char> = w.to_lowercase().chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < w.len() {
        let c = w[i];
        let next = w.get(i + 1).copied();
        let (sound, step) = match (c, next) {
            ('p', Some('h')) => (Some('f'), 2),
            ('c', Some('k')) => (Some('k'), 2),
            ('c', Some('h')) => (Some('k'), 2),
            ('t', Some('h')) => (Some('t'), 2),
            ('s', Some('h')) => (Some('s'), 2),
            ('c', Some('e' | 'i' | 'y')) => (Some('s'), 1),
            ('c' | 'q', _) => (Some('k'), 1),
            ('x', _) => {
                push_sound(&mut out, 'k');
                (Some('s'), 1)
            }
            ('z', _) => (Some('s'), 1),
            ('v', _) => (Some('f'), 1),
            ('a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' | 'w', _) => (None, 1),
            (other, _) => (Some(other), 1),
        };
        if let Some(s) = sound {
            push_sound(&mut out, s);
        }
        i += step;
    }
    out
}

/// Two sound keys close enough to be the same name: identical, or one sound
/// apart when there are enough sounds for that to still be distinctive.
fn sounds_alike(a: &str, b: &str) -> bool {
    let shortest = a.len().min(b.len());
    match levenshtein(a, b) {
        0 => shortest >= 3,
        1 => shortest >= 4,
        _ => false,
    }
}

fn push_sound(out: &mut String, s: char) {
    if !out.ends_with(s) {
        out.push(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(n: &str) -> Book {
        Book::from_name(n).unwrap()
    }

    fn m(s: &str) -> Option<(Book, usize)> {
        let w: Vec<&str> = s.split(' ').collect();
        Books::get().match_at(&w, 0)
    }

    #[test]
    fn names_numbers_and_titles() {
        assert_eq!(m("john 3"), Some((b("John"), 1)));
        assert_eq!(m("1 john 3"), Some((b("1 John"), 2)));
        assert_eq!(m("second corinthian 5"), Some((b("2 Corinthians"), 2)));
        assert_eq!(m("timothy 3"), Some((b("1 Timothy"), 1)));
        let books = Books::get();
        assert_eq!(books.apply_titles("in the acts of the apostles chapter 2"), "in the acts chapter 2");
        assert_eq!(books.apply_titles("the book of isaiah chapter 53"), "the isaiah chapter 53");
        assert_eq!(books.apply_titles("second timothy 3"), "2 timothy 3");
        assert_eq!(books.apply_titles("the song of songs 2"), "the songofsolomon 2");
        assert_eq!(books.apply_titles("second kingship"), "second kingship", "only whole words");
    }

    #[test]
    fn near_misses_need_a_citation_shape() {
        assert_eq!(m("philippine chapter 4"), Some((b("Philippians"), 1)));
        assert_eq!(m("proverbate 18"), Some((b("Proverbs"), 1)));
        assert_eq!(m("strategical 4"), None);
        assert_eq!(m("philippine people"), None);
        assert_eq!(m("number two"), None);
        assert_eq!(m("number six verse 24"), Some((b("Numbers"), 1)));
    }

    #[test]
    fn sound_alikes_come_from_how_names_sound() {
        let books = Books::get();
        assert!(books.sound_alikes(b("John")).contains(&b("Jonah")));
        assert!(books.sound_alikes(b("Philippians")).contains(&b("Ephesians")));
        assert!(books.sound_alikes(b("Job")).contains(&b("Joel")));
    }
}
