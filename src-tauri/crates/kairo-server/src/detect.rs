//! The detection engine on its own thread, the meaning models on another,
//! and every decision turned into the messages the UI already knows.
//!
//! Every speech engine (Deepgram, the offline model, test transcripts) calls
//! [`Detector::speech`] — one funnel.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

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
    /// Video Director mode with Auto-send off: a detected verse is staged in Preview, never put on the Output.
    pub hold: bool,
    /// Video Director mode is on: an operator's range step is staged like a manual send.
    pub director: bool,
}

/// Where a verse for the screen goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// On the outputs now.
    Screen,
    /// Held back (Director mode, Auto-send off): waits on the Detected chip.
    Staged,
    /// Listed in Possible Matches for the operator.
    Suggestions,
    /// The operator's own step in Director mode: the main window sends it as it sends a hand
    /// send (staged in Preview while "Stage sends on Preview" is on).
    OperatorStep,
}

/// A detected verse goes on screen only with Auto-send on. An explicit one (the operator's
/// range Next) is never held back by Auto-send: dropping it into Possible Matches would be a
/// missed send.
fn route(screen: bool, explicit: bool, p: &Presentation) -> Route {
    if !screen {
        return Route::Suggestions;
    }
    if explicit {
        return if p.director { Route::OperatorStep } else { Route::Screen };
    }
    if p.hold {
        Route::Staged
    } else if p.auto_send {
        Route::Screen
    } else {
        Route::Suggestions
    }
}

#[derive(Clone)]
pub struct Detector {
    tx: mpsc::Sender<Command>,
    /// Told when the preacher asks for another translation ("switch to the NIV"), with what was heard.
    on_translation: Arc<std::sync::OnceLock<Box<dyn Fn(&'static str, String) + Send + Sync>>>,
    /// The last finished stretch: a request split over two ("Isaiah seven seven" | "NLT or Message").
    last_final: Arc<std::sync::Mutex<String>>,
}

/// What this service put on screen and how — the sermon notes read it.
#[derive(Default)]
pub struct ServiceLog {
    pub shown: Vec<crate::notes::Shown>,
    pub ranges: Vec<(Vec<VerseId>, u64)>,
}

pub type Log = Arc<std::sync::Mutex<ServiceLog>>;

pub use crate::util::epoch_ms;

impl Detector {
    pub fn start(bible: Arc<Bible>, hub: Arc<Hub>, cfg: Config, presentation: Arc<std::sync::RwLock<Presentation>>, rt: tokio::runtime::Handle, log: Log) -> Detector {
        let (tx, rx) = mpsc::channel::<Command>();
        let me = Detector { tx: tx.clone(), on_translation: Default::default(), last_final: Default::default() };
        let on_translation = me.on_translation.clone();
        std::thread::Builder::new()
            .name("detection".into())
            .spawn(move || run(bible, hub, cfg, presentation, rt, tx, rx, log, on_translation))
            .expect("detection thread");
        me
    }

    /// What to do when a spoken command asks for another translation (set once, at start).
    pub fn on_translation(&self, f: impl Fn(&'static str, String) + Send + Sync + 'static) {
        let _ = self.on_translation.set(Box::new(f));
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
        // An interpreter's turn is not transcribed and never reaches the detectors.
        let (verdict, change, released) = crate::interpretation::with(|i| {
            let (v, c) = i.text(text, is_final);
            (v, c, i.take_released())
        });
        if let Some(c) = change {
            announce_interpretation(hub, c);
        }
        // Words kept aside during a turn that was the preacher's after all go through first, in order.
        for (t, f) in released {
            self.forward(hub, &t, f, confidence, false, None, false);
        }
        if verdict == crate::interpretation::Verdict::Interpretation && !text.trim().is_empty() {
            hub.broadcast(json!({ "type": "transcript", "text": "(interpretation)", "isFinal": is_final, "confidence": confidence, "interpretation": true }));
            return;
        }
        self.forward(hub, text, is_final, confidence, speech_final, words, continues_live);
    }

    /// Transcript and detectors, for what is the preacher's.
    fn forward(&self, hub: &Hub, text: &str, is_final: bool, confidence: f32, speech_final: bool, words: Option<Value>, continues_live: bool) {
        let mut msg = json!({ "type": "transcript", "text": text, "isFinal": is_final, "confidence": confidence });
        if let Some(w) = words {
            msg["words"] = w;
        }
        hub.broadcast(msg);
        if text.trim().is_empty() {
            return;
        }
        // Finished stretches go in debug.log, so a run can be diagnosed from the log alone; the
        // in-progress text too, since detection acts on it and a run can't be replayed exactly
        // without it (Ezekiel 12:25 sent early live, never in a replay of the finals, 2026-10-28).
        if !is_final {
            crate::routes::write_debug_global("server:interim", json!({ "text": text }));
        }
        if is_final {
            crate::routes::write_debug_global("server:transcript", json!({ "text": text, "speechFinal": speech_final, "continues": continues_live }));
            use kairo_detect::reference::translation_command::translation_command;
            let prev = std::mem::replace(&mut *crate::util::lock(&self.last_final), text.to_string());
            let joined = format!("{prev} {text}");
            // This stretch, or this one with the last when the last alone wasn't a request already.
            let heard = translation_command(text).map(|c| (c, text.to_string())).or_else(|| {
                translation_command(&prev).is_none().then(|| translation_command(&joined)).flatten().map(|c| (c, joined.clone()))
            });
            if let (Some((code, heard)), Some(f)) = (heard, self.on_translation.get()) {
                f(code, heard);
            }
        } else if std::env::var_os("KAIRO_LOG_INTERIMS").is_some() {
            // Live text too, so a test run can be replayed as it was heard (off unless asked for).
            crate::routes::write_debug_global("server:interim", json!({ "text": text }));
        }
        self.send(Event::Speech(Segment { text: text.to_string(), is_final, speech_final, continues_live }));
    }
}

/// The interpreter began or finished a turn: the transcript marks it, and the screen may offer the
/// Multi-Language theme.
pub fn announce_interpretation(hub: &Hub, change: crate::interpretation::Change) {
    let active = change == crate::interpretation::Change::Started;
    crate::routes::write_debug_global("server:interpretation", json!({ "active": active }));
    hub.broadcast(json!({ "type": "interpretation", "active": active }));
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
    on_translation: Arc<std::sync::OnceLock<Box<dyn Fn(&'static str, String) + Send + Sync>>>,
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
            let _below = crate::util::below_speech();
            for r in mrx {
                let Some(m) = models_for_thread.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone() else { continue };
                // A panic answering one request costs that answer, never the meaning thread.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer(&m, &r))) {
                    Ok(a) => { let _ = back.send(Command::Event(Event::Answer(a))); }
                    Err(p) => crate::routes::write_debug_global("server:meaning-panic", json!({ "message": panic_text(&p) })),
                }
            }
        })
        .expect("meaning thread");

