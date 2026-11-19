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

/// The way preachers really say them, from the recorded Spanish sermons (eval/fixtures/es-*.json).
#[test]
fn citations_as_spanish_sermons_say_them() {
    let cases = [
        // The chapter before the book.
        ("Y el capítulo 8 de romanos, que vamos a continuar estudiando", "Romans 8:"),
        ("los versículos 5 al 11 de romanos 8, que vamos a considerar", "Romans 8:5-11"),
        ("el versículo 5 de romanos 8", "Romans 8:5"),
        ("en el capítulo 5 de la carta a los Romanos", "Romans 5:"),
        // An ordinal written as a figure: 1º, 2º.
        ("Pablo dice en 1º a los Corintios, capítulo 2, versículo 14, que el hombre natural", "1 Corinthians 2:14"),
        ("Pablo dice en 1º los Corintios, capítulo 6, versículo 19", "1 Corinthians 6:19"),
        ("en 2º de Timoteo capítulo 3", "2 Timothy 3:"),
        // Already understood: kept so they stay that way.
        ("en Juan capítulo 3, dice", "John 3:"),
        ("¿Qué dice Pablo en Efesios capítulo 2?", "Ephesians 2:"),
    ];
    let mut failed = Vec::new();
    for (text, want) in cases {
        let got = refs(text, Lang::Es);
        if got != [want] {
            failed.push(format!("{text:?} → {got:?}, want {want}"));
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

/// Lines from the recorded Portuguese, French and German sermons (eval/fixtures/{pt,fr,de}-*.json).
#[test]
fn citations_as_other_sermons_say_them() {
    let cases = [
        (Lang::Fr, "Si je tombe un Jean, un 9, je confesse ma faute", vec!["1 John 1:9"]),
        // Segond numbers the title as verse 1: its 59:2 is the KJV's 59:1.
        (Lang::Fr, "David dit encore dans les psaumes 52, pardon 59, verset 2", vec!["Psalms 59:1"]),
        (Lang::Fr, "Psaume 51 verset 3", vec!["Psalms 51:1"]),
        (Lang::Es, "Salmo 51 versículo 3", vec!["Psalms 51:3"]),
        // German Bibles number Psalm titles and Genesis 32 as the Hebrew does.
        (Lang::De, "Psalm 51 Vers 3", vec!["Psalms 51:1"]),
        (Lang::De, "1. Mose 32, Vers 26", vec!["Genesis 32:25"]),
        (Lang::De, "Psalm 23 Vers 1", vec!["Psalms 23:1"]),
        // Lines from the German test sermons (de-*.json).
        (Lang::De, "Hebräerbrief, Kapitel 11, die Verse 1 bis 22. Das ist unser Ziel heute.", vec!["Hebrews 11:1-22"]),
        (Lang::De, "Und mein Vater, der Weingärtner, Johannes 15 zu 1, Jesus Christus", vec!["John 15:1"]),
        (Lang::De, "Klagelieder 3 zu 33.", vec!["Lamentations 3:33"]),
        (Lang::De, "In Hos. 11, 8 hören wir den Herrn", vec!["Hosea 11:8"]),
        (Lang::Es, "¿Se dan cuenta el contraste?", vec![]),
        (Lang::Pt, "os versículos 3 e 4 de romanos 8", vec!["Romans 8:3-4"]),
        (Lang::Pt, "2º, toda a pendência que aquela pessoa tinha. 3º, o filho adotivo", vec![]),
        (Lang::Es, "¿Se dan cuenta el contraste? En el versículo 6", vec![]),
        (Lang::Es, "en 2º de Timoteo capítulo 3", vec!["2 Timothy 3:"]),
        (Lang::Pt, "o fato glorioso de romanos 8 versículo 1º", vec!["Romans 8:1"]),
        (Lang::De, "Römer 8 ab Vers 18", vec!["Romans 8:18"]),
    ];
    let mut failed = Vec::new();
    for (lang, text, want) in cases {
        let got = refs(text, lang);
        if got != want {
            failed.push(format!("{lang:?} {text:?} → {got:?}, want {want:?}"));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}
