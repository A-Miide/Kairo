//! Passages referred to by name ("the Lord's Prayer", "the prodigal son",
//! "David and Goliath"), and a few famous lines too short to match ("Jesus
//! wept"). A name is usually a passing allusion, not an
//! instruction to read, so the passage's opening verse is offered in Possible
//! Matches only — never sent. Only names that point to one passage are listed;
//! stories told in several Gospels (the feeding of the five thousand) are left out.

use std::sync::LazyLock;

use kairo_bible::{Book, VerseRef};

use super::core::{Candidate, Engine};
use super::types::{Hold, Method, Target};
use crate::re::re;

const NAMED_PASSAGE_SCORE: f32 = 0.70;

/// (name pattern, book, chapter, opening verse).
const PASSAGES: &[(&str, &str, u16, u16)] = &[
    (r"the lord'?s prayer", "Matthew", 6, 9),
    (r"the beatitudes", "Matthew", 5, 3),
    (r"sermon on the mount", "Matthew", 5, 1),
    (r"the love chapter", "1 Corinthians", 13, 1),
    (r"the great commission", "Matthew", 28, 18),
    (r"the great commandment", "Matthew", 22, 37),
    (r"the golden rule", "Matthew", 7, 12),
    (r"(whole )?armou?r of god", "Ephesians", 6, 10),
    (r"the fruits? of the spirit", "Galatians", 5, 22),
    (r"the ten commandments", "Exodus", 20, 1),
    (r"the shepherd psalm", "Psalms", 23, 1),
    (r"the faith chapter|hall of faith|heroes of faith", "Hebrews", 11, 1),
    (r"prodigal son", "Luke", 15, 11),
    (r"good samaritan", "Luke", 10, 30),
    (r"(priestly|aaronic) blessing", "Numbers", 6, 24),
    (r"the shema", "Deuteronomy", 6, 4),
    (r"the magnificat", "Luke", 1, 46),
    (r"valley of (the )?dry bones", "Ezekiel", 37, 1),
    (r"parable of the sower", "Matthew", 13, 3),
    (r"parable of the talents", "Matthew", 25, 14),
    (r"the burning bush", "Exodus", 3, 1),
    (r"jacob'?s ladder", "Genesis", 28, 10),
    (r"tower of babel", "Genesis", 11, 1),
    (r"day of pentecost", "Acts", 2, 1),
    (r"wedding (at|in) cana", "John", 2, 1),
    // Stories, each told in one place.
    (r"david and goliath", "1 Samuel", 17, 4),
    (r"daniel in the lions'? den|the lions'? den", "Daniel", 6, 16),
    (r"fiery furnace|shadrach,? meshach,? and abednego", "Daniel", 3, 19),
    (r"jonah and the (whale|big fish)", "Jonah", 1, 17),
    (r"noah'?s ark", "Genesis", 6, 14),
    (r"walls of jericho", "Joshua", 6, 1),
    (r"zacchaeus", "Luke", 19, 1),
    (r"raising of lazarus|lazarus,? come forth", "John", 11, 38),
    (r"(parting|crossing) (of )?the red sea", "Exodus", 14, 21),
    (r"woman at the well", "John", 4, 5),
    (r"mount carmel", "1 Kings", 18, 20),
    (r"samson and delilah", "Judges", 16, 4),
    (r"coat of many colou?rs", "Genesis", 37, 3),
    (r"(wise and foolish|ten) virgins", "Matthew", 25, 1),
    (r"peter walk(ed|ing|s)? on (the )?water", "Matthew", 14, 28),
    (r"ananias and sapphira", "Acts", 5, 1),
    (r"road to damascus|damascus road", "Acts", 9, 1),
    (r"cain and abel", "Genesis", 4, 1),
    (r"abraham (offer|sacrific)\w* isaac|binding of isaac", "Genesis", 22, 1),
    // Famous lines too short for the text matchers to trust, each found in
    // one verse only (a line in several books is left to the matchers).
    (r"i am that i am", "Exodus", 3, 14),
    (r"jesus wept", "John", 11, 35),
    (r"it is finished", "John", 19, 30),
    (r"be still,? and know that i am god", "Psalms", 46, 10),
    (r"pray without ceasing", "1 Thessalonians", 5, 17),
    (r"rejoice evermore", "1 Thessalonians", 5, 16),
    (r"quench not the spirit", "1 Thessalonians", 5, 19),
    (r"remember lot'?s wife", "Luke", 17, 32),
];

static PATTERNS: LazyLock<Vec<(regex::Regex, Book, u16, u16)>> = LazyLock::new(|| {
    PASSAGES
        .iter()
        .map(|(p, book, ch, v)| (re(&format!(r"(?i)\b({p})\b")), Book::from_name(book).expect("book name"), *ch, *v))
        .collect()
});

impl Engine {
    /// Offers every passage named in a final with no reference of its own.
    pub(crate) fn offer_named_passages(&mut self, text: &str, now: u64) {
        let text = text.replace('\u{2019}', "'");
        let named: Vec<_> = PATTERNS.iter().filter(|(re, ..)| re.is_match(&text)).map(|(_, b, c, v)| (*b, *c, *v)).collect();
        for (book, chapter, verse) in named {
            if let Some(v) = self.k.id(VerseRef::new(book, chapter, verse)) {
                let mut c = Candidate::new(vec![v], Method::NamedPassage, NAMED_PASSAGE_SCORE, Target::Suggestions);
                c.hold = Some(Hold::NamedPassage);
                self.decide(c, now);
            }
        }
    }
}
