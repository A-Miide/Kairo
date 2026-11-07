//! Text as the word matchers see it.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Lowercase, accents folded, only letters, digits and single spaces
/// ("The LORD's" → "the lords", "João" → "joao").
pub fn norm(s: &str) -> String {
    let lower = s.to_lowercase();
    let folded = if lower.is_ascii() { lower } else { crate::reference::i18n::fold(&lower) };
    let kept: String = folded.chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c.is_whitespace()).collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    // "are become" (KJV) and "have become" (said today) are one form: "all things are become new" is
    // said "all things have become new" (owner review 2026-11-05).
    // The KJV's split compounds as one word, as they're said and transcribed: "to day", "to morrow",
    // "for ever", "every one" ("Jesus Christ the same yesterday, and to day, and for ever" is said "the
    // same yesterday today and forever"; owner review 2026-11-05).
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next = words.get(i + 1).copied().unwrap_or("");
        let joined = match (w, next) {
            ("to", "day") => Some("today"),
            ("to", "morrow") => Some("tomorrow"),
            ("for", "ever") => Some("forever"),
            ("for", "evermore") => Some("forevermore"),
            ("every", "one") => Some("everyone"),
            _ => None,
        };
        if let Some(j) = joined {
            out.push(j);
            i += 2;
            continue;
        }
        if !(matches!(w, "are" | "is" | "art" | "have" | "has" | "hath" | "hast") && next == "become") {
            out.push(w);
        }
        i += 1;
    }
    out.join(" ")
}

