//! The detection engine: candidates from every path, one decider.
//!
//! ```text
//!  speech (finals and interims)
//!     │
//!     ├─ citations.rs  "Romans 8:28", "Matthew chapter six", "chapter 7 verse 22" (no book)
//!     ├─ triggers.rs   "verse 14", "next verse", "go back to verse 3", corrections
//!     ├─ paths.rs      word-for-word quotes, stream alignment, fingerprints
//!     ├─ meaning.rs    a verse in the preacher's own words (meaning model)
//!     ├─ named.rs      "the prodigal son", "the Lord's Prayer"
//!     ├─ check.rs, correction.rs   a citation the words that follow contradict
//!     └─ ranges.rs     a cited range reading on to its next verse
//!     │
//!     │  each builds a Candidate: verses, method, score, evidence, and — when the path
//!     │  judged its own evidence too thin to send alone — a named Hold (types.rs)
//!     ▼
//!  decide.rs  Engine::decide, in order:
//!      0a a refrain up: the verse found next tells which repeat it was
//!      0  a range reading on goes up as cited          7  a twin of the passage in play is explained
//!      0b a book-less "chapter:verse" names the book  7b part of a verse needs the sermon behind it
//!      1  a range's next verse moves the range         8  look-alike verses: the better-supported one
//!      2  the chapter on screen continued              9  don't replace what's up without reason
//!      3  one score (scoring.rs)                       10 offer a verse once per half minute
//!      4  a held stream match corroborated             11 nothing corrected away comes back
//!      5  a Hold (or the cross-check) demotes          12 no flooding with the same verse
//!      6  fingerprints and scoped meaning only agree   13 show(): the one way out
//!     │
//!     ▼  Effect::Show { target: Screen | Suggestions }
//!  kairo-server detect.rs route(): Screen → on screen with Auto-send on, Possible Matches with it
//!  off, Preview in Director mode; Suggestions → Possible Matches.
//! ```
//!
//! Rules belong in `decide`; a path only reports what it found and, through a `Hold`, why it
//! shouldn't go up alone. The replay trace (`replay --trace a..b`) prints each decision with its
//! score and hold.

mod check;
mod citations;
mod core;
mod correction;
mod decide;
mod detour;
pub mod event;
pub mod evidence;
mod meaning;
mod named;
mod paths;
mod ranges;
pub mod scoring;
pub mod state;
mod triggers;
mod twins;
pub mod types;
pub mod words;

pub use self::core::{Config, Engine};
pub use self::meaning::MeaningThresholds;

pub use event::{Answer, Effect, Event, MeaningHit, Output, Request, Segment, TimerId};
pub use scoring::{Ledger, Score, ScoreContext};
pub use types::{Evidence, Hold, Method, OnScreen, Target};
