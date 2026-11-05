//! Citations spoken in Spanish, Portuguese, French and German read the same
//! as English ones; ordinary sentences in those languages stay untouched.

use std::path::PathBuf;
use std::sync::LazyLock;

use kairo_bible::Corpus;
use kairo_detect::reference::i18n::localize;
use kairo_detect::reference::{Active, Books, Citation, Lang, Parser, Shape, Verses};

static SHAPE: LazyLock<Shape> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Shape::new(&Corpus::load(&dir).unwrap())
});

fn fmt(c: &Citation) -> String {
    let v = match &c.verses {
        Verses::Chapter => String::new(),
        Verses::One(v) => v.to_string(),
        Verses::Span(a, z) => format!("{a}-{z}"),
        Verses::Spans(s) => format!("{s:?}"),
    };
    format!("{} {}:{v}", c.book, c.chapter)
}

fn refs(text: &str, lang: Lang) -> Vec<String> {
    Parser { books: Books::get(), shape: &SHAPE, lang }.parse_all(text, Active::default()).iter().map(fmt).collect()
}

#[test]
fn citations_in_four_languages() {
    let cases = [
        (Lang::Es, "Juan capítulo tres versículo dieciséis", "John 3:16"),
        (Lang::Es, "primera de Corintios trece, cuatro al siete", "1 Corinthians 13:4-7"),
        (Lang::Es, "la primera carta a los Corintios capítulo trece versículos del cuatro al siete", "1 Corinthians 13:4-7"),
        (Lang::Es, "Salmo ciento diecinueve versículo ciento cinco", "Psalms 119:105"),
        (Lang::Es, "Hechos dos treinta y ocho", "Acts 2:38"),
        (Lang::Es, "Cantar de los Cantares dos versículo cuatro", "Song of Solomon 2:4"),
        (Lang::Es, "Juan 3:16", "John 3:16"),
        (Lang::Es, "en Juan capitulo III versiculo XVI", "John 3:16"),
        (Lang::Es, "Salmos capitulo CXIX versiculo CV", "Psalms 119:105"),
        (Lang::Pt, "João três dezesseis", "John 3:16"),
        (Lang::Pt, "Salmos vinte e três versículo um", "Psalms 23:1"),
        (Lang::Pt, "Salmo cento e dezenove versículo cento e cinco", "Psalms 119:105"),
        (Lang::Fr, "Jean chapitre trois verset seize", "John 3:16"),
        (Lang::Fr, "Psaume quatre-vingt-onze verset un", "Psalms 91:1"),
        (Lang::Fr, "première épître aux Corinthiens treize verset quatre à sept", "1 Corinthians 13:4-7"),
        (Lang::De, "Johannes drei Vers sechzehn", "John 3:16"),
        (Lang::De, "1. Korinther dreizehn, Vers vier bis sieben", "1 Corinthians 13:4-7"),
        (Lang::De, "Psalm hundertneunzehn Vers hundertfünf", "Psalms 119:105"),
        (Lang::De, "erstes Buch Mose Kapitel eins Vers eins", "Genesis 1:1"),
        (Lang::De, "zweiter Korintherbrief fünf Vers siebzehn", "2 Corinthians 5:17"),
    ];
    let mut failed = Vec::new();
    for (lang, text, want) in cases {
        let got = refs(text, lang);
        if got != [want] {
            failed.push(format!("{lang:?} {text:?} → {got:?}, want {want}"));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

#[test]
fn ordinary_sentences_are_not_citations() {
    assert_eq!(refs("había un hombre que tenía dos hijos", Lang::Es), Vec::<String>::new());
    assert_eq!(refs("il a vingt ans et il est venu", Lang::Fr), Vec::<String>::new());
    assert_eq!(refs("Jean a un fils", Lang::Fr), Vec::<String>::new());
    assert_eq!(refs("ein Mann hatte zwei Söhne", Lang::De), Vec::<String>::new());
    assert_eq!(refs("um homem tinha dois filhos", Lang::Pt), Vec::<String>::new());
}

#[test]
fn connectors_change_only_between_numbers() {
    assert_eq!(localize("il a vingt ans", Lang::Fr), "il a 20 ans");
    assert_eq!(localize("la fe y el amor", Lang::Es), "la fe y el amor");
}

#[test]
fn next_and_previous_verse_are_understood() {
    assert!(localize("vamos al siguiente versículo", Lang::Es).contains("next verse"));
    assert!(localize("le verset suivant", Lang::Fr).contains("next verse"));
    assert!(localize("der nächste Vers", Lang::De).contains("next verse"));
    assert!(localize("o versículo anterior", Lang::Pt).contains("previous verse"));
}

#[test]
fn english_passes_through() {
    assert_eq!(localize("John 3:16", Lang::En), "John 3:16");
    assert_eq!(Lang::from_code("en-US"), Lang::En);
    assert_eq!(Lang::from_code("pt-BR"), Lang::Pt);
}
