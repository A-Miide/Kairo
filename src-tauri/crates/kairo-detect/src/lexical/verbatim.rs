//! A verse read (nearly) word for word.
//!
//! Candidates come from the words spoken (a verse must share at least 40% of
//! them, and two); each is then checked three ways, strictest first:
//! 1. A stretch of the speech, 6–12 words, found word for word (by stem, so
//!    "keeping" meets "kept") in the verse — KJV first, then each other wording.
//! 2. For verses longer than that window: the longest run of the verse's own
//!    words found in the speech.
//! 3. What share of the verse's 4-word phrases occur anywhere in the speech —
//!    survives a substituted word or two, scored below the first two tiers.
//!
//! A match is scored by the identifying weight it carries (IDF), not its word
//! count: missing "for" costs almost nothing, missing the verse's one rare
//! word costs a lot.


use kairo_bible::VerseId;

use super::lexicon::{Lexicon, SpeechWord, WordId};
use super::window::longest_run;

/// Matched identifying weight at which a match's length stops limiting its score.
pub const IDF_FULL_CONFIDENCE: f32 = 8.0;
const MAX_WINDOW: usize = 12;
const LONG_VERSE_SCAN_MAX: usize = 45;
const CANDIDATES: usize = 300;
const NGRAM_MIN_COVERAGE: f32 = 0.40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Phrase,
    LongVerse,
    Phrases4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerbatimHit {
    pub verse: VerseId,
    pub score: f32,
    pub tier: Tier,
    /// Identifying weight of the matched run (tiers 1 and 2).
    pub matched_idf: Option<f32>,
}

pub fn verbatim(lex: &Lexicon, text: &str, min_words: usize, limit: usize) -> Vec<VerbatimHit> {
    let speech = lex.speech(text);
    if speech.len() < min_words {
        return Vec::new();
    }
    let stems: Vec<Option<WordId>> = speech.iter().map(|w| w.stem).collect();
    let candidates = candidates(lex, &speech);

    let mut results: Vec<VerbatimHit> = Vec::new();
    let mut found: Vec<VerseId> = Vec::new();
    let score = |matched: f32, total: f32| -> f32 {
        let coverage = (matched / total).min(1.0);
        let length = (matched / IDF_FULL_CONFIDENCE).min(1.0);
        ((0.75 + length * 0.24) * coverage.sqrt()).min(0.99)
    };

    // 1. Speech windows, longest first, found in the verse.
    'verse: for &v in &candidates {
        if results.len() >= limit {
            break;
        }
        let kjv_stems = lex.kjv().stems(v).unwrap_or(&[]);
        if kjv_stems.len() > MAX_WINDOW {
            continue;
        }
        let sources = std::iter::once((kjv_stems, lex.kjv().words(v).unwrap_or(&[])))
            .chain(lex.others().iter().filter_map(|o| Some((o.stems(v)?, o.words(v)?))));
        let sources: Vec<(&[WordId], &[WordId])> = sources.collect();
        // For each source, the longest run shared with the speech ending at each spoken word:
        // a window occurs in the verse exactly when a run that long ends where it ends.
        let ends: Vec<Vec<usize>> = sources.iter().map(|(vs, _)| run_ends(vs, &stems)).collect();
        for len in (min_words..=MAX_WINDOW.min(stems.len())).rev() {
            for start in 0..=stems.len() - len {
                for (k, (verse_stems, verse_words)) in sources.iter().enumerate() {
                    if ends[k][start + len - 1] < len {
                        continue;
                    }
                    let at = find(verse_stems, &stems[start..start + len]).expect("a shared run that long ends here");
                    let matched = lex.idf_span(verse_words, at, len);
                    results.push(VerbatimHit { verse: v, score: score(matched, lex.idf_total(verse_words)), tier: Tier::Phrase, matched_idf: Some(matched) });
                    found.push(v);
                    continue 'verse;
                }
            }
        }
    }

    // 2. Long verses: the longest run of the verse's words in the speech.
    if results.len() < limit {
        for &v in &candidates {
            if results.len() >= limit {
                break;
            }
            if found.contains(&v) {
                continue;
            }
            let kjv_stems = lex.kjv().stems(v).unwrap_or(&[]);
            if kjv_stems.len() <= MAX_WINDOW {
                continue;
            }
            let cap = kjv_stems.len().min(LONG_VERSE_SCAN_MAX);
            let mut best = longest_run(kjv_stems, &stems, min_words, cap).map(|r| (r, lex.kjv().words(v).unwrap_or(&[])));
            for o in lex.others() {
                let (Some(s), Some(w)) = (o.stems(v), o.words(v)) else { continue };
                if let Some(r) = longest_run(s, &stems, min_words, cap) {
                    if best.is_none_or(|(b, _)| r.len > b.len) {
                        best = Some((r, w));
                    }
                }
            }
            if let Some((run, words)) = best {
                let matched = lex.idf_span(words, run.start, run.len);
                results.push(VerbatimHit { verse: v, score: score(matched, lex.idf_total(words)), tier: Tier::LongVerse, matched_idf: Some(matched) });
                found.push(v);
            }
        }
    }

    // 3. Share of the verse's 4-word phrases spoken anywhere.
    if results.len() < limit {
        let spoken: std::collections::HashSet<[WordId; 4]> = stems
            .windows(4)
            .filter_map(|w| Some([w[0]?, w[1]?, w[2]?, w[3]?]))
            .collect();
        for &v in &candidates {
            if results.len() >= limit {
                break;
            }
            if found.contains(&v) {
                continue;
            }
            let s = lex.kjv().stems(v).unwrap_or(&[]);
            if s.len() < 4 {
                continue;
            }
            let total = s.len() - 3;
            let hit = s.windows(4).filter(|w| spoken.contains(&[w[0], w[1], w[2], w[3]])).count();
            let coverage = hit as f32 / total as f32;
            if coverage >= NGRAM_MIN_COVERAGE {
                let score = (0.65 + (coverage - NGRAM_MIN_COVERAGE) / 0.60 * 0.17).min(0.82);
                results.push(VerbatimHit { verse: v, score, tier: Tier::Phrases4, matched_idf: None });
                found.push(v);
            }
        }
    }

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results
}

