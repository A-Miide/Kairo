//! Meaning-based detection: which stretches of recent speech to search by
//! meaning (`detection-spec/04-meaning-and-evidence.md`).
//!
//! A paraphrase arrives wrapped in commentary and recogniser noise; embedding
//! a whole segment dilutes it. So several short windows are searched at once —
//! the last sentence or two, the last few seconds, and the words right after a
//! quote signal ("the Bible says", "Paul wrote") — and the best-fitting window
//! speaks for each verse.

use std::collections::HashSet;
use std::sync::LazyLock;

use kairo_bible::VerseId;

use super::core::{Candidate, Engine};
use super::event::{MeaningHit, Request};
use super::types::{Evidence, Hold, Method, Target};
use crate::lexical::stem::heal;
use crate::lexical::text::{is_stop_word, norm};
use crate::re::re;

const MAX_WINDOW_WORDS: usize = 30;
const MAX_WINDOWS: usize = 5;
/// An aside between two parts of a verse.
const SHORT_SENTENCE_WORDS: usize = 7;
const AFTER_SIGNAL_WORDS: usize = 28;
const FUSED_WORDS: usize = 26;
const TAIL_WORDS: usize = 24;
const SHORT_TAIL_WORDS: usize = 14;

/// Crowd work and fillers dropped into the middle of a verse — only phrases
/// that are never scripture wording themselves.
static INTERJECTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    let phrases = [
        "(?:somebody |can i get an? |let me hear (?:you )?)?say amen",
        "amen",
        "hallelujah",
        "glory to god",
        "thank you jesus",
        "are you (?:with me|listening|there|still with me)",
        "can you hear me",
        "do you hear me",
        "listen to me",
        "listen",
        "(?:turn to|look at|tell) (?:your|the person next to you|somebody)(?: neighbou?r)?(?: and say)?",
        "say it with me",
        "say it",
        "watch this",
        "come on",
        "i said",
        "you know",
        "i mean",
        "right",
        "okay",
        "alright",
        "hello",
        "church",
        "uh+",
        "um+",
        "hmm+",
    ];
    re(&format!(r"(?i)\b(?:{})\b[,.!?]*", phrases.join("|")))
});
static SPACE_BEFORE_PUNCT: LazyLock<regex::Regex> = LazyLock::new(|| re(r"\s+([,.!?;])"));

fn without_interjections(text: &str) -> String {
    let t = INTERJECTION.replace_all(text, " ");
    let t = SPACE_BEFORE_PUNCT.replace_all(&t, "$1");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn norm_word(w: &str) -> String {
    w.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect()
}

/// Sentences, split after `.`, `!` or `?` followed by a space.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for w in text.split_whitespace() {
        cur.push(w);
        if w.ends_with(['.', '!', '?']) {
            out.push(cur.join(" "));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.join(" "));
    }
    out
}

