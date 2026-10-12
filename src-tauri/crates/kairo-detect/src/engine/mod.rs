//! The detection engine: candidates from every path, one decider.

mod check;
mod citations;
mod core;
mod correction;
mod decide;
pub mod event;
pub mod evidence;
mod meaning;
mod paths;
mod ranges;
pub mod scoring;
pub mod state;
mod triggers;
mod twins;
pub mod types;

pub use self::core::{Config, Engine};

pub use event::{Answer, Effect, Event, MeaningHit, Output, Request, Segment, TimerId};
pub use scoring::{Ledger, Score, ScoreContext};
pub use types::{Evidence, Method, OnScreen, Target};
