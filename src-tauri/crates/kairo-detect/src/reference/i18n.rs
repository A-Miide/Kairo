//! Citations spoken in Spanish, Portuguese, French and German, rewritten into
//! the English tokens the grammar reads:
//!   "Juan capítulo tres versículo dieciséis"        → "john chapter 3 verse 16"
//!   "primera de Corintios trece, cuatro al siete"   → "1 corinthians 13 4 to 7"
//!   "Psaume quatre-vingt-onze verset un"            → "psalms 91 verse 1"
//!   "zweiter Korintherbrief fünf Vers siebzehn"     → "2 corinthians 5 verse 17"
//! Only citation-shaped words change (book names, chapter/verse words, number
//! words, connectors between numbers); everything else passes through.
//!
//! The tables are language data: each language's book names, ordinals,
//! number words and the words for chapter, verse, "to" and "and".

use std::collections::HashMap;
use std::sync::LazyLock;

use unicode_normalization::UnicodeNormalization;

use crate::re::re;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    Es,
    Pt,
    Fr,
    De,
}

impl Lang {
    /// From a language code ("es", "pt-BR", "en-US"); anything else is English.
    pub fn from_code(code: &str) -> Lang {
        match code.get(..2).unwrap_or("").to_ascii_lowercase().as_str() {
            "es" => Lang::Es,
            "pt" => Lang::Pt,
            "fr" => Lang::Fr,
            "de" => Lang::De,
            _ => Lang::En,
        }
    }
}

/// Accents removed, lowercased ("João" → "joao").
pub fn fold(s: &str) -> String {
    s.nfd().filter(|c| !('\u{0300}'..='\u{036f}').contains(c)).collect::<String>().to_lowercase()
}

struct Data {
    books: &'static [(&'static str, &'static [&'static str])],
    ordinals: &'static [(&'static str, u8)],
    ordinal_fillers: &'static [&'static str],
    chapter: &'static [&'static str],
    verse: &'static [&'static str],
    to: &'static [&'static str],
    and: &'static [&'static str],
    verse_articles: &'static [&'static str],
    triggers: &'static [(&'static str, &'static [&'static str])],
    units: &'static [&'static str],
    tens: &'static [(&'static str, u32)],
    hundred: &'static [&'static str],
    article_one: &'static [&'static str],
}

/// Books that take a 1/2/3 prefix, by their English stem.
const NUMBERED: &[&str] = &["Samuel", "Kings", "Chronicles", "Corinthians", "Thessalonians", "Timothy", "Peter", "John"];

const ES: Data = Data {
    books: &[
        ("Genesis", &["genesis"]), ("Exodus", &["exodo"]), ("Leviticus", &["levitico"]), ("Numbers", &["numeros"]), ("Deuteronomy", &["deuteronomio"]),
        ("Joshua", &["josue"]), ("Judges", &["jueces"]), ("Ruth", &["rut"]), ("Samuel", &["samuel"]), ("Kings", &["reyes"]), ("Chronicles", &["cronicas"]),
        ("Ezra", &["esdras"]), ("Nehemiah", &["nehemias"]), ("Esther", &["ester"]), ("Job", &["job"]), ("Psalms", &["salmos", "salmo"]), ("Proverbs", &["proverbios"]),
        ("Ecclesiastes", &["eclesiastes"]), ("Song of Solomon", &["cantar de los cantares", "cantares", "cantar"]), ("Isaiah", &["isaias"]), ("Jeremiah", &["jeremias"]),
        ("Lamentations", &["lamentaciones"]), ("Ezekiel", &["ezequiel"]), ("Daniel", &["daniel"]), ("Hosea", &["oseas"]), ("Joel", &["joel"]), ("Amos", &["amos"]),
        ("Obadiah", &["abdias"]), ("Jonah", &["jonas"]), ("Micah", &["miqueas"]), ("Nahum", &["nahum"]), ("Habakkuk", &["habacuc"]), ("Zephaniah", &["sofonias"]),
        ("Haggai", &["hageo"]), ("Zechariah", &["zacarias"]), ("Malachi", &["malaquias"]), ("Matthew", &["mateo"]), ("Mark", &["marcos"]), ("Luke", &["lucas"]),
        ("John", &["juan"]), ("Acts", &["hechos de los apostoles", "hechos"]), ("Romans", &["romanos"]), ("Corinthians", &["corintios"]), ("Galatians", &["galatas"]),
        ("Ephesians", &["efesios"]), ("Philippians", &["filipenses"]), ("Colossians", &["colosenses"]), ("Thessalonians", &["tesalonicenses"]), ("Timothy", &["timoteo"]),
        ("Titus", &["tito"]), ("Philemon", &["filemon"]), ("Hebrews", &["hebreos"]), ("James", &["santiago"]), ("Peter", &["pedro"]), ("Jude", &["judas"]),
        ("Revelation", &["apocalipsis"]),
    ],
    ordinals: &[("primera", 1), ("primero", 1), ("primer", 1), ("segunda", 2), ("segundo", 2), ("tercera", 3), ("tercero", 3), ("tercer", 3)],
    ordinal_fillers: &["de", "a los", "a", "carta de", "carta a los", "epistola de", "epistola a los", "libro de", "libro de los"],
    chapter: &["capitulo", "capitulos"],
    verse: &["versiculos", "versiculo", "versos", "verso"],
    to: &["hasta el", "hasta", "al", "a"],
    and: &["y"],
    verse_articles: &["del", "de"],
    triggers: &[("next verse", &["siguiente versiculo", "proximo versiculo", "versiculo siguiente"]), ("previous verse", &["versiculo anterior"])],
    units: &[
        "cero", "uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez", "once", "doce", "trece", "catorce", "quince",
        "dieciseis", "diecisiete", "dieciocho", "diecinueve", "veinte", "veintiuno", "veintidos", "veintitres", "veinticuatro", "veinticinco",
        "veintiseis", "veintisiete", "veintiocho", "veintinueve",
    ],
    tens: &[("veinte", 20), ("treinta", 30), ("cuarenta", 40), ("cincuenta", 50), ("sesenta", 60), ("setenta", 70), ("ochenta", 80), ("noventa", 90)],
    hundred: &["ciento", "cien"],
    article_one: &["un", "una"],
};

