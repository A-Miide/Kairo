//! Every word heard, in order, each with when it was first heard and whether
//! the recogniser has settled it — so detectors read speech as it was said,
//! not as the recogniser happened to cut it into segments
//! (`detection-spec/08-word-buffer.md`).

use std::collections::VecDeque;
use std::ops::Range;

/// How long heard words are kept.
const KEEP_MS: u64 = 90_000;
/// …and at most this many.
const KEEP_WORDS: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Locked in by a final.
    Settled,
    /// In the interim still growing: the recogniser may revise it.
    Live,
    /// Heard in an interim that the final after it never repeated: a lagging
    /// recogniser cut there ("Psalm sixty seven it" | "says let the people…").
    /// Still what was said.
    Unconfirmed,
}

#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    /// When it was first heard (its first interim, else its final).
    pub at: u64,
    pub status: Status,
}

/// Positions are absolute: word `n` of the service keeps its number while
/// older words are dropped, so a detector can remember where something was said.
#[derive(Clone, Debug, Default)]
pub struct WordBuffer {
    words: VecDeque<Word>,
    /// The position of `words[0]`.
    base: u64,
    /// Where this segment's words begin: the live words, or what the final settled
    /// (after any interim words it left unconfirmed).
    changed_from: u64,
}

/// Lowercase letters and digits: how two hearings of a word are compared.
fn norm(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The same word, maybe misheard once: "for"/"four", "got"/"God".
fn alike(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()] <= (a.len().max(b.len()) / 3).max(1)
}

/// Each run of number words or digits as one "#": "sixty seven" and "67" alike.
fn numbers_as_one(words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words {
        let number = (!w.is_empty() && w.chars().all(|c| c.is_ascii_digit())) || crate::reference::numbers::spoken_to_number(w).is_some();
        if !number {
            out.push(w.clone());
        } else if out.last().is_none_or(|l| l != "#") {
            out.push("#".to_string());
        }
    }
    out
}

impl WordBuffer {
    /// One past the newest word.
    pub fn end(&self) -> u64 {
        self.base + self.words.len() as u64
    }

    pub fn start(&self) -> u64 {
        self.base
    }

    /// Where the live (interim) words begin; `end()` when there are none.
    pub fn live_from(&self) -> u64 {
        let live = self.words.iter().rev().take_while(|w| w.status == Status::Live).count();
        self.end() - live as u64
    }

    /// The words the latest segment brought (its live words, or what its final settled).
    pub fn changed(&self) -> Range<u64> {
        self.changed_from.max(self.base)..self.end()
    }

    pub fn word(&self, at: u64) -> Option<&Word> {
        at.checked_sub(self.base).and_then(|i| self.words.get(i as usize))
    }

    /// An interim: it replaces the live words it re-hears, which keep the time they were first
    /// heard. Live words before where it begins are left `Unconfirmed` (a recogniser that cut and
    /// started again).
    pub fn interim(&mut self, text: &str, now: u64) {
        let said: Vec<&str> = text.split_whitespace().collect();
        let at = self.rehears(&said);
        self.unconfirm_to(at);
        self.replace_from(at, text, Status::Live, now);
        self.changed_from = at;
        self.trim(now);
    }

    /// Where the live words `said` re-hears begin: at the start of the live words (the usual),
    /// further in (the first ones were dropped), or — re-hearing none — after them all.
    fn rehears(&self, said: &[&str]) -> u64 {
        let from = self.live_from();
        let live: Vec<String> = self.words.iter().skip((from - self.base) as usize).map(|w| norm(&w.text)).collect();
        let first = said.first().map(|w| norm(w)).unwrap_or_default();
        if live.is_empty() || first.is_empty() || alike(&live[0], &first) {
            return from;
        }
        // Further in only when it re-hears the live words from there to the end: a verse repeats
        // itself ("let the people praise thee … let all the | people praise thee"), and new speech
        // that happens to repeat a word isn't a re-hearing.
        let heard: Vec<String> = said.iter().map(|w| norm(w)).collect();
        if let Some(k) = (1..live.len()).find(|&k| live.len() - k <= heard.len() && live[k..].iter().zip(&heard).all(|(a, b)| alike(a, b))) {
            return from + k as u64;
        }
        // Not where its first word is, but the same words reworded ("some sixty seven verse
        // three" → "Psalm 67 verse 3"): re-heard if most of the live words are in it.
        let (live, heard) = (numbers_as_one(&live), numbers_as_one(&said.iter().map(|w| norm(w)).collect::<Vec<_>>()));
        let shared = live.iter().filter(|w| heard.iter().any(|h| alike(h, w))).count();
        if shared * 2 >= live.len() { from } else { self.end() }
    }

