//! What detectors produce and the decider acts on.

use kairo_bible::VerseRef;

use crate::lexical::Confidence;

/// How a candidate verse was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    /// A full citation spoken: book, chapter, verse.
    Direct,
    /// "chapter N verse M", the book taken from what's being read.
    ContextCitation,
    /// "next verse", the next number, a verse number called after finishing one.
    Continuation,
    /// "verse N" resolved against the passage being read.
    DirectPartial,
    /// A chapter named with no verse; the verse found from what followed.
    ChapterKeyword,
    /// Read (nearly) word for word.
    Verbatim,
    /// Followed word by word as it was read.
    Stream,
    /// Its identifying words were said.
    Fingerprint,
    /// Close in meaning (scoped search).
    Semantic,
    /// The preacher's own words for it (meaning + cross-encoder + shared wording).
    Paraphrase,
    /// A passage named by its title ("the Lord's Prayer").
    NamedPassage,
}

impl Method {
    /// Names its own passage — a spoken citation or trigger, not a text match.
    pub fn is_explicit_book_citation(self) -> bool {
        matches!(self, Method::Direct | Method::ChapterKeyword | Method::Continuation | Method::ContextCitation)
    }

    /// Evidence from matching the Bible's text, as opposed to anything spoken as a reference.
    pub fn is_text_match(self) -> bool {
        matches!(self, Method::Stream | Method::Verbatim | Method::Fingerprint | Method::Semantic | Method::Paraphrase)
    }

    /// Resolves its own verse, so near-duplicate twins don't apply.
    pub fn names_its_verse(self) -> bool {
        matches!(
            self,
            Method::Direct | Method::DirectPartial | Method::ChapterKeyword | Method::Continuation | Method::ContextCitation | Method::NamedPassage
        )
    }

    /// The wire name the UI already knows.
    pub fn wire(self) -> &'static str {
        match self {
            Method::Direct => "direct",
            Method::ContextCitation => "context-citation",
            Method::Continuation => "continuation",
            Method::DirectPartial => "direct-partial",
            Method::ChapterKeyword => "chapter-keyword",
            Method::Verbatim => "verbatim",
            Method::Stream => "stream",
            Method::Fingerprint => "fingerprint",
            Method::Semantic => "semantic",
            Method::Paraphrase => "paraphrase",
            Method::NamedPassage => "named-passage",
        }
    }
}

/// Where a decision puts a verse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Screen,
    Suggestions,
    Drop,
}

/// The raw evidence a method found, in its own terms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Evidence {
    /// The method's own 0–1 score (verbatim coverage, fingerprint coverage, cosine).
    pub similarity: Option<f32>,
    /// Identifying weight of a matched run (verbatim, confirmed stream), or of the
    /// verse's wording a paraphrase window shares.
    pub matched_idf: Option<f32>,
    pub confidence: Option<Confidence>,
    /// A stream alignment that reached confirmation.
    pub confirmed: bool,
    /// Words aligned (confirmed stream).
    pub matched: Option<u32>,
    /// Verses a stream anchor's phrase occurs in, and its weight (unconfirmed anchors).
    pub df: Option<u8>,
    pub idf: Option<f32>,
    /// The paraphrase decision's own score.
    pub paraphrase_score: Option<f32>,
    /// The cross-encoder's score, when one was computed.
    pub rerank: Option<f32>,
}

/// The verse on screen, and when it went up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnScreen {
    pub verse: VerseRef,
    pub at_ms: u64,
}