const PT: Data = Data {
    books: &[
        ("Genesis", &["genesis"]), ("Exodus", &["exodo"]), ("Leviticus", &["levitico"]), ("Numbers", &["numeros"]), ("Deuteronomy", &["deuteronomio"]),
        ("Joshua", &["josue"]), ("Judges", &["juizes"]), ("Ruth", &["rute"]), ("Samuel", &["samuel"]), ("Kings", &["reis"]), ("Chronicles", &["cronicas"]),
        ("Ezra", &["esdras"]), ("Nehemiah", &["neemias"]), ("Esther", &["ester"]), ("Job", &["jo"]), ("Psalms", &["salmos", "salmo"]), ("Proverbs", &["proverbios"]),
        ("Ecclesiastes", &["eclesiastes"]), ("Song of Solomon", &["cantico dos canticos", "canticos", "cantares"]), ("Isaiah", &["isaias"]), ("Jeremiah", &["jeremias"]),
        ("Lamentations", &["lamentacoes"]), ("Ezekiel", &["ezequiel"]), ("Daniel", &["daniel"]), ("Hosea", &["oseias"]), ("Joel", &["joel"]), ("Amos", &["amos"]),
        ("Obadiah", &["obadias"]), ("Jonah", &["jonas"]), ("Micah", &["miqueias"]), ("Nahum", &["naum"]), ("Habakkuk", &["habacuque"]), ("Zephaniah", &["sofonias"]),
        ("Haggai", &["ageu"]), ("Zechariah", &["zacarias"]), ("Malachi", &["malaquias"]), ("Matthew", &["mateus"]), ("Mark", &["marcos"]), ("Luke", &["lucas"]),
        ("John", &["joao"]), ("Acts", &["atos dos apostolos", "atos"]), ("Romans", &["romanos"]), ("Corinthians", &["corintios"]), ("Galatians", &["galatas"]),
        ("Ephesians", &["efesios"]), ("Philippians", &["filipenses"]), ("Colossians", &["colossenses"]), ("Thessalonians", &["tessalonicenses"]), ("Timothy", &["timoteo"]),
        ("Titus", &["tito"]), ("Philemon", &["filemom", "filemon"]), ("Hebrews", &["hebreus"]), ("James", &["tiago"]), ("Peter", &["pedro"]), ("Jude", &["judas"]),
        ("Revelation", &["apocalipse"]),
    ],
    ordinals: &[("primeira", 1), ("primeiro", 1), ("segunda", 2), ("segundo", 2), ("terceira", 3), ("terceiro", 3)],
    ordinal_fillers: &["de", "aos", "a", "carta de", "carta aos", "epistola de", "epistola aos", "livro de", "livro dos"],
    chapter: &["capitulo", "capitulos"],
    verse: &["versiculos", "versiculo", "versos", "verso"],
    to: &["ate o", "ate", "ao", "a"],
    and: &["e"],
    verse_articles: &["do", "de"],
    triggers: &[("next verse", &["proximo versiculo", "versiculo seguinte"]), ("previous verse", &["versiculo anterior"])],
    units: &[
        "zero", "um", "dois", "tres", "quatro", "cinco", "seis", "sete", "oito", "nove", "dez", "onze", "doze", "treze", "catorze", "quinze",
        "dezesseis", "dezessete", "dezoito", "dezenove",
    ],
    tens: &[("vinte", 20), ("trinta", 30), ("quarenta", 40), ("cinquenta", 50), ("sessenta", 60), ("setenta", 70), ("oitenta", 80), ("noventa", 90)],
    hundred: &["cento", "cem"],
    article_one: &["um", "uma"],
};