    fn unconfirm_to(&mut self, at: u64) {
        let from = self.live_from();
        for w in self.words.iter_mut().skip((from - self.base) as usize).take((at - from) as usize) {
            w.status = Status::Unconfirmed;
        }
    }

    /// A final. It settles the live words it re-hears: all of them, or with `continues_live`
    /// (an engine locking a few words at a time) the ones it covers, the rest staying live.
    /// Live words it doesn't begin with and doesn't re-hear are kept as `Unconfirmed`.
    pub fn settle(&mut self, text: &str, now: u64, continues_live: bool) {
        let said: Vec<&str> = text.split_whitespace().collect();
        let at = self.rehears(&said);
        self.unconfirm_to(at);
        let kept_live: Vec<Word> = if continues_live {
            self.words.iter().skip((at - self.base) as usize + said.len()).cloned().collect()
        } else {
            Vec::new()
        };
        // Without `continues_live` the final is the whole utterance: live words past its end were
        // the same speech differently cut ("sixty seven verse three" → "67:3") and go.
        self.replace_from(at, text, Status::Settled, now);
        self.words.extend(kept_live);
        self.changed_from = at;
        self.trim(now);
    }

    /// Words from `at` on become `text`'s, keeping the times of the words they replace.
    fn replace_from(&mut self, at: u64, text: &str, status: Status, now: u64) {
        let i = (at - self.base) as usize;
        let old: Vec<u64> = self.words.drain(i..).map(|w| w.at).collect();
        for (n, w) in text.split_whitespace().enumerate() {
            self.words.push_back(Word { text: w.to_string(), at: old.get(n).copied().unwrap_or(now), status });
        }
    }

    fn trim(&mut self, now: u64) {
        while self.words.len() > KEEP_WORDS || self.words.front().is_some_and(|w| w.at + KEEP_MS < now && w.status != Status::Live) {
            self.words.pop_front();
            self.base += 1;
        }
    }

    /// The utterance in progress was abandoned (the recogniser reconnected): its live words go.
    pub fn drop_live(&mut self) {
        let from = self.live_from();
        self.words.truncate((from - self.base) as usize);
        self.changed_from = self.end();
    }

    /// Where what's being said now begins: the live words, else the latest segment's.
    pub fn newest_from(&self) -> u64 {
        let live = self.live_from();
        if live < self.end() { live } else { self.changed().start }
    }

    /// What's being said now: the live words, else the latest segment's.
    pub fn newest(&self) -> String {
        self.text(self.newest_from()..self.end())
    }

    /// What was said in the last `ms` before `now`, and all of what's being said now however
    /// long it has been going on.
    pub fn recent(&self, now: u64, ms: u64) -> String {
        self.text(self.heard_since(now.saturating_sub(ms)).min(self.newest_from())..self.end())
    }

    /// The last `n` words heard.
    pub fn last(&self, n: usize) -> String {
        self.text(self.end().saturating_sub(n as u64)..self.end())
    }

