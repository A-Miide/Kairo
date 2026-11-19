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
//! 4. The verse's words in the verse's order, with gaps: an in-order alignment
//!    (a weighted longest common subsequence over stems) that survives a word
//!    skipped, added, swapped or misheard mid-phrase ("Cast not therefore away
//!    your confidence" for "Cast not away therefore your confidence"). The
//!    matched words must sit close together in the speech. A verse keeps the
//!    best score any tier gives it (owner review 2026-11-05).
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
/// An in-order match's words fill at least this share of the stretch of speech they span…
const ORDERED_MIN_DENSITY: f32 = 0.6;
/// …and at least this many of them are identifying words (this weight or more), not filler.
const ORDERED_MIN_TELLING: usize = 3;
const ORDERED_TELLING_IDF: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Phrase,
    LongVerse,
    Phrases4,
    /// The verse's words in its order, with gaps.
    Ordered,
    /// The verse's words in its order, the only gaps filler words ("the same yesterday today and
    /// forever" for "the same yesterday, and to day, and for ever"): an unbroken run but for those.
    NearRun,
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
    search(lex, text, min_words, limit, false)
}

/// `verbatim`, and tier 4: the verse's words in its order with gaps. For the search box, and for a
/// settled stretch of speech that the first three tiers found nothing certain in (engine unique_phrase):
/// it adds weight up from scattered words, so it is kept out of the paths tuned on tiers 1-3.
pub fn verbatim_in_order(lex: &Lexicon, text: &str, min_words: usize, limit: usize) -> Vec<VerbatimHit> {
    search(lex, text, min_words, limit, true)
}

fn search(lex: &Lexicon, text: &str, min_words: usize, limit: usize, in_order: bool) -> Vec<VerbatimHit> {
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

    // 4. In the verse's order, with gaps: kept where it beats what the tiers above found.
    for &v in candidates.iter().filter(|_| in_order) {
        let sources = std::iter::once((lex.kjv().stems(v), lex.kjv().words(v))).chain(lex.others().iter().map(|o| (o.stems(v), o.words(v))));
        let mut best: Option<VerbatimHit> = None;
        for (verse_stems, verse_words) in sources {
            let (Some(vs), Some(vw)) = (verse_stems, verse_words) else { continue };
            let Some((matched, filler_gaps)) = ordered_match(lex, vs, vw, &stems, min_words) else { continue };
            let tier = if filler_gaps { Tier::NearRun } else { Tier::Ordered };
            let hit = VerbatimHit { verse: v, score: score(matched, lex.idf_total(vw)), tier, matched_idf: Some(matched) };
            if best.is_none_or(|b| hit.score > b.score) {
                best = Some(hit);
            }
        }
        let Some(hit) = best else { continue };
        match results.iter().position(|r| r.verse == v) {
            Some(i) if results[i].score >= hit.score => {}
            Some(i) => results[i] = hit,
            None => results.push(hit),
        }
    }

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results.truncate(limit);
    prefer_what_was_said(&mut results);
    results
}

