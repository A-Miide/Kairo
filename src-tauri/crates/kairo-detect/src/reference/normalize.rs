//! Speech → citation-ready text: lowercase words, single spaces, numbers in a
//! form the grammar can read, and the colon kept as its own token (it says
//! which number is the verse).
//!
//! Each pass handles one way speech-to-text formats a spoken citation:
//! - Roman numerals after "chapter"/"verse" ("capítulo III" → 3), never a lone
//!   "i" ("the verse I love").
//! - Possessives ("John's gospel" → "john gospel").
//! - Digits of one number split by a space next to a colon ("3:2 7" → 3:27,
//!   "4 1: 1" → 41:1).
//! - Tens figure + units word before a colon ("60 six:" → 66); a colon
//!   between numbers is the chapter/verse break; an ascending pair of number
//!   words joined by a hyphen is a range ("five-seven"), unless it is one
//!   compound number ("twenty-four").
//! - A comma between two numbers is a list ("4, 5 and 6"), never for words
//!   that only double as numbers ("verse 12, for we wrestle").
//! - The word "colon"; a dash between digits is "to".
//! - Stutters ("in in", "of of") collapse; a repeated number word stays
//!   ("one one zero" is 110).
//! - Ordinal forms: "the twenty third psalm" → "psalm 23"; "the eighth
//!   chapter of" → "chapter 8 of" when a passage follows; "the second epistle
//!   of Timothy" → "second timothy"; "4 to the end (of the chapter)" →
//!   "4 to end" (the grammar reads `end` as the chapter's last verse).
//! - Self-corrections: only what the preacher settled on is kept ("3 17 sorry
//!   16" → 3 16; "8 28 I mean 8 29" → 8 29; "3 7 3 17 sorry" → 3 17).

use std::sync::LazyLock;

use super::books::{numbered_prefix, Books};
use super::i18n::{localize, Lang};
use super::numbers::{consume_number, is_ambiguous_number_word, plain_number, spoken_to_number, word_value, ORDINALS, ORDINALS_ORDERED, WORD_TO_NUM_ORDERED};
use crate::re::{fre, freplace, re, words, WB};

pub fn normalize(text: &str, books: &Books, lang: Lang) -> String {
    let t = if lang == Lang::En { text.to_lowercase() } else { localize(text, lang) };
    let t = drop_fillers(&t);
    let t = roman_after_keywords(&t);
    let t = if lang == Lang::En { spoken_variants(&t, books) } else { t };
    let t = drop_possessives(&t);
    let t = join_split_digits(&t);
    let t = number_punctuation(&t);
    let t = comma_lists(&t);
    let t = spacing(&t);
    let t = collapse_stutters(&t);
    let t = ordinal_forms(&t, books);
    self_corrections(&t)
}

