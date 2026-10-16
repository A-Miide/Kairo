//! How spoken citations are read. Each case is a speech pattern the parser
//! must handle, gathered from live services; the parser itself has no
//! knowledge of these verses.

use std::path::PathBuf;
use std::sync::LazyLock;

use kairo_bible::{Book, Corpus};
use kairo_detect::reference::{Lang, book_mentions, Active, Books, Citation, Parser, PartialResolver, ReadingContext, Shape, Verses};

static SHAPE: LazyLock<Shape> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Shape::new(&Corpus::load(&dir).unwrap())
});

fn parser() -> Parser<'static> {
    Parser { books: Books::get(), shape: &SHAPE, lang: Lang::En }
}

fn resolver() -> PartialResolver<'static> {
    PartialResolver { books: Books::get(), shape: &SHAPE, lang: Lang::En }
}

fn b(name: &str) -> Book {
    Book::from_name(name).unwrap()
}

fn active(book: &str, chapter: u32) -> Active {
    Active { book: Some(b(book)), chapter: Some(chapter) }
}

/// "Book C:V", "Book C:V-W", "Book C:" (chapter only), "Book C:1-2+5".
fn fmt(c: &Citation) -> String {
    let v = match &c.verses {
        Verses::Chapter => String::new(),
        Verses::One(v) => v.to_string(),
        Verses::Span(a, z) => format!("{a}-{z}"),
        Verses::Spans(s) => s.iter().map(|(a, z)| if a == z { a.to_string() } else { format!("{a}-{z}") }).collect::<Vec<_>>().join("+"),
    };
    let alt = c.alt_chapter.map(|a| format!(" or {a}")).unwrap_or_default();
    format!("{} {}:{v}{alt}", c.book, c.chapter)
}

fn all(text: &str) -> Vec<String> {
    parser().parse_all(text, Active::default()).iter().map(fmt).collect()
}

fn all_in(text: &str, a: Active) -> Vec<String> {
    parser().parse_all(text, a).iter().map(fmt).collect()
}

fn partial(text: &str, a: Active, bare: bool) -> Option<String> {
    resolver().resolve(text, a, bare).map(|p| fmt(&p.citation) + if p.chapter_given { " (chapter said)" } else { "" })
}

#[test]
fn continuation_text_is_not_a_second_verse() {
    assert_eq!(all("Ephesians six verse 12 for we not against flesh and blood"), ["Ephesians 6:12"]);
}

#[test]
fn ranges_and_lists() {
    assert_eq!(all("Luke chapter 10 from verse one to two and seventeen to nineteen"), ["Luke 10:1-2+17-19"]);
    assert_eq!(all("psalm chapter one and verse one to three"), ["Psalms 1:1-3"]);
    // "Psalm one twenty-five" heard as "someone 25".
    assert_eq!(all("Someone 25, verse one and two, he say"), ["Psalms 125:1-2"]);
    assert_eq!(all("someone 19 verse 105"), ["Psalms 119:105"]);
    assert!(all("someone 25 years ago I was in Lagos").is_empty());
    assert_eq!(all("genesis chapter one verse one, verse three"), ["Genesis 1:1+3"]);
    assert_eq!(all("Romans 8 verses 28, 29 and 30"), ["Romans 8:28-30"]);
    assert_eq!(all("Romans eight from twenty eight down to thirty one"), ["Romans 8:28-31"]);
    assert_eq!(all("Romans 8 from 28 to 31"), ["Romans 8:28-31"]);
    assert_eq!(all("Psalm 91 from verse 14 to the end"), ["Psalms 91:14-16"]);
}

#[test]
fn chapter_of_book_order() {
    assert_eq!(all("the Centurion in chapter 7 of Luke from verse 1-6 a servant was there"), ["Luke 7:1-6"]);
    assert_eq!(all("verse 28 of Romans 8"), ["Romans 8:28"]);
    assert_eq!(all("twenty four of Genesis verse twelve"), ["Genesis 24:12"]);
    assert_eq!(all("one of John's disciples"), Vec::<String>::new());
}

