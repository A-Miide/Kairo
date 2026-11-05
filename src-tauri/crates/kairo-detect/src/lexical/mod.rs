//! Word matching: a verse read word for word (verbatim), followed as it is
//! read (stream), or loosely paraphrased with its identifying words
//! (fingerprint) — all over one shared `Lexicon`.

pub mod fingerprint;
pub mod lexicon;
pub mod stem;
pub mod stream;
pub mod text;
pub mod verbatim;
pub mod window;

pub use fingerprint::{fingerprint, topic_library, Confidence, FingerprintResult, RecentCitation};
pub use lexicon::{Lexicon, Twin, Wordings};
pub use stream::{AnchorHit, Confirmed, StreamMatcher, StreamTick};
pub use verbatim::{read_share, verbatim, Tier, VerbatimHit};
