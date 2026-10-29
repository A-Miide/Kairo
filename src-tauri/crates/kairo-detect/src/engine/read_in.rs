//! The translation the preacher is reading from. Every wording is matched (KJV, NLT, NIV…), but the
//! screen stayed in the church's while Daniel 9:5–6 was read word for word in the NLT (live,
//! 2026-10-28). Two verses of a passage read clearly in one wording say which it is; the server
//! then shows the passage in it, as if it had been asked for ("give me the NLT"), and the church's
//! own comes back at the next passage (kairo-server spoken.rs).

use kairo_bible::{Book, VerseId};

use super::core::Engine;
use super::event::Effect;
use super::evidence::plain_tokens;
use super::ranges::read_run;

/// What was said this recently is the reading of the verse up.
const READ_IN_SPEECH_MS: u64 = 20_000;
/// A wording is the one read with a stretch this long read word for word, this far ahead of every other.
const READ_IN_MIN_RUN: usize = 6;
const READ_IN_MIN_LEAD: usize = 3;

#[derive(Default)]
pub struct ReadIn {
    /// The last verse whose wording was told apart, and that wording.
    last: Option<(VerseId, String)>,
    /// The passage the server was told about, and in what.
    told: Option<(Book, u16, String)>,
}

impl Engine {
    /// On each final: the verse up, read clearly in one wording, and the verse before it read in the
    /// same one in the same chapter, tell the server which translation the passage is being read in.
    pub(crate) fn note_read_in(&mut self, now: u64) {
        let Some((v, _)) = self.screen.up else { return };
        if self.read_in.last.as_ref().is_some_and(|(l, _)| *l == v) {
            return;
        }
        let said = plain_tokens(&self.t.words.recent(now, READ_IN_SPEECH_MS));
        let mut runs: Vec<(usize, &str)> = std::iter::once(self.k.corpus.kjv())
            .map(|t| (t.text(v), t.code.as_str()))
            .chain(self.k.lexicon.others().iter().map(|o| (o.text(v), o.code.as_str())))
            .filter_map(|(text, code)| Some((read_run(&said, &plain_tokens(text.filter(|t| !t.is_empty())?)).0, code)))
            .collect();
        runs.sort_by(|a, b| b.0.cmp(&a.0));
        let Some(&(best, code)) = runs.first() else { return };
        let second = runs.get(1).map_or(0, |r| r.0);
        if best < READ_IN_MIN_RUN || best < second + READ_IN_MIN_LEAD {
            return;
        }
        let code = code.to_string();
        let r = self.r(v);
        let before = self.read_in.last.replace((v, code.clone()));
        let same_passage = before.is_some_and(|(p, c)| c == code && self.r(p).book == r.book && self.r(p).chapter == r.chapter);
        if !same_passage || self.read_in.told.as_ref().is_some_and(|(b, ch, c)| *b == r.book && *ch == r.chapter && *c == code) {
            return;
        }
        self.read_in.told = Some((r.book, r.chapter, code.clone()));
        self.note(|e| format!("{} read in the {code} ({best} words in a row, {second} in the next wording)", e.label(v)));
        self.effect(Effect::ReadIn { verse: v, translation: code });
    }
}