#[test]
fn unnumbered_stem_offers_both_books() {
    let refs = parser().parse_all("corinthians ten three to five", Active::default());
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].book, b("1 Corinthians"));
    assert_eq!(refs[0].alternatives, vec![b("2 Corinthians")]);
    assert_eq!(book_mentions("the book of Corinthians", Books::get(), Lang::En), vec![]);
    assert_eq!(book_mentions("read Timothy chapter three", Books::get(), Lang::En), vec![]);
}

// A near-miss book name followed by the speech-engine homophone of a number
// ("for", "won", "too", "ate") is ordinary speech, not a citation: "the
// reasons for which" was read as "Romans 4" and put Romans 4:12 on screen.
#[test]
fn near_miss_book_before_a_homophone_is_not_a_citation() {
    for said in ["one of the reasons for which we are engaging power", "the reasons won", "reasons too many to name", "reasons ate it"] {
        assert_eq!(book_mentions(said, Books::get(), Lang::En), vec![], "{said}");
    }
    // A real number after it still counts.
    assert_eq!(book_mentions("in romans four", Books::get(), Lang::En), vec![b("Romans")]);
}

// A list of verses is said in order. "Romans 8 verse 26 and verse 20" (for 27) put
// Romans 8:20 on screen next to the right verse; a lower number is a slip, not a verse.
#[test]
fn a_verse_listed_out_of_order_is_not_added() {
    assert_eq!(all("romans chapter eight and verse 26 and verse 20"), ["Romans 8:26"]);
    assert_eq!(all("romans chapter eight verse 26 and 27"), ["Romans 8:26-27"]);
    assert_eq!(all("john three verse 16 and verse 18"), ["John 3:16+18"]);
}

#[test]
fn near_miss_book_names() {
    assert_eq!(all("first king nineteen and verse four"), ["1 Kings 19:4"]);
    assert_eq!(all("second corinthian ten verse three"), ["2 Corinthians 10:3"]);
    assert_eq!(all("first timothi three sixteen"), ["1 Timothy 3:16"]);
    assert_eq!(all("mork chapter two verse one"), ["Mark 2:1"]);
    assert_eq!(all("ruthe chapter one verse sixteen"), ["Ruth 1:16"]);
    assert_eq!(all("mork two verse one"), Vec::<String>::new());
}

#[test]
fn ordinary_sentences_are_not_citations() {
    for t in [
        "he had a great job chapter closing today",
        "first time nineteen ninety nine",
        "the first game nineteen to twenty",
        "mark was born in nineteen and eighty",
        "point number two is prayer",
        "the number one thing you need",
        "that was number three on the list",
        "someone 25 years ago I was in Lagos",
        "I love the psalms",
        "the second time he read a psalm",
        "a verse I learned",
    ] {
        assert_eq!(all(t), Vec::<String>::new(), "{t}");
    }
}

#[test]
fn n_and_m_is_chapter_and_verse_unless_that_chapter_is_being_read() {
    assert_eq!(all("john fifteen and sixteen"), ["John 15:16"]);
    assert_eq!(all("john chapter fifteen and sixteen"), ["John 15:16"]);
    assert_eq!(all_in("john fifteen and sixteen", active("John", 15)), Vec::<String>::new());
}

#[test]
fn growing_three_digit_chapters_wait() {
    assert_eq!(all("Psalm one one"), ["Psalms 1:"]);
    assert_eq!(all("Psalm one one zero verse one to three"), ["Psalms 110:1-3"]);
    assert_eq!(all("john three sixteen"), ["John 3:16"]);
    assert_eq!(all("psalm twenty three one"), ["Psalms 23:1"]);
    assert_eq!(all("Psalm 8:4"), ["Psalms 8:4"]);
    assert_eq!(all("Psalm 1:1."), ["Psalms 1:1"]);
}

#[test]
fn three_word_numbers() {
    assert_eq!(all("Psalm one twenty five five verse one and two."), ["Psalms 125:1-2"]);
    assert_eq!(all("Psalm one twenty verse five."), ["Psalms 120:5"]);
}

#[test]
fn homophones_and_ordinals_need_an_announcement() {
    assert_eq!(all("Psalm 23 for the Lord is my shepherd"), ["Psalms 23:"]);
    assert_eq!(all("Psalm one twenty five for one and two"), ["Psalms 125:"]);
    assert_eq!(all("John three verse for"), ["John 3:4"]);
    assert_eq!(all("John for verse one"), ["John 4:1"]);
    assert_eq!(all("possible in Matthew eleven second to me twenty eight all you"), ["Matthew 11:"]);
    assert_eq!(all("Matthew eleven two"), ["Matthew 11:2"]);
    assert_eq!(all("Genesis one second verse"), ["Genesis 1:2"]);
}

