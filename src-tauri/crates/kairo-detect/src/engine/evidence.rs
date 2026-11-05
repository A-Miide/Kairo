//! Comparing what was said with verses, on the words that can tell verses
//! apart. Used by the decider's guards, the twin resolver, ranges and the
//! citation check — one definition for all of them.

use std::collections::HashSet;

/// Words a verse ending, opening or comparison is judged on: letters only,
/// three or more, minus the words nearly every verse and sentence has.
/// Every word, lowercased, letters and digits only ("LORD," → "lord"): for comparing word order.
pub fn plain_tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()).filter(|w| !w.is_empty()).collect()
}

pub fn meaningful_words(text: &str) -> Vec<String> {
    const SKIP: &[&str] = &[
        "a", "an", "the", "and", "but", "or", "in", "of", "to", "is", "it", "be", "he", "she", "ye", "thy", "thee", "thou", "unto",
        "his", "her", "its", "my", "our", "your", "their", "was", "are", "were", "that", "this", "from", "with", "for", "not", "i",
        "we", "by", "at",
    ];
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() >= 3 && !SKIP.contains(w))
        .map(str::to_string)
        .collect()
}

/// A light stem: "loveth"/"loves" meet "love".
pub fn stem_lite(w: &str) -> String {
    w.strip_suffix("eth").or_else(|| w.strip_suffix('s')).unwrap_or(w).to_string()
}

/// Pronouns and helper verbs are in nearly every verse and sentence of
/// preaching; they are never evidence for one verse over another.
const NON_EVIDENCE: &[&str] = &[
    "them", "they", "him", "you", "me", "us", "who", "whom", "which", "what", "when", "then", "there", "shall", "will", "have",
    "hath", "had", "did", "all", "said", "saith", "say", "upon", "into", "also", "even",
];

/// The everyday words of preaching, said whatever is quoted.
pub fn preaching_common(stem: &str) -> bool {
    ["jesu", "christ", "lord", "god", "name", "holy", "ghost", "spirit", "father", "amen", "praise", "glory", "hallelujah"].contains(&stem)
}

pub fn evidence_words(text: &str) -> HashSet<String> {
    meaningful_words(text).into_iter().filter(|w| !NON_EVIDENCE.contains(&w.as_str())).map(|w| stem_lite(&w)).collect()
}

/// Words that can tell one verse from another: evidence words minus the
/// everyday words of preaching.
pub fn telling_words(text: &str) -> HashSet<String> {
    evidence_words(text).into_iter().filter(|w| !preaching_common(w)).collect()
}

/// Does `speech` fit verse A at least as well as verse B? Only the words one
/// verse has and the other doesn't are counted — shared words can't tell them
/// apart. A holds when it has words of its own heard, at least as many as B;
/// or when B has none of its own heard and A explains what was said.
pub fn fits_first_at_least_as_well(a: &str, b: &str, speech: &str) -> bool {
    let heard = evidence_words(speech);
    if heard.is_empty() {
        return false;
    }
    let (aw, bw) = (evidence_words(a), evidence_words(b));
    let (mut a_only, mut b_only, mut shared) = (0, 0, 0);
    for w in &aw {
        if heard.contains(w) {
            if bw.contains(w) { shared += 1 } else { a_only += 1 }
        }
    }
    for w in &bw {
        if !aw.contains(w) && heard.contains(w) {
            b_only += 1;
        }
    }
    if b_only == 0 {
        return a_only + shared > 0;
    }
    a_only > 0 && a_only >= b_only
}

/// Words of `verse` (its evidence words, minus everyday preaching words)
/// heard in `speech`.
pub fn explains(verse: &str, heard: &HashSet<String>) -> usize {
    evidence_words(verse).iter().filter(|w| heard.contains(*w) && !preaching_common(w)).count()
}

/// Of `rivals`, the one that explains clearly more of what was said than
/// `verse`: at least 3 of its words heard and 2 more than `verse`, with no
/// tie for that. Returns the rival's index and its count.
pub fn explained_better_by(verse: &str, rivals: &[&str], speech: &str) -> Option<(usize, usize)> {
    let heard: HashSet<String> = meaningful_words(speech).into_iter().filter(|w| !NON_EVIDENCE.contains(&w.as_str())).map(|w| stem_lite(&w)).collect();
    let own = explains(verse, &heard);
    let mut scored: Vec<(usize, usize)> = rivals.iter().enumerate().map(|(i, r)| (i, explains(r, &heard))).filter(|(_, n)| *n >= 3.max(own + 2)).collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    match scored.as_slice() {
        [] => None,
        [best, second, ..] if second.1 == best.1 => None,
        [best, ..] => Some(*best),
    }
}

/// Words only one of two verses has, heard in speech: (a's, b's).
pub fn own_words_heard(a: &HashSet<String>, b: &HashSet<String>, heard: &HashSet<String>) -> (usize, usize) {
    let a_only = a.iter().filter(|w| !b.contains(*w) && heard.contains(*w)).count();
    let b_only = b.iter().filter(|w| !a.contains(*w) && heard.contains(*w)).count();
    (a_only, b_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verse_on_screen_keeps_its_place_when_the_words_are_its_own() {
        let on_screen = "The LORD said unto my Lord, Sit thou at my right hand, until I make thine enemies thy footstool.";
        let twin = "But to which of the angels said he at any time, Sit on my right hand, until I make thine enemies thy footstool?";
        assert!(fits_first_at_least_as_well(on_screen, twin, "until I make thine enemies thy footstool"));
        assert!(!fits_first_at_least_as_well(on_screen, twin, "to which of the angels said he at any time"));
        assert!(!fits_first_at_least_as_well(on_screen, twin, "good morning church"), "speech with neither verse's words decides nothing");
    }

    #[test]
    fn a_rival_must_explain_clearly_more() {
        let thin = "This cup is the new testament in my blood, which is shed for you.";
        let better = "For this is my blood of the new testament, which is shed for many for the remission of sins.";
        // Two of the thin match's own words heard, four of the rival's.
        let said = "this is my blood which is shed for the remission of the sins";
        assert_eq!(explained_better_by(thin, &[better], said), Some((0, 4)));
        // Only the words they share: nothing clearly better.
        assert_eq!(explained_better_by(thin, &[better], "this is my blood which is shed"), None);
    }
}