/// Ways a citation is said, or written by a recogniser, that the grammar never sees as such: Roman numerals
/// and "Jon" for a numbered John, "John the first", "twenty and eight", "twenty-eighth", "point"/"stroke"
/// for the colon, "versus"/"vs"/"v" for verse, "one-ten" for Psalm 110, accent spellings ("tree", "wan",
/// "fo", "tin") and "nought"/"nil" for zero. Each rewrite needs the citation's own context (a book, a
/// verse or chapter word, a neighbouring number) so ordinary speech is left alone.
fn spoken_variants(t: &str, books: &Books) -> String {
    static NUM: LazyLock<String> = LazyLock::new(|| {
        let mut keys: Vec<&str> = WORD_TO_NUM_ORDERED.iter().map(|(w, _)| w.as_str()).collect();
        keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
        format!("{}|[0-9]{{1,3}}", keys.join("|"))
    });
    const NUMBERED: &str = "john|peter|corinthians|thessalonians|timothy|kings|samuel|chronicles";
    const TENS: &str = "twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety";
    const TEENS: &str = "ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen";
    // "I John four four", "II Timothy one seven": the numeral is the book's number.
    static ROMAN_BOOK: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}(iii|ii|i) ({NUMBERED}){WB}(?= (?:chapter|{}){WB})", *NUM)));
    // "first Jon four four": no book is called "first Jonah", so the recogniser dropped the "h" of John.
    static JON: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)(first|second|third|1st|2nd|3rd|one|two|three) jon(?-u:\b)"));
    // "John the first, four four", "Timothy the second chapter one verse seven".
    static BOOK_THE_ORD: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}({NUMBERED}) the (first|second|third),?(?= (?:chapter|{}){WB})", *NUM)));
    // "verse twenty and eight" is 28.
    static TENS_AND: LazyLock<regex::Regex> =
        LazyLock::new(|| re(&format!(r"(?-u:\b)(verses?|chapter) ({TENS}) and (one|two|three|four|five|six|seven|eight|nine)(?-u:\b)")));
    // "Romans eight twenty-eighth" is 8:28.
    static TENS_ORD: LazyLock<regex::Regex> = LazyLock::new(|| {
        re(&format!(r"(?-u:\b)({}|verse|verses|chapter) ({TENS})[ -](first|second|third|fourth|fifth|sixth|seventh|eighth|ninth)(?-u:\b)", *NUM))
    });
    // "Romans eight point twenty eight": a point, stroke or slash after a book and its chapter is the colon.
    static SEPARATOR: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}([a-z]+) ({}) (?:point|stroke|slash|dot) (?=(?:{}){WB})", *NUM, *NUM)));
    // "Psalm one ten, versus one", "ten vs 1", "ten v one".
    static VERSUS: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}({}),? (?:versus|vss?|v)\.? (?=(?:{}){WB})", *NUM, *NUM)));
    // "Elihu said in chapter thirty two and verse eight of Job": the book said last, put first where the grammar looks.
    static BOOK_LAST: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
        fre(&format!(
            r"{WB}chapter ((?:(?:{TENS})[ -])?(?:{n})),? (?:and )?(?:the )?verses? ((?:(?:{TENS})[ -])?(?:{n})) of (?:the )?(?:book of )?((?:(?:first|second|third|[123]) )?[a-z]+){WB}",
            n = *NUM
        ))
    });
    static VERSE_OF_CHAPTER_OF_BOOK: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
        fre(&format!(
            r"{WB}verses? ((?:(?:{TENS})[ -])?(?:{n})) of (?:the )?chapter ((?:(?:{TENS})[ -])?(?:{n})) of (?:the )?(?:book of )?((?:(?:first|second|third|[123]) )?[a-z]+){WB}",
            n = *NUM
        ))
    });
    // "five dash seven": a spoken dash is "to".
    static DASH: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}({}) (?:dash|hyphen) (?=(?:{}){WB})", *NUM, *NUM)));
    // "Psalm one-ten" is 110, not a range of psalms; "Psalm one oh ten" likewise.
    static PSALM_ONE_HYPHEN: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}(psalms?) one-(?=(?:{TEENS}|{TENS}){WB})")));
    static PSALM_ONE_OH: LazyLock<regex::Regex> =
        LazyLock::new(|| re(&format!(r"(?-u:\b)(psalms?) one (?:oh|zero|nought|naught|nil) ({TEENS})(?-u:\b)")));
    static FOURTY: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)fourty(?-u:\b)"));
    // Accent spellings and zero words, read as numbers only beside a citation's own words.
    static ACCENT: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
        fre(&format!(
            r"{WB}([a-z]+) (tree|thri|free|fo|wan|tin|nought|naught|nil){WB}(?= (?:{n}|tree|thri|free|fo|wan|tin|verse|verses|chapter|to|and){WB}|\s*[:,.]|\s*$)",
            n = *NUM
        ))
    });
    let accent_value = |w: &str| match w {
        "tree" | "thri" | "free" => "three",
        "fo" => "four",
        "wan" => "one",
        "tin" => "ten",
        _ => "zero",
    };

    let mut t = t.to_string();
    if t.contains("fourty") {
        t = FOURTY.replace_all(&t, "forty").into_owned();
    }
    if t.contains("i ") || t.contains("ii") {
        t = freplace(&ROMAN_BOOK, &t, |c| format!("{} {}", match &c[1] { "i" => "first", "ii" => "second", _ => "third" }, &c[2])).into_owned();
    }
    if t.contains(" jon") {
        t = JON.replace_all(&t, "$1 john").into_owned();
    }
    if t.contains(" the ") {
        t = freplace(&BOOK_THE_ORD, &t, |c| format!("{} {}", &c[2], &c[1])).into_owned();
    }
    if t.contains(" and ") {
        t = TENS_AND.replace_all(&t, "$1 $2 $3").into_owned();
    }
    if ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth"].iter().any(|o| t.contains(o)) {
        t = TENS_ORD
            .replace_all(&t, |c: &regex::Captures| {
                let unit = match &c[3] {
                    "first" => "one",
                    "second" => "two",
                    "third" => "three",
                    "fourth" => "four",
                    "fifth" => "five",
                    "sixth" => "six",
                    "seventh" => "seven",
                    "eighth" => "eight",
                    _ => "nine",
                };
                format!("{} {} {}", &c[1], &c[2], unit)
            })
            .into_owned();
    }
    if t.contains(" point ") || t.contains(" stroke ") || t.contains(" slash ") || t.contains(" dot ") {
        t = freplace(&SEPARATOR, &t, |c| if books.is_single_word(&c[1]) { format!("{} {} : ", &c[1], &c[2]) } else { c[0].to_string() }).into_owned();
    }
    if t.contains("chapter") && t.contains(" of ") {
        let is_book = |b: &str| b.rsplit(' ').next().is_some_and(|w| books.is_single_word(w) || books.alias(w).is_some());
        t = freplace(&BOOK_LAST, &t, |c| if is_book(&c[3]) { format!("{} chapter {} verse {}", &c[3], &c[1], &c[2]) } else { c[0].to_string() }).into_owned();
        t = freplace(&VERSE_OF_CHAPTER_OF_BOOK, &t, |c| if is_book(&c[3]) { format!("{} chapter {} verse {}", &c[3], &c[2], &c[1]) } else { c[0].to_string() }).into_owned();
    }
    if t.contains(" dash ") || t.contains(" hyphen ") {
        t = freplace(&DASH, &t, |c| format!("{} to ", &c[1])).into_owned();
    }
    if t.contains(" v ") || t.contains(" vs") || t.contains("versus") {
        t = freplace(&VERSUS, &t, |c| format!("{} verse ", &c[1])).into_owned();
    }
    if t.contains("psalm") {
        t = freplace(&PSALM_ONE_HYPHEN, &t, |c| format!("{} one ", &c[1])).into_owned();
        t = PSALM_ONE_OH.replace_all(&t, "$1 one $2").into_owned();
    }
    // One word at a time, so "wan wan zero" settles from left to right.
    for _ in 0..4 {
        let next = freplace(&ACCENT, &t, |c| {
            let (prev, word) = (&c[1], &c[2]);
            let zero = matches!(word, "nought" | "naught" | "nil");
            let numeric = plain_number(prev).is_some() || matches!(prev, "oh" | "hundred") || is_accent_word(prev);
            let announced = books.is_single_word(prev) || matches!(prev, "chapter" | "verse" | "verses" | "and");
            let ok = if zero { numeric } else if word == "tin" { numeric || prev == "and" } else { numeric || announced };
            if ok { format!("{prev} {}", accent_value(word)) } else { c[0].to_string() }
        })
        .into_owned();
        if next == t {
            break;
        }
        t = next;
    }
    t
}

