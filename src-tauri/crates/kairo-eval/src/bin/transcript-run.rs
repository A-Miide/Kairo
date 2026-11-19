//! Feeds a YouTube caption file (json3) to the detector as a service in a language, and lists what it put on
//! screen and what it offered, with the words just spoken, so the sends can be judged by eye. There is no
//! ground truth: this is for false sends on real preaching and for quotes the preacher reads aloud.
//!
//! `transcript-run <captions.json3> --lang es [--meaning-other e5trim.json --thresholds-other th.json] [--own-pack]`

use std::sync::Arc;

use kairo_bible::{Corpus, VerseId};
use kairo_detect::engine::{Config, Effect, Engine, Event, MeaningThresholds, Output, Segment, Target, TimerId};
use kairo_detect::knowledge::Knowledge;
use kairo_detect::lexical::Wordings;
use kairo_detect::reference::Lang;
use kairo_eval::{answer, meaning, root};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let file = args.iter().find(|a| !a.starts_with("--") && a.ends_with(".json3")).expect("captions file");
    let code = opt("--lang").unwrap_or_else(|| "es".into());
    let lang = Lang::from_code(&code);
    let dir = root().join("databases/bibles");
    let corpus = Corpus::load(&dir).expect("kjv");
    let pack = corpus.load_flat(&code, &dir.join(format!("packs/{code}.json"))).ok();
    let k = Arc::new(Knowledge::build(Corpus::load(&dir).expect("kjv"), Wordings { own: pack, also: vec![] }));
    let service = opt("--meaning-other").map(|e| meaning(&e, None, &["KJV"]));
    let thresholds: MeaningThresholds = opt("--thresholds-other").map(|p| serde_json::from_slice(&std::fs::read(p).expect("thr")).expect("thr json")).unwrap_or_default();

    // The words with their times.
    let j: serde_json::Value = serde_json::from_slice(&std::fs::read(file).expect("read")).expect("json3");
    let mut words: Vec<(u64, String, bool)> = Vec::new(); // (ms, word, ends an event)
    for ev in j["events"].as_array().into_iter().flatten() {
        let start = ev["tStartMs"].as_u64().unwrap_or(0);
        let mut last = None;
        for seg in ev["segs"].as_array().into_iter().flatten() {
            let t = seg["utf8"].as_str().unwrap_or("").replace('\n', " ");
            for w in t.split_whitespace() {
                words.push((start + seg["tOffsetMs"].as_u64().unwrap_or(0), w.to_string(), false));
                last = Some(words.len() - 1);
            }
        }
        if let Some(i) = last {
            words[i].2 = true;
        }
    }
    eprintln!("{} words, {:.0} minutes", words.len(), words.last().map_or(0, |w| w.0) as f64 / 60000.0);

    let mut e = Engine::new(k.clone(), Config { offline: true, lang, meaning: service.is_some(), meaning_thresholds: thresholds, ..Config::default() });
    let mut wakes: Vec<(u64, TimerId)> = Vec::new();
    let label = |v: VerseId| {
        let r = k.verse_ref(v);
        format!("{} {}:{}", r.book.name(), r.chapter, r.verse)
    };
    let (mut screens, mut offers) = (0, 0);
    let mut segment: Vec<String> = Vec::new();
    let mut spoken: Vec<String> = Vec::new();
    let mut handle = |e: &mut Engine, o: Output, t: u64, wakes: &mut Vec<(u64, TimerId)>, spoken: &Vec<String>| {
        let mut pending = vec![o];
        while let Some(o) = pending.pop() {
            for eff in o.effects {
                match eff {
                    Effect::Show { verses, target, .. } => {
                        let near = spoken[spoken.len().saturating_sub(24)..].join(" ");
                        let names: Vec<String> = verses.iter().map(|v| label(*v)).collect();
                        match target {
                            Target::Screen => {
                                screens += 1;
                                println!("SCREEN t={:>6.1}s  {}  <- \"{}\"", t as f64 / 1000.0, names.join(", "), near);
                            }
                            Target::Suggestions => {
                                offers += 1;
                                println!("offer  t={:>6.1}s  {}  <- \"{}\"", t as f64 / 1000.0, names.join(", "), near);
                            }
                            _ => {}
                        }
                    }
                    Effect::WakeAt { id, at_ms } => wakes.push((at_ms, id)),
                    _ => {}
                }
            }
            if let Some(m) = service.as_ref() {
                for req in o.requests {
                    pending.push(e.handle(Event::Answer(answer(m, &req)), t));
                }
            }
        }
    };
    let mut last_interim = 0usize;
    for (i, (ms, w, end)) in words.iter().enumerate() {
        let t = *ms + 10_000;
        wakes.sort_by_key(|x| x.0);
        while wakes.first().is_some_and(|x| x.0 <= t) {
            let (when, id) = wakes.remove(0);
            let o = e.handle(Event::Timer(id), when);
            handle(&mut e, o, when, &mut wakes, &spoken);
        }
        segment.push(w.clone());
        spoken.push(w.clone());
        let fin = *end || segment.len() >= 14;
        if fin || segment.len() - last_interim >= 3 {
            let seg = Segment { text: segment.join(" "), is_final: fin, speech_final: fin, continues_live: !fin };
            let o = e.handle(Event::Speech(seg), t);
            handle(&mut e, o, t, &mut wakes, &spoken);
            last_interim = segment.len();
        }
        if fin {
            segment.clear();
            last_interim = 0;
        }
        let _ = i;
    }
    println!("-- {screens} on screen, {offers} offered over {} words", words.len());
}
