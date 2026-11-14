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
use kairo_detect::knowledge::Knowledge;
use kairo_detect::reference::Lang;
use kairo_semantic::MeaningService;
use serde_json::{json, Value};

use crate::bible::Bible;
use crate::hub::Hub;

/// What the detection thread is asked to do.
pub enum Command {
    /// An input, and when it arrived (ms on the detector's clock): the engine takes it at that time
    /// however long the thread takes to get to it.
    Event(Event, u64),
    /// The clock has reached this time with nothing heard: timers due by then fire.
    Tick(u64),
    Configure(Box<dyn FnOnce(&mut Config) + Send>),
    /// The meaning models are ready (or replaced).
    Meaning(Arc<MeaningService>),
    /// Knowledge built for the service's spoken language, and that language (service_bible.rs).
    Knowledge(Arc<Knowledge>, Lang),
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
    /// The detector's clock starts here.
    started: Instant,
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
        let started = Instant::now();
        let me = Detector { tx: tx.clone(), started, on_translation: Default::default(), last_final: Default::default() };
        let on_translation = me.on_translation.clone();
        std::thread::Builder::new()
            .name("detection".into())
            .spawn(move || run(bible, hub, cfg, presentation, rt, tx, rx, log, on_translation, started))
            .expect("detection thread");
        me
    }

    /// What to do when a spoken command asks for another translation (set once, at start).
    pub fn on_translation(&self, f: impl Fn(&'static str, String) + Send + Sync + 'static) {
        let _ = self.on_translation.set(Box::new(f));
    }

    pub fn send(&self, e: Event) {
        let _ = self.tx.send(Command::Event(e, self.started.elapsed().as_millis() as u64));
    }

    pub fn configure(&self, f: impl FnOnce(&mut Config) + Send + 'static) {
        let _ = self.tx.send(Command::Configure(Box::new(f)));
    }

    /// The engine starts again on knowledge built for another service language: what it was
    /// following belongs to the old one.
    pub fn relearn(&self, k: Arc<Knowledge>, lang: Lang) {
        let _ = self.tx.send(Command::Knowledge(k, lang));
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
        if is_final {
            crate::learned::heard(text);
        }
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

/// How long the engine waits for the meaning answers it is owed before going on without them.
const ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// The id a request is answered under.
fn request_id(r: &Request) -> u32 {
    match r {
        Request::Paraphrase { id, .. } | Request::Scoped { id, .. } | Request::Compare { id, .. } => *id,
    }
}

/// Words still being said (an interim) don't wait for meaning answers; anything else does.
fn waits_for_answers(e: &Option<Event>) -> bool {
    !matches!(e, Some(Event::Speech(s)) if !s.is_final)
}

/// …but an answer is owed for at most this many inputs of speech: a recogniser that runs a minute
/// without a pause (the Core ML engine) sends nothing but interims, and with answers waiting for a
/// final, meaning was searched once in 80 s instead of every few words (13 searches against 31 on a
/// 2.5-minute clip, 14 Nov 2026). Counted in inputs, not time, so it is the same on every computer.
pub const OWED_INPUTS: u64 = 2;

/// The kinds of match only offered that are wrong too often to show alone (accuracy.rs).
pub fn hidden_offers() -> Vec<kairo_detect::engine::Method> {
    kairo_detect::engine::Method::ALL.into_iter().filter(|m| !crate::accuracy::worth_offering(m.wire())).collect()
}

#[allow(clippy::too_many_arguments)]
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
    started: Instant,
) {
    let mut engine = Engine::new(bible.k(), cfg);
    record_config(0, engine.config(), true);
    let mut meaning: Option<Arc<MeaningService>> = None;
    let mut range = RangeMirror::default();
    // The meaning models answer on their own thread (below speech, so the recogniser keeps its CPU).
    // Every request gets a reply under its id, None when answering it failed.
    let (mtx, mrx) = mpsc::channel::<Request>();
    let (atx, arx) = mpsc::channel::<(u32, Option<Answer>)>();
    let models: Arc<std::sync::RwLock<Option<Arc<MeaningService>>>> = Arc::new(std::sync::RwLock::new(None));
    let models_for_thread = models.clone();
    std::thread::Builder::new()
        .name("meaning".into())
        .spawn(move || {
            let _below = crate::util::below_speech();
            for r in mrx {
                let m = models_for_thread.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                // A panic answering one request costs that answer, never the meaning thread.
                let a = m.and_then(|m| match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer(&m, &r))) {
                    Ok(a) => Some(a),
                    Err(p) => {
                        crate::routes::write_debug_global("server:meaning-panic", json!({ "message": panic_text(&p) }));
                        None
                    }
                });
                let _ = atx.send((request_id(&r), a));
            }
        })
        .expect("meaning thread");

    // The engine's clock: each input is taken at the time it arrived, never earlier than the last.
    // Timers fire on the same clock, before any input that arrived after they were due. Meaning
    // answers are owed, not waited for, while words are still being said: they are taken before the
    // next finished stretch (or any other input, or OWED_INPUTS inputs of speech later), in the order
    // asked, at the time of the input before it. So the same inputs give the same decisions however busy the computer is (owner, 13 Nov 2026:
    // "consistent result every time on every platform"), and a citation heard while the models work
    // goes up at once instead of queueing behind them for up to 8 s.
    let mut clock = 0u64;
    let mut wakes: Vec<(u64, kairo_detect::engine::TimerId)> = Vec::new();
    let mut owed: std::collections::VecDeque<(u32, u64)> = std::collections::VecDeque::new();
    let mut meaning_down = false;
    // Inputs of speech taken so far: how long an answer has been owed.
    let mut heard = 0u64;
    for cmd in rx {
        let (event, at) = match cmd {
            Command::Event(e, at) => (Some(e), at),
            Command::Tick(at) => (None, at),
            Command::Configure(f) => {
                f(engine.config_mut());
                record_config(clock, engine.config(), false);
                continue;
            }
            Command::Knowledge(k, lang) => {
                let mut cfg = engine.config().clone();
                cfg.lang = lang;
                engine = engine.restarted(k, cfg);
                wakes.clear();
                owed.clear();
                record_config(clock, engine.config(), true);
                continue;
            }
            Command::Meaning(m) => {
                *models.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(m.clone());
                meaning = Some(m);
                engine.config_mut().meaning = true;
                record_config(clock, engine.config(), false);
                continue;
            }
        };
        let now = at.max(clock);
        if matches!(event, Some(Event::Speech(_))) {
            heard += 1;
        }
        let overdue = owed.front().is_some_and(|(_, at)| heard.saturating_sub(*at) > OWED_INPUTS);
        let mut t = Taking { bible: &bible, hub: &hub, range: &mut range, presentation: &presentation, log: &log, meaning: meaning.is_some(), mtx: &mtx, on_translation: &on_translation, rt: &rt, tx: &tx, started, owed: &mut owed, heard };
        // The answers owed, before anything but words still being said, or once owed too long.
        if waits_for_answers(&event) || overdue {
            settle(clock, &mut engine, &mut wakes, &mut t, &arx, &mut meaning_down);
        }
        // Timers due by now, each at its own time.
        wakes.sort_by_key(|w| w.0);
        while wakes.first().is_some_and(|w| w.0 <= now) {
            let (due, id) = wakes.remove(0);
            clock = due.max(clock);
            take(Event::Timer(id), clock, &mut engine, &mut wakes, &mut t);
        }
        clock = now;
        if let Some(e) = event {
            take(e, now, &mut engine, &mut wakes, &mut t);
        }
    }
}

