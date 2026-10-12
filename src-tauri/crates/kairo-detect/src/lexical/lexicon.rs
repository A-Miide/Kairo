//! Everything the word matchers know about the Bible, built once from the
//! corpus and shared read-only.
//!
//! Words are interned as integer ids; every verse is an id array in each
//! wording (normalized words and their stems). On top of that:
//! - which verses use each word, and each stem;
//! - how identifying each word is (IDF);
//! - each verse's fingerprint: its most identifying words;
//! - every distinctive 4-word phrase (stems) and where it occurs — the
//!   streaming matcher's anchors;
//! - near-duplicate verse pairs ("twins").
//!
//! Wordings: the KJV always; the church's own translation (or the service
//! language's Bible) if it has one; and modern translations matched for every
//! English service. Only the KJV and the church's own wording feed the
//! anchors — everyday modern phrasing fires on ordinary preaching. Word
//! weights and fingerprints come from the KJV and the first other wording
//! (the church's own, else the first matched for everyone): a third wording's
//! everyday words would dilute them.

use std::collections::HashMap;

use kairo_bible::{Corpus, Translation, VerseId};

use super::stem::heal;
use super::text::{is_stop_word, norm};

pub type WordId = u32;

/// A 4-gram of stems.
pub type Gram = [WordId; 4];

/// Where a kept 4-gram occurs: a verse in the KJV or the church's own wording, and the word position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GramEntry {
    pub verse: VerseId,
    /// In the church's own wording rather than the KJV.
    pub own: bool,
    pub pos: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Twin {
    pub verse: VerseId,
    /// Distinctive 4-word phrases the two verses share.
    pub shared: u32,
}

/// A 4-gram is an anchor if it occurs in at most this many verses.
pub const ANCHOR_DF_MAX: u8 = 5;
/// Two verses are twins if they share at least `TWIN_MIN_SHARED` 4-grams
/// that each occur in no more than `TWIN_GRAM_DF_MAX` verses.
const TWIN_GRAM_DF_MAX: u16 = 6;
const TWIN_MIN_SHARED: u32 = 2;
/// Words a fingerprint holds, at most, and the least weight they need.
const SIGNATURE_SIZE: usize = 10;
const SIGNATURE_IDF_FLOOR: f32 = 1.5;

#[derive(Default)]
pub struct Vocab {
    ids: HashMap<String, WordId>,
    words: Vec<String>,
}

impl Vocab {
    fn intern(&mut self, w: &str) -> WordId {
        if let Some(id) = self.ids.get(w) {
            return *id;
        }
        let id = self.words.len() as WordId;
        self.words.push(w.to_string());
        self.ids.insert(w.to_string(), id);
        id
    }

    pub fn id(&self, w: &str) -> Option<WordId> {
        self.ids.get(w).copied()
    }

