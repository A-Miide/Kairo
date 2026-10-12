//! The detection engine on its own thread, the meaning models on another,
//! and every decision turned into the messages the UI already knows.
//!
//! Every speech engine (Deepgram, the offline model, test transcripts) calls
//! [`Detector::speech`] — one funnel.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use kairo_bible::VerseId;
use kairo_detect::engine::{Answer, Config, Effect, Engine, Event, MeaningHit, Output, Request, Segment, Target};
use kairo_semantic::MeaningService;
use serde_json::{json, Value};

use crate::bible::Bible;
use crate::hub::Hub;

/// What the detection thread is asked to do.
pub enum Command {
    Event(Event),
    Configure(Box<dyn FnOnce(&mut Config) + Send>),
    /// The meaning models are ready (or replaced).
    Meaning(Arc<MeaningService>),
}

/// How the church shows scripture, read from settings on each decision.
#[derive(Clone, Debug, Default)]
pub struct Presentation {
    pub translation: String,
    pub language: String,
    pub auto_send: bool,
}

#[derive(Clone)]
pub struct Detector {
    tx: mpsc::Sender<Command>,
}

/// What this service put on screen and how — the sermon notes read it.
#[derive(Default)]
pub struct ServiceLog {
    pub shown: Vec<crate::notes::Shown>,
    pub ranges: Vec<(Vec<VerseId>, u64)>,
}

pub type Log = Arc<std::sync::Mutex<ServiceLog>>;

pub fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Detector {
    pub fn start(bible: Arc<Bible>, hub: Arc<Hub>, cfg: Config, presentation: Arc<std::sync::RwLock<Presentation>>, rt: tokio::runtime::Handle, log: Log) -> Detector {
        let (tx, rx) = mpsc::channel::<Command>();
        let me = Detector { tx: tx.clone() };
        std::thread::Builder::new()
            .name("detection".into())
            .spawn(move || run(bible, hub, cfg, presentation, rt, tx, rx, log))
            .expect("detection thread");
        me
    }

    pub fn send(&self, e: Event) {
        let _ = self.tx.send(Command::Event(e));
    }

    pub fn configure(&self, f: impl FnOnce(&mut Config) + Send + 'static) {
        let _ = self.tx.send(Command::Configure(Box::new(f)));
    }

    pub fn meaning_ready(&self, m: Arc<MeaningService>) {
        let _ = self.tx.send(Command::Meaning(m));
    }

    /// One recognised stretch of speech, from any engine.
    pub fn speech(&self, hub: &Hub, text: &str, is_final: bool, confidence: f32, speech_final: bool, words: Option<Value>, continues_live: bool) {
        let mut msg = json!({ "type": "transcript", "text": text, "isFinal": is_final, "confidence": confidence });
        if let Some(w) = words {
            msg["words"] = w;
        }
        hub.broadcast(msg);
        if text.trim().is_empty() {
            return;
        }
        self.send(Event::Speech(Segment { text: text.to_string(), is_final, speech_final, continues_live }));
    }
}

/// The range as the UI is told about it.
#[derive(Default)]
struct RangeMirror {
    all: Vec<VerseId>,
    current: usize,
}

fn run(
    bible: Arc<Bible>,
    hub: Arc<Hub>,
    cfg: Config,
    presentation: Arc<std::sync::RwLock<Presentation>>,
    rt: tokio::runtime::Handle,
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Command>,
    log: Log,
) {
    let started = Instant::now();
    let mut engine = Engine::new(bible.k.clone(), cfg);
    let mut meaning: Option<Arc<MeaningService>> = None;
    let mut range = RangeMirror::default();
    let (mtx, mrx) = mpsc::channel::<Request>();
    // The meaning models answer on their own thread; answers come back as events.
    let back = tx.clone();
    let models: Arc<std::sync::RwLock<Option<Arc<MeaningService>>>> = Arc::new(std::sync::RwLock::new(None));
    let models_for_thread = models.clone();
    std::thread::Builder::new()
        .name("meaning".into())
        .spawn(move || {
            for r in mrx {
                let Some(m) = models_for_thread.read().unwrap().clone() else { continue };
                let a = answer(&m, &r);
                let _ = back.send(Command::Event(Event::Answer(a)));
            }
        })
        .expect("meaning thread");

    for cmd in rx {
        let now = started.elapsed().as_millis() as u64;
        let out = match cmd {
            Command::Event(Event::Reset) => {
                *log.lock().unwrap() = ServiceLog::default();
                engine.handle(Event::Reset, now)
            }
            Command::Event(e) => engine.handle(e, now),
            Command::Configure(f) => {
                f(engine.config_mut());
                continue;
            }
            Command::Meaning(m) => {
                *models.write().unwrap() = Some(m.clone());
                meaning = Some(m);
                engine.config_mut().meaning = true;
                continue;
            }
        };
        let p = presentation.read().unwrap().clone();
        apply(&bible, &hub, &mut range, out.clone(), &p, &log);
        for r in out.requests {
            if meaning.is_some() {
                let _ = mtx.send(r);
            }
        }
        for e in &out.effects {
            if let Effect::WakeAt { id, at_ms } = e {
                let tx = tx.clone();
                let delay = at_ms.saturating_sub(now);
                let id = *id;
                rt.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    let _ = tx.send(Command::Event(Event::Timer(id)));
                });
            }
        }
    }
}