/// Verses sharing enough of the speech's words, most shared first.
fn candidates(lex: &Lexicon, speech: &[SpeechWord]) -> Vec<VerseId> {
    let mut query: Vec<&SpeechWord> = Vec::new();
    for w in speech {
        if w.text.len() >= 3 && !query.iter().any(|q| q.text == w.text) {
            query.push(w);
        }
    }
    let n = lex.verse_count();
    let mut counts = vec![0u32; n];
    // Which query word last counted a verse through its own form.
    let mut direct_for = vec![0u32; n];
    let mut order: Vec<VerseId> = Vec::new();
    for (qi, w) in query.iter().enumerate() {
        let tag = qi as u32 + 1;
        let direct: &[VerseId] = w.word.map_or(&[], |id| lex.verses_with_word(id));
        for &v in direct {
            let i = v.index();
            if counts[i] == 0 {
                order.push(v);
            }
            counts[i] += 1;
            direct_for[i] = tag;
        }
        if w.stem_text != w.text {
            if let Some(s) = w.stem {
                for &v in lex.verses_with_stem(s) {
                    let i = v.index();
                    if direct_for[i] == tag {
                        continue;
                    }
                    if counts[i] == 0 {
                        order.push(v);
                    }
                    counts[i] += 1;
                }
            }
        }
    }
    let threshold = 2.max(query.len() * 2 / 5) as u32;
    let mut qualifying: Vec<VerseId> = order.into_iter().filter(|v| counts[v.index()] >= threshold).collect();
    qualifying.sort_by(|a, b| counts[b.index()].cmp(&counts[a.index()]));
    qualifying.truncate(CANDIDATES);
    qualifying
}

/// For each spoken word, the longest run of consecutive words shared with
/// `verse` that ends on it (unknown words break a run).
fn run_ends(verse: &[WordId], speech: &[Option<WordId>]) -> Vec<usize> {
    let mut prev = vec![0usize; verse.len() + 1];
    let mut cur = vec![0usize; verse.len() + 1];
    let mut out = Vec::with_capacity(speech.len());
    for s in speech {
        let mut best = 0;
        for (j, w) in verse.iter().enumerate() {
            cur[j + 1] = if *s == Some(*w) { prev[j] + 1 } else { 0 };
            best = best.max(cur[j + 1]);
        }
        out.push(best);
        std::mem::swap(&mut prev, &mut cur);
    }
    out
}

/// Where `needle` occurs in `hay` as consecutive words.
fn find(hay: &[WordId], needle: &[Option<WordId>]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].iter().zip(needle).all(|(h, n)| Some(*h) == *n))
}