impl Engine {
    /// Where the last phrase introducing scripture ends, as a word index:
    /// "the Bible says", "it is written", "Jesus said", "<a person> wrote"…
    /// A person is any proper name the Bible has.
    fn after_quote_signal(&self, words: &[String]) -> Option<usize> {
        let w = |i: usize| words.get(i).map_or("", String::as_str);
        let says = |s: &str| matches!(s, "says" | "said" | "tells" | "told" | "declares" | "teaches");
        let person_verb = |i: usize| -> Option<usize> {
            match w(i) {
                "says" | "said" | "wrote" | "writes" | "declared" => Some(i + 1),
                "told" if !w(i + 1).is_empty() => Some(i + 2),
                _ => None,
            }
        };
        let mut last = None;
        for i in 0..words.len() {
            let end = match w(i) {
                // the Bible / scripture / the word (of God) says
                "the" if matches!(w(i + 1), "bible" | "scripture" | "scriptures" | "word") => {
                    let j = if w(i + 1) == "word" && w(i + 2) == "of" && w(i + 3) == "god" { i + 4 } else { i + 2 };
                    match w(j) {
                        "tells" | "told" if w(j + 1) == "us" => Some(j + 2),
                        s if says(s) && s != "tells" && s != "told" => Some(j + 1),
                        _ => None,
                    }
                }
                "it" if matches!(w(i + 1), "is" | "was") && w(i + 2) == "written" => Some(i + 3),
                "thus" if matches!(w(i + 1), "says" | "saith") && w(i + 2) == "the" && w(i + 3) == "lord" => Some(i + 4),
                "jesus" | "christ" | "god" => match w(i + 1) {
                    "says" | "said" => Some(i + 2),
                    "told" if matches!(w(i + 2), "us" | "them" | "him" | "her") => Some(i + 3),
                    _ => None,
                },
                "the" if matches!(w(i + 1), "lord" | "spirit") => match w(i + 2) {
                    "says" | "said" => Some(i + 3),
                    "told" if matches!(w(i + 3), "us" | "them" | "him" | "her") => Some(i + 4),
                    _ => None,
                },
                "the" if matches!(w(i + 1), "psalmist" | "apostle" | "prophet") => person_verb(i + 2),
                "in" if w(i + 1) == "the" && w(i + 2) == "words" && w(i + 3) == "of" => {
                    let x = w(i + 4);
                    if x == "jesus" || x == "scripture" || self.k.is_name(x) {
                        Some(i + 5)
                    } else if x == "the" && w(i + 5) == "psalmist" {
                        Some(i + 6)
                    } else {
                        None
                    }
                }
                x if self.k.is_name(x) => person_verb(i + 1),
                // "…, said Abraham, …"
                "said" | "says" | "saith" if self.k.is_name(w(i + 1)) => Some(i + 2),
                _ => None,
            };
            if let Some(e) = end {
                last = Some(e.min(words.len()));
            }
        }
        last
    }

    /// Did the preacher just introduce scripture ("the Bible says", "said Abraham")?
    pub(crate) fn quote_signalled(&self, recent: &str) -> bool {
        let normed: Vec<String> = recent.split_whitespace().map(norm_word).collect();
        self.after_quote_signal(&normed).is_some()
    }

