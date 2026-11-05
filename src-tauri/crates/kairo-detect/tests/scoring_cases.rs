//! The scorer's reasoning, case by case: continuity, catch-up reads, cold
//! starts, momentum, self-sufficient evidence, ranges holding, confusable
//! passages, meaning-only ceilings, corroboration.

use kairo_bible::{Book, VerseRef};
use kairo_detect::engine::scoring::{calibrate, distance, score, target, Ledger, ScoreContext, VIEWER_MIN_SCORE};
use kairo_detect::engine::{Evidence, Method, OnScreen, Target};
use kairo_detect::lexical::Confidence;

const T0: u64 = 1_000_000;

fn v(book: &str, c: u16, n: u16) -> VerseRef {
    VerseRef::new(Book::from_name(book).unwrap(), c, n)
}
fn up(book: &str, c: u16, n: u16, at: u64) -> Option<OnScreen> {
    Some(OnScreen { verse: v(book, c, n), at_ms: at })
}
fn verbatim(sim: f32, idf: f32) -> Evidence {
    Evidence { similarity: Some(sim), matched_idf: Some(idf), ..Default::default() }
}
fn stream(matched: u32) -> Evidence {
    Evidence { confirmed: true, matched: Some(matched), ..Default::default() }
}
fn anchor(df: u8, idf: f32, sim: f32) -> Evidence {
    Evidence { df: Some(df), idf: Some(idf), similarity: Some(sim), ..Default::default() }
}
fn ctx(now: u64, on_screen: Option<OnScreen>) -> ScoreContext {
    ScoreContext { now_ms: now, on_screen, ..Default::default() }
}
fn sends(s: f32) -> bool {
    s >= VIEWER_MIN_SCORE
}

#[test]
fn forward_reading_in_the_chapter_on_screen_clears_the_bar() {
    let s = score(v("Ezekiel", 47, 2), Method::Verbatim, &verbatim(0.80, 10.0), &ctx(T0 + 3000, up("Ezekiel", 47, 1, T0)), &Ledger::default());
    assert!(sends(s.total), "{s:?}");
}

#[test]
fn an_unshown_earlier_verse_is_a_catch_up_read() {
    assert_eq!(distance(v("Matthew", 15, 24), T0 + 5000, up("Matthew", 15, 28, T0), false), 0.0);
    let s = score(v("Matthew", 15, 24), Method::Verbatim, &verbatim(0.82, 11.0), &ctx(T0 + 5000, up("Matthew", 15, 28, T0)), &Ledger::default());
    assert!(sends(s.total));
    assert!(distance(v("Matthew", 15, 23), T0 + 2000, up("Matthew", 15, 28, T0), true) < 0.0, "an already-shown verse echoed is penalised");
}

#[test]
fn two_methods_agreeing_clear_the_bar() {
    let mut l = Ledger::default();
    l.record(v("Jeremiah", 30, 17), Method::Stream, T0, true);
    l.record(v("Jeremiah", 30, 17), Method::Verbatim, T0 + 500, true);
    let s = score(v("Jeremiah", 30, 17), Method::Verbatim, &verbatim(0.78, 9.0), &ctx(T0 + 600, None), &l);
    assert!(sends(s.total), "{s:?}");
}

#[test]
fn cold_start_weak_evidence_never_sends_alone() {
    let l = Ledger::default();
    assert!(!sends(score(v("Esther", 6, 1), Method::DirectPartial, &Evidence::default(), &ctx(T0, None), &l).total));
    assert!(!sends(score(v("Acts", 3, 6), Method::Stream, &Evidence::default(), &ctx(T0, None), &l).total));
    let joshua = up("Joshua", 1, 8, T0 - 5000);
    assert!(!sends(score(v("Esther", 6, 1), Method::DirectPartial, &Evidence::default(), &ctx(T0, joshua), &l).total));
}

