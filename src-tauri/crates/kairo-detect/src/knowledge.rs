//! What the engine knows about the Bible, built once and shared read-only:
//! the corpus and its shape, the reference parser's tables, the word
//! matchers' lexicon, which chapters are confusable, and where each proper
//! name occurs.

use std::collections::{HashMap, HashSet};

use kairo_bible::{Book, Corpus, VerseId, VerseRef};

use crate::lexical::{Lexicon, Wordings};
use crate::reference::{Books, Lang, Parser, PartialResolver, Shape};

/// A verse with more twins than this is a formula ("And the LORD spake unto
/// Moses, saying,") — it links chapters that have nothing else in common.
const FORMULA_TWINS: usize = 6;
/// Twin pairs (between non-formula verses) that make two chapters confusable.
const CONFUSABLE_MIN_PAIRS: usize = 2;
/// A word counts as a proper name when at least this share of its mid-sentence
/// occurrences are capitalized.
const PROPER_NOUN_SHARE: f32 = 0.95;

pub type Chapter = (Book, u16);

pub struct Knowledge {
    pub corpus: Corpus,
    pub shape: Shape,
    pub books: &'static Books,
    pub lexicon: Lexicon,
    confusable: HashSet<(Chapter, Chapter)>,
    /// lowercase proper name → chapters it occurs in, and its number of occurrences
    names: HashMap<String, (Vec<Chapter>, u32)>,
}

impl Knowledge {
    pub fn build(corpus: Corpus, wordings: Wordings) -> Knowledge {
        let shape = Shape::new(&corpus);
        let lexicon = Lexicon::build(&corpus, wordings);
        let confusable = confusable_chapters(&corpus, &lexicon);
        let names = proper_names(&corpus);
        Knowledge { corpus, shape, books: Books::get(), lexicon, confusable, names }
    }

    pub fn parser(&self, lang: Lang) -> Parser<'_> {
        Parser { books: self.books, shape: &self.shape, lang }
    }

    pub fn partials(&self, lang: Lang) -> PartialResolver<'_> {
        PartialResolver { books: self.books, shape: &self.shape, lang }
    }

    pub fn id(&self, r: VerseRef) -> Option<VerseId> {
        self.corpus.id(r)
    }

    pub fn verse_ref(&self, id: VerseId) -> VerseRef {
        self.corpus.verse_ref(id)
    }

    /// Two chapters (in either order) that share wording verse to verse.
    pub fn confusable(&self, a: Chapter, b: Chapter) -> bool {
        a != b && self.confusable.contains(&(a.min(b), a.max(b)))
    }

    /// Chapters a proper name (lowercase) occurs in.
    pub fn name_chapters(&self, name: &str) -> Option<&[Chapter]> {
        self.names.get(name).map(|(c, _)| c.as_slice())
    }

    pub fn is_name(&self, word: &str) -> bool {
        self.names.contains_key(word)
    }

    /// The `n` names the Bible mentions most, for steering speech recognition.
    pub fn frequent_names(&self, n: usize) -> Vec<&str> {
        let mut all: Vec<(&String, u32)> = self.names.iter().map(|(k, (_, count))| (k, *count)).collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        all.into_iter().take(n).map(|(k, _)| k.as_str()).collect()
    }
}

fn confusable_chapters(corpus: &Corpus, lex: &Lexicon) -> HashSet<(Chapter, Chapter)> {
    let mut pairs: HashMap<(Chapter, Chapter), usize> = HashMap::new();
    for v in corpus.ids() {
        let twins = lex.twins(v);
        if twins.len() > FORMULA_TWINS {
            continue;
        }
        let a = corpus.verse_ref(v);
        for t in twins {
            if t.verse <= v || lex.twins(t.verse).len() > FORMULA_TWINS {
                continue;
            }
            let b = corpus.verse_ref(t.verse);
            let (ca, cb) = ((a.book, a.chapter), (b.book, b.chapter));
            if ca != cb {
                *pairs.entry((ca.min(cb), ca.max(cb))).or_default() += 1;
            }
        }
    }
    pairs.into_iter().filter(|(_, n)| *n >= CONFUSABLE_MIN_PAIRS).map(|(k, _)| k).collect()
}

/// Words the KJV capitalizes in the middle of a sentence almost every time —
/// names of people and places — minus the divine names and the words every
/// sermon says, which identify no passage.
fn proper_names(corpus: &Corpus) -> HashMap<String, (Vec<Chapter>, u32)> {
    let mut capital: HashMap<String, u32> = HashMap::new();
    let mut lower: HashMap<String, u32> = HashMap::new();
    let mut chapters: HashMap<String, HashSet<Chapter>> = HashMap::new();
    for id in corpus.ids() {
        let r = corpus.verse_ref(id);
        let text = corpus.kjv().text(id).unwrap_or("");
        let mut sentence_start = true;
        for raw in text.split_whitespace() {
            let word: String = raw.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            let ends_sentence = raw.ends_with(['.', '?', '!', ':', ';']);
            if word.len() >= 3 {
                let key = word.to_lowercase();
                // All capitals is "LORD"/"GOD" typography, not a name.
                let is_capital = word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !word.chars().all(|c| c.is_ascii_uppercase());
                if !sentence_start {
                    if is_capital {
                        *capital.entry(key.clone()).or_default() += 1;
                    } else {
                        *lower.entry(key.clone()).or_default() += 1;
                    }
                }
                if is_capital {
                    chapters.entry(key).or_default().insert((r.book, r.chapter));
                }
            }
            sentence_start = ends_sentence;
        }
    }
    const NOT_IDENTIFYING: &[&str] = &[
        "god", "lord", "jesus", "christ", "spirit", "ghost", "holy", "father", "son", "amen", "selah", "israel", "jerusalem",
    ];
    capital
        .into_iter()
        .filter(|(w, c)| {
            let l = lower.get(w).copied().unwrap_or(0);
            *c as f32 / (*c + l) as f32 >= PROPER_NOUN_SHARE && !NOT_IDENTIFYING.contains(&w.as_str())
        })
        .map(|(w, count)| {
            let mut ch: Vec<Chapter> = chapters.remove(&w).unwrap_or_default().into_iter().collect();
            ch.sort();
            (w, (ch, count))
        })
        .collect()
}
