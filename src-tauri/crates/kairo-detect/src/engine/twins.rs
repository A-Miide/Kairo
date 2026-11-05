//! Near-duplicate verses ("twins": verses sharing distinctive wording) and
//! the passage in play (`detection-spec/05-citation-check.md`).

use std::collections::{HashMap, HashSet};

use kairo_bible::{VerseId, VerseRef};

use super::core::{Engine, CONTEXT_WINDOW_MS, NAMED_PERSON_WINDOW_MS};
use super::evidence::{meaningful_words, telling_words};
use super::scoring::SAME_BOOK_WINDOW_MS;

/// One verse's own words heard this many more times than the other's decides between them.
pub const TWIN_SPEECH_MARGIN: usize = 2;
/// Twins sharing this much wording are offered together when nothing separates them.
const TWIN_HOLD_MIN_SHARED: u32 = 3;
const TWIN_DECISION_TTL_MS: u64 = 20_000;
const TWINS_COMPARED: usize = 4;
/// Speech this recent is what a twin decision weighs.
const RECENT_SPEECH_MS: u64 = 6_000;
/// The verses just after the one on screen that the reading may have reached.
const READ_ON_VERSES: u16 = 3;
/// How strongly the sermon's announced passage points at its verses.
const ANNOUNCED_AFFINITY: f32 = 2.5;

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
    /// A refrain put up on its words alone: the verses worded like it, and when it went up. The
    /// words read next tell which one it was.
    refrain: Option<(Vec<VerseId>, u64)>,
}

/// The words after a refrain can still correct which one it was this long after.
const REFRAIN_FOLLOW_MS: u64 = 30_000;