    pub fn word(&self, id: WordId) -> &str {
        &self.words[id as usize]
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// One wording's words and stems, per verse.
pub struct Wording {
    pub code: String,
    norm: Vec<Option<Box<[WordId]>>>,
    stem: Vec<Option<Box<[WordId]>>>,
    /// As written, for the reranker.
    text: Translation,
}

impl Wording {
    pub fn words(&self, v: VerseId) -> Option<&[WordId]> {
        self.norm[v.index()].as_deref()
    }

    pub fn stems(&self, v: VerseId) -> Option<&[WordId]> {
        self.stem[v.index()].as_deref()
    }

    pub fn text(&self, v: VerseId) -> Option<&str> {
        self.text.text(v)
    }
}

/// Which translations the word matchers read, besides the KJV.
pub struct Wordings {
    /// The church's own translation or the service language's Bible.
    pub own: Option<Translation>,
    /// Matched for every service of that language, in order.
    pub also: Vec<Translation>,
}

pub struct Lexicon {
    vocab: Vocab,
    kjv: Wording,
    /// The church's own (if any) first, then those matched for everyone.
    others: Vec<Wording>,
    has_own: bool,
    by_word: HashMap<WordId, Vec<VerseId>>,
    by_stem: HashMap<WordId, Vec<VerseId>>,
    idf: Vec<f32>,
    signatures: Vec<Box<[(WordId, f32)]>>,
    signature_weight: Vec<f32>,
    grams: HashMap<Gram, u32>,
    gram_df: Vec<u8>,
    gram_start: Vec<u32>,
    gram_entries: Vec<GramEntry>,
    twins: HashMap<VerseId, Vec<Twin>>,
}

impl Lexicon {
    pub fn build(corpus: &Corpus, wordings: Wordings) -> Lexicon {
        let mut vocab = Vocab::default();
        let wording = |t: Translation, vocab: &mut Vocab| -> Wording {
            let mut norm_ids = Vec::with_capacity(corpus.len());
            let mut stem_ids = Vec::with_capacity(corpus.len());
            let mut stems: HashMap<WordId, WordId> = HashMap::new();
            for id in corpus.ids() {
                match t.text(id) {
                    Some(text) => {
                        let words: Vec<WordId> = norm(text).split(' ').filter(|w| !w.is_empty()).map(|w| vocab.intern(w)).collect();
                        let st: Vec<WordId> = words
                            .iter()
                            .map(|w| *stems.entry(*w).or_insert_with(|| {
                                let s = heal(vocab.word(*w));
                                vocab.intern(&s)
                            }))
                            .collect();
                        norm_ids.push(Some(words.into_boxed_slice()));
                        stem_ids.push(Some(st.into_boxed_slice()));
                    }
                    None => {
                        norm_ids.push(None);
                        stem_ids.push(None);
                    }
                }
            }
            Wording { code: t.code.clone(), norm: norm_ids, stem: stem_ids, text: t }
        };
        let kjv = wording(corpus.kjv().clone(), &mut vocab);
        let has_own = wordings.own.is_some();
        let mut others = Vec::new();
        for t in wordings.own.into_iter().chain(wordings.also) {
            if t.code != "KJV" && !others.iter().any(|w: &Wording| w.code == t.code) {
                others.push(wording(t, &mut vocab));
            }
        }

        let n = corpus.len();
        let long_word = |vocab: &Vocab, w: WordId| vocab.word(w).len() >= 3;

        // Which verses use each word (any wording), and each stem.
        let mut by_word: HashMap<WordId, Vec<VerseId>> = HashMap::new();
        for v in corpus.ids() {
            let mut seen: Vec<WordId> = Vec::new();
            for words in std::iter::once(kjv.words(v)).chain(others.iter().map(|o| o.words(v))).flatten() {
                for &w in words {
                    if long_word(&vocab, w) && !seen.contains(&w) {
                        seen.push(w);
                        by_word.entry(w).or_default().push(v);
                    }
                }
            }
        }
        let mut by_stem: HashMap<WordId, Vec<VerseId>> = HashMap::new();
        for (&w, verses) in &by_word {
            let s = vocab.id(&heal(vocab.word(w))).expect("stems are interned with their words");
            if s == w {
                continue;
            }
            let bucket = by_stem.entry(s).or_default();
            bucket.extend_from_slice(verses);
        }
        for bucket in by_stem.values_mut() {
            bucket.sort_unstable();
            bucket.dedup();
        }

        // How identifying each word is, over the KJV and the first other wording.
        let first_other = others.first();
        let mut base_df: HashMap<WordId, u32> = HashMap::new();
        for v in corpus.ids() {
            let mut seen: Vec<WordId> = Vec::new();
            for words in [kjv.words(v), first_other.and_then(|o| o.words(v))].into_iter().flatten() {
                for &w in words {
                    if long_word(&vocab, w) && !seen.contains(&w) {
                        seen.push(w);
                        *base_df.entry(w).or_default() += 1;
                    }
                }
            }
        }
        let mut idf = vec![0f32; vocab.len()];
        for (&w, verses) in &by_word {
            let df = base_df.get(&w).copied().unwrap_or(verses.len() as u32) as f64;
            idf[w as usize] = ((n as f64 - df + 0.5) / (df + 0.5) + 1.0).ln() as f32;
        }

        // Fingerprints: each verse's most identifying words.
        let mut signatures = Vec::with_capacity(n);
        let mut signature_weight = Vec::with_capacity(n);
        for v in corpus.ids() {
            let mut words: Vec<WordId> = Vec::new();
            for list in [kjv.words(v), first_other.and_then(|o| o.words(v))].into_iter().flatten() {
                for &w in list {
                    let s = vocab.word(w);
                    if s.len() >= 4 && !is_stop_word(s) && !words.contains(&w) {
                        words.push(w);
                    }
                }
            }
            let mut top: Vec<(WordId, f32)> = words.into_iter().map(|w| (w, idf[w as usize])).filter(|(_, i)| *i >= SIGNATURE_IDF_FLOOR).collect();
            top.sort_by(|a, b| b.1.total_cmp(&a.1));
            top.truncate(SIGNATURE_SIZE);
            signature_weight.push(top.iter().map(|(_, i)| i).sum());
            signatures.push(top.into_boxed_slice());
        }

        let mut lex = Lexicon {
            vocab,
            kjv,
            others,
            has_own,
            by_word,
            by_stem,
            idf,
            signatures,
            signature_weight,
            grams: HashMap::new(),
            gram_df: Vec::new(),
            gram_start: Vec::new(),
            gram_entries: Vec::new(),
            twins: HashMap::new(),
        };
        lex.build_grams(corpus);
        lex
    }

    /// The anchor index and the twin table, from 4-grams of stems in the KJV
    /// and the church's own wording. A verse's two wordings count once toward
    /// how many verses share a phrase.
    fn build_grams(&mut self, corpus: &Corpus) {
        let grams_of = |s: &[WordId]| -> Vec<Gram> { s.windows(4).map(|w| [w[0], w[1], w[2], w[3]]).collect() };
        // Every slot (a verse in the KJV, then in the church's own wording) and its phrases.
        let mut slots: Vec<(VerseId, bool, Vec<Gram>)> = corpus.ids().map(|v| (v, false, grams_of(self.slot_stems(v, false)))).collect();
        let kjv_grams: Vec<Vec<Gram>> = slots.iter().map(|(_, _, g)| g.clone()).collect();
        if self.own().is_some() {
            for v in corpus.ids() {
                if self.own().and_then(|o| o.stems(v)).is_some() {
                    slots.push((v, true, grams_of(self.slot_stems(v, true))));
                }
            }
        }

        let mut df: HashMap<Gram, u16> = HashMap::new();
        for (v, own, grams) in &slots {
            let mut counted: Vec<Gram> = if *own { kjv_grams[v.index()].clone() } else { Vec::new() };
            for &g in grams {
                if !counted.contains(&g) {
                    counted.push(g);
                    *df.entry(g).or_default() += 1;
                }
            }
        }

        // Twins: pairs of verses sharing several distinctive-but-shared phrases.
        let mut shared_by: HashMap<Gram, Vec<VerseId>> = HashMap::new();
        let mut gram_order: Vec<Gram> = Vec::new();
        for (v, _, grams) in &slots {
            let v = *v;
            for &g in grams {
                let d = df[&g];
                if !(2..=TWIN_GRAM_DF_MAX).contains(&d) {
                    continue;
                }
                let list = shared_by.entry(g).or_insert_with(|| {
                    gram_order.push(g);
                    Vec::new()
                });
                if !list.contains(&v) {
                    list.push(v);
                }
            }
        }
        let mut pair_count: HashMap<(VerseId, VerseId), u32> = HashMap::new();
        let mut pair_order: Vec<(VerseId, VerseId)> = Vec::new();
        for g in &gram_order {
            let ids = &shared_by[g];
            for x in 0..ids.len() {
                for y in x + 1..ids.len() {
                    let key = (ids[x].min(ids[y]), ids[x].max(ids[y]));
                    let c = pair_count.entry(key).or_insert_with(|| {
                        pair_order.push(key);
                        0
                    });
                    *c += 1;
                }
            }
        }
        // Verses worded identically are twins however many share the wording
        // (a refrain repeated seven times has no phrase in "2 to 6" verses).
        let mut by_wording: HashMap<&[WordId], Vec<VerseId>> = HashMap::new();
        for v in corpus.ids() {
            let stems = self.kjv.stems(v).unwrap_or(&[]);
            if stems.len() >= 4 {
                by_wording.entry(stems).or_default().push(v);
            }
        }
        let mut identical: Vec<((VerseId, VerseId), u32)> = Vec::new();
        for (stems, group) in by_wording {
            for x in 0..group.len() {
                for y in x + 1..group.len() {
                    identical.push(((group[x], group[y]), (stems.len() - 3) as u32));
                }
            }
        }
        identical.sort();
        for (key, shared) in identical {
            let c = pair_count.entry(key).or_insert_with(|| {
                pair_order.push(key);
                0
            });
            *c = (*c).max(shared).max(TWIN_MIN_SHARED);
        }

        for key in pair_order {
            let shared = pair_count[&key];
            if shared < TWIN_MIN_SHARED {
                continue;
            }
            self.twins.entry(key.0).or_default().push(Twin { verse: key.1, shared });
            self.twins.entry(key.1).or_default().push(Twin { verse: key.0, shared });
        }

        // Anchors: kept 4-grams and their entries, in slot then position order.
        let mut counts: Vec<u32> = Vec::new();
        for (_, _, grams) in &slots {
            for &g in grams {
                if df[&g] as u32 > ANCHOR_DF_MAX as u32 {
                    continue;
                }
                let next = self.grams.len() as u32;
                let id = *self.grams.entry(g).or_insert(next);
                if id as usize == counts.len() {
                    counts.push(0);
                    self.gram_df.push(df[&g] as u8);
                }
                counts[id as usize] += 1;
            }
        }
        self.gram_start = Vec::with_capacity(counts.len() + 1);
        let mut acc = 0u32;
        for c in &counts {
            self.gram_start.push(acc);
            acc += c;
        }
        self.gram_start.push(acc);
        let mut fill = self.gram_start.clone();
        self.gram_entries = vec![GramEntry { verse: VerseId(0), own: false, pos: 0 }; acc as usize];
        for (v, own, grams) in &slots {
            for (pos, g) in grams.iter().enumerate() {
                if let Some(&id) = self.grams.get(g) {
                    let at = &mut fill[id as usize];
                    self.gram_entries[*at as usize] = GramEntry { verse: *v, own: *own, pos: pos as u16 };
                    *at += 1;
                }
            }
        }
    }

    pub fn vocab(&self) -> &Vocab {
        &self.vocab
    }

    pub fn kjv(&self) -> &Wording {
        &self.kjv
    }

    /// Every other wording, the church's own first.
    pub fn others(&self) -> &[Wording] {
        &self.others
    }

    pub fn own(&self) -> Option<&Wording> {
        if self.has_own { self.others.first() } else { None }
    }

    /// The one other wording that weighs words and counts as a verse's own
    /// wording in evidence: the church's own, else the first matched for everyone.
    pub fn first_other(&self) -> Option<&Wording> {
        self.others.first()
    }

    /// A verse's words in the KJV or the church's own wording.
    pub fn slot_words(&self, v: VerseId, own: bool) -> &[WordId] {
        if own { self.own().and_then(|o| o.words(v)).unwrap_or(&[]) } else { self.kjv.words(v).unwrap_or(&[]) }
    }

    pub fn slot_stems(&self, v: VerseId, own: bool) -> &[WordId] {
        if own { self.own().and_then(|o| o.stems(v)).unwrap_or(&[]) } else { self.kjv.stems(v).unwrap_or(&[]) }
    }

    pub fn idf(&self, w: WordId) -> f32 {
        self.idf.get(w as usize).copied().unwrap_or(0.0)
    }

    pub fn idf_of(&self, word: &str) -> f32 {
        self.vocab.id(word).map_or(0.0, |w| self.idf(w))
    }

    pub fn verses_with_word(&self, w: WordId) -> &[VerseId] {
        self.by_word.get(&w).map_or(&[], Vec::as_slice)
    }

    pub fn verses_with_stem(&self, s: WordId) -> &[VerseId] {
        self.by_stem.get(&s).map_or(&[], Vec::as_slice)
    }

    pub fn signature(&self, v: VerseId) -> &[(WordId, f32)] {
        &self.signatures[v.index()]
    }

    /// Verses in the Bible (every per-verse table has this many rows).
    pub fn verse_count(&self) -> usize {
        self.signature_weight.len()
    }

    pub fn signature_weight(&self, v: VerseId) -> f32 {
        self.signature_weight[v.index()]
    }

    pub fn anchor(&self, g: &Gram) -> Option<u32> {
        self.grams.get(g).copied()
    }

    pub fn anchor_df(&self, gram: u32) -> u8 {
        self.gram_df[gram as usize]
    }

    pub fn anchor_entries(&self, gram: u32) -> &[GramEntry] {
        &self.gram_entries[self.gram_start[gram as usize] as usize..self.gram_start[gram as usize + 1] as usize]
    }

    pub fn anchor_count(&self) -> usize {
        self.gram_df.len()
    }

    pub fn twins(&self, v: VerseId) -> &[Twin] {
        self.twins.get(&v).map_or(&[], Vec::as_slice)
    }

    pub fn verses_with_twins(&self) -> usize {
        self.twins.len()
    }

    /// Sum of IDF over `len` words starting at `start`.
    pub fn idf_span(&self, words: &[WordId], start: usize, len: usize) -> f32 {
        words.iter().skip(start).take(len).map(|w| self.idf(*w)).sum()
    }

    /// Sum of IDF over all of `words` (1 if they weigh nothing).
    pub fn idf_total(&self, words: &[WordId]) -> f32 {
        let s: f32 = words.iter().map(|w| self.idf(*w)).sum();
        if s > 0.0 { s } else { 1.0 }
    }

    /// A transcript's words as ids, and their stems as ids (`None` for words
    /// the Bible doesn't use — they can't match anything).
    pub fn speech(&self, text: &str) -> Vec<SpeechWord> {
        norm(text)
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let stem = heal(w);
                SpeechWord { word: self.vocab.id(w), stem: self.vocab.id(&stem), text: w.to_string(), stem_text: stem }
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct SpeechWord {
    pub word: Option<WordId>,
    pub stem: Option<WordId>,
    pub text: String,
    pub stem_text: String,
}