const FR: Data = Data {
    books: &[
        ("Genesis", &["genese"]), ("Exodus", &["exode"]), ("Leviticus", &["levitique"]), ("Numbers", &["nombres"]), ("Deuteronomy", &["deuteronome"]),
        ("Joshua", &["josue"]), ("Judges", &["juges"]), ("Ruth", &["ruth"]), ("Samuel", &["samuel"]), ("Kings", &["rois"]), ("Chronicles", &["chroniques"]),
        ("Ezra", &["esdras"]), ("Nehemiah", &["nehemie"]), ("Esther", &["esther"]), ("Job", &["job"]), ("Psalms", &["psaumes", "psaume"]), ("Proverbs", &["proverbes"]),
        ("Ecclesiastes", &["ecclesiaste"]), ("Song of Solomon", &["cantique des cantiques", "cantique"]), ("Isaiah", &["esaie", "isaie"]), ("Jeremiah", &["jeremie"]),
        ("Lamentations", &["lamentations"]), ("Ezekiel", &["ezechiel"]), ("Daniel", &["daniel"]), ("Hosea", &["osee"]), ("Joel", &["joel"]), ("Amos", &["amos"]),
        ("Obadiah", &["abdias"]), ("Jonah", &["jonas"]), ("Micah", &["michee"]), ("Nahum", &["nahum"]), ("Habakkuk", &["habacuc"]), ("Zephaniah", &["sophonie"]),
        ("Haggai", &["aggee"]), ("Zechariah", &["zacharie"]), ("Malachi", &["malachie"]), ("Matthew", &["matthieu"]), ("Mark", &["marc"]), ("Luke", &["luc"]),
        ("John", &["jean"]), ("Acts", &["actes des apotres", "actes"]), ("Romans", &["romains"]), ("Corinthians", &["corinthiens"]), ("Galatians", &["galates"]),
        ("Ephesians", &["ephesiens"]), ("Philippians", &["philippiens"]), ("Colossians", &["colossiens"]), ("Thessalonians", &["thessaloniciens"]), ("Timothy", &["timothee"]),
        ("Titus", &["tite"]), ("Philemon", &["philemon"]), ("Hebrews", &["hebreux"]), ("James", &["jacques"]), ("Peter", &["pierre"]), ("Jude", &["jude"]),
        ("Revelation", &["apocalypse"]),
    ],
    ordinals: &[("premier", 1), ("premiere", 1), ("deuxieme", 2), ("second", 2), ("seconde", 2), ("troisieme", 3)],
    ordinal_fillers: &["aux", "a", "de", "epitre de", "epitre aux", "lettre de", "lettre aux", "livre de", "livre des"],
    chapter: &["chapitre", "chapitres"],
    verse: &["versets", "verset"],
    to: &["jusqu au", "jusqu a", "au", "a"],
    and: &["et"],
    verse_articles: &["du", "de"],
    triggers: &[("next verse", &["verset suivant", "prochain verset"]), ("previous verse", &["verset precedent"])],
    units: &[
        "zero", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze", "douze", "treize", "quatorze", "quinze",
        "seize", "dix sept", "dix huit", "dix neuf",
    ],
    tens: &[
        ("vingt", 20), ("trente", 30), ("quarante", 40), ("cinquante", 50), ("soixante", 60), ("soixante dix", 70), ("quatre vingt", 80),
        ("quatre vingts", 80), ("quatre vingt dix", 90),
    ],
    hundred: &["cent"],
    article_one: &["un", "une"],
};

