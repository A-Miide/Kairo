//! The one cleaning rule for verse text.
//!
//! Source KJV text carries translator apparatus nobody reads aloud. Cleaned
//! text is what goes on screen AND what every detection path matches against:
//! what's shown is what's said. (The Node server cleaned three different ways —
//! its matching index kept marginal-note words in 6,687 verses, and its twin
//! resolver dropped italic words the preacher does read.)
//!
//! - `{are}` — words the translators supplied, printed in italics. Read aloud,
//!   so kept, braces dropped.
//! - `{undefiled: or, perfect}` — marginal notes (a colon inside the braces).
//!   Removed.
//! - `[A Psalm of David.]` — superscriptions and headings. Removed.
//! - `ALEPH.` … `TAU.` — the acrostic letters opening each stanza of Psalm 119.
//!   Removed.
//! - `«Written to the Romans from Corinthus…»` — the epistle subscriptions
//!   (14 verses). Not scripture text. Removed.
//!
//! Braces nest in the source (`{{and from} the fortified cities: or, …}`,
//! Micah 7:12), so a group is judged as a whole: a colon anywhere inside makes
//! it a note. A stray `}` means the source split a note in two (Hebrews 10:34
//! `{…: or, … for} yourselves}`); the fragment back to the note is dropped.

const ACROSTIC: [&str; 22] = [
    "ALEPH", "BETH", "GIMEL", "DALETH", "HE", "VAU", "ZAIN", "CHETH", "TETH", "JOD", "CAPH",
    "LAMED", "MEM", "NUN", "SAMECH", "AIN", "PE", "TZADDI", "KOPH", "RESH", "SCHIN", "TAU",
];

pub fn clean(raw: &str) -> String {
    let s = strip_acrostic(raw.trim_start());
    let mut out = String::with_capacity(s.len());
    // Where the output stood when the last note was removed — a stray `}`
    // later truncates back to here.
    let mut after_note: Option<usize> = None;
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        match c {
            '{' => {
                let end = matching(s, i, '{', '}');
                let inner = &s[i + 1..end.unwrap_or(s.len())];
                if inner.contains(':') {
                    out.push(' ');
                    after_note = Some(out.len());
                } else {
                    out.push_str(&clean_inner(inner));
                }
                i = end.map_or(s.len(), |e| e + 1);
            }
            '}' => {
                if let Some(at) = after_note {
                    out.truncate(at);
                }
                i += 1;
            }
            '[' => {
                out.push(' ');
                i = matching(s, i, '[', ']').map_or(s.len(), |e| e + 1);
            }
            '«' => {
                out.push(' ');
                i = s[i..].find('»').map_or(s.len(), |e| i + e + '»'.len_utf8());
            }
            _ => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    collapse_spaces(&out)
}

/// Italic words inside an italic group (`{{the} LORD}`): braces dropped.
fn clean_inner(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '{' | '}')).collect()
}

/// Byte index of the bracket closing the one at `open`, counting nesting.
fn matching(s: &str, open: usize, l: char, r: char) -> Option<usize> {
    let mut depth = 0usize;
    for (j, c) in s[open..].char_indices() {
        if c == l {
            depth += 1;
        } else if c == r {
            depth -= 1;
            if depth == 0 {
                return Some(open + j);
            }
        }
    }
    None
}

fn strip_acrostic(s: &str) -> &str {
    for letter in ACROSTIC {
        if let Some(after) = s.strip_prefix(letter).and_then(|r| r.strip_prefix(". ")) {
            return after;
        }
    }
    s
}

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            // No space before punctuation a removed note used to sit in front of.
            if !matches!(word.chars().next(), Some(',' | ';' | ':' | '.' | '!' | '?')) {
                out.push(' ');
            }
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn italics_are_kept_as_words() {
        assert_eq!(clean("The LORD {is} my shepherd; I shall not want."), "The LORD is my shepherd; I shall not want.");
    }

    #[test]
    fn marginal_notes_go() {
        assert_eq!(
            clean("ALEPH. Blessed {are} the undefiled in the way, who walk in the law of the LORD. {undefiled: or, perfect, or, sincere}"),
            "Blessed are the undefiled in the way, who walk in the law of the LORD."
        );
        assert_eq!(clean("He brought me to the banqueting house {banqueting…: Heb. house of wine}, and his banner"), "He brought me to the banqueting house, and his banner");
    }

    #[test]
    fn nested_and_broken_notes() {
        assert_eq!(
            clean("and {from} mountain to mountain. {{and from} the fortified cities: or, even to the fortified cities}"),
            "and from mountain to mountain."
        );
        assert_eq!(
            clean("a better and an enduring substance. {in yourselves...: or, that ye have in or, for} yourselves}"),
            "a better and an enduring substance."
        );
    }

    #[test]
    fn subscriptions_go() {
        assert_eq!(
            clean("To God only wise, {be} glory through Jesus Christ for ever. Amen. «{Written to the Romans from Corinthus, and sent} by Phebe servant of the church at Cenchrea.}»"),
            "To God only wise, be glory through Jesus Christ for ever. Amen."
        );
    }

    #[test]
    fn headings_go() {
        assert_eq!(clean("[A Psalm of David.] The LORD {is} my shepherd; I shall not want."), "The LORD is my shepherd; I shall not want.");
    }

    #[test]
    fn only_the_acrostic_letters_are_stripped() {
        assert_eq!(clean("TAU. Let my cry come near before thee, O LORD"), "Let my cry come near before thee, O LORD");
        assert_eq!(clean("HEAR my prayer, O LORD"), "HEAR my prayer, O LORD");
        assert_eq!(clean("HE. Teach me, O LORD"), "Teach me, O LORD");
        assert_eq!(clean("He that dwelleth in the secret place"), "He that dwelleth in the secret place");
    }

    #[test]
    fn plain_text_is_unchanged() {
        let s = "For God so loved the world, that he gave his only begotten Son.";
        assert_eq!(clean(s), s);
        assert_eq!(clean("Ní ìbẹ̀rẹ̀ ohun gbogbo Ọlọ́run dá àwọn ọ̀run àti ayé."), "Ní ìbẹ̀rẹ̀ ohun gbogbo Ọlọ́run dá àwọn ọ̀run àti ayé.");
    }
}