    for cmd in rx {
        let now = started.elapsed().as_millis() as u64;
        // The operator's Next (button, remote, OSC): the verse it puts up is an explicit send.
        let operator_step = matches!(cmd, Command::Event(Event::RangeNext));
        let out = match cmd {
            Command::Event(Event::Reset) => {
                *log.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = ServiceLog::default();
                engine.handle(Event::Reset, now)
            }
            Command::Event(e) => {
                // A panic in the engine used to end detection for the rest of the service while
                // transcripts kept scrolling. Now it costs this one stretch of speech: the engine
                // starts again with the same settings and detection carries on.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.handle(e, now))) {
                    Ok(out) => out,
                    Err(p) => {
                        let cfg = engine.config().clone();
                        crate::routes::write_debug_global("server:detection-panic", json!({ "message": panic_text(&p) }));
                        eprintln!("[Detect] the engine panicked and was restarted: {}", panic_text(&p));
                        engine = Engine::new(bible.k.clone(), cfg);
                        continue;
                    }
                }
            }
            Command::Configure(f) => {
                f(engine.config_mut());
                continue;
            }
            Command::Meaning(m) => {
                *models.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(m.clone());
                meaning = Some(m);
                engine.config_mut().meaning = true;
                continue;
            }
        };
        let p = presentation.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        apply(&bible, &hub, &mut range, out.clone(), &p, &log, operator_step);
        for r in out.requests {
            if meaning.is_some() {
                let _ = mtx.send(r);
            }
        }
        for e in &out.effects {
            // The passage is being read in another translation: shown in it as if it had been asked
            // for, back to the church's at the next passage (spoken.rs).
            if let (Effect::ReadIn { translation, .. }, Some(f)) = (e, on_translation.get()) {
                if let Some(code) = ["KJV", "BSB"].into_iter().find(|c| c.eq_ignore_ascii_case(translation)) {
                    f(code, format!("reading from the {code}"));
                }
            }
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

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into())
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
/// `operator_step`: these effects come from the operator's range Next. The verse the range
/// moved to is then an explicit send; anything else in the same output is gated as usual.
fn apply(bible: &Bible, hub: &Hub, range: &mut RangeMirror, out: Output, p: &Presentation, log: &Log, operator_step: bool) {
    let mut stepped: Option<VerseId> = None;
    for e in out.effects {
        match e {
            Effect::Show { verses, method, score, target, corrected_from } => {
                if target == Target::Drop || verses.is_empty() {
                    continue;
                }
                let mut list: Vec<Value> = verses.iter().map(|v| bible.verse_json(*v, score, method.wire())).collect();
                // A verse for the screen goes there only with Auto-send on. With it off it waits in
                // Possible Matches for the operator (or, in Director mode, staged in Preview).
                let screen = target == Target::Screen;
                let explicit = stepped.is_some_and(|s| verses.len() == 1 && verses[0] == s);
                let to = route(screen, explicit, p);
                if to == Route::OperatorStep {
                    bible.apply_translation(&mut list, &p.translation);
                    bible.apply_language(&mut list, &p.language);
                    eprintln!("[Detect] {} → range step (Director)", bible.reference(verses[0]));
                    crate::routes::write_debug_global("server:detect", json!({
                        "verses": [bible.reference(verses[0])], "target": "range-step", "method": method.wire(),
                    }));
                    hub.broadcast(json!({ "type": "range-step", "verses": list, "timestamp": epoch_ms() }));
                    continue;
                }
                let held = to == Route::Staged;
                let viewer = to == Route::Screen || held;
                if viewer {
                    // A translation the preacher asked for is its passage's: a verse from another goes up
                    // in the church's own, and back in the passage it's the asked-for one again (spoken.rs).
                    let ended = (to == Route::Screen)
                        .then(|| list.first().and_then(|v| Some((v["book"].as_str()?.to_string(), v["chapter"].as_u64()?))))
                        .flatten()
                        .and_then(|(b, c)| crate::spoken::translation_for(&b, c));
                    bible.apply_translation(&mut list, ended.as_deref().unwrap_or(&p.translation));
                    bible.apply_language(&mut list, &p.language);
                }
                let mut msg = json!({
                    "type": "detection",
                    "verses": list,
                    "method": method.wire(),
                    "topScore": score,
                    "target": if viewer && !held { "viewer" } else { "suggestions" },
                    "timestamp": epoch_ms(),
                });
                if held {
                    msg["staged"] = json!(true);
                }
                if let Some(from) = corrected_from {
                    msg["corrected"] = json!(true);
                    msg["correctedFrom"] = json!(bible.reference(from));
                }
                eprintln!(
                    "[Detect] {} → {} ({} {score:.2}){}",
                    verses.iter().map(|v| bible.reference(*v)).collect::<Vec<_>>().join(", "),
                    if viewer && !held { "screen" } else if held { "staged" } else { "Possible Matches" },
                    method.wire(),
                    corrected_from.map_or(String::new(), |f| format!(", corrects {}", bible.reference(f)))
                );
                hub.broadcast(msg);
                crate::routes::write_debug_global("server:detect", json!({
                    "verses": verses.iter().map(|v| bible.reference(*v)).collect::<Vec<_>>(),
                    "target": if held { "staged" } else if viewer { "screen" } else { "suggestions" },
                    "method": method.wire(),
                    "score": (score * 100.0).round() / 100.0,
                    "correctsFrom": corrected_from.map(|f| bible.reference(f)),
                }));
                if to == Route::Screen {
                    hub.set_bible_on_top(true);
                    let how = crate::notes::How::of(method.wire());
                    let at = epoch_ms();
                    log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).shown.extend(verses.iter().take(1).map(|v| crate::notes::Shown { verse: *v, at, how }));
                }
            }
            Effect::RangeSet { verses, current } => {
                log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).ranges.push((verses.clone(), epoch_ms()));
                range.all = verses.clone();
                range.current = verses.iter().position(|v| *v == current).unwrap_or(0);
                range_state(bible, hub, range);
                let all: Vec<Value> = verses.iter().map(|v| bible.verse_json(*v, 1.0, "direct")).collect();
                hub.broadcast(json!({ "type": "range-verses", "verses": all, "activeRef": bible.reference(current) }));
            }
            Effect::RangeMoved { current, .. } => {
                if operator_step {
                    stepped = Some(current);
                }
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
            Effect::Settled { text } => hub.broadcast(json!({ "type": "transcript-settled", "text": text })),
            Effect::WakeAt { .. } | Effect::ReadIn { .. } => {}
        }
    }
}