    /// Windows of recent speech to search by meaning, most specific first.
    pub(crate) fn paraphrase_windows(&self, recent: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let add = |out: &mut Vec<String>, text: &str| {
            let w: Vec<&str> = text.split_whitespace().collect();
            let t = w[w.len().saturating_sub(MAX_WINDOW_WORDS)..].join(" ");
            if t.split_whitespace().count() >= 6 && !out.contains(&t) {
                out.push(t);
            }
        };
        let raw: Vec<&str> = recent.split_whitespace().collect();
        let normed: Vec<String> = raw.iter().map(|w| norm_word(w)).collect();
        // After the last quote signal: the verse itself, asides taken out.
        if let Some(end) = self.after_quote_signal(&normed) {
            let after = without_interjections(&raw[end..].join(" "));
            add(&mut out, &after.split_whitespace().take(AFTER_SIGNAL_WORDS).collect::<Vec<_>>().join(" "));
        }
        // The last few seconds with the asides removed, so a verse's pieces read as one.
        let fused = without_interjections(recent);
        let fw: Vec<&str> = fused.split_whitespace().collect();
        add(&mut out, &fw[fw.len().saturating_sub(FUSED_WORDS)..].join(" "));
        // The last sentence, the last two, and two around a short aside.
        let s = sentences(recent);
        if let Some(last) = s.last() {
            add(&mut out, last);
        }
        if s.len() > 1 {
            add(&mut out, &s[s.len() - 2..].join(" "));
        }
        if s.len() > 2 && s[s.len() - 2].split_whitespace().count() <= SHORT_SENTENCE_WORDS {
            add(&mut out, &format!("{} {}", s[s.len() - 3], s[s.len() - 1]));
        }
        // The last few seconds regardless of punctuation.
        add(&mut out, &raw[raw.len().saturating_sub(TAIL_WORDS)..].join(" "));
        if out.len() < 3 {
            add(&mut out, &raw[raw.len().saturating_sub(SHORT_TAIL_WORDS)..].join(" "));
        }
        out.truncate(MAX_WINDOWS);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asides_are_taken_out() {
        assert_eq!(without_interjections("come to me, church, are you with me? all you that labour"), "come to me, all you that labour");
    }

    #[test]
    fn sentence_split() {
        assert_eq!(sentences("One two. Three four! Five"), vec!["One two.", "Three four!", "Five"]);
    }
}

// ── Deciding on what meaning found ─────────────────────────────────────────

/// Words of speech between meaning searches.
const PARAPHRASE_EVERY_WORDS: u64 = 6;
const PARAPHRASE_LIMIT: usize = 8;
const SCOPED_LIMIT: usize = 5;
/// A meaning match just after a spoken citation is offered, not sent.
const JUST_CITED_MS: u64 = 4_000;
const OFFER_SCORE: f32 = 0.7;
const SEND_SCORE: f32 = 0.9;

/// Where a meaning match needs to be, in the scales of the embedder and
/// cross-encoder in use (cosines differ between models; see the calibration
/// in `docs/RUST_REWRITE_LOG.md`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeaningThresholds {
    /// Possible Matches: cross-encoder, cosine, shared identifying wording.
    pub offer: (f32, f32, f32),
    /// …when the passage is already in play.
    pub offer_in_play: (f32, f32, f32),
    /// No cross-encoder (a service in another language): cosine, wording.
    pub offer_no_rerank: (f32, f32),
    /// Read by the cross-encoder in modern wording: rr, cos, wording, lead over the next.
    pub offer_modern: (f32, f32, f32, f32),
    /// The screen: rr, cos, wording, lead over the next.
    pub send: (f32, f32, f32, f32),
    /// A meaning search within the chapters the sermon points at: cosine, lead.
    pub scoped: (f32, f32),
}

impl MeaningThresholds {
    /// The values tuned for the original model usage (mean-pooled
    /// EmbeddingGemma without prompts, MiniLM cross-encoder).
    pub const LEGACY: MeaningThresholds = MeaningThresholds {
        offer: (0.98, 0.80, 10.0),
        offer_in_play: (0.95, 0.78, 8.0),
        offer_no_rerank: (0.82, 10.0),
        offer_modern: (0.98, 0.75, 15.0, 0.2),
        send: (0.99, 0.84, 15.0, 0.2),
        scoped: (0.60, 0.03),
    };

    /// bge-small-en-v1.5 (CLS pooling, its query prompt; MiniLM cross-encoder):
    /// LEGACY carried over by `calibrate` (the same share of sermon speech
    /// clears each cosine), then the offer cosines lowered so the cross-encoder
    /// decides — calibrated, right paraphrases sat a hair under them (John 3:16
    /// at 0.719, Matthew 26:75 at 0.725). Measured 2026-10-16 against LEGACY on
    /// EmbeddingGemma: dev found 225/259 both, wrong sends 2 both; held-out
    /// found 61/76 vs 62/76, 0 wrong both, Possible Matches 57% vs 54% right.
    pub const BGE_SMALL: MeaningThresholds = MeaningThresholds {
        offer: (0.98, 0.70, 10.0),
        offer_in_play: (0.95, 0.68, 8.0),
        offer_no_rerank: (0.748, 10.0),
        offer_modern: (0.98, 0.65, 15.0, 0.2),
        send: (0.99, 0.767, 15.0, 0.2),
        scoped: (0.546, 0.03),
    };