/// A Portuguese Bible in the spelling before the 1911/1943 reforms ("Christo", "peccado", "condemnação",
/// "aquelle", "considerae-vos") as it is spoken and transcribed today ("cristo", "pecado", "condenacao",
/// "aquele", "considerai vos"): lowercased, accents folded. For matching only; the text shown is
/// untouched. The Almeida pack is in the old spelling (pt-hernandes-romanos8: Romans 8:1 unmatched).
pub fn modern_portuguese(text: &str) -> String {
    let t = norm(text);
    t.split(' ')
        .map(|w| {
            let mut w = w.replace("chr", "cr").replace("th", "t").replace("ph", "f").replace('y', "i");
            for (old, new) in [("cc", "c"), ("ll", "l"), ("mm", "m"), ("nn", "n"), ("pp", "p"), ("tt", "t"), ("ff", "f"), ("bb", "b"), ("dd", "d"), ("gg", "g"), ("mn", "n"), ("ct", "t"), ("pt", "t")] {
                w = w.replace(old, new);
            }
            if w.len() > 2 && w.ends_with("ae") {
                w.replace_range(w.len() - 2.., "ai");
            }
            match w.as_str() {
                "egreja" => "igreja".to_string(),
                _ => w,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A language's Bible as detection matches it: Portuguese in today's spelling, the others as they are.
pub fn for_matching(own: &kairo_bible::Translation, lang: crate::reference::Lang) -> kairo_bible::Translation {
    match lang {
        crate::reference::Lang::Pt => own.mapped(modern_portuguese),
        _ => own.clone(),
    }
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

/// The structural words of the other languages detection reads (Spanish, Portuguese, French, German),
/// accents folded. None is a word of the KJV or the BSB ("sin", "son", "sea", "die", "man", "war", "den",
/// "pour", "o", "plus" are left out), so English matching is unchanged.
pub static OTHER_STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Spanish
        "de", "la", "que", "y", "los", "se", "del", "las", "un", "por", "con", "una", "su", "para", "es", "al", "lo", "como",
        "mas", "pero", "sus", "le", "si", "sobre", "este", "ya", "entre", "cuando", "todo", "esta", "ser", "dos", "tambien",
        "fue", "habia", "era", "muy", "hasta", "desde", "mi", "porque", "solo", "han", "yo", "vez", "nos", "ni", "uno", "donde",
        "ese", "eso", "esa", "ahora", "cada", "e", "te", "otro", "otros", "otra", "aunque", "tan", "tanto", "ella", "ellos", "ellas",
        "sido", "os", "vosotros", "nosotros", "les", "tu", "ti", "mis", "tus", "esto", "estos", "estas", "aquel", "aquella", "aquellos",
        "pues", "cual", "cuales", "quien", "quienes", "asi", "u", "estan", "estaba", "estoy", "somos", "sois", "soy", "eres", "fueron",
        "sean", "seria", "tiene", "tienen", "tenia", "hace", "hizo", "dijo", "dice", "decir", "aqui", "alli", "entonces", "sino", "segun",
        "contra", "hacia", "mismo", "misma", "todos", "todas", "toda",
        // Portuguese
        "do", "da", "um", "com", "nao", "uma", "os", "na", "dos", "ao", "ele", "das", "seu", "sua", "ou", "quando", "muito", "ja",
        "eu", "pelo", "pela", "isso", "depois", "aos", "seus", "nas", "esse", "eles", "voce", "essa", "num", "nem", "suas",
        "meu", "minha", "numa", "pelos", "elas", "lhe", "deles", "essas", "esses", "pelas", "dele", "vos", "lhes", "meus", "minhas",
        "teu", "tua", "teus", "tuas", "nosso", "nossa", "nossos", "nossas", "dela", "delas", "estes", "aquele", "aqueles", "aquelas",
        "isto", "aquilo", "estamos", "estao", "sou", "sao", "foi", "fui", "tem", "ter", "havia", "eis", "pois", "entao", "ali", "disse",
        "diz",
        // French
        "le", "les", "des", "du", "une", "et", "est", "qui", "dans", "ce", "il", "ne", "aux", "avec", "sa", "ses", "par",
        "mais", "comme", "nous", "vous", "ils", "elle", "elles", "je", "leur", "leurs", "ont", "sont", "ete", "etre", "avait",
        "cette", "cet", "ces", "mon", "ma", "mes", "ton", "ta", "tes", "notre", "votre", "tout", "tous", "toute", "toutes", "donc",
        "quand", "aussi", "bien", "c", "l", "d", "j", "m", "n", "qu", "avons", "avez", "etait", "etaient", "fut", "sera", "dit", "alors",
        "ici", "voici", "voila", "meme",
        // German
        "der", "das", "und", "von", "zu", "mit", "sich", "des", "auf", "ist", "im", "dem", "nicht", "ein", "eine", "als", "auch",
        "dass", "sie", "nach", "wird", "bei", "einer", "um", "sind", "noch", "wie", "einem", "uber", "einen", "zum", "haben", "nur",
        "oder", "aber", "vor", "bis", "mehr", "durch", "sein", "wurde", "sei", "ich", "du", "wir", "ihr", "mich", "mir", "dich",
        "dir", "uns", "euch", "ihn", "ihm", "ihnen", "mein", "dein", "unser", "euer", "seine", "ihre", "kein", "keine", "wenn", "weil",
        "denn", "doch", "ja", "nein", "hat", "hatte", "wer", "wo", "da", "dann", "hier", "diese", "dieser", "dieses", "werden", "wurden",
        "habe", "seid", "bin", "bist", "sondern", "seinen", "seinem", "seiner", "meine", "meinen", "meiner", "deine", "deinen", "unsere",
        "unseren", "eure", "euren", "alle", "allen", "aller", "alles",
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

/// The same everyday words of preaching in the other languages detection reads, accents folded. None
/// is a word of the KJV or the BSB, so English matching is unchanged.
pub const OTHER_PREACHING_COMMON: &[&str] = &[
    "dios", "senor", "cristo", "jesucristo", "espiritu", "santo", "santa", "padre", "gloria", "aleluya",
    "deus", "senhor", "espirito", "aleluia",
    "dieu", "seigneur", "esprit", "pere", "gloire",
    "gott", "gottes", "herr", "herrn", "christus", "christi", "jesu", "geist", "geistes", "heilig", "heilige", "heiligen", "heiliger",
    "vater", "herrlichkeit", "halleluja",
];

pub fn is_preaching_common(w: &str) -> bool {
    PREACHING_COMMON_WORDS.contains(w) || OTHER_PREACHING_COMMON.contains(&w)
}

pub fn is_stop_word(w: &str) -> bool {
    STOP_WORDS.contains(w) || OTHER_STOP_WORDS.contains(w)
}