#[test]
fn momentum_lets_a_passage_being_read_through() {
    let mut l = Ledger::default();
    l.record(v("Psalms", 110, 1), Method::Stream, T0 + 1000, true);
    l.record(v("Psalms", 110, 2), Method::Stream, T0 + 2000, true);
    assert!(sends(score(v("Psalms", 110, 2), Method::Stream, &stream(8), &ctx(T0 + 2100, up("Psalms", 111, 1, T0)), &l).total));

    let mut l = Ledger::default();
    for (n, t) in [(1, 1000), (2, 2000), (3, 3000)] {
        l.record(v("Psalms", 1, n), Method::Stream, T0 + t, true);
    }
    assert!(sends(score(v("Psalms", 1, 3), Method::Stream, &stream(7), &ctx(T0 + 3050, up("Joshua", 1, 8, T0)), &l).total));

    let mut l = Ledger::default();
    l.record(v("Psalms", 1, 1), Method::Stream, T0 + 1000, true);
    let s = score(v("Psalms", 1, 1), Method::Stream, &stream(6), &ctx(T0 + 1050, up("Joshua", 1, 8, T0)), &l);
    assert_eq!(s.a, 0.0, "a single hit earns no momentum");
}

#[test]
fn citations_send_alone_even_for_a_new_book() {
    let l = Ledger::default();
    let s = score(v("John", 3, 16), Method::Direct, &Evidence::default(), &ctx(T0, None), &l);
    assert!(sends(s.total));
    assert_eq!(target(s.total, Method::Direct, false, s.very_high), Target::Screen);
    let s = score(v("Genesis", 24, 3), Method::Direct, &Evidence::default(), &ctx(T0, up("Joshua", 1, 8, T0 - 5000)), &l);
    assert!(sends(s.total));
    assert_eq!(s.d, 0.0, "a citation isn't weighed against distance");
}

#[test]
fn self_sufficient_text_evidence_skips_distance() {
    let l = Ledger::default();
    let s = score(v("Genesis", 26, 14), Method::Stream, &stream(6), &ctx(T0, up("Psalms", 1, 3, T0 - 5000)), &l);
    assert!(sends(s.total));
    assert_eq!(s.d, 0.0);
    let weak = Evidence { df: Some(5), ..Default::default() };
    let s = score(v("Genesis", 26, 14), Method::Stream, &weak, &ctx(T0, up("Psalms", 1, 3, T0 - 5000)), &l);
    assert!(s.d != 0.0 || !sends(s.total));
}

#[test]
fn unconfirmed_anchors_are_graded_by_uniqueness_and_weight() {
    let l = Ledger::default();
    let s = score(v("Genesis", 26, 14), Method::Stream, &anchor(1, 19.0, 0.85), &ctx(T0, None), &l);
    assert!(sends(s.total) && s.b >= VIEWER_MIN_SCORE);
    assert!(!sends(score(v("Isaiah", 1, 18), Method::Stream, &anchor(1, 8.8, 0.85), &ctx(T0, None), &l).total));
    let s = score(v("Genesis", 26, 14), Method::Stream, &anchor(1, 13.7, 0.55), &ctx(T0, None), &l);
    assert_eq!(target(s.total, Method::Stream, false, s.very_high), Target::Suggestions);
    assert!(!sends(score(v("Genesis", 26, 14), Method::Stream, &anchor(3, 20.0, 0.76), &ctx(T0, None), &l).total), "a shared phrase never sends alone");
}

#[test]
fn a_named_person_helps_only_alongside_other_evidence() {
    let e = anchor(1, 13.7, 0.55);
    let named = ScoreContext { named_person_in_chapter: true, ..ctx(T0, None) };
    assert!(!sends(score(v("Genesis", 26, 14), Method::Stream, &e, &named, &Ledger::default()).total));
    let mut l = Ledger::default();
    l.record(v("Genesis", 26, 14), Method::Verbatim, T0 - 3000, true);
    l.record(v("Genesis", 26, 14), Method::Fingerprint, T0 - 2000, true);
    assert!(!sends(score(v("Genesis", 26, 14), Method::Stream, &e, &ctx(T0, None), &l).total));
    assert!(sends(score(v("Genesis", 26, 14), Method::Stream, &e, &named, &l).total));
}