fn is_accent_word(w: &str) -> bool {
    matches!(w, "tree" | "thri" | "free" | "fo" | "wan" | "tin")
}

/// Hesitations inside a citation: "in chapter um one verse thirteen" (Piper, dev build 2026-10-21).
fn drop_fillers(t: &str) -> String {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)(?:um+|uh+|uhm|erm|er)(?-u:\b)[,.]?\s*"));
    RE.replace_all(t, "").into_owned()
}

fn roman_after_keywords(t: &str) -> String {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)(chapter|chapters|verse|verses)(\s+)([ivxlc]+)(?-u:\b)"));
    static ROMAN: LazyLock<regex::Regex> = LazyLock::new(|| re(r"^(c{0,1})(xc|xl|l?x{0,3})(ix|iv|v?i{0,3})$"));
    RE.replace_all(t, |c: &regex::Captures| {
        let r = &c[3];
        if r == "i" || !ROMAN.is_match(r) {
            return c[0].to_string();
        }
        format!("{}{}{}", &c[1], &c[2], roman_value(r))
    })
    .into_owned()
}

fn roman_value(r: &str) -> u32 {
    let v = |c: u8| match c {
        b'i' => 1,
        b'v' => 5,
        b'x' => 10,
        b'l' => 50,
        b'c' => 100,
        _ => 0,
    };
    let b = r.as_bytes();
    let mut n: i64 = 0;
    for k in 0..b.len() {
        let here = v(b[k]) as i64;
        let next = b.get(k + 1).map_or(0, |&c| v(c) as i64);
        n += if here < next { -here } else { here };
    }
    n.max(0) as u32
}

