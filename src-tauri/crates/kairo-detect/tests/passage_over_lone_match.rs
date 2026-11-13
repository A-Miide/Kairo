//! A verse that goes on from the one on screen outranks a lone match from another book.
//!
//! Live, 13 Nov 2026: the preacher quoted Ephesians 1:18-20 from memory and said "according to the
//! greatness of his power which he wrought in Christ". Ephesians 1:18 was up; Psalm 79:11 ("according to
//! the greatness of thy power") went to the screen instead of the passage he was in (about 1.7 s, until
//! Ephesians 1:20 replaced it).

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use kairo_bible::{Corpus, VerseId};
use kairo_detect::engine::{Config, Effect, Engine, Event, Segment, Target};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;

static K: LazyLock<Arc<Knowledge>> = LazyLock::new(|| {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../databases/bibles");
    Arc::new(Knowledge::build(Corpus::load(&dir).unwrap(), Wordings { own: None, also: vec![] }))
});

const MS_PER_WORD: u64 = 380;

fn label(v: VerseId) -> String {
    let r = K.verse_ref(v);
    format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
}

/// Every send, in order, as (screen?, verse). Each event is (ms since the first, final?, text), at the
/// pace the live app heard it.
fn sends(events: &[(u64, bool, &str)]) -> Vec<(bool, String)> {
    let mut e = Engine::new(K.clone(), Config::default());
    let mut out = Vec::new();
    for &(ms, fin, text) in events {
        let step = e.handle(Event::Speech(Segment { text: text.to_string(), is_final: fin, speech_final: fin, continues_live: false }), 10_000 + ms);
        for eff in step.effects {
            if let Effect::Show { verses, target, .. } = eff {
                out.push((target == Target::Screen, label(verses[0])));
            }
        }
    }
    out
}

/// Word for word what the live app heard, when it heard it (from its log, 13 Nov 2026).
const EPHESIANS_FROM_MEMORY: &[(u64, bool, &str)] = &[
    (0, false, "If I"),
    (578, false, "If I ask you"),
    (1741, false, "If I ask you how"),
    (2257, false, "If I ask you how is a man born"),
    (2818, false, "If I ask you how is a man born again, how"),
    (3399, false, "If I ask you how is a man born again, how is a man saved"),
    (3972, false, "If I ask you how is a man born again, how is a man saved? You"),
    (4494, false, "If I ask you how is a man born again, how is a man saved? You say the Bible"),
    (5088, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says"),
    (5648, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your"),
    (6231, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart"),
    (6727, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus"),
    (7306, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your"),
    (7871, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God"),
    (8466, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from"),
    (8954, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead"),
    (9562, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved"),
    (10098, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning"),
    (11202, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death"),
    (11787, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial,"),
    (12357, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the"),
    (12921, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit"),
    (13444, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God"),
    (14602, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him"),
    (15183, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy"),
    (15694, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit"),
    (16261, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our"),
    (17407, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our answer"),
    (18517, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our answer when"),
    (19088, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our answer when it comes to salvation"),
    (20162, false, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our answer when it comes to salvation. Is that true"),
    (21312, true, "If I ask you how is a man born again, how is a man saved? You say the Bible says, believe in your heart, the Lord Jesus, confess with your mouth that God raised Him from the dead you shall be saved. So meaning the death, burial, resurrection of the Holy Spirit of of God and Him giving us the Holy Spirit was our answer when it comes to salvation. Is that true"),
    (21906, false, "If I now ask"),
    (22402, false, "If I now ask you"),
    (23553, false, "If I now ask you how can"),
    (24138, false, "If I now ask you how can a man"),
    (24642, false, "If I now ask you how can a man walk in the power"),
    (25218, false, "If I now ask you how can a man walk in the power of God"),
    (25797, false, "If I now ask you how can a man walk in the power of God same thing"),
    (26365, false, "If I now ask you how can a man walk in the power of God same thing the Bible"),
    (26861, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the"),
    (27458, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understand"),
    (28026, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded"),
    (28597, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light"),
    (29128, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to"),
    (29688, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of"),
    (30269, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling"),
    (30834, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches"),
    (31362, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of"),
    (31933, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance"),
    (32522, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints"),
    (33098, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according"),
    (33605, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the"),
    (34170, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his"),
    (34752, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought"),
    (35343, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when"),
    (35842, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when he raised it from the"),
    (36418, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when he raised it from the dead"),
    (36989, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when he raised it from the dead so"),
    (37562, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when he raised it from the dead so the resurrection"),
    (38078, false, "If I now ask you how can a man walk in the power of God same thing the Bible says the eyes of your understanding will be flooded with light to know the hope of his calling, the riches of the glory of his inheritance in the saints according to the greatness of his power which he wrought in Christ when he raised it from the dead so the resurrection of Jesus"),
];

#[test]
fn the_passage_on_screen_is_not_replaced_by_a_lone_match_from_another_book() {
    let s = sends(EPHESIANS_FROM_MEMORY);
    let screen: Vec<&str> = s.iter().filter(|(on, _)| *on).map(|(_, v)| v.as_str()).collect();
    // Psalm 79:11 shares "according to the greatness of ... power" and is offered, never sent.
    assert!(!screen.contains(&"Psalms 79:11"), "Psalm 79:11 went to the screen: {s:?}");
    assert!(s.contains(&(false, "Psalms 79:11".to_string())), "Psalm 79:11 should still be offered: {s:?}");
    // The passage he was in goes on: 1:18, then 1:19 (said in his own words), then 1:20.
    let order: Vec<&str> = screen.iter().copied().filter(|v| v.starts_with("Ephesians")).collect();
    let (i18, i19, i20) = (order.iter().position(|v| *v == "Ephesians 1:18"), order.iter().position(|v| *v == "Ephesians 1:19"), order.iter().position(|v| *v == "Ephesians 1:20"));
    assert!(i18 < i19 && i19 < i20 && i20.is_some(), "{s:?}");
}

/// The speech `text` arriving a word at a time at the live pace, from `start` ms.
fn grow(text: &str, start: u64) -> Vec<(u64, bool, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    (1..=words.len()).map(|n| (start + n as u64 * 570, n == words.len(), words[..n].join(" "))).collect()
}

/// Ephesians 1:18 read, then a real move to Psalm 79:11 (nothing in Ephesians says these words): the
/// Psalm still goes straight up. The competing-passage rule only holds back a match when the same words
/// also found the passage's own next verse.
#[test]
fn a_real_move_to_another_verse_still_sends() {
    let mut ev = grow("the eyes of your understanding being enlightened that ye may know what is the hope of his calling and what the riches of the glory of his inheritance in the saints", 0);
    let t = ev.last().map_or(0, |e| e.0);
    ev.extend(grow("now David prayed let the sighing of the prisoner come before thee according to the greatness of thy power preserve thou those that are appointed to die", t + 2_000));
    let owned: Vec<(u64, bool, &str)> = ev.iter().map(|(ms, f, tx)| (*ms, *f, tx.as_str())).collect();
    let s = sends(&owned);
    let screen: Vec<&str> = s.iter().filter(|(on, _)| *on).map(|(_, v)| v.as_str()).collect();
    assert!(screen.contains(&"Ephesians 1:18"), "{s:?}");
    assert!(screen.contains(&"Psalms 79:11"), "a real move to Psalm 79:11 must still send: {s:?}");
}