/// Takes the meaning answers owed, in the order asked (an answer may ask again: that one too). Each
/// reply carries its request's id, so a late one is told apart: one answer arriving after the wait
/// used to be taken as the next request's, and every answer after it was for the wrong request.
fn settle(at: u64, engine: &mut Engine, wakes: &mut Vec<(u64, kairo_detect::engine::TimerId)>, t: &mut Taking, arx: &mpsc::Receiver<(u32, Option<Answer>)>, down: &mut bool) {
    let deadline = std::time::Instant::now() + ANSWER_WAIT;
    while let Some(&(id, _)) = t.owed.front() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match arx.recv_timeout(left) {
            Ok((got, a)) if got == id => {
                t.owed.pop_front();
                if *down {
                    *down = false;
                    t.hub.broadcast(json!({ "type": "detection-problem", "key": "meaning", "ok": true }));
                }
                if let Some(a) = a {
                    take(Event::Answer(a), at, engine, wakes, t);
                }
            }
            // A reply to a request already given up on.
            Ok(_) => {}
            Err(_) => {
                crate::routes::write_debug_global("server:meaning-late", json!({ "waitedMs": ANSWER_WAIT.as_millis() as u64, "owed": t.owed.len() }));
                t.owed.clear();
                if !*down {
                    *down = true;
                    t.hub.broadcast(json!({ "type": "detection-problem", "key": "meaning", "message": "Matching by meaning is running too slowly on this computer; verses said in other words may be missed." }));
                }
                return;
            }
        }
    }
}