fn drop_possessives(t: &str) -> String {
    static S: LazyLock<regex::Regex> = LazyLock::new(|| re(r"([a-z])['’]s(?-u:\b)"));
    static BARE: LazyLock<regex::Regex> = LazyLock::new(|| re(r"([a-z])['’](\s|$)"));
    let t = S.replace_all(t, "$1");
    BARE.replace_all(&t, "$1$2").into_owned()
}

fn join_split_digits(t: &str) -> String {
    static VERSE: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"{WB}([0-9]{{1,3}}):([0-9]) ([0-9])(?![0-9:])(?!\s+[0-9])")));
    static CHAPTER: LazyLock<fancy_regex::Regex> =
        LazyLock::new(|| fre(&format!(r"(?<![0-9:])(?<![0-9:]\s){WB}([0-9]) ([0-9]):(?=\s*[0-9])")));
    let t = freplace(&VERSE, t, |c| format!("{}:{}{}", &c[1], &c[2], &c[3]));
    freplace(&CHAPTER, &t, |c| format!("{}{}:", &c[1], &c[2])).into_owned()
}

fn number_punctuation(t: &str) -> String {
    static TENS_UNIT: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fre(&format!(r"{WB}([2-9])0 ([a-z]+)(?=\s*:)")));
    static COLON: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)([a-z0-9]+)\s*:\s*([a-z0-9]+)(?-u:\b)"));
    static HYPHEN: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)([a-z]+)-([a-z]+)(?-u:\b)"));
    let t = freplace(&TENS_UNIT, t, |c| match plain_number(&c[2]) {
        Some(u) if (1..=9).contains(&u) => format!("{}{}", &c[1], u),
        _ => c[0].to_string(),
    });
    let t = COLON.replace_all(&t, |c: &regex::Captures| {
        if plain_number(&c[1]).is_some() && plain_number(&c[2]).is_some() {
            format!("{} : {}", &c[1], &c[2])
        } else {
            c[0].to_string()
        }
    });
    HYPHEN
        .replace_all(&t, |c: &regex::Captures| match (plain_number(&c[1]), plain_number(&c[2])) {
            (Some(x), Some(y)) if !(x >= 20 && x % 10 == 0 && (1..=9).contains(&y)) && y > x => format!("{} to {}", &c[1], &c[2]),
            _ => c[0].to_string(),
        })
        .into_owned()
}

