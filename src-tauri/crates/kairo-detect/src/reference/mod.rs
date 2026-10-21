//! Spoken Bible references: "John chapter three verse sixteen", "Romans 8
//! 28", "the twenty third psalm", "verse 17" against the passage being read.

pub mod books;
pub mod citation;
pub mod context;
pub mod i18n;
pub mod mentions;
pub mod normalize;
pub mod numbers;
pub mod parse;
pub mod partial;
pub mod translation_command;

pub use books::{AmbiguityRule, Books};
pub use citation::{Citation, Shape, Verses};
pub use context::ReadingContext;
pub use i18n::Lang;
pub use mentions::book_mentions;
pub use parse::{Active, Parser};
pub use partial::{Partial, PartialResolver};