/// What taking an input needs besides the engine.
struct Taking<'a> {
    bible: &'a Arc<Bible>,
    hub: &'a Hub,
    range: &'a mut RangeMirror,
    presentation: &'a std::sync::RwLock<Presentation>,
    log: &'a Log,
    meaning: bool,
    mtx: &'a mpsc::Sender<Request>,
    on_translation: &'a std::sync::OnceLock<Box<dyn Fn(&'static str, String) + Send + Sync>>,
    rt: &'a tokio::runtime::Handle,
    tx: &'a mpsc::Sender<Command>,
    started: Instant,
    /// Meaning requests sent and not yet answered, oldest first, with the input of speech they were sent at.
    owed: &'a mut std::collections::VecDeque<(u32, u64)>,
    /// Inputs of speech taken so far.
    heard: u64,
}

/// One input into the engine at `now`. The meaning requests it makes are sent, and owed.
fn take(e: Event, now: u64, engine: &mut Engine, wakes: &mut Vec<(u64, kairo_detect::engine::TimerId)>, t: &mut Taking) {
    {
        // The operator's Next (button, remote, OSC): the verse it puts up is an explicit send.
        let operator_step = matches!(e, Event::RangeNext);
        if matches!(e, Event::Reset) {
            *t.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = ServiceLog::default();
            wakes.clear();
            t.owed.clear();
            // The engine forgets the last service's range: so does what the UI is told.
            *t.range = RangeMirror::default();
            range_state(t.bible, t.hub, t.range);
        }
        record(now, &e);
        // A panic in the engine used to end detection for the rest of the service while
        // transcripts kept scrolling. Now it costs this one stretch of speech: the engine
        // starts again with the same settings, still knowing the verse up and whether people are
        // singing, and the operator is told. A panic turning its decisions into messages is caught
        // too: it used to end the detection thread, and every send after it went nowhere.
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let out = engine.handle(e, now);
            let p = t.presentation.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            apply(t.bible, t.hub, t.range, out.clone(), &p, t.log, operator_step);
            out
        }));
        let out = match step {
            Ok(out) => out,
            Err(p) => {
                let cfg = engine.config().clone();
                crate::routes::write_debug_global("server:detection-panic", json!({ "message": panic_text(&p) }));
                eprintln!("[Detect] the engine panicked and was restarted: {}", panic_text(&p));
                *engine = engine.restarted(t.bible.k(), cfg);
                record_config(now, engine.config(), true);
                wakes.clear();
                t.owed.clear();
                t.hub.broadcast(json!({ "type": "detection-problem", "key": "restarted", "message": "Detection hit an error and started again: the passage it was following is forgotten." }));
                return;
            }
        };
        for e in &out.effects {
            // The passage is being read in another translation: shown in it as if it had been asked
            // for, back to the church's at the next passage (spoken.rs).
            if let (Effect::ReadIn { translation, .. }, Some(f)) = (e, t.on_translation.get()) {
                if let Some(code) = ["KJV", "BSB"].into_iter().find(|c| c.eq_ignore_ascii_case(translation)) {
                    f(code, format!("reading from the {code}"));
                }
            }
            if let Effect::WakeAt { id, at_ms } = e {
                wakes.push((*at_ms, *id));
                // With nothing heard by then, the clock is moved on so the timer still fires.
                let tx = t.tx.clone();
                let (at, started) = (*at_ms, t.started);
                t.rt.spawn(async move {
                    let delay = at.saturating_sub(started.elapsed().as_millis() as u64);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    let _ = tx.send(Command::Tick(at));
                });
            }
        }
        if !t.meaning {
            return;
        }
        for r in out.requests {
            let id = request_id(&r);
            if t.mtx.send(r).is_ok() {
                t.owed.push_back((id, t.heard));
            }
        }
    }
}