fn is_list_number(w: &str) -> bool {
    (!w.is_empty() && w.bytes().all(|b| b.is_ascii_digit())) || (word_value(w).is_some() && !is_ambiguous_number_word(w) && w != "oh")
}

fn comma_lists(t: &str) -> String {
    static RE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fre(r"([A-Za-z0-9_-]+)\s*,\s*(?=([A-Za-z0-9_-]+))"));
    freplace(&RE, t, |c| if is_list_number(&c[1]) && is_list_number(&c[2]) { format!("{} and ", &c[1]) } else { c[0].to_string() })
        .into_owned()
}

fn spacing(t: &str) -> String {
    static COLON_WORD: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)colon(?-u:\b)"));
    static DIGIT_COLON: LazyLock<regex::Regex> = LazyLock::new(|| re(r"([0-9])\s*:\s*([0-9])"));
    static DIGIT_DASH: LazyLock<regex::Regex> = LazyLock::new(|| re(r"([0-9])\s*-\s*([0-9])"));
    static WORD_DASH: LazyLock<regex::Regex> = LazyLock::new(|| re(r"([a-z])-([a-z])"));
    let t: String = t.chars().map(|c| if matches!(c, '.' | ',' | '!' | '?' | ';') { ' ' } else { c }).collect();
    let t = COLON_WORD.replace_all(&t, ":");
    let t = DIGIT_COLON.replace_all(&t, "$1 : $2");
    let t = DIGIT_DASH.replace_all(&t, "$1 to $2");
    let t = WORD_DASH.replace_all(&t, "$1 $2");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapse_stutters(t: &str) -> String {
    static RE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fre(&format!(r"{WB}([a-z]+)(?: \1{WB})+")));
    freplace(&RE, t, |c| if spoken_to_number(&c[1]).is_some() { c[0].to_string() } else { c[1].to_string() }).into_owned()
}