    /// The thresholds for the embedder of this name (`EmbedderConfig::name`).
    pub fn for_embedder(name: &str) -> MeaningThresholds {
        match name {
            "bge-small" => MeaningThresholds::BGE_SMALL,
            _ => MeaningThresholds::LEGACY,
        }
    }
}

impl Default for MeaningThresholds {
    fn default() -> Self {
        MeaningThresholds::LEGACY
    }
}

#[derive(Default)]
pub(crate) struct MeaningState {
    /// Words heard when meaning was last searched.
    last_count: u64,
    /// The search in flight: its id, windows, and whether a citation had just been heard.
    asked: Option<(u32, Vec<String>, bool)>,
    /// The scoped search in flight, and the verses the full search already found.
    scoped: Option<(u32, Vec<VerseId>)>,
    /// The previous search's decision: a match must hold for two searches running.
    prev: Option<Decision>,
    /// The previous search's cross-encoder top, when it nearly made a decision.
    prev_near: Option<VerseId>,
}

/// A paraphrase decision: (verse, where, score, cross-encoder, shared wording).
type Decision = (VerseId, Target, f32, Option<f32>, f32);

/// The second search of a pair may confirm with its cross-encoder top at this,
/// short of the offer's own bar, if the offer's similarity and wording floors
/// still hold. Measured 2026-10-17 (replay, meaning on, dev): John 3:16 topped
/// three searches running at 0.990, 0.977, 0.974 and Psalms 81:14 at 0.979 then
/// 1.000, and neither was offered; at 0.95 three noise offers came in too.
const NEAR_REPEAT_RR: f32 = 0.97;

/// A meaning match with the wording it shares with its window.
struct Weighed {
    hit: MeaningHit,
    lex: f32,
}

impl Engine {
    /// Every few words of speech, search the recent windows by meaning. Speech
    /// in progress skips a round while a search is out; a finished sentence
    /// always gets its own.
    pub(crate) fn maybe_search_meaning(&mut self, finished: bool, now: u64) {
        if !self.cfg.meaning {
            return;
        }
        if self.meaning.asked.is_some() && !finished {
            return;
        }
        let count = self.t.words_heard + self.t.interim.split_whitespace().count() as u64;
        if count.saturating_sub(self.meaning.last_count) < PARAPHRASE_EVERY_WORDS {
            return;
        }
        self.meaning.last_count = count;
        let windows = self.paraphrase_windows(&self.paraphrase_recent_text());
        if windows.is_empty() {
            return;
        }
        let just_cited = now.saturating_sub(self.screen.last_citation_at) < JUST_CITED_MS;
        let id = self.next_request_id();
        self.out.requests.push(Request::Paraphrase { id, windows: windows.clone(), limit: PARAPHRASE_LIMIT });
        self.meaning.asked = Some((id, windows, just_cited));
    }

    /// Identifying weight (IDF sum) of the verse's words said in `text`, each stem once.
    fn shared_identifying_weight(&self, text: &str, v: VerseId) -> f32 {
        let said: HashSet<String> = norm(text).split(' ').filter(|w| w.len() >= 3 && !is_stop_word(w)).map(heal).collect();
        let lex = &self.k.lexicon;
        let mut seen: HashSet<String> = HashSet::new();
        let mut idf = 0.0;
        for words in [lex.kjv().words(v), lex.first_other().and_then(|o| o.words(v))].into_iter().flatten() {
            for &w in words {
                let s = lex.vocab().word(w);
                if s.len() < 3 || is_stop_word(s) {
                    continue;
                }
                let stem = heal(s);
                if said.contains(&stem) && seen.insert(stem) {
                    idf += lex.idf(w);
                }
            }
        }
        idf
    }

