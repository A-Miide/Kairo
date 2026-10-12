//! Meaning-based detection: which stretches of recent speech to search by
//! meaning (`detection-spec/04-meaning-and-evidence.md`).
//!
//! A paraphrase arrives wrapped in commentary and recogniser noise; embedding
//! a whole segment dilutes it. So several short windows are searched at once —
//! the last sentence or two, the last few seconds, and the words right after a
//! quote signal ("the Bible says", "Paul wrote") — and the best-fitting window
//! speaks for each verse.

use std::sync::LazyLock;

use super::core::Engine;
use crate::re::re;

const MAX_WINDOW_WORDS: usize = 30;
const MAX_WINDOWS: usize = 5;
/// An aside between two parts of a verse.
const SHORT_SENTENCE_WORDS: usize = 7;
const AFTER_SIGNAL_WORDS: usize = 28;
const FUSED_WORDS: usize = 26;
const TAIL_WORDS: usize = 24;
const SHORT_TAIL_WORDS: usize = 14;

/// Crowd work and fillers dropped into the middle of a verse — only phrases
/// that are never scripture wording themselves.
static INTERJECTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    let phrases = [
        "(?:somebody |can i get an? |let me hear (?:you )?)?say amen",
        "amen",
        "hallelujah",
        "glory to god",
        "thank you jesus",
        "are you (?:with me|listening|there|still with me)",
        "can you hear me",
        "do you hear me",
        "listen to me",
        "listen",
        "(?:turn to|look at|tell) (?:your|the person next to you|somebody)(?: neighbou?r)?(?: and say)?",
        "say it with me",
        "say it",
        "watch this",
        "come on",
        "i said",
        "you know",
        "i mean",
        "right",
        "okay",
        "alright",
        "hello",
        "church",
        "uh+",
        "um+",
        "hmm+",
    ];
    re(&format!(r"(?i)\b(?:{})\b[,.!?]*", phrases.join("|")))
});
static SPACE_BEFORE_PUNCT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"\s+([,.!?;])"));

fn without_interjections(text: &str) -> String {
    let t = INTERJECTION.replace_all(text, " ");
    let t = SPACE_BEFORE_PUNCT.replace_all(&t, "$1");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn norm_word(w: &str) -> String {
    w.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect()
}

/// Sentences, split after `.`, `!` or `?` followed by a space.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for w in text.split_whitespace() {
        cur.push(w);
        if w.ends_with(['.', '!', '?']) {
            out.push(cur.join(" "));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.join(" "));
    }
    out
}

impl Engine {
    /// Where the last phrase introducing scripture ends, as a word index:
    /// "the Bible says", "it is written", "Jesus said", "<a person> wrote"…
    /// A person is any proper name the Bible has.
    fn after_quote_signal(&self, words: &[String]) -> Option<usize> {
        let w = |i: usize| words.get(i).map_or("", String::as_str);
        let says = |s: &str| matches!(s, "says" | "said" | "tells" | "told" | "declares" | "teaches");
        let person_verb = |i: usize| -> Option<usize> {
            match w(i) {
                "says" | "said" | "wrote" | "writes" | "declared" => Some(i + 1),
                "told" if !w(i + 1).is_empty() => Some(i + 2),
                _ => None,
            }
        };
        let mut last = None;
        for i in 0..words.len() {
            let end = match w(i) {
                // the Bible / scripture / the word (of God) says
                "the" if matches!(w(i + 1), "bible" | "scripture" | "scriptures" | "word") => {
                    let j = if w(i + 1) == "word" && w(i + 2) == "of" && w(i + 3) == "god" { i + 4 } else { i + 2 };
                    match w(j) {
                        "tells" | "told" if w(j + 1) == "us" => Some(j + 2),
                        s if says(s) && s != "tells" && s != "told" => Some(j + 1),
                        _ => None,
                    }
                }
                "it" if matches!(w(i + 1), "is" | "was") && w(i + 2) == "written" => Some(i + 3),
                "thus" if matches!(w(i + 1), "says" | "saith") && w(i + 2) == "the" && w(i + 3) == "lord" => Some(i + 4),
                "jesus" | "christ" | "god" => match w(i + 1) {
                    "says" | "said" => Some(i + 2),
                    "told" if matches!(w(i + 2), "us" | "them" | "him" | "her") => Some(i + 3),
                    _ => None,
                },
                "the" if matches!(w(i + 1), "lord" | "spirit") => match w(i + 2) {
                    "says" | "said" => Some(i + 3),
                    "told" if matches!(w(i + 3), "us" | "them" | "him" | "her") => Some(i + 4),
                    _ => None,
                },
                "the" if matches!(w(i + 1), "psalmist" | "apostle" | "prophet") => person_verb(i + 2),
                "in" if w(i + 1) == "the" && w(i + 2) == "words" && w(i + 3) == "of" => {
                    let x = w(i + 4);
                    if x == "jesus" || x == "scripture" || self.k.is_name(x) {
                        Some(i + 5)
                    } else if x == "the" && w(i + 5) == "psalmist" {
                        Some(i + 6)
                    } else {
                        None
                    }
                }
                x if self.k.is_name(x) => person_verb(i + 1),
                _ => None,
            };
            if let Some(e) = end {
                last = Some(e.min(words.len()));
            }
        }
        last
    }

    /// Windows of recent speech to search by meaning, most specific first.
    pub(crate) fn paraphrase_windows(&self, recent: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let add = |out: &mut Vec<String>, text: &str| {
            let w: Vec<&str> = text.split_whitespace().collect();
            let t = w[w.len().saturating_sub(MAX_WINDOW_WORDS)..].join(" ");
            if t.split_whitespace().count() >= 6 && !out.contains(&t) {
                out.push(t);
            }
        };
        let raw: Vec<&str> = recent.split_whitespace().collect();
        let normed: Vec<String> = raw.iter().map(|w| norm_word(w)).collect();
        // After the last quote signal: the verse itself, asides taken out.
        if let Some(end) = self.after_quote_signal(&normed) {
            let after = without_interjections(&raw[end..].join(" "));
            add(&mut out, &after.split_whitespace().take(AFTER_SIGNAL_WORDS).collect::<Vec<_>>().join(" "));
        }
        // The last few seconds with the asides removed, so a verse's pieces read as one.
        let fused = without_interjections(recent);
        let fw: Vec<&str> = fused.split_whitespace().collect();
        add(&mut out, &fw[fw.len().saturating_sub(FUSED_WORDS)..].join(" "));
        // The last sentence, the last two, and two around a short aside.
        let s = sentences(recent);
        if let Some(last) = s.last() {
            add(&mut out, last);
        }
        if s.len() > 1 {
            add(&mut out, &s[s.len() - 2..].join(" "));
        }
        if s.len() > 2 && s[s.len() - 2].split_whitespace().count() <= SHORT_SENTENCE_WORDS {
            add(&mut out, &format!("{} {}", s[s.len() - 3], s[s.len() - 1]));
        }
        // The last few seconds regardless of punctuation.
        add(&mut out, &raw[raw.len().saturating_sub(TAIL_WORDS)..].join(" "));
        if out.len() < 3 {
            add(&mut out, &raw[raw.len().saturating_sub(SHORT_TAIL_WORDS)..].join(" "));
        }
        out.truncate(MAX_WINDOWS);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asides_are_taken_out() {
        assert_eq!(without_interjections("come to me, church, are you with me? all you that labour"), "come to me, all you that labour");
    }

    #[test]
    fn sentence_split() {
        assert_eq!(sentences("One two. Three four! Five"), vec!["One two.", "Three four!", "Five"]);
    }
}