fn ordinal_forms(t: &str, books: &Books) -> String {
    static TO_THE_END: LazyLock<regex::Regex> = LazyLock::new(|| {
        let nums: Vec<&str> = WORD_TO_NUM_ORDERED.iter().map(|(w, _)| w.as_str()).collect();
        re(&format!(
            r"(?-u:\b)([0-9]{{1,3}}|{}) (to|till|until|through|thru) the end(?: of (?:the|that|this) chapter)?(?-u:\b)",
            nums.join("|")
        ))
    });
    // "28 down to 31": a range, said as moving down the page.
    static DOWN_TO: LazyLock<regex::Regex> = LazyLock::new(|| {
        let nums: Vec<&str> = WORD_TO_NUM_ORDERED.iter().map(|(w, _)| w.as_str()).collect();
        let n = format!("[0-9]{{1,3}}|{}", nums.join("|"));
        re(&format!(r"(?-u:\b)({n}) down to ({n})(?-u:\b)"))
    });
    static ORD_ALT: LazyLock<String> = LazyLock::new(|| {
        let mut keys: Vec<&str> = ORDINALS_ORDERED.iter().map(|(w, _)| w.as_str()).collect();
        keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
        keys.join("|")
    });
    static UNIT_OF: LazyLock<regex::Regex> = LazyLock::new(|| {
        re(&format!(r"(?-u:\b)(?:the )?(?:({})|([0-9]{{1,3}})(?:st|nd|rd|th)) (chapter|verse) of(?-u:\b)", *ORD_ALT))
    });
    // "Isaiah forty first chapter, tenth verse": both said as ordinals.
    static ORD_CHAPTER_VERSE: LazyLock<regex::Regex> = LazyLock::new(|| {
        re(&format!(r"(?-u:\b)(?:the )?({o}) chapter,?(?: and)?(?: the)? ({o}) verse(?-u:\b)", o = *ORD_ALT))
    });
    static ORD_PSALM: LazyLock<regex::Regex> = LazyLock::new(|| re(&format!(r"(?-u:\b)(?:the )?({}) psalm(?-u:\b)", *ORD_ALT)));
    static NUM_PSALM: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)(?:the )?([0-9]{1,3})(?:st|nd|rd|th) psalm(?-u:\b)"));
    static BOOK_PHRASE_TEST: LazyLock<regex::Regex> = LazyLock::new(|| re(r" (?:book|epistle|letter) (?:of|to) "));
    static BOOK_PHRASE: LazyLock<regex::Regex> = LazyLock::new(|| {
        re(r"(?-u:\b)(?:the )?(first|second|third|1st|2nd|3rd) (?:book|epistle|letter) (?:of|to) (?:the )?([a-z]+)(?-u:\b)")
    });

    // "Psalm one twenty-five" is heard as "someone 25": read as Psalm 125 only
    // when a verse or chapter word follows ("someone 25 years ago" is untouched).
    static SOMEONE_PSALM: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)some ?one ([0-9]{2}),? (verse|verses|vers|chapter)(?-u:\b)"));

    // …and as the offline engine writes it, the number spelled out ("someone twenty five verse one").
    static SOMEONE_PSALM_WORDS: LazyLock<regex::Regex> = LazyLock::new(|| {
        re(r"(?-u:\b)some ?one ((?:ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen)|(?:twenty|thirty|forty|fifty)(?: (?:one|two|three|four|five|six|seven|eight|nine))?),? (verse|verses|vers|chapter)(?-u:\b)")
    });

    // "Psalm one ten, one to three" heard as "San one one one zero one to three" (the same on both speech
    // engines, live 2026-10-29): "san" is Psalm only before a run of spoken digits that ends at a "to" or a
    // verse word, so "San Francisco" and "San Antonio" are untouched.
    static SAN_PSALM: LazyLock<regex::Regex> = LazyLock::new(|| {
        re(r"(?-u:\b)san ((?:(?:zero|oh|one|two|three|four|five|six|seven|eight|nine) ){3,}(?:to|through|verse|verses|chapter)(?-u:\b))")
    });
    // …and the doubled "one" in front of that "zero" ("one one one zero" is Psalm 110, not 111 with a verse
    // starting at zero): no psalm is 1110, and a verse never begins at zero, so one of the ones is the recogniser's.
    static DOUBLED_ONE_BEFORE_ZERO: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)psalm one one one (zero|oh)(?-u:\b)"));

    let mut t = t.to_string();
    if t.contains("san ") {
        t = SAN_PSALM.replace_all(&t, "psalm $1").into_owned();
    }
    if t.contains("psalm one one one ") {
        t = DOUBLED_ONE_BEFORE_ZERO.replace_all(&t, "psalm one one $1").into_owned();
    }
    if t.contains("one ") {
        t = SOMEONE_PSALM.replace_all(&t, "psalm 1$1 $2").into_owned();
        t = SOMEONE_PSALM_WORDS
            .replace_all(&t, |c: &regex::Captures| match super::numbers::spoken_to_number(&c[1].replace(' ', "-")).or_else(|| words_value(&c[1])) {
                Some(n) if (10..=50).contains(&n) => format!("psalm {} {}", 100 + n, &c[2]),
                _ => c[0].to_string(),
            })
            .into_owned();
    }
    if t.contains(" the end") {
        t = TO_THE_END.replace_all(&t, "$1 to end").into_owned();
    }
    if t.contains(" down to ") {
        t = DOWN_TO.replace_all(&t, "$1 to $2").into_owned();
    }
    if t.contains(" chapter") && t.contains(" verse") {
        t = ORD_CHAPTER_VERSE
            .replace_all(&t, |c: &regex::Captures| format!("chapter {} verse {}", ORDINALS[&c[1]], ORDINALS[&c[2]]))
            .into_owned();
    }
    if t.contains("chapter of ") || t.contains("verse of ") {
        let src = t.clone();
        t = UNIT_OF
            .replace_all(&src, |c: &regex::Captures| {
                let m = c.get(0).unwrap();
                let rest: Vec<&str> = src[m.end()..].trim().split(' ').collect();
                let (a, b) = (rest.first().copied().unwrap_or(""), rest.get(1).copied().unwrap_or(""));
                let passage = a == "chapter" || books.is_single_word(a) || numbered_prefix(a).is_some_and(|n| books.numbered(n, b).is_some());
                if !passage {
                    return m.as_str().to_string();
                }
                let n = match c.get(1) {
                    Some(w) => ORDINALS[w.as_str()].to_string(),
                    None => c[2].to_string(),
                };
                format!("{} {} of", &c[3], n)
            })
            .into_owned();
    }
    if t.contains("psalm") {
        t = ORD_PSALM.replace_all(&t, |c: &regex::Captures| format!("psalm {}", ORDINALS[&c[1]])).into_owned();
        t = NUM_PSALM.replace_all(&t, "psalm $1").into_owned();
    }
    if BOOK_PHRASE_TEST.is_match(&t) {
        t = BOOK_PHRASE
            .replace_all(&t, |c: &regex::Captures| {
                let ord = match &c[1] {
                    "1st" => "first",
                    "2nd" => "second",
                    "3rd" => "third",
                    o => o,
                };
                format!("{ord} {}", &c[2])
            })
            .into_owned();
    }
    t
}