#[test]
fn a_short_real_excerpt_is_offered_a_weak_one_dropped() {
    let l = Ledger::default();
    let s = score(v("Isaiah", 1, 18), Method::Verbatim, &verbatim(0.373, 12.7), &ctx(T0, None), &l);
    assert_eq!(target(s.total, Method::Verbatim, false, false), Target::Suggestions);
    let s = score(v("Romans", 8, 1), Method::Verbatim, &verbatim(0.30, 4.5), &ctx(T0, None), &l);
    assert_eq!(target(s.total, Method::Verbatim, false, false), Target::Drop);
}

#[test]
fn a_cited_range_holds_against_collisions() {
    let l = Ledger::default();
    let range = |c: ScoreContext, collision: Option<bool>| ScoreContext { range_book: Some(Book::from_name("Psalms").unwrap()), range_collision: collision, ..c };
    let e = Evidence { matched_idf: Some(15.9), ..stream(6) };
    let s = score(v("Jeremiah", 17, 8), Method::Stream, &e, &range(ctx(T0, up("Psalms", 1, 3, T0 - 3000)), None), &l);
    assert!(!sends(s.total) && s.d != 0.0);
    let s = score(v("Jeremiah", 17, 8), Method::Stream, &stream(6), &range(ctx(T0, up("Psalms", 1, 3, T0 - 24000)), None), &l);
    assert!(s.total <= VIEWER_MIN_SCORE - 0.05, "absolute, not a decaying penalty");
    let s = score(v("Romans", 8, 28), Method::Direct, &Evidence::default(), &range(ctx(T0, up("Psalms", 1, 3, T0 - 3000)), None), &l);
    assert!(sends(s.total) && s.d == 0.0, "a new citation always wins");
    let s = score(v("Psalms", 1, 2), Method::Stream, &Evidence { matched_idf: Some(14.9), ..stream(6) }, &range(ctx(T0, up("Psalms", 1, 1, T0 - 3000)), None), &l);
    assert!(sends(s.total) && s.d == 0.0, "the range's own book stays exempt");
}

#[test]
fn speech_decides_range_collisions_inside_and_outside_the_book() {
    let l = Ledger::default();
    let joel = ScoreContext { range_book: Some(Book::from_name("Joel").unwrap()), ..ctx(T0 + 1000, up("Joel", 2, 5, T0)) };
    assert!(!sends(score(v("Daniel", 11, 32), Method::Stream, &stream(7), &joel, &l).total));
    assert!(sends(score(v("Daniel", 11, 32), Method::Stream, &stream(7), &ScoreContext { range_collision: Some(false), ..joel }, &l).total));
    let exodus = ScoreContext { range_book: Some(Book::from_name("Exodus").unwrap()), ..ctx(T0 + 1000, up("Exodus", 12, 31, T0)) };
    assert!(!sends(score(v("Exodus", 10, 16), Method::Stream, &stream(7), &ScoreContext { range_collision: Some(true), ..exodus }, &l).total));
    assert!(sends(score(v("Exodus", 12, 34), Method::Stream, &stream(7), &ScoreContext { range_collision: Some(false), ..exodus }, &l).total));
}

#[test]
fn strong_evidence_reshows_an_earlier_verse_at_once() {
    let l = Ledger::default();
    let luke = ScoreContext { already_shown: true, range_book: Some(Book::from_name("Luke").unwrap()), ..ctx(T0, up("Luke", 10, 19, T0 - 3000)) };
    let s = score(v("Luke", 10, 17), Method::Verbatim, &verbatim(0.69, 26.0), &luke, &l);
    assert!(sends(s.total) && s.d == 0.0);
    let matt = ScoreContext { range_book: Some(Book::from_name("Matthew").unwrap()), ..ctx(T0, up("Matthew", 15, 28, T0 - 3000)) };
    let s = score(v("Matthew", 15, 23), Method::Stream, &stream(6), &matt, &l);
    assert!(sends(s.total) && s.d == 0.0);
}