#[test]
fn enumerations_are_not_numbers_the_book() {
    assert_eq!(all("number six verse twenty four"), ["Numbers 6:24"]);
    assert_eq!(all("Numbers 6 24"), ["Numbers 6:24"]);
}

#[test]
fn ordinal_forms() {
    assert_eq!(all("the twenty third psalm"), ["Psalms 23:"]);
    assert_eq!(all("the 23rd psalm"), ["Psalms 23:"]);
    assert_eq!(all("the hundred and nineteenth psalm verse 105"), ["Psalms 119:105"]);
    assert_eq!(all("the second book of Timothy chapter two verse fifteen"), ["2 Timothy 2:15"]);
    assert_eq!(all("the first epistle of John chapter one verse nine"), ["1 John 1:9"]);
    assert_eq!(all("second letter to the Corinthians five seventeen"), ["2 Corinthians 5:17"]);
}

#[test]
fn a_chapter_the_book_lacks_goes_to_the_book_it_sounds_like() {
    assert_eq!(all("Philippians 6 verse 12"), ["Ephesians 6:12"]);
    assert_eq!(all("Philippians chapter 5 verse 18"), ["Ephesians 5:18"]);
    assert_eq!(all("Joel twenty eight verse twenty eight"), ["Job 28:28"]);
    assert_eq!(all("Philippians 4 verse 13"), ["Philippians 4:13"]);
    assert_eq!(all("Ephesians 4 verse 13"), ["Ephesians 4:13"]);
}

#[test]
fn titles_and_stutters() {
    assert_eq!(all("In the book of of Apostle one verse eight, he say"), ["Acts 1:8"]);
    assert_eq!(all("In In Matthew 11 verse 28"), ["Matthew 11:28"]);
    assert_eq!(all("Psalm one one zero verse one"), ["Psalms 110:1"]);
}

#[test]
fn one_chapter_books() {
    let refs = parser().parse_all("But hear this, third John five verse 19 have said", Active::default());
    assert_eq!(refs.iter().map(fmt).collect::<Vec<_>>(), ["1 John 5:19"]);
    assert_eq!(refs[0].alternatives, vec![b("John")]);
    assert_eq!(all("3 John 4"), ["3 John 1:4"]);
    assert_eq!(all("2 John 1:7"), ["2 John 1:7"]);
    assert_eq!(all("3 John 1:2"), ["3 John 1:2"]);
}

#[test]
fn speech_to_text_number_formatting() {
    assert_eq!(all("Psalm 60 six:five-seven."), ["Psalms 66:5-7"]);
    assert_eq!(all("John three:sixteen"), ["John 3:16"]);
    assert_eq!(all("Hebrews eleven one-three"), ["Hebrews 11:1-3"]);
    assert_eq!(all("Matthew twenty-four verse fourteen"), ["Matthew 24:14"]);
    assert_eq!(all("Exodus 20 five"), ["Exodus 20:5"]);
    assert_eq!(all("Romans eight twenty-eight"), ["Romans 8:28"]);
    assert_eq!(all("able Psalm 4 1: 1-3 he will not deliver"), ["Psalms 41:1-3"]);
    assert_eq!(all("the temple of God 1 Corinthians 3:7 3:17 sorry we are"), ["1 Corinthians 3:17"]);
    assert_eq!(all("Romans 8:28, I mean 8:29"), ["Romans 8:29"]);
    assert_eq!(all("Romans chapter VIII verse XXVIII"), ["Romans 8:28"]);
    assert_eq!(all("the verse I love is John 3:16"), ["John 3:16"]);
}

#[test]
fn everyday_word_books_need_a_citation_or_introduction() {
    assert_eq!(book_mentions("So there is a revelation price", Books::get(), Lang::En), vec![]);
    assert_eq!(book_mentions("the book of Revelation says", Books::get(), Lang::En), vec![b("Revelation")]);
    assert_eq!(all("Revelation 3:20"), ["Revelation 3:20"]);
}

