//! Near-duplicate verses ("twins": verses sharing distinctive wording) and
//! the passage in play (`detection-spec/05-citation-check.md`).

use std::collections::{HashMap, HashSet};

use kairo_bible::VerseId;

use super::core::{Engine, CONTEXT_WINDOW_MS, NAMED_PERSON_WINDOW_MS};
use super::evidence::{meaningful_words, stem_lite};
use super::scoring::SAME_BOOK_WINDOW_MS;

/// One verse's own words heard this many more times than the other's decides between them.
pub const TWIN_SPEECH_MARGIN: usize = 2;
/// Twins sharing this much wording are offered together when nothing separates them.
const TWIN_HOLD_MIN_SHARED: u32 = 3;
const TWIN_DECISION_TTL_MS: u64 = 20_000;
const TWINS_COMPARED: usize = 4;
/// Speech this recent is what a twin decision weighs.
const RECENT_SPEECH_MS: u64 = 6_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TwinOutcome {
    /// The twin is the verse being said.
    Switch(VerseId),
    /// Nothing separates them: offer both.
    Hold(VerseId),
}

#[derive(Clone, Copy, Debug)]
enum Decided {
    Switch(VerseId),
    /// The speech holds the candidate's own words.
    Keep,
}

#[derive(Default)]
pub(crate) struct TwinState {
    decided: HashMap<VerseId, (Decided, u64)>,
}

impl Engine {
    /// The verse a text match landed on, or its twin, or both offered.
    pub(crate) fn resolve_twin(&mut self, v: VerseId, now: u64) -> Option<TwinOutcome> {
        let mut twins: Vec<_> = self.k.lexicon.twins(v).to_vec();
        if twins.is_empty() {
            return None;
        }
        twins.sort_by(|a, b| b.shared.cmp(&a.shared));
        twins.truncate(TWINS_COMPARED);
        if let Some((d, at)) = self.twins.decided.get(&v) {
            if now.saturating_sub(*at) < TWIN_DECISION_TTL_MS {
                return match d {
                    Decided::Switch(t) => Some(TwinOutcome::Switch(*t)),
                    Decided::Keep => None,
                };
            }
        }
        // A citation or a range decides between twins.
        if self.cited_recently(v, now) || self.range.all.contains(&v) {
            return None;
        }
        if let Some(t) = twins.iter().find(|t| self.cited_recently(t.verse, now)) {
            return Some(TwinOutcome::Switch(t.verse));
        }
        self.twins.decided.retain(|_, (_, at)| now.saturating_sub(*at) <= TWIN_DECISION_TTL_MS);

        let speech: HashSet<String> = self.recent_speech_seconds(30).iter().map(|w| stem_lite(w)).collect();
        if speech.is_empty() {
            return None;
        }
        let words = |e: &Engine, x: VerseId| -> HashSet<String> { meaningful_words(&e.match_text(x)).iter().map(|w| stem_lite(w)).collect() };
        let cand = words(self, v);
        let cand_aff = self.passage_affinity(v, now);
        let mut best: Option<(TwinOutcome, usize)> = None;
        let mut speech_keeps = false;
        for t in &twins {
            let tw = words(self, t.verse);
            let cand_only = cand.iter().filter(|w| !tw.contains(*w) && speech.contains(*w)).count();
            let twin_only = tw.iter().filter(|w| !cand.contains(*w) && speech.contains(*w)).count();
            if cand_only >= twin_only + TWIN_SPEECH_MARGIN {
                speech_keeps = true;
            }
            let aff = self.passage_affinity(t.verse, now);
            let is_switch = |b: &Option<(TwinOutcome, usize)>| matches!(b, Some((TwinOutcome::Switch(_), _)));
            if twin_only >= cand_only + TWIN_SPEECH_MARGIN {
                if !is_switch(&best) || best.is_some_and(|(_, hits)| twin_only > hits) {
                    best = Some((TwinOutcome::Switch(t.verse), twin_only));
                }
            } else if cand_only == 0 && twin_only == 0 {
                if aff >= 2.0 && aff > cand_aff && self.passage_pointed_at(t.verse, now) {
                    if !is_switch(&best) {
                        best = Some((TwinOutcome::Switch(t.verse), 0));
                    }
                } else if best.is_none() && ((aff >= 2.0 && aff > cand_aff) || (aff == cand_aff && t.shared >= TWIN_HOLD_MIN_SHARED)) {
                    best = Some((TwinOutcome::Hold(t.verse), 0));
                }
            }
        }
        match best {
            Some((TwinOutcome::Switch(t), _)) => {
                self.twins.decided.insert(v, (Decided::Switch(t), now));
                Some(TwinOutcome::Switch(t))
            }
            Some((hold, _)) => Some(hold),
            None => {
                if speech_keeps {
                    self.twins.decided.insert(v, (Decided::Keep, now));
                }
                None
            }
        }
    }