    pub(crate) fn on_paraphrase(&mut self, id: u32, hits: Vec<MeaningHit>, now: u64) {
        let Some((asked, windows, just_cited)) = self.meaning.asked.take() else { return };
        if asked != id {
            self.meaning.asked = Some((asked, windows, just_cited));
            return;
        }
        let found: Vec<VerseId> = hits.iter().map(|h| h.verse).collect();
        let weighed: Vec<Weighed> = hits.into_iter().map(|hit| Weighed { lex: self.shared_identifying_weight(windows.get(hit.window).map_or("", String::as_str), hit.verse), hit }).collect();
        let decision = self.decide_paraphrase(&weighed, now);
        let near = self.near_top(&weighed);
        let held = match (decision, self.meaning.prev) {
            (Some(d), prev) if prev.map(|p| p.0) == Some(d.0) || self.meaning.prev_near == Some(d.0) => Some(d),
            (None, Some(p)) if near == Some(p.0) => Some(p),
            _ => None,
        };
        self.meaning.prev = decision;
        self.meaning.prev_near = near;
        if let Some((v, target, score, rr, lex)) = held {
            let target = if just_cited { Target::Suggestions } else { target };
            let on_screen = self.screen.last_detected.map(|(d, _, _)| d) == Some(v);
            let r = self.r(v);
            let in_cited_chapter = just_cited
                && (self.sermon.iter().rev().find(|c| c.named).is_some_and(|c| c.book == r.book && c.chapter == Some(r.chapter))
                    || self.screen.up_verse().is_some_and(|u| {
                        let u = self.r(u);
                        u.book == r.book && u.chapter == r.chapter
                    }));
            if !(target == Target::Suggestions && (on_screen || in_cited_chapter)) {
                // An offer comes with the verses worded the same (twins::worded_alike).
                let mut verses = vec![v];
                if target == Target::Suggestions {
                    verses.extend(self.worded_alike(v));
                }
                let mut c = Candidate::new(verses, Method::Paraphrase, score, target)
                    .evidence(Evidence { paraphrase_score: Some(score), rerank: rr, similarity: Some(score), matched_idf: Some(lex), ..Default::default() })
                    .from_text(&windows[0]);
                c.hold = (target == Target::Suggestions).then_some(Hold::ParaphraseUnsure);
                self.decide(c, now);
            }
        }
        // Then within the chapters the sermon points at.
        if !just_cited {
            if let Some(scope) = self.scope_verses(now) {
                let id = self.next_request_id();
                let mut verses: Vec<VerseId> = scope.into_iter().collect();
                verses.sort();
                self.out.requests.push(Request::Scoped { id, text: windows[0].clone(), verses, limit: SCOPED_LIMIT });
                self.meaning.scoped = Some((id, found));
            }
        }
    }

    pub(crate) fn on_scoped(&mut self, id: u32, hits: &[(VerseId, f32)], now: u64) {
        let Some((asked, found)) = self.meaning.scoped.take() else { return };
        if asked != id {
            self.meaning.scoped = Some((asked, found));
            return;
        }
        let Some(&(v, s)) = hits.first() else { return };
        let margin = hits.get(1).map_or(1.0, |h| s - h.1);
        let (min, lead) = self.cfg.meaning_thresholds.scoped;
        if s >= min && margin >= lead && !found.contains(&v) {
            self.decide(Candidate::new(vec![v], Method::Semantic, s, Target::Suggestions), now);
        }
    }

    /// The cross-encoder's top verse, when it clears NEAR_REPEAT_RR and the offer's floors.
    fn near_top(&self, results: &[Weighed]) -> Option<VerseId> {
        let (_, cos, lex) = self.cfg.meaning_thresholds.offer;
        let top = results.iter().filter(|w| w.hit.rerank.is_some()).max_by(|a, b| a.hit.rerank.unwrap_or(0.0).total_cmp(&b.hit.rerank.unwrap_or(0.0)))?;
        (top.hit.rerank.unwrap_or(0.0) >= NEAR_REPEAT_RR && top.hit.cos >= cos && top.lex >= lex).then_some(top.hit.verse)
    }

