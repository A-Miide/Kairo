//! Regex helpers that keep JavaScript's semantics, which the detection rules
//! were tuned against: `\b`, `\w` and `\d` are ASCII-only there.
//!
//! The `regex` crate takes `(?-u:\b)` directly. `fancy-regex` (needed only for
//! lookaround and backreferences) can't switch Unicode off, so those patterns
//! spell the ASCII word boundary out as `WB`.

use std::borrow::Cow;

/// JavaScript's `\b`: a change between `[A-Za-z0-9_]` and anything else.
pub(crate) const WB: &str = r"(?:(?<=[A-Za-z0-9_])(?![A-Za-z0-9_])|(?<![A-Za-z0-9_])(?=[A-Za-z0-9_]))";

pub(crate) fn re(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).unwrap_or_else(|e| panic!("bad pattern {pattern}: {e}"))
}

pub(crate) fn fre(pattern: &str) -> fancy_regex::Regex {
    fancy_regex::Regex::new(pattern).unwrap_or_else(|e| panic!("bad pattern {pattern}: {e}"))
}

/// `fancy-regex` replacement that can't panic on live input: if backtracking
/// hits its limit the text is left as it was.
pub(crate) fn freplace<'t>(
    re: &fancy_regex::Regex,
    text: &'t str,
    rep: impl FnMut(&fancy_regex::Captures<'_, str>) -> String,
) -> Cow<'t, str> {
    re.try_replacen(text, 0, rep).unwrap_or(Cow::Borrowed(text))
}

/// `s.split(/\s+/)` on text the cleaner has already single-spaced and trimmed.
pub(crate) fn words(s: &str) -> Vec<&str> {
    s.split(' ').collect()
}
