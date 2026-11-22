//! Two verses called in one breath: send the first, hold the rest (`detection-spec/02-citations.md`).
//!
//! "Jonah chapter two verse eight, and also Second Corinthians two verse fourteen. I want to talk
//! briefly on…" (live, 22 Nov 2026). Both went up at once, the second over the first before either
//! was read, and when 2 Corinthians 2:14 was read a minute later it was dropped as a repeat of what
//! had gone up. Now the first goes up and the others wait, shown in the Live Queue as called
//! (`Effect::Called`), so the operator sees they weren't missed. A waiting verse goes up the usual
//! ways: its words read, the operator sending it, or the preacher calling it again on its own.

use kairo_bible::VerseId;

use super::{Effect, Engine};

/// A citation this soon after the verse up was itself cited, with none of it read yet, was called
/// with it.
const CALLED_TOGETHER_MS: u64 = 8_000;
/// "None of it read": less than this share of the first verse's own words said since it was cited.
const FIRST_READ_SHARE: f32 = 0.25;
/// A verse called and never read stops waiting after this long, and at most this many wait.
const WAITS_MS: u64 = 15 * 60_000;
const MAX_WAITING: usize = 6;

impl Engine {
    /// Was `v`, cited in `text`, called together with the verse up? Not when it is in the same chapter
    /// (a range, or the reading moving on), and not when the preacher takes the first back ("sorry").
    pub(crate) fn called_with_another(&self, v: VerseId, text: &str, now: u64) -> bool {
        let Some((first, at, words_from)) = self.screen.last_citation else { return false };
        if first == v || now.saturating_sub(at) > CALLED_TOGETHER_MS || self.screen.up_verse() != Some(first) {
            return false;
        }
        let (a, b) = (self.r(first), self.r(v));
        if (a.book, a.chapter) == (b.book, b.chapter) || super::triggers::CORRECTION_WORD.is_match(text) {
            return false;
        }
        let since = self.t.words.text(words_from..self.t.words.end());
        self.share_said(first, &since) < FIRST_READ_SHARE
    }

    /// `v` waits for its reading; the app shows it as called. Once per verse while it waits.
    pub(crate) fn wait_called(&mut self, v: VerseId, now: u64) {
        self.called_waiting.retain(|(_, at)| now.saturating_sub(*at) < WAITS_MS);
        if self.called_waiting.iter().any(|(w, _)| *w == v) {
            return;
        }
        if self.called_waiting.len() >= MAX_WAITING {
            self.called_waiting.remove(0);
        }
        self.called_waiting.push((v, now));
        self.note(|e| format!("{} called with {}: waits for its reading", e.label(v), self_label_up(e)));
        self.effect(Effect::Called { verse: v });
    }

    /// Up at last (or sent by the operator): no longer waiting.
    pub(crate) fn called_shown(&mut self, v: VerseId) {
        self.called_waiting.retain(|(w, _)| *w != v);
    }
}

fn self_label_up(e: &Engine) -> String {
    e.screen.up_verse().map_or_else(|| "the verse up".into(), |u| e.label(u))
}