    /// The paraphrase decision, or nothing.
    fn decide_paraphrase(&self, results: &[Weighed], now: u64) -> Option<Decision> {
        let t = self.cfg.meaning_thresholds;
        let rr_of = |w: &Weighed| w.hit.rerank.unwrap_or(-1.0);
        let mut ranked: Vec<&Weighed> = results.iter().collect();
        ranked.sort_by(|a, b| rr_of(b).total_cmp(&rr_of(a)).then(b.hit.cos.total_cmp(&a.hit.cos)));
        let top = *ranked.first()?;
        let second = ranked.iter().find(|r| r.hit.verse != top.hit.verse);
        let named = book_mentions_recent(self);
        let affinity = |v: VerseId| self.passage_affinity(v, now).max(if named.contains(&self.r(v).book) { 2.0 } else { 0.0 });
        let out = |target: Target, score: f32| Some((top.hit.verse, target, score, top.hit.rerank, top.lex));
        match top.hit.rerank {
            Some(rr) => {
                let (s_rr, s_cos, s_lex, s_margin) = t.send;
                let margin = second.map_or(1.0, |s| rr - rr_of(s).max(0.0));
                let sendable = rr >= s_rr && top.hit.cos >= s_cos && top.lex >= s_lex;
                // Even the surest paraphrase is offered, never put up by itself: a paraphrase is
                // the preacher's own words, not the verse cited or read, and the owner's rule is
                // that only those go on screen unasked. Measured 2026-10-16 (replay, meaning on,
                // dev): paraphrase-to-screen was 8 right and 2 wrong, and live a passing "thank God
                // victory is here" put 1 Corinthians 15:57 up. Sure ones head Possible Matches.
                if sendable && margin >= s_margin {
                    return out(Target::Suggestions, SEND_SCORE);
                }
                if sendable && second.is_some_and(|s| affinity(top.hit.verse) >= 2.0 && affinity(s.hit.verse) < 2.0) {
                    return out(Target::Suggestions, SEND_SCORE);
                }
                let (o_rr, o_cos, o_lex) = if affinity(top.hit.verse) >= 2.0 { t.offer_in_play } else { t.offer };
                if rr >= o_rr && top.hit.cos >= o_cos && top.lex >= o_lex {
                    return out(Target::Suggestions, OFFER_SCORE);
                }
                let (m_rr, m_cos, m_lex, m_margin) = t.offer_modern;
                let mut by_modern: Vec<&Weighed> = results.iter().filter(|r| r.hit.rerank_modern.is_some()).collect();
                by_modern.sort_by(|a, b| b.hit.rerank_modern.unwrap_or(0.0).total_cmp(&a.hit.rerank_modern.unwrap_or(0.0)));
                let m_top = by_modern.first()?;
                let m_second = by_modern.iter().find(|r| r.hit.verse != m_top.hit.verse).and_then(|r| r.hit.rerank_modern).unwrap_or(0.0);
                let m = m_top.hit.rerank_modern.unwrap_or(0.0);
                (m >= m_rr && m_top.hit.cos >= m_cos && m_top.lex >= m_lex && m - m_second.max(0.0) >= m_margin)
                    .then_some((m_top.hit.verse, Target::Suggestions, OFFER_SCORE, m_top.hit.rerank, m_top.lex))
            }
            None => {
                let (n_cos, n_lex) = t.offer_no_rerank;
                (top.hit.cos >= n_cos && top.lex >= n_lex).then_some((top.hit.verse, Target::Suggestions, OFFER_SCORE, None, top.lex))
            }
        }
    }
}

/// Books named in the last 25 words of speech.
fn book_mentions_recent(e: &Engine) -> Vec<kairo_bible::Book> {
    let recent = e.paraphrase_recent_text();
    let words: Vec<&str> = recent.split_whitespace().collect();
    crate::reference::book_mentions(&words[words.len().saturating_sub(25)..].join(" "), e.k.books, e.cfg.lang)
}
