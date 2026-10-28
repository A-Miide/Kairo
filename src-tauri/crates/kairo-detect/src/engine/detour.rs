//! A verse called out, then a digression: the preacher names a verse ("go from verse twenty one",
//! Ezekiel 12:21 up), says something that matches another ("God rains on the just and the unjust",
//! Matthew 5:45 up on its words), and goes back to the one he called. Once the digression is over
//! (its verse up a while, nothing said matching it lately) the called verse goes back up without
//! waiting for its words (owner, live 2026-10-27). Only then: a verse named by reference is never
//! displaced this way, and the called verse read, another one named, or minutes passing end it.

use kairo_bible::VerseId;

use super::core::{Candidate, Engine, CHAPTER_NAMED_WAITS_MS};
use super::evidence::{meaningful_words, stem_lite};
use super::types::{Method, Target};

/// The digression's verse has been up at least this long, and nothing said has matched it for as long.
const DIGRESSION_OVER_MS: u64 = 12_000;
/// Speech saying at least this share of a verse's meaningful words has read it (or is still on it).
const READ_SHARE: f32 = 0.5;
const STILL_ON_SHARE: f32 = 0.3;

/// A digression starts soon after the call, before the called verse is read (Ezekiel 12:21 called,
/// Matthew 5:45 41 s later); later, the preacher has moved on.
const DIGRESSION_STARTS_WITHIN_MS: u64 = 90_000;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Called {
    pub verse: VerseId,
    pub at: u64,
    /// The verse a word match put up instead, when it went up, and when speech last matched it.
    pub detour: Option<(VerseId, u64, u64)>,
}

/// A verse named by reference (or its number called out), not found in the words.
fn names_its_verse(m: Method) -> bool {
    matches!(m, Method::Direct | Method::ContextCitation | Method::DirectPartial | Method::ChapterKeyword)
}

/// The reference announced a reading ("go to Ezekiel twelve", "turn with me to", "let's read"), not
/// cited in passing: only then is the verse waited for through a digression. On the dev sermons a
/// verse cited while preaching was brought back three times, the preacher never returning to it.
static READING_CUE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)\b(go to|go from|turn to|turn with me|open (your bibles?|to)|let'?s read|let us read|read with me|read it together|we('re| are) reading|start(ing)? (from|at) verse|from verse)\b").unwrap()
});

impl Engine {
    /// Every candidate, before anything is decided: the called verse being read ends the wait; the
    /// digression's verse matched again keeps it going.
    pub(crate) fn detour_seen(&mut self, c: &Candidate, now: u64) {
        let Some(called) = self.called.as_mut() else { return };
        let Some(&top) = c.verses.first() else { return };
        if top == called.verse && c.method.is_text_match() {
            self.called = None;
            return;
        }
        if let Some((v, _, seen)) = called.detour.as_mut() {
            if *v == top {
                *seen = now;
            }
        }
    }

    /// A verse went on screen.
    pub(crate) fn detour_shown(&mut self, top: VerseId, method: Method, returning: bool, now: u64) {
        if returning {
            return;
        }
        if names_its_verse(method) {
            // A chapter named, then its verse, is a reading announced; otherwise the words say so.
            let announced = method == Method::ContextCitation || READING_CUE.is_match(&self.t.words.recent(now, 15_000));
            self.called = announced.then_some(Called { verse: top, at: now, detour: None });
            return;
        }
        let Some(called) = self.called else { return };
        if top == called.verse {
            return;
        }
        // Another chapter, soon after the call: reading on in the same chapter isn't a digression.
        let (t, c) = (self.r(top), self.r(called.verse));
        let elsewhere = (t.book, t.chapter) != (c.book, c.chapter);
        // Read already ("Psalm 67:5… let the people praise thee"), there's nothing to come back to.
        let unread = self.share_said(called.verse, &self.t.words.since(called.at)) < READ_SHARE;
        if method.is_text_match() && elsewhere && unread && now.saturating_sub(called.at) <= DIGRESSION_STARTS_WITHIN_MS {
            self.called = Some(Called { detour: Some((top, now, now)), ..called });
        } else {
            // Moved on: nothing to come back to.
            self.called = None;
        }
    }

    /// The share of `v`'s meaningful words said in `text`.
    pub(crate) fn share_said(&self, v: VerseId, text: &str) -> f32 {
        let said: std::collections::HashSet<String> = meaningful_words(text).iter().map(|w| stem_lite(w)).collect();
        let own: std::collections::HashSet<String> = meaningful_words(self.verse_text(v)).iter().map(|w| stem_lite(w)).collect();
        if own.is_empty() {
            return 0.0;
        }
        own.iter().filter(|w| said.contains(*w)).count() as f32 / own.len() as f32
    }

    /// After each final: the digression over, the called verse goes back up.
    pub(crate) fn maybe_return_to_called(&mut self, now: u64) {
        let Some(called) = self.called else { return };
        if now.saturating_sub(called.at) > CHAPTER_NAMED_WAITS_MS {
            self.called = None;
            return;
        }
        let Some((detour, up_at, mut seen)) = called.detour else { return };
        // Still on it: its words in what was just said (a verse on screen isn't found again).
        let recent = self.t.words.within(now, DIGRESSION_OVER_MS);
        if self.share_said(detour, &recent) >= STILL_ON_SHARE {
            seen = now;
            if let Some(c) = self.called.as_mut() {
                c.detour = Some((detour, up_at, now));
            }
        }
        // The called verse read meanwhile: the reading has it, nothing to put back.
        if self.share_said(called.verse, &self.t.words.since(called.at)) >= READ_SHARE {
            self.called = None;
            return;
        }
        if self.screen.up_verse() != Some(detour) || now.saturating_sub(up_at) < DIGRESSION_OVER_MS || now.saturating_sub(seen) < DIGRESSION_OVER_MS {
            return;
        }
        self.note(|e| format!("the digression on {} is over: back to {}, called before it", e.label(detour), e.label(called.verse)));
        if let Some(c) = self.called.as_mut() {
            c.detour = None;
        }
        let mut back = Candidate::new(vec![called.verse], Method::ContextCitation, 1.0, Target::Screen);
        back.twin_resolved = true;
        back.returning = true;
        self.decide(back, now);
    }
}
