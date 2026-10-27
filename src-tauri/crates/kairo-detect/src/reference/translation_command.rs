//! Spoken commands that change the Bible translation: "switch to the NIV", "let's read this in the
//! New Living Translation", "can we have it in the Message". Ported from 3.x
//! (`server/translation_commands.js`, v3.2.13), which the 4.0 rewrite left out.
//!
//! A translation's letters said anywhere ("NIV", "N L T", "the ESV says…") switch to it: the
//! preacher naming the version by its letters is reading from it (owner, 2026-10-27, after "go
//! from verse twenty one, give me the NLT" changed nothing live). Its full name is said in passing
//! far more often ("the King James was translated in 1611"), so a full name counts only in a
//! COMMAND: a verb phrase that opens its clause, then the translation.

use std::sync::LazyLock;

use regex::Regex;

/// Code → the ways it is said.
const ALIASES: &[(&str, &[&str])] = &[
    ("NKJV", &["new king james version", "new king james bible", "new king james", "nkjv"]),
    ("KJV", &["king james version", "king james bible", "king james", "authorized version", "kjv"]),
    ("NLT", &["new living translation", "new living", "nlt"]),
    ("NIV", &["new international version", "new international", "niv"]),
    ("ESV", &["english standard version", "english standard", "esv"]),
    ("NASB", &["new american standard bible", "new american standard version", "new american standard", "nasb"]),
    // Not bundled: recognised so a request for it can be logged, never shown.
    ("MSG", &["message bible", "message translation", "message version", "msg"]),
];

fn code_of(alias: &str) -> Option<&'static str> {
    let a = alias.split_whitespace().collect::<Vec<_>>().join(" ");
    ALIASES.iter().find(|(_, list)| list.contains(&a.as_str())).map(|(c, _)| *c)
}

/// Letters said or written one by one ("N I V", "N.I.V.", "e-s-v") become the code.
static SPELLED: LazyLock<Vec<(String, Regex)>> = LazyLock::new(|| {
    let mut codes: Vec<&str> = ALIASES.iter().map(|(c, _)| *c).collect();
    codes.sort_by_key(|c| std::cmp::Reverse(c.len()));
    codes
        .into_iter()
        .map(|c| {
            let letters: Vec<String> = c.to_lowercase().chars().map(|ch| ch.to_string()).collect();
            (c.to_lowercase(), Regex::new(&format!(r"\b{}\b", letters.join(r"[.\s-]*"))).unwrap())
        })
        .collect()
});

fn normalize(text: &str) -> String {
    let mut t = text.to_lowercase().replace(['’', '‘', '`'], "'").replace(['—', '–'], ", ");
    for (code, re) in SPELLED.iter() {
        t = re.replace_all(&t, code.as_str()).into_owned();
    }
    // Contractions become two words ("let's" → "let s") so they aren't confused with other words.
    t.replace('\'', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

static COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    let mut aliases: Vec<&str> = ALIASES.iter().flat_map(|(_, l)| l.iter().copied()).collect();
    aliases.sort_by_key(|a| std::cmp::Reverse(a.len()));
    let alias_re = aliases.iter().map(|a| a.replace(' ', r"\s+")).collect::<Vec<_>>().join("|");
    // The translation, captured ("the Message" alone only as "the message bible/translation/version").
    let t = format!(r"(?:the\s+)?(?:{alias_re})(?:[^a-z]|$)");
    let obj = r"(?:it|this|that|us|everything|over|back|the\s+(?:bible|scripture|scriptures|translation|version|text|screen|display)|our\s+(?:bible|translation|version))";
    let obj_v = r"(?:it|this|that|us|the\s+(?:verse|scripture|passage|text|bible))";
    let filler = r"(?:(?:please|kindly|okay|ok|now|alright|all\s+right|so|and|then|just|well|right|yes)\s+)*";
    let modal = [
        r"(?:can|could|would|will|may)\s+(?:we|you|i|somebody|someone|the\s+(?:tech|media|sound)\s+(?:team|guys|crew)|tech|media)\s+(?:please\s+)?",
        r"let(?:\s+s|s|\s+us)\s+",
        r"i\s+(?:want|need|would\s+like|d\s+like)\s+(?:us\s+)?to\s+",
        r"we(?:\s+will|\s+ll|\s+are\s+going\s+to|\s+re\s+going\s+to|\s+are\s+gonna|\s+re\s+gonna)\s+",
        r"i(?:\s+am|\s+m|\s+will|\s+ll)\s+(?:going\s+to\s+|gonna\s+)?",
    ]
    .join("|");
    let bare = [
        format!(r"(?:switch|change|swap|flip|set)\s+(?:{obj}\s+){{0,3}}(?:to|over\s+to|into|back\s+to)\s+{t}"),
        format!(r"(?:put|pull\s+up|bring\s+up|display)\s+(?:{obj_v}\s+){{0,2}}(?:in|on|with|using)\s+{t}"),
        // "Go from verse twenty one, give me the NLT" (live, 2026-10-27).
        format!(r"give\s+(?:us|me)\s+{t}"),
    ]
    .join("|");
    let with_modal = [
        bare.clone(),
        format!(r"(?:go|move|jump|head)\s+(?:over\s+|back\s+)?(?:to|into)\s+{t}"),
        format!(r"(?:use|read|show|get|display|put|have)\s+(?:{obj_v}\s+){{0,2}}(?:in|from|on|with|using)?\s*{t}"),
        format!(r"(?:pull\s+up|bring\s+up)\s+(?:{obj_v}\s+){{0,2}}(?:in\s+)?{t}"),
    ]
    .join("|");
    let clause = r"(?:^|[.!?;:,]\s+|\b(?:and|so|okay|ok|now|alright|then|please)\s+)";
    Regex::new(&format!(r"{clause}{filler}(?:(?:{modal})(?:{with_modal})|(?:{bare}))")).unwrap()
});

