//! Citations as people say them and recognisers write them: digit by digit, with accents, with a
//! mis-heard book, with "point" or "versus" in place of the colon. Each row is a speech pattern and the
//! citation it must give (checked against the real Bible's shape). The second table is ordinary speech
//! and unrelated numbers that must NOT become a different citation.

use std::path::PathBuf;
use std::sync::LazyLock;

use kairo_bible::Corpus;
use kairo_detect::reference::{Active, Books, Citation, Lang, Parser, Shape, Verses};

static SHAPE: LazyLock<Shape> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Shape::new(&Corpus::load(&dir).unwrap())
});

fn parser() -> Parser<'static> {
    Parser { books: Books::get(), shape: &SHAPE, lang: Lang::En }
}

fn fmt(c: &Citation) -> String {
    let v = match &c.verses {
        Verses::Chapter => String::new(),
        Verses::One(v) => v.to_string(),
        Verses::Span(a, z) => format!("{a}-{z}"),
        Verses::Spans(s) => s.iter().map(|(a, z)| if a == z { a.to_string() } else { format!("{a}-{z}") }).collect::<Vec<_>>().join("+"),
    };
    format!("{} {}:{v}", c.book, c.chapter)
}

fn first(text: &str) -> Option<String> {
    parser().parse_all(text, Active::default()).first().map(fmt)
}

/// (what was said or written, the citation it is)
const SAID: &[(&str, &str)] = &[
    // Psalm 110, every way
    ("Psalm one ten verse one", "Psalms 110:1"),
    ("Psalm one-ten verse one", "Psalms 110:1"),
    ("Psalm one oh ten verse one", "Psalms 110:1"),
    ("Psalm one one oh verse one", "Psalms 110:1"),
    ("Psalm one one nought verse one", "Psalms 110:1"),
    ("Psalm one one nil verse one", "Psalms 110:1"),
    ("Psalm one one naught verse one", "Psalms 110:1"),
    ("Psalm wan wan zero verse one", "Psalms 110:1"),
    ("Psalm one hundred and tin verse one", "Psalms 110:1"),
    ("Psalm one ten verse won", "Psalms 110:1"),
    ("San one one one zero one to three", "Psalms 110:1-3"),
    ("Psalm one one one zero one to three", "Psalms 110:1-3"),
    ("Psalm one ten one to three", "Psalms 110:1-3"),
    // digit-by-digit verses and chapters
    ("John three one six", "John 3:16"),
    ("John chapter three verse one six", "John 3:16"),
    ("Luke four one eight", "Luke 4:18"),
    ("Luke chapter four verse one eight", "Luke 4:18"),
    ("Romans eight two eight", "Romans 8:28"),
    ("Mark eleven two four", "Mark 11:24"),
    ("Psalm two three verse one", "Psalms 23:1"),
    ("Genesis one two verse three", "Genesis 12:3"),
    // "and" and ordinals inside a verse number
    ("Romans eight verse twenty and eight", "Romans 8:28"),
    ("Romans eight twenty-eighth", "Romans 8:28"),
    ("Romans eight twenty eighth", "Romans 8:28"),
    ("Isaiah forty first chapter tenth verse", "Isaiah 41:10"),
    ("Isaiah forty-first chapter and the tenth verse", "Isaiah 41:10"),
    // accents and spellings
    ("John tree verse sixteen", "John 3:16"),
    ("John thri verse sixteen", "John 3:16"),
    ("John free sixteen", "John 3:16"),
    ("Luke fo verse eighteen", "Luke 4:18"),
    ("Psalm twenty free", "Psalms 23:"),
    ("Isaiah fourty one ten", "Isaiah 41:10"),
    // numbered books
    ("I John four four", "1 John 4:4"),
    ("II Timothy one seven", "2 Timothy 1:7"),
    ("III John four", "3 John 1:4"),
    ("first Jon four four", "1 John 4:4"),
    ("John the first, four four", "1 John 4:4"),
    ("Timothy the second chapter one verse seven", "2 Timothy 1:7"),
    // separators
    ("Romans eight point twenty eight", "Romans 8:28"),
    ("Romans eight stroke twenty eight", "Romans 8:28"),
    ("Romans eight slash twenty eight", "Romans 8:28"),
    ("Romans eight, twenty eight", "Romans 8:28"),
    ("Romans eight and twenty eight", "Romans 8:28"),
    ("Romans eight dash twenty eight", "Romans 8:28"),
    ("Romans eight to twenty eight", "Romans 8:28"),
    ("Romans 8-28", "Romans 8:28"),
    ("Isaiah four one ten", "Isaiah 41:10"),
    ("Psalm one ten, versus one", "Psalms 110:1"),
    ("Psalm one ten vs one", "Psalms 110:1"),
    ("Psalm one ten vs. one", "Psalms 110:1"),
    ("Psalm one ten v one", "Psalms 110:1"),
    // the book said last (live, 2026-10-29)
    ("Elihu said in chapter thirty two and verse eight of Job", "Job 32:8"),
    ("chapter thirty two verse eight of Job", "Job 32:8"),
    ("verse eight of chapter thirty two of Job", "Job 32:8"),
];

/// Said or written plainly: these must stay what they always were.
const PLAIN: &[(&str, Option<&str>)] = &[
    ("John three sixteen", Some("John 3:16")),
    ("Genesis one one", Some("Genesis 1:1")),
    ("Genesis chapter one verse one", Some("Genesis 1:1")),
    ("Psalm one one one verse one", Some("Psalms 111:1")),
    ("Psalm one one zero verse one", Some("Psalms 110:1")),
    ("Psalm one nineteen one oh five", Some("Psalms 119:105")),
    ("Romans eight twenty eight", Some("Romans 8:28")),
    ("Luke four verse eighteen to twenty one", Some("Luke 4:18-21")),
    ("First John four four", Some("1 John 4:4")),
    ("Romans eight verse one and two", Some("Romans 8:1-2")),
    // a range of chapters the book does have stays a range of chapters (no verse is made up)
    ("Genesis one two three", Some("Genesis 1:2-3")),
    ("Isaiah four one two", Some("Isaiah 4:1-2")),
    // verses said straight after each other are the next verse, not a two-digit number
    ("Luke four verse one two", Some("Luke 4:1-2")),
    // a two-digit verse the chapter doesn't have is not made up
    ("Jude one nine nine", Some("Jude 1:9")),
    // ordinary speech
    ("we went to San Francisco and then to San Antonio", None),
    ("I John said that we should go", None),
    ("the first verse of the song was twenty four", None),
    ("it was free for twenty years", None),
    ("I came and had a tin of beans", None),
];

#[test]
fn how_people_say_and_recognisers_write_citations() {
    let mut wrong = Vec::new();
    for (said, want) in SAID {
        let got = first(said);
        if got.as_deref() != Some(*want) {
            wrong.push(format!("{said:55} want {want:16} got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "\n{}\n", wrong.join("\n"));
}

#[test]
fn plain_speech_is_unchanged() {
    let mut wrong = Vec::new();
    for (said, want) in PLAIN {
        let got = first(said);
        if got.as_deref() != *want {
            wrong.push(format!("{said:55} want {want:?} got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "\n{}\n", wrong.join("\n"));
}