impl Engine {
    /// The verse a text match landed on, or its twin, or both offered.
    pub(crate) fn resolve_twin(&mut self, v: VerseId, now: u64) -> Option<TwinOutcome> {
        let mut twins: Vec<_> = self.k.lexicon.twins(v).to_vec();
        if twins.is_empty() {
            return None;
        }
        twins.sort_by(|a, b| b.shared.cmp(&a.shared));
        twins.truncate(TWINS_COMPARED);
        if let Some((d, at)) = self.twins.decided.get(&v).copied() {
            if now.saturating_sub(at) < TWIN_DECISION_TTL_MS {
                self.note(|e| format!("twin decision for {} reused: {d:?}", e.label(v)));
                return match d {
                    Decided::Switch(t) => Some(TwinOutcome::Switch(t)),
                    Decided::Keep => None,
                };
            }
        }
        // A citation or a range decides between twins.
        if self.cited_recently(v, now) || self.range.all.contains(&v) {
            return None;
        }
        if let Some(t) = twins.iter().find(|t| self.cited_recently(t.verse, now)) {
            self.note(|e| format!("twin {} of {} was cited", e.label(t.verse), e.label(v)));
            return Some(TwinOutcome::Switch(t.verse));
        }
        self.twins.decided.retain(|_, (_, at)| now.saturating_sub(*at) <= TWIN_DECISION_TTL_MS);

        let speech = telling_words(&self.recent_speech_seconds(30).join(" "));
        if speech.is_empty() {
            return None;
        }
        let words = |e: &Engine, x: VerseId| telling_words(&e.match_text(x));
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
            } else if cand_aff == 0.0 && cand_only < TWIN_SPEECH_MARGIN && { let r = self.r(t.verse); self.in_announced_passage(r.book, r.chapter) } {
                // A word apart ("mysteries", "mystery") doesn't outweigh the passage the sermon is
                // on: Mark 4:11, not Matthew 13:11 (MacArthur on Mark 4, dev build 2026-10-20).
                if !is_switch(&best) {
                    best = Some((TwinOutcome::Switch(t.verse), 0));
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
        self.note(|e| format!("twin check {}: speech {:?}, candidate affinity {cand_aff}, best {:?}", e.label(v), speech, best.map(|(o, h)| (o, h))));
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

    /// The verses worded like `v` that nothing said lately tells apart from it: "O give thanks unto
    /// the LORD; for he is good: for his mercy endureth for ever" is Psalm 107:1, 118:1, 118:29,
    /// 136:1 and 1 Chronicles 16:34. A paraphrase offer of one offers them together, so the
    /// operator picks; offered one at a time, 136:1 came 48 s after 107:1 (owner, live 2026-10-27).
    pub(crate) fn worded_alike(&self, v: VerseId) -> Vec<VerseId> {
        let speech = telling_words(&self.recent_speech_seconds(30).join(" "));
        let words = |e: &Engine, x: VerseId| telling_words(&e.match_text(x));
        let cand = words(self, v);
        let mut twins: Vec<_> = self.k.lexicon.twins(v).iter().filter(|t| t.shared >= TWIN_HOLD_MIN_SHARED).collect();
        twins.sort_by(|a, b| b.shared.cmp(&a.shared));
        twins
            .into_iter()
            .filter(|t| {
                let tw = words(self, t.verse);
                let cand_only = cand.iter().filter(|w| !tw.contains(*w) && speech.contains(*w)).count();
                let twin_only = tw.iter().filter(|w| !cand.contains(*w) && speech.contains(*w)).count();
                cand_only == 0 && twin_only == 0
            })
            .map(|t| t.verse)
            .take(TWINS_COMPARED)
            .collect()
    }

    fn cited_recently(&self, v: VerseId, now: u64) -> bool {
        let r = self.r(v);
        self.sermon.iter().any(|c| c.named && c.book == r.book && c.chapter == Some(r.chapter) && c.verse == Some(r.verse) && now.saturating_sub(c.at) < CONTEXT_WINDOW_MS)
    }

    /// A refrain: the verses of `v`'s chapter worded exactly as it is (Psalm 67:3 and 67:5, Psalm
    /// 107:8, 15, 21 and 31), while that chapter is in play. Reading it word for word, the one the
    /// reading has reached goes up — the first after the verse of the chapter on screen, else the
    /// chapter's first — and the others are offered (owner, 2026-10-26). The same words in another
    /// chapter are another passage, and stay a choice for the operator.
    pub(crate) fn refrain_reached(&self, v: VerseId, now: u64) -> Option<(VerseId, Vec<VerseId>)> {
        let r = self.r(v);
        let up = self.screen.up.map(|(u, at)| (self.r(u), at)).filter(|(u, _)| u.book == r.book && u.chapter == r.chapter);
        // In play: named a moment ago and waiting for its verse (the live citation context), being
        // read, pointed at by the sermon, or on screen.
        let waiting = self.chapter_pending.as_ref().is_some_and(|p| p.book == r.book && p.chapter == r.chapter);
        let reading = { let a = self.active(now); a.book == Some(r.book) && a.chapter == Some(r.chapter as u32) };
        if !waiting && !reading && !self.passage_pointed_at(v, now) && !up.is_some_and(|(_, at)| now.saturating_sub(at) < SAME_BOOK_WINDOW_MS) {
            return None;
        }
        let words = |e: &Engine, x: VerseId| super::citations::tokens(e.verse_text(x));
        let said = words(self, v);
        let group: Vec<VerseId> = (1..=self.k.corpus.verse_count(r.book, r.chapter).unwrap_or(0))
            .filter_map(|n| self.k.id(VerseRef::new(r.book, r.chapter, n)))
            .filter(|x| words(self, *x) == said)
            .collect();
        if group.len() < 2 {
            return None;
        }
        // The next one only once the refrain has been said again since the one up went up: the same
        // reading matched twice (its last words in a final of their own) is still the one up.
        let refrain = meaningful_words(self.verse_text(v));
        let said_again = |at: u64| {
            let since: HashSet<String> = meaningful_words(&self.t.words.since(at)).into_iter().collect();
            refrain.iter().filter(|w| since.contains(*w)).count() * 5 >= refrain.len() * 4
        };
        let reached = match up {
            Some((u, at)) if group.iter().any(|x| self.r(*x).verse == u.verse) && !said_again(at + 1) => group.iter().copied().find(|x| self.r(*x).verse == u.verse),
            Some((u, _)) => group.iter().copied().find(|x| self.r(*x).verse > u.verse),
            None => None,
        }
        .unwrap_or(group[0]);
        Some((reached, group.into_iter().filter(|x| *x != reached).collect()))
    }

    /// A refrain went up as `up`; the words read next may show it was another of `group`.
    pub(crate) fn refrain_up(&mut self, up: VerseId, group: Vec<VerseId>, now: u64) {
        self.set_soft_range(up, now);   // the next verses held in case they're read on
        self.twins.refrain = Some((std::iter::once(up).chain(group).collect(), now));
    }

    /// A path proposes `v` while a refrain is up: if `v` follows another of the refrain's repeats
    /// (67:6 found while 67:3 is up — "…let all the people praise thee. Then shall the earth yield
    /// her increase"), the refrain read was that one, and it goes up in place of the one guessed;
    /// its next verses are then held as for any verse up (owner, 2026-10-26). Whichever path heard
    /// the next verse — the stream, a quote, the range's own words — makes the correction.
    pub(crate) fn refrain_followed_by(&mut self, v: VerseId, now: u64) {
        let Some((group, at)) = self.twins.refrain.clone() else { return };
        let up = self.screen.up_verse().filter(|u| group.contains(u));
        let Some(up) = up.filter(|_| now.saturating_sub(at) <= REFRAIN_FOLLOW_MS) else {
            self.twins.refrain = None;
            return;
        };
        let r = self.r(v);
        let Some(before) = r.verse.checked_sub(1).and_then(|n| self.k.id(VerseRef::new(r.book, r.chapter, n))) else { return };
        if !group.contains(&before) || v == up {
            return;
        }
        self.twins.refrain = None;
        if before == up {
            return; // read on from the one up: it was right
        }
        self.note(|e| format!("{} follows {}: the refrain read was {}, not {}", e.label(v), e.label(before), e.label(before), e.label(up)));
        let mut c = super::core::Candidate::new(vec![before], super::types::Method::Verbatim, 1.0, super::types::Target::Screen);
        c.corrected_from = Some(up);
        c.twin_resolved = true;
        if self.decide(c, now) == super::types::Target::Screen {
            self.set_soft_range(before, now);   // its next verses held in case they're read on
        }
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
        // The sermon's announced passage: Mark 4:11 over its twin Matthew 13:11 in a sermon on
        // Mark 4 (MacArthur, dev build).
        if self.in_announced_passage(r.book, r.chapter) {
            a = a.max(ANNOUNCED_AFFINITY);
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
            // …or a few verses on: Colossians 3:4 up, 3:5 read in passing, then 3:6 is "the sons of
            // disobedience", not its twin Ephesians 5:6 (Charles Stanley, dev build 2026-10-21).
            if u.book == r.book && u.chapter == r.chapter && now.saturating_sub(at) < SAME_BOOK_WINDOW_MS && r.verse >= u.verse && r.verse <= u.verse + READ_ON_VERSES {
                return true;
            }
        }
        self.in_announced_passage(r.book, r.chapter) || self.sermon.iter().any(|c| c.named && c.book == r.book && c.chapter == Some(r.chapter) && now.saturating_sub(c.at) < CONTEXT_WINDOW_MS)
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
        let words = meaningful_words(&self.t.words.recent(self.last_now, RECENT_SPEECH_MS));
        words[words.len().saturating_sub(max)..].to_vec()
    }
}