const DE: Data = Data {
    books: &[
        ("Genesis", &["genesis", "erste mose", "1 mose"]), ("Exodus", &["exodus", "zweite mose", "2 mose"]), ("Leviticus", &["levitikus", "dritte mose", "3 mose"]),
        ("Numbers", &["numeri", "vierte mose", "4 mose"]), ("Deuteronomy", &["deuteronomium", "funfte mose", "5 mose"]),
        ("Joshua", &["josua"]), ("Judges", &["richter"]), ("Ruth", &["rut", "ruth"]), ("Samuel", &["samuel"]), ("Kings", &["konige"]), ("Chronicles", &["chronik"]),
        ("Ezra", &["esra"]), ("Nehemiah", &["nehemia"]), ("Esther", &["ester", "esther"]), ("Job", &["hiob", "ijob"]), ("Psalms", &["psalmen", "psalm"]),
        ("Proverbs", &["spruche", "sprichworter"]), ("Ecclesiastes", &["prediger", "kohelet"]), ("Song of Solomon", &["hohelied"]), ("Isaiah", &["jesaja"]),
        ("Jeremiah", &["jeremia"]), ("Lamentations", &["klagelieder"]), ("Ezekiel", &["hesekiel", "ezechiel"]), ("Daniel", &["daniel"]), ("Hosea", &["hosea"]),
        ("Joel", &["joel"]), ("Amos", &["amos"]), ("Obadiah", &["obadja"]), ("Jonah", &["jona"]), ("Micah", &["micha"]), ("Nahum", &["nahum"]), ("Habakkuk", &["habakuk"]),
        ("Zephaniah", &["zefanja", "zephanja"]), ("Haggai", &["haggai"]), ("Zechariah", &["sacharja"]), ("Malachi", &["maleachi"]), ("Matthew", &["matthaus"]),
        ("Mark", &["markus"]), ("Luke", &["lukas"]), ("John", &["johannes"]), ("Acts", &["apostelgeschichte"]), ("Romans", &["romer"]), ("Corinthians", &["korinther"]),
        ("Galatians", &["galater"]), ("Ephesians", &["epheser"]), ("Philippians", &["philipper"]), ("Colossians", &["kolosser"]), ("Thessalonians", &["thessalonicher"]),
        ("Timothy", &["timotheus"]), ("Titus", &["titus"]), ("Philemon", &["philemon"]), ("Hebrews", &["hebraer"]), ("James", &["jakobus"]), ("Peter", &["petrus"]),
        ("Jude", &["judas"]), ("Revelation", &["offenbarung"]),
    ],
    ordinals: &[
        ("erste", 1), ("erster", 1), ("ersten", 1), ("erstes", 1), ("zweite", 2), ("zweiter", 2), ("zweiten", 2), ("zweites", 2),
        ("dritte", 3), ("dritter", 3), ("dritten", 3), ("drittes", 3),
    ],
    ordinal_fillers: &["brief an die", "an die", "brief des", "buch der", "buch"],
    chapter: &["kapitel"],
    verse: &["verse", "vers"],
    to: &["bis zu", "bis"],
    and: &["und"],
    verse_articles: &["von"],
    triggers: &[
        ("next verse", &["nachste vers", "nachster vers", "nachsten vers", "folgende vers", "folgenden vers"]),
        ("previous verse", &["vorherige vers", "vorherigen vers"]),
    ],
    units: &[
        "null", "eins", "zwei", "drei", "vier", "funf", "sechs", "sieben", "acht", "neun", "zehn", "elf", "zwolf", "dreizehn", "vierzehn",
        "funfzehn", "sechzehn", "siebzehn", "achtzehn", "neunzehn",
    ],
    tens: &[("zwanzig", 20), ("dreissig", 30), ("vierzig", 40), ("funfzig", 50), ("sechzig", 60), ("siebzig", 70), ("achtzig", 80), ("neunzig", 90)],
    hundred: &["hundert", "einhundert"],
    article_one: &["ein", "eine"],
};

/// German letter compounds ("erster Korintherbrief") and the five books of
/// Moses by number ("erstes Buch Mose").
const DE_LETTER_COMPOUNDS: &[(&str, &str)] = &[
    ("Corinthians", "korintherbrief"), ("Thessalonians", "thessalonicherbrief"), ("Timothy", "timotheusbrief"), ("Peter", "petrusbrief"), ("John", "johannesbrief"),
];
const DE_MOSE: &[(&str, u8, &[&str])] = &[
    ("Genesis", 1, &["erste", "erstes", "ersten"]), ("Exodus", 2, &["zweite", "zweites", "zweiten"]), ("Leviticus", 3, &["dritte", "drittes", "dritten"]),
    ("Numbers", 4, &["vierte", "viertes", "vierten"]), ("Deuteronomy", 5, &["funfte", "funftes", "funften"]),
];