static ALIAS: LazyLock<Regex> = LazyLock::new(|| {
    let mut aliases: Vec<&str> = ALIASES.iter().flat_map(|(_, l)| l.iter().copied()).collect();
    aliases.sort_by_key(|a| std::cmp::Reverse(a.len()));
    Regex::new(&format!(r"\b(?:{})\b", aliases.iter().map(|a| a.replace(' ', r"\s+")).collect::<Vec<_>>().join("|"))).unwrap()
});

/// A citation with its translation said straight after the numbers: "Isaiah seven seven NLT or
/// message", "John 3:16 in the NIV" (dev build, Hallelujah Challenge day 15). Not when the
/// translation is what the preacher is quoting from ("Psalm 23 in the King James says…").
static CITED_IN: LazyLock<Regex> = LazyLock::new(|| {
    let mut aliases: Vec<&str> = ALIASES.iter().flat_map(|(_, l)| l.iter().copied()).collect();
    aliases.sort_by_key(|a| std::cmp::Reverse(a.len()));
    let a = aliases.iter().map(|a| a.replace(' ', r"\s+")).collect::<Vec<_>>().join("|");
    let n = r"(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred)";
    Regex::new(&format!(r"\b{n}[.,]?\s+(?:in\s+|from\s+)?(?:the\s+)?(?P<list>(?:{a})(?:\s*(?:,|or|and)\s*(?:the\s+)?(?:{a}))*)(?:[^a-z]|$)(?P<rest>.{{0,30}})")).unwrap()
});
/// A translation's letters as a word of their own ("niv", "nkjv"; spelled ones are joined by `normalize`).
static ACRONYM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:nkjv|kjv|nlt|niv|esv|nasb)\b").unwrap());

static QUOTING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:version\s+|translation\s+|bible\s+)?(?:says|said|reads|read\s+it|puts\s+it|renders|rendered|translates|translated|has\s+it|calls|called|uses)\b").unwrap());

/// The last translation command in `text`: its code ("NIV", or "MSG" when it isn't bundled).
/// With several named ("NLT or message"), the first bundled one.
pub fn translation_command(text: &str) -> Option<&'static str> {
    let t = normalize(text);
    if t.is_empty() {
        return None;
    }
    let pick = |words: &str| -> Option<&'static str> {
        let codes: Vec<&'static str> = ALIAS.find_iter(words).filter_map(|a| code_of(a.as_str())).collect();
        codes.iter().copied().find(|c| *c != "MSG").or_else(|| codes.first().copied())
    };
    // The translation named in each command is the last one in its words.
    let commanded = COMMAND.find_iter(&t).filter_map(|m| ALIAS.find_iter(m.as_str()).last().and_then(|a| code_of(a.as_str()))).last();
    let cited = CITED_IN.captures_iter(&t).filter(|c| !QUOTING.is_match(c.name("rest").map_or("", |r| r.as_str()))).filter_map(|c| pick(c.name("list")?.as_str())).last();
    let said = ACRONYM.find_iter(&t).last().and_then(|a| code_of(a.as_str()));
    cited.or(commanded).or(said)
}

#[cfg(test)]
mod tests {
    use super::translation_command as cmd;

    #[test]
    fn commands_switch() {
        assert_eq!(cmd("Switch to the NIV."), Some("NIV"));
        assert_eq!(cmd("let's read this in the New Living Translation"), Some("NLT"));
        assert_eq!(cmd("Can we put it in the King James"), Some("KJV"));
        assert_eq!(cmd("could the media team please switch it to the N.I.V."), Some("NIV"));
        assert_eq!(cmd("okay, can I have it in NLT"), Some("NLT"));
        assert_eq!(cmd("let me see. Can we have that in the message bible"), Some("MSG"));
        assert_eq!(cmd("now change over to the new king james"), Some("NKJV"));
        assert_eq!(cmd("me. Isaiah seven seven Nlt or message one of my"), Some("NLT"));
        assert_eq!(cmd("John 3:16 in the NIV please"), Some("NIV"));
        assert_eq!(cmd("Go from verse twenty one, give me the NLT"), Some("NLT"));
        assert_eq!(cmd("give me the New Living Translation"), Some("NLT"));
    }

    #[test]
    fn letters_said_anywhere_switch() {
        assert_eq!(cmd("in the NIV it says the Lord is my shepherd"), Some("NIV"));
        assert_eq!(cmd("in verse seven the N. L. T. renders it as"), Some("NLT"));
        assert_eq!(cmd("I love the ESV for this one"), Some("ESV"));
        assert_eq!(cmd("if you go to the ESV you will find"), Some("ESV"));
    }

    #[test]
    fn mentions_never_switch() {
        assert_eq!(cmd("the King James was translated in 1611"), None);
        assert_eq!(cmd("use the message of the cross"), None);
        assert_eq!(cmd("I have the message for you tonight"), None);
        assert_eq!(cmd("Psalm 23 in the King James says the Lord is my shepherd"), None);
        assert_eq!(cmd("I preached it seven times, the message is the same"), None);
        assert_eq!(cmd("he will give me the message tonight"), None);
    }
}