    fn cited_recently(&self, v: VerseId, now: u64) -> bool {
        let r = self.r(v);
        self.sermon.iter().any(|c| c.named && c.book == r.book && c.chapter == Some(r.chapter) && c.verse == Some(r.verse) && now.saturating_sub(c.at) < CONTEXT_WINDOW_MS)
    }

    /// How strongly the sermon points at this verse's passage (0–3).
    pub(crate) fn passage_affinity(&self, v: VerseId, now: u64) -> f32 {
        let r = self.r(v);
        let mut a: f32 = 0.0;
        if self.range.all.iter().any(|x| {
            let x = self.r(*x);
            x.book == r.book && x.chapter == r.chapter
        }) {
            a = a.max(3.0);
        }
        if let Some((up, at)) = self.screen.up {
            let u = self.r(up);
            if u.book == r.book && u.chapter == r.chapter && now.saturating_sub(at) < SAME_BOOK_WINDOW_MS {
                a = a.max(2.0);
                if r.verse == u.verse + 1 {
                    a = a.max(2.5);
                }
            }
        }
        let active = self.active(now);
        if active.book == Some(r.book) && active.chapter == Some(r.chapter as u32) {
            a = a.max(2.0);
        }
        if self.reading_mode.chapter().is_some_and(|(b, c)| b == r.book && c == r.chapter) {
            a = a.max(2.0);
        }
        for c in self.recently_cited(now) {
            if c.book == r.book {
                a = a.max(if c.chapter == Some(r.chapter) { 2.0 } else { 1.0 });
            }
        }
        if self.person.as_ref().is_some_and(|(name, at)| {
            now.saturating_sub(*at) < NAMED_PERSON_WINDOW_MS && self.k.name_chapters(name).is_some_and(|c| c.binary_search(&(r.book, r.chapter)).is_ok())
        }) {
            a = a.max(1.0);
        }
        a
    }

    /// Something points at this very verse: the range holds it, it's (the
    /// verse after) the verse on screen, or its chapter was cited.
    pub(crate) fn passage_pointed_at(&self, v: VerseId, now: u64) -> bool {
        if self.range.all.contains(&v) {
            return true;
        }
        let r = self.r(v);
        if let Some((up, at)) = self.screen.up {
            let u = self.r(up);
            if u.book == r.book && u.chapter == r.chapter && now.saturating_sub(at) < SAME_BOOK_WINDOW_MS && (r.verse == u.verse || r.verse == u.verse + 1) {
                return true;
            }
        }
        self.sermon.iter().any(|c| c.named && c.book == r.book && c.chapter == Some(r.chapter) && now.saturating_sub(c.at) < CONTEXT_WINDOW_MS)
    }

    /// A text match explained by the passage in play: its twin (or a chapter
    /// confusable with its own) is on screen, in the range, or cited.
    pub(crate) fn twin_in_play(&self, v: VerseId, now: u64) -> bool {
        let r = self.r(v);
        let mut in_play: Vec<(kairo_bible::Book, Option<u16>, Option<VerseId>)> = Vec::new();
        if let Some((up, at)) = self.screen.up {
            if now.saturating_sub(at) < SAME_BOOK_WINDOW_MS {
                let u = self.r(up);
                in_play.push((u.book, Some(u.chapter), Some(up)));
            }
        }
        for x in &self.range.all {
            let u = self.r(*x);
            in_play.push((u.book, Some(u.chapter), Some(*x)));
        }
        for c in self.sermon.iter().filter(|c| c.named && now.saturating_sub(c.at) < CONTEXT_WINDOW_MS) {
            let id = c.chapter.zip(c.verse).and_then(|(ch, vs)| self.k.id(kairo_bible::VerseRef::new(c.book, ch, vs)));
            in_play.push((c.book, c.chapter, id));
        }
        if in_play.iter().any(|(_, _, id)| *id == Some(v)) {
            return false; // it is the passage in play
        }
        let twins: HashSet<VerseId> = self.k.lexicon.twins(v).iter().map(|t| t.verse).collect();
        in_play.iter().any(|(book, chapter, id)| {
            id.is_some_and(|id| twins.contains(&id)) || (*book != r.book && chapter.is_some_and(|ch| self.k.confusable((*book, ch), (r.book, r.chapter))))
        })
    }

    /// Meaningful words of speech in the last few seconds (or the last final).
    pub(crate) fn recent_speech_seconds(&self, max: usize) -> Vec<String> {
        let now = self.t.finals.back().map_or(0, |f| f.at).max(self.last_now);
        let mut recent: Vec<&str> = self.t.finals.iter().filter(|f| now.saturating_sub(f.at) < RECENT_SPEECH_MS).map(|f| f.text.as_str()).collect();
        if recent.is_empty() {
            if let Some(f) = self.t.finals.back() {
                recent.push(&f.text);
            }
        }
        let words = meaningful_words(&format!("{} {}", recent.join(" "), self.t.interim));
        words[words.len().saturating_sub(max)..].to_vec()
    }
}