struct Compiled {
    data: &'static Data,
    lang: Lang,
    /// phrase → English tokens, longest phrase first
    phrases: Vec<(String, String)>,
    /// English book stems ("john", "corinthians") — citation positions for "un"
    book_stems: Vec<String>,
    nums: HashMap<String, u32>,
    article: regex::Regex,
    to: regex::Regex,
    and: regex::Regex,
}

fn data(lang: Lang) -> Option<&'static Data> {
    match lang {
        Lang::Es => Some(&ES),
        Lang::Pt => Some(&PT),
        Lang::Fr => Some(&FR),
        Lang::De => Some(&DE),
        Lang::En => None,
    }
}

fn alternation(list: &[&str]) -> String {
    let mut v: Vec<&str> = list.to_vec();
    v.sort_by_key(|w| std::cmp::Reverse(w.len()));
    v.iter().map(|w| regex::escape(w)).collect::<Vec<_>>().join("|")
}

fn compile(lang: Lang) -> Compiled {
    let d = data(lang).expect("only non-English languages compile");
    let mut phrases: Vec<(String, String)> = Vec::new();
    let mut add = |from: String, to: String| {
        if !phrases.iter().any(|(f, _)| *f == from) {
            phrases.push((from, to));
        }
    };
    let mut book_stems = Vec::new();
    for (en, names) in d.books {
        let target = en.to_lowercase();
        book_stems.push(target.rsplit(' ').next().unwrap().to_string());
        let compound = if lang == Lang::De { DE_LETTER_COMPOUNDS.iter().find(|(e, _)| e == en).map(|(_, c)| *c) } else { None };
        for name in names.iter().copied().chain(compound) {
            add(name.to_string(), target.clone());
            if !NUMBERED.contains(en) {
                continue;
            }
            for (o, n) in d.ordinals {
                add(format!("{o} {name}"), format!("{n} {target}"));
                for f in d.ordinal_fillers {
                    add(format!("{o} {f} {name}"), format!("{n} {target}"));
                }
            }
        }
    }
    if lang == Lang::De {
        for (en, n, ords) in DE_MOSE {
            let target = en.to_lowercase();
            add(format!("{n} mose"), target.clone());
            add(format!("{n} buch mose"), target.clone());
            for o in *ords {
                add(format!("{o} mose"), target.clone());
                add(format!("{o} buch mose"), target.clone());
            }
        }
    }
    for (en, list) in d.triggers {
        for w in *list {
            add(w.to_string(), en.to_string());
        }
    }
    for w in d.chapter {
        add(w.to_string(), "chapter".into());
    }
    for w in d.verse {
        add(w.to_string(), "verse".into());
    }
    phrases.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));

    let mut nums: HashMap<String, u32> = d.units.iter().enumerate().map(|(i, w)| (w.to_string(), i as u32)).collect();
    if lang == Lang::Pt {
        nums.insert("duas".into(), 2);
    }
    for (w, n) in d.tens {
        nums.insert(w.to_string(), *n);
    }
    let articles = alternation(d.verse_articles);
    Compiled {
        data: d,
        lang,
        phrases,
        book_stems,
        nums,
        article: re(&format!(r"(?-u:\b)(chapter|verse) (?:{articles}) ([0-9])")),
        to: re(&format!(r"([0-9]) (?:{}) (?:(?:verse|{articles}) )?([0-9])", alternation(d.to))),
        and: re(&format!(r"([0-9]) (?:{}) (?:verse )?([0-9])", alternation(d.and))),
    }
}

static COMPILED: LazyLock<HashMap<Lang, Compiled>> =
    LazyLock::new(|| [Lang::Es, Lang::Pt, Lang::Fr, Lang::De].into_iter().map(|l| (l, compile(l))).collect());