fn range_state(bible: &Bible, hub: &Hub, range: &RangeMirror) {
    let remaining = range.all.len().saturating_sub(range.current + 1);
    let next = range.all.get(range.current + 1).map(|v| bible.verse_json(*v, 1.0, "direct"));
    hub.broadcast(json!({ "type": "range-state", "remaining": remaining, "total": range.all.len(), "next": next }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(auto_send: bool, director: bool) -> Presentation {
        Presentation { auto_send, director, hold: !auto_send && director, ..Default::default() }
    }

    #[test]
    fn detected_verses_keep_the_auto_send_gate() {
        assert_eq!(route(true, false, &p(true, false)), Route::Screen);
        assert_eq!(route(true, false, &p(false, false)), Route::Suggestions);
        assert_eq!(route(true, false, &p(false, true)), Route::Staged);
        assert_eq!(route(true, false, &p(true, true)), Route::Screen);
        assert_eq!(route(false, false, &p(true, false)), Route::Suggestions);
    }

    #[test]
    fn an_operator_range_step_is_never_dropped_by_auto_send_off() {
        assert_eq!(route(true, true, &p(false, false)), Route::Screen);
        assert_eq!(route(true, true, &p(true, false)), Route::Screen);
        // Director mode: staged like a hand send, whatever Auto-send says.
        assert_eq!(route(true, true, &p(false, true)), Route::OperatorStep);
        assert_eq!(route(true, true, &p(true, true)), Route::OperatorStep);
    }
}