#[test]
fn meaning_only_matches_have_a_ceiling() {
    let l = Ledger::default();
    let sem = |sim: f32| Evidence { similarity: Some(sim), ..Default::default() };
    let s = score(v("Romans", 8, 28), Method::Semantic, &sem(0.90), &ctx(T0, None), &l);
    assert!(!s.very_high && target(s.total, Method::Semantic, false, s.very_high) != Target::Screen);
    let s = score(v("Romans", 8, 28), Method::Semantic, &sem(0.99), &ctx(T0, None), &l);
    assert!(s.very_high && target(s.total, Method::Semantic, false, s.very_high) == Target::Screen);
    let fp = |sim: f32, c: Confidence| Evidence { similarity: Some(sim), confidence: Some(c), ..Default::default() };
    assert!(!sends(score(v("Romans", 8, 28), Method::Fingerprint, &fp(0.90, Confidence::High), &ctx(T0, None), &l).total));
    assert!(sends(score(v("Romans", 8, 28), Method::Fingerprint, &fp(0.97, Confidence::High), &ctx(T0, None), &l).total));
    assert!(!sends(score(v("Romans", 8, 28), Method::Fingerprint, &fp(0.97, Confidence::Medium), &ctx(T0, None), &l).total));
    let rr = Evidence { similarity: Some(0.55), rerank: Some(0.999), ..Default::default() };
    let s = score(v("Genesis", 24, 63), Method::Semantic, &rr, &ctx(T0, None), &l);
    assert!(!s.very_high, "a reranker score alone never sends");
    assert_eq!(target(0.95, Method::Semantic, false, false), Target::Suggestions);
}

#[test]
fn corroboration_lifts_the_meaning_ceiling_not_the_bar() {
    let mut l = Ledger::default();
    l.record(v("Romans", 8, 28), Method::Stream, T0 - 3000, true);
    l.record(v("Romans", 8, 28), Method::Fingerprint, T0 - 2000, true);
    let corroborated = l.has_corroboration(v("Romans", 8, 28));
    assert!(corroborated);
    let s = score(v("Romans", 8, 28), Method::Semantic, &Evidence { similarity: Some(0.99), ..Default::default() }, &ctx(T0, None), &l);
    assert_eq!(target(s.total, Method::Semantic, corroborated, s.very_high), Target::Screen);
    let s = score(v("Romans", 8, 28), Method::Semantic, &Evidence { similarity: Some(0.3), ..Default::default() }, &ctx(T0, None), &l);
    assert!(target(s.total, Method::Semantic, corroborated, s.very_high) != Target::Screen);
}

#[test]
fn only_hits_that_landed_corroborate() {
    let mut l = Ledger::default();
    l.record(v("1 Peter", 1, 3), Method::Stream, T0 - 3000, true);
    l.record(v("1 Peter", 1, 3), Method::Verbatim, T0 - 2000, false);
    assert!(!l.has_corroboration(v("1 Peter", 1, 3)));
    assert!(l.agreement_bonus(v("1 Peter", 1, 3)) > 0.0);
    l.record(v("1 Peter", 1, 3), Method::Semantic, T0 - 1000, true);
    assert!(l.has_corroboration(v("1 Peter", 1, 3)));
}

#[test]
fn a_confusable_passage_never_takes_over_but_a_citation_does() {
    let mut l = Ledger::default();
    l.record(v("Psalms", 1, 3), Method::Stream, T0 + 5000, true);
    l.record(v("Psalms", 1, 3), Method::Stream, T0 + 6000, true);
    l.record(v("Psalms", 1, 3), Method::Verbatim, T0 + 7000, true);
    let confusable = ScoreContext { confusable_with_screen: true, ..ctx(T0 + 30_000, up("Jeremiah", 17, 7, T0)) };
    assert!(!sends(score(v("Psalms", 1, 3), Method::Stream, &stream(6), &confusable, &l).total));
    assert!(sends(score(v("Psalms", 1, 3), Method::Direct, &Evidence::default(), &confusable, &Ledger::default()).total));
    let named = ScoreContext { confusable_with_screen: true, ..ctx(T0 + 100, up("Psalms", 1, 3, T0)) };
    assert!(sends(score(v("Jeremiah", 17, 8), Method::ChapterKeyword, &Evidence::default(), &named, &Ledger::default()).total));
}

#[test]
fn calibration_examples() {
    assert_eq!(calibrate(Method::Direct, &Evidence::default()), 0.93);
    assert_eq!(calibrate(Method::DirectPartial, &Evidence::default()), 0.75);
    assert!((calibrate(Method::Stream, &stream(13)) - 0.97).abs() < 1e-6);
}