/// A spoken citation in `lang` as English citation tokens. English passes
/// through untouched.
pub fn localize(text: &str, lang: Lang) -> String {
    let Some(c) = COMPILED.get(&lang) else { return text.to_string() };
    static DE_ORDINAL_DOT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)([1-5])\.\s"));
    static NUM_SEP: LazyLock<regex::Regex> = LazyLock::new(|| re(r"([0-9])\s*[:.]\s*([0-9])"));
    let mut t = fold(text).replace('ß', "ss");
    if lang == Lang::De {
        t = DE_ORDINAL_DOT.replace_all(&t, "$1 ").into_owned();
    }
    t = NUM_SEP.replace_all(&t, "$1 $2").into_owned();
    let t: String = t.chars().map(|ch| if ".,;:!?¿¡\"()-'".contains(ch) { ' ' } else { ch }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = replace_phrases(&t, &c.phrases);
    let t = numbers_to_digits(&t.split(' ').collect::<Vec<_>>(), c).join(" ");
    let t = c.article.replace_all(&t, "$1 $2");
    let t = c.to.replace_all(&t, "$1 to $2");
    c.and.replace_all(&t, "$1 and $2").into_owned()
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// Whole-phrase replacement, longest phrase first at each position.
fn replace_phrases(t: &str, phrases: &[(String, String)]) -> String {
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        let at_start = i == 0 || !t[..i].ends_with(is_word_char);
        let hit = at_start
            .then(|| {
                phrases.iter().find(|(p, _)| t[i..].starts_with(p.as_str()) && !t[i + p.len()..].starts_with(is_word_char))
            })
            .flatten();
        match hit {
            Some((p, to)) => {
                out.push_str(to);
                i += p.len();
            }
            None => {
                let ch = t[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// German writes numbers as one word: "dreiundzwanzig", "hundertneunzehn".
fn german_number(w: &str, nums: &HashMap<String, u32>) -> Option<u32> {
    if let Some(n) = nums.get(w) {
        return Some(*n);
    }
    for h in ["einhundert", "hundert"] {
        if let Some(rest) = w.strip_prefix(h).filter(|r| !r.is_empty()) {
            return german_number(rest, nums).map(|r| 100 + r);
        }
    }
    let (u, t) = w.split_once("und")?;
    let unit = if u == "ein" { Some(1) } else { nums.get(u).copied() }?;
    let tens = DE.tens.iter().find(|(word, _)| *word == t).map(|(_, n)| *n)?;
    (unit > 0 && unit < 10).then_some(tens + unit)
}

fn numbers_to_digits(words: &[&str], c: &Compiled) -> Vec<String> {
    let d = c.data;
    let citation_slot = |prev: Option<&String>| {
        prev.is_some_and(|p| p == "chapter" || p == "verse" || p.bytes().all(|b| b.is_ascii_digit()) || c.book_stems.iter().any(|s| s == p))
    };
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if c.lang == Lang::De {
            if let Some(n) = german_number(w, &c.nums).filter(|n| *n > 0) {
                out.push(n.to_string());
                i += 1;
                continue;
            }
        }
        if d.article_one.contains(&w) {
            out.push(if citation_slot(out.last()) { "1".into() } else { w.into() });
            i += 1;
            continue;
        }
        let mut value: Option<u32> = None;
        let mut j = i;
        if d.hundred.contains(&w) {
            value = Some(100);
            j = i + 1;
            if words.get(j).is_some_and(|x| d.and.contains(x)) && words.get(j + 1).is_some_and(|x| c.nums.contains_key(*x)) {
                j += 1;
            }
        }
        let part = |j: &mut usize| -> Option<u32> {
            for len in [3, 2] {
                if *j + len <= words.len() {
                    if let Some(n) = c.nums.get(&words[*j..*j + len].join(" ")) {
                        *j += len;
                        return Some(*n);
                    }
                }
            }
            let n = words.get(*j).and_then(|x| c.nums.get(*x)).copied();
            if n.is_some() {
                *j += 1;
            }
            n
        };
        if let Some(first) = part(&mut j) {
            let mut v = value.unwrap_or(0) + first;
            if first >= 20 && first % 10 == 0 {
                let mut k = j;
                if words.get(k).is_some_and(|x| d.and.contains(x)) {
                    k += 1;
                }
                let unit = words.get(k).and_then(|x| if matches!(*x, "un" | "une" | "uma" | "una") { Some(1) } else { c.nums.get(*x).copied() });
                let max_unit = if c.lang == Lang::Fr && (first == 60 || first == 80) { 20 } else { 10 };
                if let Some(u) = unit.filter(|u| *u > 0 && *u < max_unit) {
                    v += u;
                    j = k + 1;
                }
            }
            value = Some(v);
        }
        match value.filter(|v| *v > 0) {
            Some(v) => {
                out.push(v.to_string());
                i = j.max(i + 1);
            }
            None => {
                out.push(w.to_string());
                i += 1;
            }
        }
    }
    out
}