    /// The words in `range` (clamped to what's kept), as said.
    pub fn text(&self, range: Range<u64>) -> String {
        let (a, b) = (range.start.max(self.base), range.end.min(self.end()));
        if a >= b {
            return String::new();
        }
        self.words.range((a - self.base) as usize..(b - self.base) as usize).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// The first position heard at or after `t`.
    pub fn heard_since(&self, t: u64) -> u64 {
        self.base + self.words.iter().position(|w| w.at >= t).unwrap_or(self.words.len()) as u64
    }

    /// Everything heard since `t`, settled or not, as said.
    pub fn since(&self, t: u64) -> String {
        self.text(self.heard_since(t)..self.end())
    }

    /// At most the last `n` words before `before` that run on to `now` without a pause of `ms`:
    /// the stretch of speech this one continues.
    pub fn before(&self, before: u64, n: usize, now: u64, ms: u64) -> Range<u64> {
        let end = before.clamp(self.base, self.end());
        let (mut start, mut newer) = (end, now);
        while start > self.base && end - start < n as u64 {
            let w = &self.words[(start - 1 - self.base) as usize];
            if newer.saturating_sub(w.at) >= ms {
                break;
            }
            newer = newer.min(w.at);
            start -= 1;
        }
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(b: &WordBuffer) -> String {
        b.text(b.start()..b.end())
    }

    #[test]
    fn interims_are_revised_in_place() {
        let mut b = WordBuffer::default();
        b.interim("for God", 100);
        b.interim("for got so", 500);
        b.interim("for God so loved", 900);
        assert_eq!(all(&b), "for God so loved");
        assert_eq!(b.word(0).unwrap().at, 100, "a word keeps when it was first heard");
        assert_eq!(b.word(2).unwrap().at, 500);
        b.settle("For God so loved the world.", 1300, false);
        assert_eq!(all(&b), "For God so loved the world.");
        assert!((0..6).all(|i| b.word(i).unwrap().status == Status::Settled));
        assert_eq!(b.word(1).unwrap().at, 100);
        assert_eq!(b.word(5).unwrap().at, 1300);
        assert_eq!(b.changed(), 0..6);
    }

    #[test]
    fn interim_words_a_final_drops_stay_heard() {
        let mut b = WordBuffer::default();
        b.settle("turn with me to", 0, false);
        b.interim("Psalm sixty seven it", 1500);
        b.settle("says let the people praise thee", 3000, false);
        assert_eq!(all(&b), "turn with me to Psalm sixty seven it says let the people praise thee");
        assert_eq!(b.word(4).unwrap().status, Status::Unconfirmed);
        assert_eq!(b.word(8).unwrap().status, Status::Settled);
        assert_eq!(b.changed(), 8..14);
        // The final beginning part-way into the live words.
        b.interim("O God let", 4000);
        b.settle("let all the people", 5000, false);
        assert_eq!(b.text(14..20), "O God let all the people");
        assert_eq!(b.word(14).unwrap().status, Status::Unconfirmed);
        assert_eq!(b.word(16).unwrap().status, Status::Settled);
    }

    #[test]
    fn an_interim_that_starts_again_leaves_the_last_one_heard() {
        let mut b = WordBuffer::default();
        b.interim("for got so", 0);
        b.interim("four God so loved", 400);
        assert_eq!(all(&b), "four God so loved", "a revision, not new speech");
        b.interim("Psalm sixty seven it", 1000);
        assert_eq!(all(&b), "four God so loved Psalm sixty seven it");
        assert_eq!(b.word(0).unwrap().status, Status::Unconfirmed);
        assert_eq!(b.live_from(), 4);
    }

    #[test]
    fn a_reworded_final_replaces_its_interim() {
        let mut b = WordBuffer::default();
        b.interim("some sixty seven verse three", 0);
        b.settle("Psalm 67 verse 3", 1000, false);
        assert_eq!(all(&b), "Psalm 67 verse 3");
    }

    #[test]
    fn locked_words_settle_and_the_rest_stays_live() {
        let mut b = WordBuffer::default();
        b.interim("the Lord is my shepherd I", 0);
        b.settle("the Lord is my", 800, true);
        assert_eq!(b.live_from(), 4);
        assert_eq!(all(&b), "the Lord is my shepherd I");
        b.interim("shepherd I shall not", 1200);
        assert_eq!(all(&b), "the Lord is my shepherd I shall not");
        assert_eq!(b.word(4).unwrap().at, 0);
        b.settle("shepherd I shall not want.", 1600, false);
        assert_eq!(b.live_from(), b.end());
        assert_eq!(b.since(1200), "shall not want.");
        assert_eq!(b.recent(1600, 100), "shepherd I shall not want.", "the utterance being said, however long");
    }

    #[test]
    fn words_before_a_position_within_a_window() {
        let mut b = WordBuffer::default();
        b.settle("long ago", 0, false);
        b.settle("Daniel", 9000, false);
        assert_eq!(b.text(b.before(b.end(), 12, 20_000, 8_000)), "", "after a pause");
        b.settle("chapter what? Chapter", 10_000, false);
        let r = b.before(b.end(), 12, 12_000, 8_000);
        assert_eq!(b.text(r), "Daniel chapter what? Chapter");
    }
}
