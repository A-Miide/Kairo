//! The detection engine: candidates from every path, one decider.

pub mod scoring;
pub mod types;

pub use scoring::{Ledger, Score, ScoreContext};
pub use types::{Evidence, Method, OnScreen, Target};
