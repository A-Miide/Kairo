//! Everything the UI windows are told, and what's live on the output so a
//! window that connects (or reconnects) catches up — the same state the
//! Node server kept: the slide or verse up, media, timer, the range's next
//! verse, the monitor message, the Bible-over-slide layering, and the
//! history Back steps through.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::{json, Value};
use tokio::sync::broadcast;

const SCREEN_HISTORY_MAX: usize = 20;

#[derive(Clone, Debug)]
pub struct Underneath {
    pub verse: Value,
    pub look: Value,
    pub next: Value,
}

#[derive(Clone, Debug)]
pub struct Earlier {
    pub slide: Option<Value>,
    pub underneath: Option<Underneath>,
    pub bible_on_top: bool,
}

#[derive(Default)]
pub struct Live {
    pub slide: Option<Value>,
    pub media: Option<Value>,
    pub timer: Option<Value>,
    pub range_state: Option<Value>,
    pub monitor_message: String,
    /// Who the message is for: "monitors", "screens" or "all".
    pub monitor_message_to: String,
    pub connection_state: String,
    pub bible_on_top: bool,
    pub underneath: Option<Underneath>,
    /// The detection(viewer) message on screen, and what was before it.
    pub screen_now: Option<Value>,
    pub history: VecDeque<Earlier>,
    /// The last per-output themes sent ("look-update" with themes, look and layers): a window
    /// that connects later gets it with the rest of what's on screen.
    pub looks: Value,
}

pub struct Hub {
    tx: broadcast::Sender<String>,
    pub live: Mutex<Live>,
}