/// Every input the engine takes, at the time it takes it, goes in debug.log: a live run replays
/// exactly from it (`replay-log` in kairo-eval), meaning answers and timers included, in the order
/// they arrived. The same inputs always give the same decisions.
fn record(now: u64, e: &Event) {
    if let Ok(event) = serde_json::to_value(e) {
        crate::routes::write_debug_global("server:engine", json!({ "now": now, "event": event }));
    }
}

/// The settings the engine decides with, after each change. `fresh`: the engine started again
/// (new knowledge), forgetting what it was following.
fn record_config(now: u64, c: &Config, fresh: bool) {
    let ids = |s: &std::collections::HashSet<VerseId>| {
        let mut v: Vec<u32> = s.iter().map(|v| v.0).collect();
        v.sort_unstable();
        v
    };
    crate::routes::write_debug_global("server:engine-config", json!({
        "now": now,
        "fresh": fresh,
        "builtFrom": crate::service_bible::built_from(),
        "lang": c.lang,
        "autoSend": c.auto_send,
        "autoCorrect": c.auto_correct,
        "offline": c.offline,
        "meaning": c.meaning,
        "thresholds": c.meaning_thresholds,
        "familiar": ids(&c.learned.familiar),
        "takenDown": ids(&c.learned.taken_down),
        "bookWords": c.learned.book_words().into_iter().map(|(w, b)| (w, b.name())).collect::<Vec<_>>(),
        "hiddenOffers": c.hidden_offers.iter().map(|m| m.wire()).collect::<Vec<_>>(),
    }));
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into())
}

fn answer(m: &MeaningService, r: &Request) -> Answer {
    match r {
        Request::Paraphrase { id, windows, limit, rerank_from } => Answer::Paraphrase {
            id: *id,
            candidates: m
                .paraphrase(windows, *limit, *rerank_from)
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
                if verses.is_empty() {
                    continue;
                }
                // An offer of a kind wrong too often alone (the engine decides: Config::hidden_offers):
                // not shown, still in debug.log.
                if target == Target::Drop {
                    crate::routes::write_debug_global("server:detect", json!({
                        "verses": verses.iter().map(|v| bible.reference(*v)).collect::<Vec<_>>(),
                        "target": "hidden", "method": method.wire(), "score": (score * 100.0).round() / 100.0,
                    }));
                    continue;
                }
                let mut list: Vec<Value> = verses.iter().map(|v| bible.verse_json(*v, score, method.wire())).collect();
                // The operator sees how often this kind of match is right; `similarity` stays the score.
                crate::accuracy::mark(&mut list, method.wire(), target == Target::Screen);
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
                    verses.iter().for_each(|v| crate::learned::shown(*v));
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
                let r = chapter.map(|v| bible.k().verse_ref(v));
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
    // `refs`: the passage's verses, so Next / Back can tell whether what is on screen is part of it.
    let refs: Vec<String> = range.all.iter().map(|v| bible.reference(*v)).collect();
    hub.broadcast(json!({ "type": "range-state", "remaining": remaining, "total": range.all.len(), "next": next, "refs": refs }));
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
