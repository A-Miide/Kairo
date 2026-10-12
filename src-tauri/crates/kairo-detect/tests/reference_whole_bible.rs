//! Citations of verses drawn from the whole Bible, each spoken the ways
//! preachers say them, must read back exactly. Nothing here is chosen: the
//! verses are a seeded random sample over every book, so the parser can't be
//! tuned to a handful of famous passages.

use std::collections::BTreeMap;
use std::path::PathBuf;

use kairo_bible::{Book, Corpus, VerseRef};
use kairo_detect::reference::{Lang, Active, Books, Citation, Parser, Shape, Verses};

const SAMPLE: usize = 4000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn words(n: u32) -> String {
    const ONES: [&str; 20] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    let under_100 = |n: u32| -> String {
        if n < 20 {
            ONES[n as usize].to_string()
        } else if n % 10 == 0 {
            TENS[(n / 10) as usize].to_string()
        } else {
            format!("{} {}", TENS[(n / 10) as usize], ONES[(n % 10) as usize])
        }
    };
    if n < 100 {
        under_100(n)
    } else if n % 100 == 0 {
        format!("{} hundred", ONES[(n / 100) as usize])
    } else {
        format!("{} hundred and {}", ONES[(n / 100) as usize], under_100(n % 100))
    }
}

/// How the book is said: "psalm", "song of solomon", "first corinthians".
fn spoken_book(b: Book, rng: &mut Rng) -> String {
    let name = b.name().to_lowercase();
    if name == "psalms" {
        return "psalm".into();
    }
    match name.split_once(' ') {
        Some((n @ ("1" | "2" | "3"), stem)) => {
            let ord = ["first", "second", "third"][n.parse::<usize>().unwrap() - 1];
            if rng.below(2) == 0 { format!("{ord} {stem}") } else { format!("{n} {stem}") }
        }
        _ => name,
    }
}

fn expect(r: VerseRef, end: Option<u16>) -> String {
    match end {
        Some(e) if e != r.verse => format!("{} {}:{}-{}", r.book, r.chapter, r.verse, e),
        _ => format!("{} {}:{}", r.book, r.chapter, r.verse),
    }
}

fn fmt(c: &Citation) -> String {
    match &c.verses {
        Verses::Chapter => format!("{} {}:", c.book, c.chapter),
        Verses::One(v) => format!("{} {}:{v}", c.book, c.chapter),
        Verses::Span(a, z) => format!("{} {}:{a}-{z}", c.book, c.chapter),
        Verses::Spans(s) => format!("{} {}:{s:?}", c.book, c.chapter),
    }
}

#[test]
fn every_book_every_way() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    let corpus = Corpus::load(&dir).unwrap();
    let shape = Shape::new(&corpus);
    let parser = Parser { books: Books::get(), shape: &shape, lang: Lang::En };
    let mut rng = Rng(0x5eed_cafe_f00d_1234);

    let mut tried: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failed: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let ids: Vec<_> = corpus.ids().collect();
    for _ in 0..SAMPLE {
        let r = corpus.verse_ref(ids[rng.below(ids.len())]);
        let book = spoken_book(r.book, &mut rng);
        let (c, v) = (r.chapter as u32, r.verse as u32);
        let one_chapter = shape.chapters(r.book) == 1;
        let last = shape.verses(r.book, c).unwrap();
        let span_end = (v + 1 + rng.below(3) as u32).min(last) as u16;

        let mut forms: Vec<(&str, String, Option<u16>)> = vec![
            ("digits", format!("{book} {c}:{v}"), None),
            ("words", format!("{book} chapter {} verse {}", words(c), words(v)), None),
            ("turn to", format!("turn with me to {book} chapter {c} and verse {v} and let us read"), None),
            ("chapter of", format!("chapter {c} of {book} verse {v}"), None),
            ("range", format!("{book} chapter {c} verses {v} to {span_end}"), Some(span_end)),
        ];
        forms.push(("in a sentence", format!("and as it says in {book} {c}:{v}, the Lord is faithful. Amen?"), None));
        forms.push(("formatted", format!("{} {c}:{v}-{span_end}.", formatted_book(r.book)), Some(span_end)));
        if !one_chapter {
            forms.push(("plain", format!("{book} {} verse {}", words(c), words(v)), None));
            forms.push(("verse first", format!("verse {} of {book} chapter {}", words(v), words(c)), None));
            // No keyword at all: only where the words have one reading — not
            // two single digit-words that could be a three-digit chapter still
            // arriving, and not a round tens chapter before a one-digit verse
            // ("thirty seven" is 37 as surely as it is 30:7).
            let round_tens = c % 100 >= 20 && c % 10 == 0;
            if !(shape.chapters(r.book) >= 100 && c < 10 && v < 10) && !(round_tens && v <= 9) {
                forms.push(("no keywords", format!("{book} {} {}", words(c), words(v)), None));
            }
            if c >= 100 && c % 100 >= 10 {
                forms.push(("hundreds short", format!("{book} one {} verse {v}", words(c % 100)), None));
            }
        } else {
            forms.push(("one chapter", format!("{book} verse {v}"), None));
        }
        if r.book == Book::from_name("Psalms").unwrap() && c <= 150 {
            forms.push(("ordinal psalm", format!("the {} psalm verse {v}", ordinal(c)), None));
        }
        for (form, text, end) in forms {
            *tried.entry(form).or_default() += 1;
            let got: Vec<String> = parser.parse_all(&text, Active::default()).iter().map(fmt).collect();
            let want = expect(r, end);
            if got != [want.clone()] {
                failed.entry(form).or_default().push(format!("{text:?} → {got:?}, want {want}"));
            }
        }
    }

    let mut report = String::new();
    for (form, n) in &tried {
        let f = failed.get(form).map_or(0, Vec::len);
        report += &format!("{form:>14}: {}/{} read back exactly\n", n - f, n);
        for example in failed.get(form).into_iter().flatten().take(6) {
            report += &format!("{:>16}{example}\n", "");
        }
    }
    let total_failed: usize = failed.values().map(Vec::len).sum();
    println!("{report}");
    assert_eq!(total_failed, 0, "\n{report}");
}

/// Speech-to-text's own formatting of a book name: "1st Corinthians", "Psalm".
fn formatted_book(b: Book) -> String {
    let name = b.name();
    if name == "Psalms" {
        return "Psalm".into();
    }
    match name.split_once(' ') {
        Some(("1", stem)) => format!("1st {stem}"),
        Some(("2", stem)) => format!("2nd {stem}"),
        Some(("3", stem)) => format!("3rd {stem}"),
        _ => name.to_string(),
    }
}

fn ordinal(n: u32) -> String {
    const UNITS: [&str; 10] = ["", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth"];
    const TEENS: [&str; 10] = [
        "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth",
        "eighteenth", "nineteenth",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    const TENS_ORD: [&str; 10] =
        ["", "", "twentieth", "thirtieth", "fortieth", "fiftieth", "sixtieth", "seventieth", "eightieth", "ninetieth"];
    let under_100 = |n: u32| -> String {
        let n = n as usize;
        if n < 10 {
            UNITS[n].into()
        } else if n < 20 {
            TEENS[n - 10].into()
        } else if n % 10 == 0 {
            TENS_ORD[n / 10].into()
        } else {
            format!("{} {}", TENS[n / 10], UNITS[n % 10])
        }
    };
    match n {
        100 => "hundredth".into(),
        n if n > 100 => format!("one hundred and {}", under_100(n - 100)),
        n => under_100(n),
    }
}
