//! A stemmer scoped to the English of the KJV and of modern speech, applied
//! the same way to verse text and to the transcript, so forms of a word meet:
//! come / coming / cometh / comes / came → "com".
//!
//! In order:
//! 1. Spellings that sound the same, which speech-to-text writes the modern
//!    American way ("honour" → "honor", "shew" → "show"), homophones a speech
//!    engine can only guess between ("profit"/"prophet"), and the articles
//!    (preachers and engines swap "a"/"the" freely).
//! 2. Irregular forms no suffix rule reaches (hath, saith, begat, brethren).
//! 3. Suffixes: -ies/-ied, -eth/-est, -ing, -ed, -es, -s.
//! 4. A final -e and a doubled final consonant drop.
//!
//! The stem needn't be a real word — it only has to be the same on both sides.

use std::collections::HashMap;
use std::sync::LazyLock;

static SPELLING: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("oh", "o"), ("unto", "to"), ("yeah", "yea"),
        ("shew", "show"), ("shewed", "showed"), ("sheweth", "showeth"), ("shewing", "showing"), ("shewn", "shown"), ("shewest", "showest"),
        ("honour", "honor"), ("honours", "honors"), ("honoured", "honored"), ("honoureth", "honoreth"), ("honourable", "honorable"),
        ("favour", "favor"), ("favours", "favors"), ("favoured", "favored"), ("favoureth", "favoreth"),
        ("labour", "labor"), ("labours", "labors"), ("laboured", "labored"), ("laboureth", "laboreth"), ("labouring", "laboring"),
        ("neighbour", "neighbor"), ("neighbours", "neighbors"),
        ("colour", "color"), ("colours", "colors"), ("coloured", "colored"),
        ("armour", "armor"), ("armours", "armors"), ("rumour", "rumor"), ("rumours", "rumors"),
        ("saviour", "savior"), ("saviours", "saviors"), ("behaviour", "behavior"), ("behaviours", "behaviors"),
        ("endeavour", "endeavor"), ("endeavoured", "endeavored"), ("endeavouring", "endeavoring"),
        ("fulness", "fullness"),
        ("a", "the"), ("an", "the"),
        ("profit", "prophet"), ("profits", "prophets"), ("wholly", "holy"), ("prey", "pray"), ("sole", "soul"), ("soles", "souls"),
    ]
    .into_iter()
    .collect()
});