const CORRECTION_MARKERS: [&[&str]; 5] = [&["i", "mean"], &["or", "rather"], &["sorry"], &["rather"], &["correction"]];

fn self_corrections(t: &str) -> String {
    static ANY: LazyLock<regex::Regex> = LazyLock::new(|| re(r"(?-u:\b)(sorry|mean|rather|correction)(?-u:\b)"));
    if !ANY.is_match(t) {
        return t.to_string();
    }
    let plain = t.replace(" : ", " ");
    let out = correct_in_place(&plain);
    if out == plain { t.to_string() } else { out }
}

struct Ending {
    start: usize,
    value: i64,
}

fn correct_in_place(t: &str) -> String {
    let w = words(t);
    let number_ending_at = |end: usize| -> Option<Ending> {
        for st in end.saturating_sub(3)..end {
            if let Some(n) = consume_number(&w, st, None) {
                if st + n.consumed == end && !(n.consumed == 1 && is_ambiguous_number_word(w[st])) {
                    return Some(Ending { start: st, value: n.value });
                }
            }
        }
        None
    };
    for m in 0..w.len() {
        let Some(marker) = CORRECTION_MARKERS.iter().find(|mk| mk.iter().enumerate().all(|(k, tok)| w.get(m + k) == Some(tok))) else {
            continue;
        };
        let after = m + marker.len();
        let Some(before1) = number_ending_at(m) else { continue };
        if let Some(next1) = consume_number(&w, after, None) {
            // A chapter and verse restated replace both; a verse alone, just the verse.
            let next2 = consume_number(&w, after + next1.consumed, None);
            let before2 = if next2.is_some() { number_ending_at(before1.start) } else { None };
            let from = before2.map_or(before1.start, |b| b.start);
            let mut v: Vec<&str> = w.clone();
            v.drain(from..after);
            return correct_in_place(&v.join(" "));
        }
        // "3 7 3 17 sorry": the same chapter restated with a new verse before the marker.
        let c2 = number_ending_at(before1.start);
        let v1 = c2.as_ref().and_then(|c| number_ending_at(c.start));
        let c1 = v1.as_ref().and_then(|v| number_ending_at(v.start));
        if let (Some(c1), Some(c2)) = (c1, c2) {
            if c1.value == c2.value {
                let mut v: Vec<&str> = w.clone();
                v.drain(m..m + marker.len());
                v.drain(c1.start..c2.start);
                return correct_in_place(&v.join(" "));
            }
        }
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> String {
        normalize(s, Books::get(), Lang::En)
    }

    #[test]
    fn punctuation_and_colons() {
        assert_eq!(n("John 3:16."), "john 3 : 16");
        assert_eq!(n("Romans 8:28-30"), "romans 8 : 28 to 30");
        assert_eq!(n("John three colon sixteen"), "john three : sixteen");
        assert_eq!(n("verses 4, 5 and 6"), "verses 4 and 5 and 6");
        assert_eq!(n("verse 12, for we wrestle"), "verse 12 for we wrestle");
    }

    #[test]
    fn caption_formatting() {
        assert_eq!(n("Proverbs 3:2 7"), "proverbs 3 : 27");
        assert_eq!(n("Psalm 4 1: 1-3"), "psalm 41 : 1 to 3");
        assert_eq!(n("sixty-six five-seven"), "sixty six five to seven");
        assert_eq!(n("sixty-six, five-seven"), "sixty six and five to seven");
        assert_eq!(n("60 six:five"), "66 : five");
        assert_eq!(n("chapter III verse I"), "chapter 3 verse i");
    }

    #[test]
    fn spoken_shapes() {
        assert_eq!(n("in in John's gospel"), "in john gospel");
        assert_eq!(n("the twenty third psalm"), "psalm 23");
        assert_eq!(n("Romans eight from twenty eight down to thirty one"), "romans eight from twenty eight to thirty one");
        assert_eq!(n("now drop down to verse thirty one"), "now drop down to verse thirty one");
        assert_eq!(n("the 23rd Psalm"), "psalm 23");
        assert_eq!(n("the second epistle of Timothy"), "second timothy");
        assert_eq!(n("the eighth chapter of Romans"), "chapter 8 of romans");
        assert_eq!(n("the first verse of the song"), "the first verse of the song");
        assert_eq!(n("from verse 4 to the end of the chapter"), "from verse 4 to end");
        assert_eq!(n("one one zero"), "one one zero");
        assert_eq!(n("Someone 25, verse one and two"), "psalm 125 verse one and two");
        assert_eq!(n("someone 25 years ago"), "someone 25 years ago");
    }

    #[test]
    fn psalm_one_ten_as_the_recogniser_wrote_it() {
        // Live 2026-10-29: both speech engines wrote "Psalm one ten, one to three" this way.
        assert_eq!(n("And then from San One One One Zero One to Three"), "and then from psalm one one zero one to three");
        assert_eq!(n("from Psalm One One One Zero One to Three"), "from psalm one one zero one to three");
        assert_eq!(n("from Psalm one one one zero one to three"), "from psalm one one zero one to three");
        // Psalm 111 spoken digit by digit, and ordinary "san" talk, are untouched.
        assert_eq!(n("Psalm one one one verse one"), "psalm one one one verse one");
        assert_eq!(n("Psalm one one one"), "psalm one one one");
        assert_eq!(n("we went to San Francisco and then to San Antonio"), "we went to san francisco and then to san antonio");
        assert_eq!(n("San one one to three"), "san one one to three");
    }

    #[test]
    fn self_corrections_keep_what_was_settled() {
        assert_eq!(n("John 3:17, sorry, 16"), "john 3 16");
        assert_eq!(n("Romans 8:28, I mean 8:29"), "romans 8 29");
        assert_eq!(n("3:7 3:17 sorry"), "3 17");
        assert_eq!(n("I'm sorry for that"), "i'm sorry for that");
    }
}

/// "twenty five" as 25 (tens, then an optional unit).
fn words_value(s: &str) -> Option<i64> {
    let mut parts = s.split(' ');
    let tens = super::numbers::spoken_to_number(parts.next()?)?;
    let unit = match parts.next() {
        Some(u) => super::numbers::spoken_to_number(u)?,
        None => 0,
    };
    Some(tens + unit)
}
