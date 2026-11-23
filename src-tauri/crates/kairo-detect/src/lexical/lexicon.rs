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
    /// Which wording: 0 the KJV, then `others` in order (the church's own first).
    pub slot: u8,
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
/// Look-alikes (`build_lookalikes`): windows of this many stems, one left out, and how many rare
/// ones two verses share. One or two are everyday phrasing (sampled by hand, 23 Nov 2026).
const LOOKALIKE_RUN: usize = 5;
const LOOKALIKE_MIN_WINDOWS: u32 = 3;
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
    lookalikes: HashMap<VerseId, Vec<VerseId>>,
}

impl Lexicon {
    pub fn build(corpus: &Corpus, wordings: Wordings) -> Lexicon {
        Lexicon::build_with(corpus, wordings, true)
    }

    /// As `build`; without `kjv_anchors` only the church's own wording feeds the anchors and twins. A
    /// service in another language never says the KJV's English phrases, and indexing them beside the
    /// language's own took ~80 MB more than an English service (replay, 2026-11-05).
    pub fn build_with(corpus: &Corpus, wordings: Wordings, kjv_anchors: bool) -> Lexicon {
        let kjv_anchors = kjv_anchors || wordings.own.is_none();
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
            lookalikes: HashMap::new(),
        };
        lex.build_grams(corpus, kjv_anchors);
        lex
    }

    /// Look-alikes in every wording Kairo matches (the KJV, the church's own, and the Bibles matched
    /// for everyone), with a word allowed to differ: verses sharing LOOKALIKE_MIN_WINDOWS rare windows
    /// of LOOKALIKE_RUN stems, each with one stem left out (a stretch of about seven words). Exodus
    /// 34:6 and Psalm 103:8 in the BSB ("compassionate and gracious, slow to anger, abounding in
    /// loving devotion"); Genesis 36 and 1 Chronicles 1's lists with a name spelled differently.
    /// Apart from the twin table on purpose: twins also correct a citation to its twin, and as twins
    /// Psalm 1:3 "corrected" Jeremiah 17:8 being read (replay). Look-alikes only hold a verse back
    /// while a passage it looks like is read (decide.rs). Measured with read-passage (23 Nov 2026).
    fn build_lookalikes(&mut self, corpus: &Corpus) {
        use std::hash::{Hash, Hasher};
        const INDEX_BITS: u32 = 15;
        debug_assert!(corpus.len() < 1 << INDEX_BITS);
        let mut windows_shared: HashMap<(VerseId, VerseId), u32> = HashMap::new();
        for w in std::iter::once(&self.kjv).chain(self.others.iter()) {
            // One u64 a window: its hash with the hole, then the verse. Sorted, equal windows sit together.
            let mut keys: Vec<u64> = Vec::new();
            for v in corpus.ids() {
                let Some(stems) = w.stems(v) else { continue };
                for win in stems.windows(LOOKALIKE_RUN) {
                    if win.iter().filter(|x| !is_stop_word(self.vocab.word(**x))).count() < 2 {
                        continue;
                    }
                    for hole in 0..LOOKALIKE_RUN {
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        hole.hash(&mut h);
                        win.iter().enumerate().filter(|(j, _)| *j != hole).for_each(|(_, x)| x.hash(&mut h));
                        keys.push((h.finish() << INDEX_BITS) | v.0 as u64);
                    }
                }
            }
            keys.sort_unstable();
            keys.dedup();
            let mut here: HashMap<(VerseId, VerseId), u32> = HashMap::new();
            for group in keys.chunk_by(|a, b| a >> INDEX_BITS == b >> INDEX_BITS) {
                if group.len() < 2 || group.len() > TWIN_GRAM_DF_MAX as usize {
                    continue;
                }
                let ids: Vec<VerseId> = group.iter().map(|k| VerseId((k & ((1 << INDEX_BITS) - 1)) as u32)).collect();
                for x in 0..ids.len() {
                    for y in x + 1..ids.len() {
                        *here.entry((ids[x].min(ids[y]), ids[x].max(ids[y]))).or_default() += 1;
                    }
                }
            }
            for (pair, n) in here {
                let e = windows_shared.entry(pair).or_default();
                *e = (*e).max(n);
            }
        }
        let mut pairs: Vec<(VerseId, VerseId)> = windows_shared.into_iter().filter(|(_, n)| *n >= LOOKALIKE_MIN_WINDOWS).map(|(p, _)| p).collect();
        pairs.sort_unstable();
        for (a, b) in pairs {
            // A verse and the next one repeating it is a passage's own parallelism, not a look-alike.
            if b.0 == a.0 + 1 {
                continue;
            }
            self.lookalikes.entry(a).or_default().push(b);
            self.lookalikes.entry(b).or_default().push(a);
        }
    }

    /// The twin table, from 4-grams of stems in the KJV and the church's own wording, and the anchor
    /// index, from every wording Kairo matches: a quote in the BSB, NIV or NLT is anchored as it's said,
    /// not found at the end of the sentence (read-aloud, 23 Nov 2026: modern versions went up at 88-96%
    /// of the verse, the KJV at 38%). A verse's wordings count once toward how many verses share a phrase.
    fn build_grams(&mut self, corpus: &Corpus, kjv_anchors: bool) {
        let grams_of = |s: &[WordId]| -> Vec<Gram> { s.windows(4).map(|w| [w[0], w[1], w[2], w[3]]).collect() };
        // Every slot (a verse in the KJV, then in the church's own wording) and its phrases.
        let mut slots: Vec<(VerseId, u8, Vec<Gram>)> =
            if kjv_anchors { corpus.ids().map(|v| (v, 0, grams_of(self.slot_stems(v, 0)))).collect() } else { Vec::new() };
        let kjv_grams: Vec<Vec<Gram>> = if kjv_anchors { slots.iter().map(|(_, _, g)| g.clone()).collect() } else { vec![Vec::new(); corpus.len()] };
        if self.own().is_some() {
            for v in corpus.ids() {
                if self.own().and_then(|o| o.stems(v)).is_some() {
                    slots.push((v, 1, grams_of(self.slot_stems(v, 1))));
                }
            }
        }

        let mut df: HashMap<Gram, u16> = HashMap::new();
        for (v, slot, grams) in &slots {
            let mut counted: Vec<Gram> = if *slot == 1 { kjv_grams[v.index()].clone() } else { Vec::new() };
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
        // In the KJV, and in the church's own wording: a refrain a Bible repeats word for word.
        let mut identical: Vec<((VerseId, VerseId), u32)> = Vec::new();
        for own in [false, true] {
            if own && self.own().is_none() {
                continue;
            }
            let mut by_wording: HashMap<&[WordId], Vec<VerseId>> = HashMap::new();
            for v in corpus.ids() {
                let stems = if own { self.own().and_then(|o| o.stems(v)).unwrap_or(&[]) } else { self.kjv.stems(v).unwrap_or(&[]) };
                if stems.len() >= 4 {
                    by_wording.entry(stems).or_default().push(v);
                }
            }
            for (stems, group) in by_wording {
                for x in 0..group.len() {
                    for y in x + 1..group.len() {
                        identical.push(((group[x], group[y]), (stems.len() - 3) as u32));
                    }
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
        self.build_lookalikes(corpus);

        // Anchors from every other wording Kairo matches too (the BSB, the NIV…), in the slots after the
        // church's own. How many verses share a phrase counts each verse once, whatever its wordings.
        if kjv_anchors {
            let first_also = if self.has_own { 1 } else { 0 };
            for i in first_also..self.others.len() {
                let slot = (i + 1) as u8;
                for v in corpus.ids() {
                    if self.others[i].stems(v).is_some() {
                        slots.push((v, slot, grams_of(self.slot_stems(v, slot))));
                    }
                }
            }
        }
        let mut per_verse: Vec<Vec<Gram>> = vec![Vec::new(); corpus.len()];
        for (v, _, grams) in &slots {
            per_verse[v.index()].extend_from_slice(grams);
        }
        let mut df: HashMap<Gram, u16> = HashMap::new();
        for mut grams in per_verse {
            grams.sort_unstable();
            grams.dedup();
            for g in grams {
                *df.entry(g).or_default() += 1;
            }
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
        self.gram_entries = vec![GramEntry { verse: VerseId(0), slot: 0, pos: 0 }; acc as usize];
        for (v, slot, grams) in &slots {
            for (pos, g) in grams.iter().enumerate() {
                if let Some(&id) = self.grams.get(g) {
                    let at = &mut fill[id as usize];
                    self.gram_entries[*at as usize] = GramEntry { verse: *v, slot: *slot, pos: pos as u16 };
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
    /// A verse's words in a slot: 0 the KJV, then `others` in order.
    pub fn slot_words(&self, v: VerseId, slot: u8) -> &[WordId] {
        let w = if slot == 0 { Some(&self.kjv) } else { self.others.get(slot as usize - 1) };
        w.and_then(|w| w.words(v)).unwrap_or(&[])
    }

    pub fn slot_stems(&self, v: VerseId, slot: u8) -> &[WordId] {
        let w = if slot == 0 { Some(&self.kjv) } else { self.others.get(slot as usize - 1) };
        w.and_then(|w| w.stems(v)).unwrap_or(&[])
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

    /// Verses that look like `v` in some wording Kairo matches, a word allowed to differ (`build_lookalikes`).
    pub fn lookalikes(&self, v: VerseId) -> &[VerseId] {
        self.lookalikes.get(&v).map_or(&[], Vec::as_slice)
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