fn screen_key(m: Option<&Value>) -> String {
    m.map(|m| format!("{}|{}", m["verses"][0]["reference"].as_str().unwrap_or(""), m["verses"][0]["text"].as_str().unwrap_or(""))).unwrap_or_default()
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Hub {
        let (tx, _) = broadcast::channel(1024);
        Hub { tx, live: Mutex::new(Live { connection_state: "disconnected".into(), ..Default::default() }) }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Tells every window, and remembers what's live.
    pub fn broadcast(&self, data: Value) {
        {
            let mut l = crate::util::lock(&self.live);
            let ty = data["type"].as_str().unwrap_or("");
            let viewer = data["target"] == "viewer";
            if ty == "detection" && viewer && data["verses"].as_array().is_some_and(|v| !v.is_empty()) {
                note_screen_change(&mut l, Some(&data), data["restoredByBack"] == true);
            } else if ty == "clear-layer" && matches!(data["layer"].as_str(), Some("slide") | Some("all")) {
                note_screen_change(&mut l, None, data["restoredByBack"] == true);
            }
            match ty {
                "detection" if viewer => l.slide = Some(data.clone()),
                "range-active" => l.slide = Some(data.clone()),
                "media" => l.media = Some(data.clone()),
                "timer-slide" => l.timer = if data["clear"] == true { None } else { Some(data.clone()) },
                "range-state" => l.range_state = (data["remaining"].as_u64().unwrap_or(0) > 0).then(|| data.clone()),
                "action" if data["actionId"] == "stage-timer" && data["payload"]["remainingMs"].as_f64().is_some_and(|ms| ms <= 0.0) => l.timer = None,
                "clear-layer" => {
                    let layer = data["layer"].as_str().unwrap_or("");
                    if layer == "slide" || layer == "all" {
                        l.slide = None;
                    }
                    if layer == "media" || layer == "all" {
                        l.media = None;
                    }
                    if layer == "timer" || layer == "all" {
                        l.timer = None;
                    }
                }
                "connection-state" => l.connection_state = data["state"].as_str().unwrap_or("").to_string(),
                "monitor-message" => {
                    l.monitor_message = data["text"].as_str().unwrap_or("").to_string();
                    l.monitor_message_to = data["to"].as_str().unwrap_or("monitors").to_string();
                }
                _ => {}
            }
        }
        let _ = self.tx.send(data.to_string());
    }

    /// Keeps one part of the per-output settings (layers, keying, fit, frames, monitors, modes) in the look-update a
    /// window gets when it connects. Only the full theme push used to be kept, so a switch changed afterwards (Scripture
    /// off for an output) was sent to the open windows and then forgotten: a window that reopened or reconnected was
    /// handed the startup switches, and the verse came back (owner, 19 Nov 2026).
    pub fn remember_look_part(&self, part: &str, value: &Value) {
        let mut l = crate::util::lock(&self.live);
        if !l.looks.is_object() {
            l.looks = json!({ "type": "look-update" });
        }
        l.looks[part] = value.clone();
    }

    /// A full theme push replaces what is kept, but not the parts it leaves out (a theme push without layers kept no
    /// layers, so the switches fell back to their defaults for a window that connected afterwards).
    pub fn remember_looks(&self, mut msg: Value) {
        let mut l = crate::util::lock(&self.live);
        if let Some(old) = l.looks.as_object() {
            for (k, v) in old {
                if k != "type" && msg.get(k).is_none_or(|m| m.is_null()) {
                    msg[k.as_str()] = v.clone();
                }
            }
        }
        l.looks = msg;
    }

    /// What a window that just connected needs to catch up.
    pub fn catch_up(&self, worker_ready: bool, animation: Value, speed: Value) -> Vec<String> {
        let l = crate::util::lock(&self.live);
        let mut out = vec![
            json!({ "type": "worker-status", "ready": worker_ready }),
            json!({ "type": "connection-state", "state": l.connection_state }),
            json!({ "type": "layer-state", "bibleOnTop": l.bible_on_top, "slideUnderneath": l.underneath.is_some() }),
            json!({ "type": "output-transition", "animation": animation, "speed": speed }),
        ];
        // Before the slide, so it is drawn in its output's theme.
        if !l.looks.is_null() {
            out.push(l.looks.clone());
        }
        out.extend([&l.slide, &l.media, &l.timer, &l.range_state].into_iter().flatten().cloned());
        if !l.monitor_message.is_empty() {
            out.push(json!({ "type": "monitor-message", "text": l.monitor_message, "to": if l.monitor_message_to.is_empty() { "monitors" } else { l.monitor_message_to.as_str() } }));
        }
        out.into_iter().map(|v| v.to_string()).collect()
    }

    pub fn layer_state(&self) {
        let (on_top, under) = {
            let l = crate::util::lock(&self.live);
            (l.bible_on_top, l.underneath.is_some())
        };
        self.broadcast(json!({ "type": "layer-state", "bibleOnTop": on_top, "slideUnderneath": under }));
    }

    pub fn set_bible_on_top(&self, on: bool) {
        let changed = {
            let mut l = crate::util::lock(&self.live);
            let changed = l.bible_on_top != on;
            l.bible_on_top = on;
            changed
        };
        if changed {
            self.layer_state();
        }
    }
}

/// `restored`: Back putting an earlier screen up again. It is what's on screen now, but not a new
/// step in the history. (Read from the message itself: a shared flag let a detection that went up
/// during Back be left out of the history.)
fn note_screen_change(l: &mut Live, next: Option<&Value>, restored: bool) {
    if restored {
        l.screen_now = next.cloned();
        return;
    }
    if screen_key(next) == screen_key(l.screen_now.as_ref()) {
        return;
    }
    let earlier = Earlier { slide: l.screen_now.take(), underneath: l.underneath.clone(), bible_on_top: l.bible_on_top };
    l.history.push_back(earlier);
    if l.history.len() > SCREEN_HISTORY_MAX {
        l.history.pop_front();
    }
    l.screen_now = next.cloned();
    // A verse from another passage, sent by anyone (the operator too, with Auto-send off), ends a
    // translation the preacher asked for (spoken.rs).
    // Not a repaint of what is up in another translation ("retranslated").
    if let Some(v) = next.filter(|n| n["retranslated"] != true).and_then(|n| n["verses"].get(0)) {
        if let (Some(book), Some(chapter)) = (v["book"].as_str(), v["chapter"].as_u64()) {
            let _ = crate::spoken::on_send(book, chapter);
        }
    }
}

#[cfg(test)]
mod look_cache_tests {
    use super::*;

    fn kept(hub: &Hub) -> Value {
        let msgs = hub.catch_up(true, Value::Null, Value::Null);
        msgs.iter().map(|m| serde_json::from_str::<Value>(m).unwrap()).find(|m| m["type"] == "look-update").unwrap_or(Value::Null)
    }

    #[test]
    fn a_window_that_connects_gets_the_switches_changed_after_the_theme_push() {
        let hub = Hub::new();
        hub.remember_looks(json!({ "type": "look-update", "themes": { "display-1": {} }, "look": null, "layers": { "display-1": { "bible": true } } }));
        // Scripture switched off for the External Display afterwards (a layers-only push).
        hub.remember_look_part("layers", &json!({ "display-1": { "bible": false } }));
        assert_eq!(kept(&hub)["layers"]["display-1"]["bible"], false, "a reopened window must not get Scripture back");
        assert!(kept(&hub)["themes"].is_object(), "the themes are kept too");
    }

    #[test]
    fn a_theme_push_without_switches_keeps_the_switches() {
        let hub = Hub::new();
        hub.remember_look_part("layers", &json!({ "monitor-1": { "bible": false } }));
        hub.remember_look_part("keying", &json!({ "ndi-1": "alpha" }));
        hub.remember_looks(json!({ "type": "look-update", "themes": { "monitor-1": {} }, "look": null, "layers": null }));
        let k = kept(&hub);
        assert_eq!(k["layers"]["monitor-1"]["bible"], false);
        assert_eq!(k["keying"]["ndi-1"], "alpha");
    }
}