static IRREGULAR: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("am", "be"), ("are", "be"), ("is", "be"), ("was", "be"), ("were", "be"), ("been", "be"), ("being", "be"), ("be", "be"), ("art", "be"),
        ("hath", "hav"), ("has", "hav"), ("have", "hav"), ("had", "hav"), ("having", "hav"), ("hast", "hav"),
        ("saith", "sai"), ("said", "sai"), ("says", "sai"), ("saying", "sai"), ("say", "sai"),
        ("doth", "do"), ("doeth", "do"), ("does", "do"), ("did", "do"), ("done", "do"), ("doing", "do"),
        ("went", "go"), ("gone", "go"), ("shown", "show"), ("came", "com"), ("saw", "se"), ("seen", "se"),
        ("knew", "kno"), ("known", "kno"), ("took", "tak"), ("taken", "tak"), ("gave", "giv"), ("given", "giv"), ("heard", "hear"),
        ("brought", "bring"), ("bought", "buy"), ("taught", "teach"), ("sought", "seek"), ("thought", "think"), ("fought", "fight"),
        ("caught", "catch"), ("wrought", "work"),
        ("ate", "eat"), ("eaten", "eat"), ("ran", "run"), ("rose", "ris"), ("risen", "ris"), ("fell", "fall"), ("fallen", "fall"),
        ("spoke", "speak"), ("spoken", "speak"), ("spake", "speak"), ("broke", "break"), ("broken", "break"), ("brake", "break"),
        ("stood", "stand"), ("sat", "sit"), ("wrote", "writ"), ("written", "writ"), ("chose", "choos"), ("chosen", "choos"),
        ("brethren", "brother"), ("men", "man"), ("women", "woman"), ("children", "child"),
        ("beget", "beget"), ("begat", "beget"), ("begetteth", "beget"), ("begotten", "beget"),
        ("bear", "bear"), ("bare", "bear"), ("bore", "bear"), ("born", "bear"), ("borne", "bear"),
        ("slay", "slay"), ("slew", "slay"), ("slain", "slay"), ("smite", "smite"), ("smote", "smite"), ("smitten", "smite"),
        ("draw", "draw"), ("drew", "draw"), ("drawn", "draw"), ("swear", "swear"), ("swore", "swear"), ("sware", "swear"), ("sworn", "swear"),
        ("tear", "tear"), ("tore", "tear"), ("torn", "tear"), ("wear", "wear"), ("wore", "wear"), ("worn", "wear"),
        ("shake", "shake"), ("shook", "shake"), ("shaken", "shake"), ("forsake", "forsak"), ("forsook", "forsak"), ("forsaken", "forsak"),
        ("hide", "hid"), ("hid", "hid"), ("hidden", "hid"), ("keep", "keep"), ("kept", "keep"), ("keepeth", "keep"),
        ("weep", "weep"), ("wept", "weep"), ("sleep", "sleep"), ("slept", "sleep"), ("leave", "leav"), ("left", "leav"),
        ("lose", "los"), ("lost", "los"), ("tell", "tell"), ("told", "tell"), ("sell", "sell"), ("sold", "sell"), ("hold", "hold"), ("held", "hold"),
        ("understand", "understand"), ("understood", "understand"), ("build", "build"), ("built", "build"), ("send", "send"), ("sent", "send"),
        ("spend", "spend"), ("spent", "spend"), ("feel", "feel"), ("felt", "feel"), ("find", "find"), ("found", "find"),
        ("bind", "bind"), ("bound", "bind"), ("mean", "mean"), ("meant", "mean"), ("deal", "deal"), ("dealt", "deal"),
        ("dwell", "dwell"), ("dwelt", "dwell"), ("cleave", "cleav"), ("clave", "cleav"), ("cloven", "cleav"),
        ("strive", "striv"), ("strove", "striv"), ("striven", "striv"), ("arise", "ris"), ("arose", "ris"), ("arisen", "ris"),
        ("fly", "fly"), ("flew", "fly"), ("flown", "fly"), ("grow", "grow"), ("grew", "grow"), ("grown", "grow"),
        ("throw", "throw"), ("threw", "throw"), ("thrown", "throw"), ("blow", "blow"), ("blew", "blow"), ("blown", "blow"),
        ("shine", "shin"), ("shone", "shin"), ("shined", "shin"), ("steal", "steal"), ("stole", "steal"), ("stolen", "steal"),
        ("ride", "ride"), ("rode", "ride"), ("ridden", "ride"),
    ]
    .into_iter()
    .collect()
});

fn suffix_strip(s: &str) -> String {
    let n = s.len();
    if n <= 3 {
        return s.to_string();
    }
    if n > 4 && (s.ends_with("ies") || s.ends_with("ied")) {
        return format!("{}y", &s[..n - 3]);
    }
    if n > 4 && (s.ends_with("eth") || s.ends_with("est")) {
        return s[..n - 3].to_string();
    }
    if n > 5 && s.ends_with("ing") {
        return s[..n - 3].to_string();
    }
    if n > 4 && (s.ends_with("ed") || s.ends_with("es")) {
        return s[..n - 2].to_string();
    }
    if s.ends_with('s') && !s.ends_with("ss") && !s.ends_with("us") && !s.ends_with("is") {
        return s[..n - 1].to_string();
    }
    s.to_string()
}

fn collapse_end(mut s: String) -> String {
    if s.len() > 3 && s.ends_with('e') {
        s.pop();
    }
    if s.len() > 3 {
        let b = s.as_bytes();
        let (a, c) = (b[b.len() - 1], b[b.len() - 2]);
        if a == c && !b"aeiou".contains(&a) {
            s.pop();
        }
    }
    s
}

/// The stem of one normalized word.
pub fn heal(w: &str) -> String {
    if w.is_empty() {
        return String::new();
    }
    if let Some(spelled) = SPELLING.get(w) {
        return match IRREGULAR.get(spelled) {
            Some(i) => i.to_string(),
            None => collapse_end(suffix_strip(spelled)),
        };
    }
    if let Some(i) = IRREGULAR.get(w) {
        return i.to_string();
    }
    if w.len() <= 2 {
        return w.to_string();
    }
    collapse_end(suffix_strip(w))
}

#[cfg(test)]
mod tests {
    use super::heal;

    #[test]
    fn forms_meet() {
        for w in ["come", "coming", "cometh", "comes", "came"] {
            assert_eq!(heal(w), "com", "{w}");
        }
        assert_eq!(heal("honour"), heal("honor"));
        assert_eq!(heal("shewed"), heal("showed"));
        assert_eq!(heal("kept"), heal("keeping"));
        assert_eq!(heal("loved"), heal("love"));
        assert_eq!(heal("a"), heal("the"));
        assert_eq!(heal("jesus"), "jesus");
        assert_eq!(heal("babies"), "baby");
    }
}
