//! Spoken numbers: "sixteen", "one hundred and nineteen", "one oh eight",
//! "one twenty five", and the homophones speech engines produce in a citation
//! ("for" 4, "won" 1, "too" 2, "ate" 8).

use std::collections::HashMap;
use std::sync::LazyLock;

/// Word → value, in the order the rules were written. The order matters:
/// `to the end` composes an alternation from these keys, and alternation
/// takes the first alternative that fits.
pub(crate) static WORD_TO_NUM_ORDERED: LazyLock<Vec<(String, i64)>> = LazyLock::new(|| {
    let units = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty",
    ];
    let mut v: Vec<(String, i64)> = units.iter().enumerate().map(|(i, w)| (w.to_string(), i as i64)).collect();
    let tens = ["twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    let ones = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
    for (t, tens_word) in tens.iter().enumerate() {
        let base = (t as i64 + 2) * 10;
        if t > 0 {
            v.push((tens_word.to_string(), base));
        }
        for (u, one) in ones.iter().enumerate() {
            v.push((format!("{tens_word}-{one}"), base + u as i64 + 1));
        }
    }
    for (w, n) in [
        ("hundred", 100), ("one hundred", 100),
        // Digit-by-digit zero: "Psalm one oh eight" = 108.
        ("oh", 0),
        ("first", 1), ("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5),
        // Speech-engine homophones of number words in a citation.
        ("for", 4), ("won", 1), ("too", 2), ("ate", 8),
    ] {
        v.push((w.to_string(), n));
    }
    v
});

pub(crate) static WORD_TO_NUM: LazyLock<HashMap<&'static str, i64>> =
    LazyLock::new(|| WORD_TO_NUM_ORDERED.iter().map(|(w, n)| (w.as_str(), *n)).collect());

pub(crate) fn word_value(w: &str) -> Option<i64> {
    WORD_TO_NUM.get(w).copied()
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn parse_digits(s: &str) -> i64 {
    s.parse::<i64>().unwrap_or(i64::MAX)
}

/// One word or a short phrase as a number.
pub fn spoken_to_number(word: &str) -> Option<i64> {
    if word.is_empty() {
        return None;
    }
    let w = word.trim().to_lowercase();
    if all_digits(&w) {
        return Some(parse_digits(&w));
    }
    if let Some(n) = word_value(&w) {
        return Some(n);
    }
    let parts: Vec<&str> = w.split_whitespace().collect();
    if parts.len() == 2 {
        if let (Some(a), Some(b)) = (word_value(parts[0]), word_value(parts[1])) {
            // A tens word takes a units word: "forty three", never "forty thirteen".
            if a >= 20 && a % 10 == 0 && a < 100 && (1..=9).contains(&b) {
                return Some(a + b);
            }
            if (1..=9).contains(&a) && (10..=99).contains(&b) {
                return Some(a * 100 + b);
            }
        }
    }
    if parts.len() == 3 {
        let (a, b, c) = (word_value(parts[0]), word_value(parts[1]), word_value(parts[2]));
        if let (Some(a), Some(b), Some(c)) = (a, b, c) {
            if (0..=9).contains(&a) && (0..=9).contains(&b) && (0..=9).contains(&c) {
                return Some(a * 100 + b * 10 + c);
            }
            // "one twenty five" = 125: a hundreds digit, a tens word and a
            // units word read as one three-digit number, tried before the
            // two-word "one twenty" can claim the first two.
            if (1..=9).contains(&a) && (20..=90).contains(&b) && b % 10 == 0 && (1..=9).contains(&c) {
                return Some(a * 100 + b + c);
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Num {
    pub value: i64,
    pub consumed: usize,
}

fn at<'a>(words: &[&'a str], i: usize) -> Option<&'a str> {
    words.get(i).copied()
}

/// The number starting at `words[idx]`, preferring the longest phrase that
/// stays within `max` (a single word is accepted whatever its value).
pub fn consume_number(words: &[&str], idx: usize, max: Option<i64>) -> Option<Num> {
    let w = at(words, idx)?;
    let max = max.unwrap_or(i64::MAX);

    // "<1-9> hundred [and] [<1-99>]", "hundred and eight", "a hundred and five".
    let lead = spoken_to_number(w);
    let bare_hundred = w == "hundred" || (w == "a" && at(words, idx + 1) == Some("hundred"));
    if bare_hundred || (lead.is_some_and(|l| (1..=9).contains(&l)) && at(words, idx + 1) == Some("hundred")) {
        let mut value = if bare_hundred { 100 } else { lead.unwrap() * 100 };
        let mut consumed = if bare_hundred && w == "hundred" { 1 } else { 2 };
        let mut j = idx + consumed;
        if at(words, j) == Some("and") {
            j += 1;
            consumed += 1;
        }
        if let Some(rest) = consume_number(words, j, Some(99)) {
            if (1..=99).contains(&rest.value) {
                value += rest.value;
                consumed += rest.consumed;
            }
        }
        if value <= max {
            return Some(Num { value, consumed });
        }
    }

    if idx + 2 < words.len() {
        if let Some(n) = spoken_to_number(&words[idx..idx + 3].join(" ")) {
            if n <= max {
                return Some(Num { value: n, consumed: 3 });
            }
        }
    }
    if idx + 1 < words.len() {
        if let Some(n) = spoken_to_number(&words[idx..idx + 2].join(" ")) {
            if n <= max {
                return Some(Num { value: n, consumed: 2 });
            }
        }
    }
    spoken_to_number(w).map(|n| Num { value: n, consumed: 1 })
}

/// Ordinal phrase → number, 1–150 ("twenty third", "one hundred and fifth"),
/// in construction order.
pub(crate) static ORDINALS_ORDERED: LazyLock<Vec<(String, i64)>> = LazyLock::new(|| {
    const UNITS: [&str; 10] = ["", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth"];
    const TEENS: [&str; 10] = [
        "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth",
        "eighteenth", "nineteenth",
    ];
    const TENS_CARD: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    const TENS_ORD: [&str; 10] =
        ["", "", "twentieth", "thirtieth", "fortieth", "fiftieth", "sixtieth", "seventieth", "eightieth", "ninetieth"];
    let under_100 = |n: usize| -> String {
        if n < 10 {
            UNITS[n].to_string()
        } else if n < 20 {
            TEENS[n - 10].to_string()
        } else {
            let (t, u) = (n / 10, n % 10);
            if u > 0 { format!("{} {}", TENS_CARD[t], UNITS[u]) } else { TENS_ORD[t].to_string() }
        }
    };
    let mut out: Vec<(String, i64)> = Vec::new();
    let mut push = |p: String, n: usize| {
        if !out.iter().any(|(k, _)| *k == p) {
            out.push((p, n as i64));
        } else if let Some(e) = out.iter_mut().find(|(k, _)| *k == p) {
            e.1 = n as i64;
        }
    };
    for n in 1..=150usize {
        if n < 100 {
            push(under_100(n), n);
        } else if n == 100 {
            push("hundredth".into(), n);
            push("one hundredth".into(), n);
        } else {
            let rest = under_100(n - 100);
            for lead in ["hundred", "one hundred"] {
                push(format!("{lead} {rest}"), n);
                push(format!("{lead} and {rest}"), n);
            }
        }
    }
    out
});

pub(crate) static ORDINALS: LazyLock<HashMap<&'static str, i64>> =
    LazyLock::new(|| ORDINALS_ORDERED.iter().map(|(w, n)| (w.as_str(), *n)).collect());

/// Number words that are far more often ordinary words — never a verse
/// number on their own.
pub const AMBIGUOUS_NUMBER_WORDS: [&str; 9] = ["for", "won", "too", "ate", "first", "second", "third", "fourth", "fifth"];

pub fn is_ambiguous_number_word(w: &str) -> bool {
    AMBIGUOUS_NUMBER_WORDS.contains(&w)
}

/// Real number words only — never the homophones ("for" is 4 in a citation,
/// but must never be read as part of a colon or hyphen citation).
pub(crate) fn plain_number(w: &str) -> Option<i64> {
    if all_digits(w) {
        return Some(parse_digits(w));
    }
    const PLAIN: [&str; 28] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
        "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty", "thirty",
        "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let l = w.to_lowercase();
    PLAIN.contains(&l.as_str()).then(|| word_value(&l).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<&str> {
        s.split(' ').collect()
    }

    #[test]
    fn spoken_forms() {
        assert_eq!(spoken_to_number("sixteen"), Some(16));
        assert_eq!(spoken_to_number("twenty five"), Some(25));
        assert_eq!(spoken_to_number("one twenty"), Some(120));
        assert_eq!(spoken_to_number("one oh eight"), Some(108));
        assert_eq!(spoken_to_number("one twenty five"), Some(125));
        assert_eq!(spoken_to_number("119"), Some(119));
        assert_eq!(spoken_to_number("apple"), None);
    }

    #[test]
    fn hundreds_compose() {
        assert_eq!(consume_number(&w("one hundred and nineteen verse"), 0, None), Some(Num { value: 119, consumed: 4 }));
        assert_eq!(consume_number(&w("hundred and eight"), 0, None), Some(Num { value: 108, consumed: 3 }));
        assert_eq!(consume_number(&w("a hundred five"), 0, None), Some(Num { value: 105, consumed: 3 }));
        assert_eq!(consume_number(&w("one twenty five five"), 0, Some(150)), Some(Num { value: 125, consumed: 3 }));
        // over the limit, a shorter phrase is tried; one word always passes
        assert_eq!(consume_number(&w("twenty eight"), 0, Some(21)), Some(Num { value: 20, consumed: 1 }));
    }

    #[test]
    fn ordinals() {
        assert_eq!(ORDINALS.get("twenty third"), Some(&23));
        assert_eq!(ORDINALS.get("one hundred and fifth"), Some(&105));
        assert_eq!(ORDINALS.get("hundredth"), Some(&100));
        assert_eq!(ORDINALS.get("hundred fiftieth"), Some(&150));
    }
}