/// The identifying weight of the verse's words found in the speech in the verse's order (gaps allowed
/// on both sides), when at least `min_words` match and they sit close together in the speech; and
/// whether every gap between the matched words, in the speech and in the verse, is filler.
fn ordered_match(lex: &Lexicon, verse_stems: &[WordId], verse_words: &[WordId], speech: &[Option<WordId>], min_words: usize) -> Option<(f32, bool)> {
    let (n, m) = (speech.len(), verse_stems.len());
    if n == 0 || m == 0 {
        return None;
    }
    let weight: Vec<f32> = verse_words.iter().map(|w| lex.idf(*w)).collect();
    // dp[i][j]: the best weight aligning speech[..i] with verse[..j].
    let mut dp = vec![0f32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in 1..=n {
        for j in 1..=m {
            let mut best = dp[at(i - 1, j)].max(dp[at(i, j - 1)]);
            if speech[i - 1] == Some(verse_stems[j - 1]) {
                best = best.max(dp[at(i - 1, j - 1)] + weight.get(j - 1).copied().unwrap_or(0.0).max(0.01));
            }
            dp[at(i, j)] = best;
        }
    }
    // Back through the table: which spoken words matched.
    let (mut i, mut j) = (n, m);
    let (mut count, mut telling, mut first, mut last) = (0usize, 0usize, usize::MAX, 0usize);
    let filler = |w: f32| w < ORDERED_TELLING_IDF;
    // Whether every word skipped between matches, either side, is filler.
    let mut filler_gaps = true;
    let mut prev: Option<(usize, usize)> = None;
    while i > 0 && j > 0 {
        if speech[i - 1] == Some(verse_stems[j - 1]) && (dp[at(i, j)] - dp[at(i - 1, j - 1)] - weight.get(j - 1).copied().unwrap_or(0.0).max(0.01)).abs() < 1e-4 {
            if let Some((pi, pj)) = prev {
                let speech_gap = speech[i..pi].iter().all(|s| s.is_some_and(|s| filler(lex.idf(s))));
                let verse_gap = weight[j..pj].iter().all(|w| filler(*w));
                filler_gaps &= speech_gap && verse_gap;
            }
            prev = Some((i - 1, j - 1));
            count += 1;
            telling += usize::from(weight.get(j - 1).is_some_and(|w| *w >= ORDERED_TELLING_IDF));
            first = i - 1;
            last = last.max(i - 1);
            i -= 1;
            j -= 1;
        } else if dp[at(i - 1, j)] >= dp[at(i, j - 1)] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    if count < min_words.max(3) || telling < ORDERED_MIN_TELLING || (count as f32) < ((last - first + 1) as f32) * ORDERED_MIN_DENSITY {
        return None;
    }
    Some((dp[at(n, m)], filler_gaps))
}

/// How much of one verse's identifying weight `text` reads in a single unbroken run, in its
/// best-matching wording (0 to 1).
pub fn read_share(lex: &Lexicon, v: VerseId, text: &str) -> f32 {
    let stems: Vec<Option<WordId>> = lex.speech(text).iter().map(|w| w.stem).collect();
    let sources = std::iter::once((lex.kjv().stems(v), lex.kjv().words(v))).chain(lex.others().iter().map(|o| (o.stems(v), o.words(v))));
    let mut best = 0.0f32;
    for (verse_stems, verse_words) in sources {
        let (Some(verse_stems), Some(verse_words)) = (verse_stems, verse_words) else { continue };
        let ends = run_ends(verse_stems, &stems);
        let Some((end, &len)) = ends.iter().enumerate().max_by_key(|(_, l)| **l) else { continue };
        if len == 0 {
            continue;
        }
        let Some(at) = find(verse_stems, &stems[end + 1 - len..=end]) else { continue };
        best = best.max(lex.idf_span(verse_words, at, len) / lex.idf_total(verse_words));
    }
    best
}

/// A score is the share of the *verse* covered, so a short verse that shares a long stretch with a
/// longer one outranks it even when the longer verse explains more of what was actually said
/// ("…should not perish, but have everlasting life": John 3:15 covers most of itself, John 3:16
/// carries 34% more of the speech). Among close results, a verse carrying clearly more identifying
/// weight goes first.
fn prefer_what_was_said(results: &mut [VerbatimHit]) {
    const CLOSE: f32 = 0.25;
    const CLEARLY_MORE: f32 = 1.2;
    let Some(top) = results.first().copied() else { return };
    let Some(top_idf) = top.matched_idf else { return };
    let best = results
        .iter()
        .enumerate()
        .skip(1)
        .take_while(|(_, h)| top.score - h.score <= CLOSE)
        .filter(|(_, h)| h.matched_idf.is_some_and(|m| m >= top_idf * CLEARLY_MORE))
        .max_by(|a, b| a.1.matched_idf.unwrap_or(0.0).total_cmp(&b.1.matched_idf.unwrap_or(0.0)))
        .map(|(i, _)| i);
    if let Some(i) = best {
        results[..=i].rotate_right(1);
    }
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
