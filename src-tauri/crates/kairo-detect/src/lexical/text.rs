//! Text as the word matchers see it.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Lowercase, accents folded, only letters, digits and single spaces
/// ("The LORD's" → "the lords", "João" → "joao").
pub fn norm(s: &str) -> String {
    let lower = s.to_lowercase();
    let folded = if lower.is_ascii() { lower } else { crate::reference::i18n::fold(&lower) };
    let kept: String = folded.chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c.is_whitespace()).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Structural words that identify nothing.
pub static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "an", "the", "and", "but", "or", "for", "nor", "yet", "so", "in", "on", "at", "to", "of", "by", "up", "as", "is", "it", "be",
        "do", "if", "no", "i", "we", "he", "me", "us", "am", "my", "was", "are", "were", "been", "being", "have", "has", "had", "does",
        "did", "will", "would", "can", "could", "shall", "should", "may", "might", "you", "she", "they", "them", "their", "this", "that",
        "these", "those", "who", "which", "what", "him", "his", "her", "its", "our", "your", "not", "all", "very", "also", "just", "more",
        "then", "than", "when", "where", "there", "here", "now", "too", "only", "even", "still", "from", "with", "into", "about", "over",
        "after", "before", "out", "down", "how", "each", "both", "some", "any", "same", "other", "such", "own", "while", "say", "said",
        "says", "come", "came", "went", "get", "got", "make", "made",
        // the KJV's structural words
        "thou", "thee", "thy", "thine", "ye", "hath", "doth", "art", "unto", "saith", "thus", "yea", "nay", "therefore", "wherefore",
        "moreover", "lo", "behold", "thereof", "therein", "whereby", "wherein", "whereof", "whatsoever", "whosoever", "thence", "hence",
        "whence",
    ]
    .into_iter()
    .collect()
});

/// Words rare in the Bible but said constantly in preaching, whatever is
/// being quoted. They still weigh in a match; they just never make one
/// "weighty" on their own.
pub static PREACHING_COMMON_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["jesus", "christ", "lord", "god", "name", "holy", "ghost", "spirit", "father", "amen", "praise", "glory", "hallelujah"]
        .into_iter()
        .collect()
});

pub fn is_stop_word(w: &str) -> bool {
    STOP_WORDS.contains(w)
}