#[test]
fn a_restated_chapter_is_kept_as_an_alternative() {
    assert_eq!(all("In Luke chapter six four and verse 14, and Jesus returned"), ["Luke 6:14 or 4"]);
    assert_eq!(all("Psalm 45 5 and verse 6"), ["Psalms 45:6"]);
    assert_eq!(all("Romans chapter eight twenty eight and verse 31"), ["Romans 8:31"]);
    assert_eq!(all("John chapter three three and verse 16"), ["John 3:16"]);
}

#[test]
fn verse_n_against_the_passage_being_read() {
    let kings = active("1 Kings", 19);
    assert_eq!(partial("in verse 15 and God began to tell him what to do", kings, true).as_deref(), Some("1 Kings 19:15"));
    assert_eq!(partial("fifteen", kings, true).as_deref(), Some("1 Kings 19:15"));
    assert_eq!(partial("and fifteen", kings, true).as_deref(), Some("1 Kings 19:15"));
    assert_eq!(partial("he waited twenty years then said", kings, true), None);
    assert_eq!(partial("for twenty years he served faithfully", kings, true), None);

    let genesis = active("Genesis", 24);
    assert_eq!(partial("One", genesis, false), None, "no bare numbers from interim transcripts");
    assert_eq!(partial("One", genesis, true).as_deref(), Some("Genesis 24:1"));
    assert_eq!(partial("verse 15", genesis, false).as_deref(), Some("Genesis 24:15"));

    let psalm = active("Psalms", 30);
    for t in ["for", "too", "won", "ate"] {
        assert_eq!(partial(t, psalm, true), None, "{t}");
    }
    assert_eq!(partial("verse for", psalm, true).as_deref(), Some("Psalms 30:4"));

    let romans5 = active("Romans", 5);
    assert_eq!(partial("First things first", romans5, true), None);
    assert_eq!(partial("second", romans5, true), None);
    assert_eq!(partial("verse first", romans5, true).as_deref(), Some("Romans 5:1"));
    assert_eq!(partial("Until one", romans5, true), None);
    assert_eq!(partial("verse one", romans5, true).as_deref(), Some("Romans 5:1"));
    assert_eq!(partial("the last verse", romans5, true).as_deref(), Some("Romans 5:21"));

    let romans8 = active("Romans", 8);
    assert_eq!(partial("is 04:10", romans8, true), None);
    assert_eq!(partial("can come before he see 01:02", romans8, true), None);
    assert_eq!(partial("verse 10", romans8, true).as_deref(), Some("Romans 8:10"));
    assert_eq!(partial("chapter 3 verse 16", romans8, true).as_deref(), Some("Romans 3:16 (chapter said)"));

    let john1 = active("1 John", 4);
    assert_eq!(partial("John fourteen", john1, true), None);
    assert_eq!(partial("Acts two", john1, true), None);
    assert_eq!(partial("fourteen", john1, true).as_deref(), Some("1 John 4:14"));
    assert_eq!(partial("now nineteen", john1, true).as_deref(), Some("1 John 4:19"));

    let gen13 = active("Genesis", 13);
    assert_eq!(partial("in the name of Jesus dude verse one it is only one chapter", gen13, true), None);
    assert_eq!(partial("like verse 12 says", gen13, true).as_deref(), Some("Genesis 13:12"));
    assert_eq!(partial("the truth verse 4", gen13, true).as_deref(), Some("Genesis 13:4"));
}

#[test]
fn reading_context_lasts_through_a_pause_and_stretches_for_a_slow_pace() {
    let mut ctx = ReadingContext::new();
    ctx.update(b("Romans"), Some(8), 0);
    assert!(ctx.is_valid(20_000));
    assert!(ctx.is_valid(44_999));
    assert!(!ctx.is_valid(46_000));
    ctx.set_pace(1.6);
    assert!(ctx.is_valid(60_000));
    ctx.set_pace(0.5);
    assert!(!ctx.is_valid(60_000), "a fast pace never shortens it below normal");
    ctx.update(b("Acts"), None, 70_000);
    assert_eq!(ctx.active(70_000), Active { book: Some(b("Acts")), chapter: None }, "a new book drops the old chapter");
}