fn answer(m: &MeaningService, r: &Request) -> Answer {
    match r {
        Request::Paraphrase { id, windows, limit } => Answer::Paraphrase {
            id: *id,
            candidates: m
                .paraphrase(windows, *limit)
                .unwrap_or_default()
                .into_iter()
                .map(|f| MeaningHit { verse: f.verse, cos: f.cos, window: f.window, rerank: f.rerank, rerank_modern: f.rerank_modern })
                .collect(),
        },
        Request::Scoped { id, text, verses, limit } => Answer::Scoped { id: *id, hits: m.scoped(text, verses, *limit).unwrap_or_default() },
        Request::Compare { id, texts, verses } => Answer::Compare { id: *id, scores: m.compare(texts, verses).unwrap_or_default() },
    }
}

/// Decisions → what the UI and outputs are told.
fn apply(bible: &Bible, hub: &Hub, range: &mut RangeMirror, out: Output, p: &Presentation, log: &Log) {
    for e in out.effects {
        match e {
            Effect::Show { verses, method, score, target, corrected_from } => {
                if target == Target::Drop || verses.is_empty() {
                    continue;
                }
                let mut list: Vec<Value> = verses.iter().map(|v| bible.verse_json(*v, score, method.wire())).collect();
                let viewer = target == Target::Screen;
                if viewer {
                    bible.apply_translation(&mut list, &p.translation);
                    bible.apply_language(&mut list, &p.language);
                }
                let mut msg = json!({
                    "type": "detection",
                    "verses": list,
                    "method": method.wire(),
                    "topScore": score,
                    "target": if viewer { "viewer" } else { "suggestions" },
                    "timestamp": epoch_ms(),
                });
                if let Some(from) = corrected_from {
                    msg["corrected"] = json!(true);
                    msg["correctedFrom"] = json!(bible.reference(from));
                }
                eprintln!(
                    "[Detect] {} → {} ({} {score:.2}){}",
                    verses.iter().map(|v| bible.reference(*v)).collect::<Vec<_>>().join(", "),
                    if viewer { "screen" } else { "Possible Matches" },
                    method.wire(),
                    corrected_from.map_or(String::new(), |f| format!(", corrects {}", bible.reference(f)))
                );
                hub.broadcast(msg);
                if viewer && p.auto_send {
                    hub.set_bible_on_top(true);
                    let how = crate::notes::How::of(method.wire());
                    let at = epoch_ms();
                    log.lock().unwrap().shown.extend(verses.iter().take(1).map(|v| crate::notes::Shown { verse: *v, at, how }));
                }
            }
            Effect::RangeSet { verses, current } => {
                log.lock().unwrap().ranges.push((verses.clone(), epoch_ms()));
                range.all = verses.clone();
                range.current = verses.iter().position(|v| *v == current).unwrap_or(0);
                range_state(bible, hub, range);
                let all: Vec<Value> = verses.iter().map(|v| bible.verse_json(*v, 1.0, "direct")).collect();
                hub.broadcast(json!({ "type": "range-verses", "verses": all, "activeRef": bible.reference(current) }));
            }
            Effect::RangeMoved { current, .. } => {
                range.current = range.all.iter().position(|v| *v == current).unwrap_or(range.current);
                range_state(bible, hub, range);
                hub.broadcast(json!({ "type": "range-active", "activeRef": bible.reference(current) }));
            }
            Effect::RangeCleared => {
                *range = RangeMirror::default();
                range_state(bible, hub, range);
            }
            Effect::ReadingMode { active, chapter } => {
                let r = chapter.map(|v| bible.k.verse_ref(v));
                hub.broadcast(json!({
                    "type": "reading-mode",
                    "active": active,
                    "book": r.map(|r| r.book.name()),
                    "chapter": r.map(|r| r.chapter),
                }));
            }
            Effect::TopicLibrary { size, words } => hub.broadcast(json!({ "type": "topic-library", "size": size, "words": words })),
            Effect::WakeAt { .. } => {}
        }
    }
}

fn range_state(bible: &Bible, hub: &Hub, range: &RangeMirror) {
    let remaining = range.all.len().saturating_sub(range.current + 1);
    let next = range.all.get(range.current + 1).map(|v| bible.verse_json(*v, 1.0, "direct"));
    hub.broadcast(json!({ "type": "range-state", "remaining": remaining, "total": range.all.len(), "next": next }));
}
