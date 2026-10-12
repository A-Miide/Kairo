//! A verse paraphrased loosely: its most identifying words were said.
//!
//! Every verse has a fingerprint — its 10 most identifying words. A verse
//! qualifies when the words spoken cover at least 35% of its fingerprint's
//! weight, with two different words and real absolute weight (a short,
//! low-information verse can't be "covered" by generic vocabulary alone).
//! Confidence then depends on how clearly one verse stands out.
//!
//! A recently cited passage nudges verses near it ahead, as a tie-breaker
//! among already-qualified verses — it never qualifies one.

use std::collections::{HashMap, HashSet};

use kairo_bible::{Corpus, VerseId};

use super::lexicon::{Lexicon, WordId};
use super::text::{is_stop_word, norm};

const COVERAGE_THRESHOLD: f32 = 0.35;
const MIN_WORD_HITS: u32 = 2;
const MIN_ABS_WEIGHT: f32 = 15.0;
const HIGH_ABS_MIN: f32 = 0.55;
const MEDIUM_ABS_MIN: f32 = 0.42;
const TOPIC_LIBRARY_SIZE: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    None,
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FingerprintResult {
    pub hits: Vec<(VerseId, f32)>,
    pub confidence: Confidence,
}

impl FingerprintResult {
    fn none() -> Self {
        FingerprintResult { hits: Vec::new(), confidence: Confidence::None }
    }
}

/// A passage cited recently, for the nudge: `verse` None means the chapter.
#[derive(Clone, Copy, Debug)]
pub struct RecentCitation {
    pub verse: VerseId,
    pub whole_chapter: bool,
    pub age_ms: u64,
}

const NUDGE_WINDOW_MS: u64 = 5 * 60 * 1000;
const NUDGE_NEIGHBOURS: u16 = 5;
const NUDGE_MAX: f32 = 0.15;

pub fn fingerprint(lex: &Lexicon, corpus: &Corpus, text: &str, limit: usize, recent: &[RecentCitation], only: Option<&HashSet<VerseId>>) -> FingerprintResult {
    let mut spoken: Vec<String> = Vec::new();
    for w in norm(text).split(' ') {
        if w.len() >= 4 && !is_stop_word(w) && !spoken.iter().any(|s| s == w) {
            spoken.push(w.to_string());
        }
    }
    if spoken.is_empty() {
        return FingerprintResult::none();
    }

    // Each spoken word scores a verse at most once, through its own form or its stem.
    let mut weight: HashMap<VerseId, f32> = HashMap::new();
    let mut words_hit: HashMap<VerseId, u32> = HashMap::new();
    let mut order: Vec<VerseId> = Vec::new();
    for w in &spoken {
        let stem = super::stem::heal(w);
        let forms: Vec<WordId> = std::iter::once(w.as_str()).chain((stem != *w).then_some(stem.as_str())).filter_map(|f| lex.vocab().id(f)).collect();
        let original = lex.vocab().id(w);
        let mut scored: Vec<VerseId> = Vec::new();
        for &form in &forms {
            for &v in lex.verses_with_word(form) {
                if only.is_some_and(|set| !set.contains(&v)) || scored.contains(&v) {
                    continue;
                }
                let sig = lex.signature(v);
                let idf = sig.iter().find(|(s, _)| *s == form).or_else(|| original.and_then(|o| sig.iter().find(|(s, _)| *s == o))).map(|(_, i)| *i);
                if let Some(idf) = idf {
                    scored.push(v);
                    if !weight.contains_key(&v) {
                        order.push(v);
                    }
                    *weight.entry(v).or_default() += idf;
                    *words_hit.entry(v).or_default() += 1;
                }
            }
        }
    }

    let mut coverage: Vec<(VerseId, f32)> = order
        .into_iter()
        .filter_map(|v| {
            let matched = weight[&v];
            let cov = matched / lex.signature_weight(v).max(1e-6);
            (cov >= COVERAGE_THRESHOLD && words_hit[&v] >= MIN_WORD_HITS && matched >= MIN_ABS_WEIGHT).then_some((v, cov))
        })
        .collect();
    if coverage.is_empty() {
        return FingerprintResult::none();
    }
    nudge(&mut coverage, corpus, recent);
    coverage.sort_by(|a, b| b.1.total_cmp(&a.1));
    coverage.truncate(limit + 1);

    let top = coverage[0].1;
    let second = coverage.get(1).map_or(0.0, |c| c.1);
    let tied: Vec<(VerseId, f32)> = coverage.iter().copied().filter(|(_, c)| *c >= top * 0.9).collect();
    let confidence = if tied.len() == 1 || second == 0.0 {
        if top >= HIGH_ABS_MIN { Confidence::High } else if top >= MEDIUM_ABS_MIN { Confidence::Medium } else { Confidence::Low }
    } else if tied.len() <= 3 && top / second >= 1.4 {
        if top >= HIGH_ABS_MIN { Confidence::High } else { Confidence::Medium }
    } else if tied.len() <= 3 {
        Confidence::Medium
    } else if tied.len() <= 5 {
        Confidence::Low
    } else {
        Confidence::None
    };
    if confidence == Confidence::None {
        return FingerprintResult::none();
    }
    FingerprintResult { hits: tied.into_iter().take(limit).map(|(v, c)| (v, c.min(0.97))).collect(), confidence }
}

/// Up to +15% for verses within five of a passage cited in the last five
/// minutes, fading with the citation's age.
fn nudge(coverage: &mut [(VerseId, f32)], corpus: &Corpus, recent: &[RecentCitation]) {
    if recent.is_empty() {
        return;
    }
    for (v, cov) in coverage.iter_mut() {
        let here = corpus.verse_ref(*v);
        let mut best = 0f32;
        for c in recent {
            let at = corpus.verse_ref(c.verse);
            if at.book != here.book || at.chapter != here.chapter || c.age_ms >= NUDGE_WINDOW_MS {
                continue;
            }
            let fresh = 1.0 - c.age_ms as f32 / NUDGE_WINDOW_MS as f32;
            if c.whole_chapter || here.verse.abs_diff(at.verse) <= NUDGE_NEIGHBOURS {
                best = best.max(fresh);
            }
        }
        if best > 0.0 {
            *cov *= 1.0 + NUDGE_MAX * best;
        }
    }
}

/// The verses most about the sermon's recurring words — searched first, so
/// a loose paraphrase competes against ~80 verses instead of the whole Bible.
pub fn topic_library(lex: &Lexicon, topic_words: &[&str]) -> Option<HashSet<VerseId>> {
    if topic_words.len() < 2 {
        return None;
    }
    let mut scores: HashMap<VerseId, f32> = HashMap::new();
    let mut order: Vec<VerseId> = Vec::new();
    for w in topic_words {
        let Some(id) = lex.vocab().id(w) else { continue };
        let idf = lex.idf(id);
        if idf < 1.5 {
            continue;
        }
        for &v in lex.verses_with_word(id) {
            if lex.signature(v).iter().any(|(s, _)| *s == id) {
                if !scores.contains_key(&v) {
                    order.push(v);
                }
                *scores.entry(v).or_default() += idf;
            }
        }
    }
    if scores.is_empty() {
        return None;
    }
    order.sort_by(|a, b| scores[b].total_cmp(&scores[a]));
    order.truncate(TOPIC_LIBRARY_SIZE);
    Some(order.into_iter().collect())
}
