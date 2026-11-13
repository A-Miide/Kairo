//! The service's lists, each a JSON file in the app-data directory, as the
//! Node server kept them: songs, triggers (stage timer, system clock) and
//! service segments (Preservice, Worship, The Word…), each with its timer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::detect::epoch_ms;
use crate::hub::Hub;

fn new_id() -> String {
    crate::util::uuid_v4()
}

fn load(path: &PathBuf) -> Vec<Value> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

fn save(path: &PathBuf, items: &[Value]) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec_pretty(items) {
        if let Err(e) = std::fs::write(path, b) {
            eprintln!("[Library] Save failed ({}): {e}", path.display());
        }
    }
}

// ── Songs ───────────────────────────────────────────────────────────────────

pub const SONG_CATEGORIES: [&str; 3] = ["worship", "praise", "hymn"];

pub struct Songs {
    path: PathBuf,
    items: Mutex<Vec<Value>>,
}

fn category(v: &Value) -> Value {
    v.as_str().filter(|c| SONG_CATEGORIES.contains(c)).map_or(Value::Null, |c| json!(c))
}

impl Songs {
    pub fn open(app_data: &std::path::Path) -> Songs {
        let path = app_data.join("songs").join("songs.json");
        Songs { items: Mutex::new(load(&path)), path }
    }

    pub fn list(&self) -> Vec<Value> {
        crate::util::lock(&self.items).clone()
    }

    pub fn add(&self, r: &Value) -> Value {
        let song = json!({
            "id": new_id(),
            "title": r["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("Untitled"),
            "author": r["author"].as_str().unwrap_or(""),
            "year": if r["year"].is_null() || r["year"] == "" { Value::Null } else { r["year"].clone() },
            "themeId": if r["themeId"].is_null() || r["themeId"] == "" { Value::Null } else { r["themeId"].clone() },
            "category": category(&r["category"]),
            "blocks": if r["blocks"].is_array() { r["blocks"].clone() } else { json!([]) },
        });
        let mut items = crate::util::lock(&self.items);
        items.push(song.clone());
        save(&self.path, &items);
        song
    }

    pub fn update(&self, id: &str, r: &Value) -> Option<Value> {
        let mut items = crate::util::lock(&self.items);
        let song = items.iter_mut().find(|s| s["id"] == id)?;
        for k in ["title", "author", "year", "themeId", "blocks"] {
            if let Some(v) = r.get(k) {
                song[k] = v.clone();
            }
        }
        if let Some(c) = r.get("category") {
            song["category"] = category(c);
        }
        let out = song.clone();
        save(&self.path, &items);
        Some(out)
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut items = crate::util::lock(&self.items);
        let before = items.len();
        items.retain(|s| s["id"] != id);
        save(&self.path, &items);
        items.len() != before
    }
}

// ── Triggers ────────────────────────────────────────────────────────────────

pub struct Triggers {
    path: PathBuf,
    items: Mutex<Vec<Value>>,
    active: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    hub: Arc<Hub>,
    /// Milliseconds added to a running timer's end ("Add 5 min"), by trigger id.
    extra: Arc<Mutex<HashMap<String, i64>>>,
}

/// "HH:MM" today, as epoch milliseconds.
fn end_at_time(hhmm: &str) -> Option<u64> {
    let (h, m) = hhmm.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    if h > 23 || m > 59 {
        return None;
    }
    crate::clock::today_at(h, m)
}

fn end_at(t: &Value) -> Option<u64> {
    if t["params"]["mode"] == "duration" {
        let sec = t["params"]["durationSec"].as_f64().or_else(|| t["params"]["durationSec"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
        return (sec > 0.0).then(|| epoch_ms() + (sec * 1000.0) as u64);
    }
    end_at_time(t["params"]["endAtTime"].as_str().unwrap_or(""))
}

pub fn trigger_types() -> Value {
    json!([
        { "id": "stage-timer", "label": "Stage Timer", "paramsSchema": {
            "mode": { "type": "string", "label": "Mode (endAt/duration)", "default": "endAt" },
            "endAtTime": { "type": "time", "label": "Ends at", "default": "" },
            "durationSec": { "type": "number", "label": "Duration (seconds)", "default": 0 },
        } },
        { "id": "clock-message", "label": "System Clock", "paramsSchema": {
            "format": { "type": "string", "label": "Format (24h/12h)", "default": "24h" },
        } },
    ])
}

impl Triggers {
    pub fn open(app_data: &std::path::Path, hub: Arc<Hub>) -> Triggers {
        let path = app_data.join("triggers").join("triggers.json");
        Triggers { items: Mutex::new(load(&path)), path, active: Mutex::new(HashMap::new()), extra: Arc::new(Mutex::new(HashMap::new())), hub }
    }

    pub fn list(&self) -> Vec<Value> {
        crate::util::lock(&self.items).clone()
    }

    pub fn is_active(&self, id: &str) -> bool {
        crate::util::lock(&self.active).contains_key(id)
    }

    pub fn add(&self, r: &Value) -> Value {
        let type_label = match r["typeId"].as_str() {
            Some("stage-timer") => "Stage Timer",
            Some("clock-message") => "System Clock",
            _ => "Trigger",
        };
        let t = json!({
            "id": new_id(),
            "typeId": r["typeId"],
            "label": r["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(type_label),
            "params": if r["params"].is_object() { r["params"].clone() } else { json!({}) },
            "lastFiredAt": null,
        });
        let mut items = crate::util::lock(&self.items);
        items.push(t.clone());
        save(&self.path, &items);
        t
    }

    pub fn update(&self, id: &str, r: &Value) -> Option<Value> {
        let mut items = crate::util::lock(&self.items);
        let t = items.iter_mut().find(|t| t["id"] == id)?;
        if let Some(l) = r.get("label") {
            t["label"] = l.clone();
        }
        if let Some(Value::Object(p)) = r.get("params") {
            let mut merged = t["params"].as_object().cloned().unwrap_or_default();
            for (k, v) in p {
                merged.insert(k.clone(), v.clone());
            }
            t["params"] = Value::Object(merged);
        }
        let out = t.clone();
        save(&self.path, &items);
        Some(out)
    }

    /// More time for a running timer: its end moves out by `ms` and the next tick says so. False when it isn't running.
    pub fn extend(&self, id: &str, ms: i64) -> bool {
        if !self.is_active(id) {
            return false;
        }
        *crate::util::lock(&self.extra).entry(id.to_string()).or_insert(0) += ms;
        true
    }

    pub fn stop(&self, id: &str) {
        crate::util::lock(&self.extra).remove(id);
        if let Some(h) = crate::util::lock(&self.active).remove(id) {
            h.abort();
        }
        if let Some(t) = crate::util::lock(&self.items).iter().find(|t| t["id"] == id) {
            self.hub.broadcast(json!({ "type": "action", "actionId": t["typeId"], "target": "viewer", "triggerId": id, "payload": { "cleared": true } }));
        }
    }

    pub fn remove(&self, id: &str) -> bool {
        self.stop(id);
        let mut items = crate::util::lock(&self.items);
        let before = items.len();
        items.retain(|t| t["id"] != id);
        save(&self.path, &items);
        items.len() != before
    }

    pub fn fire(&self, id: &str) -> Value {
        let Some(t) = crate::util::lock(&self.items).iter().find(|t| t["id"] == id).cloned() else { return json!({ "error": "No such trigger" }) };
        let kind = t["typeId"].as_str().unwrap_or("").to_string();
        if kind != "stage-timer" && kind != "clock-message" {
            return json!({ "error": format!("Unknown trigger type \"{kind}\"") });
        }
        if kind == "stage-timer" {
            if t["params"]["mode"] == "duration" {
                if end_at(&t).is_none() {
                    return json!({ "error": "Enter a duration" });
                }
            } else {
                match end_at_time(t["params"]["endAtTime"].as_str().unwrap_or("")) {
                    None => return json!({ "error": "Enter an end time" }),
                    Some(at) if at <= epoch_ms() => return json!({ "error": "That time has already passed today — pick a later time" }),
                    _ => {}
                }
            }
        }
        self.stop(id); // re-firing replaces the running one
        let fired = {
            let mut items = crate::util::lock(&self.items);
            // Deleted between the check above and here: nothing to fire.
            let Some(t) = items.iter_mut().find(|t| t["id"] == id) else { return json!({ "error": "No such trigger" }) };
            t["lastFiredAt"] = json!(epoch_ms());
            let out = t.clone();
            save(&self.path, &items);
            out
        };
        let hub = self.hub.clone();
        let (tid, label) = (id.to_string(), fired["label"].clone());
        let handle = if kind == "stage-timer" {
            let end = end_at(&fired).unwrap_or_else(epoch_ms);
            let total = end as i64 - epoch_ms() as i64;
            let style = fired["params"]["style"].clone();
            let extra = self.extra.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tick.tick().await;
                    let more = crate::util::lock(&extra).get(&tid).copied().unwrap_or(0);
                    let remaining = end as i64 + more - epoch_ms() as i64;
                    hub.broadcast(json!({ "type": "action", "actionId": "stage-timer", "target": "viewer", "triggerId": tid,
                        "payload": { "remainingMs": remaining, "label": label, "style": style, "totalMs": total + more } }));
                }
            })
        } else {
            let format = fired["params"]["format"].as_str().unwrap_or("24h").to_string();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tick.tick().await;
                    hub.broadcast(json!({ "type": "action", "actionId": "clock-message", "target": "viewer", "triggerId": tid,
                        "payload": { "now": epoch_ms(), "format": format, "label": label } }));
                }
            })
        };
        crate::util::lock(&self.active).insert(id.to_string(), handle);
        json!({ "ok": true, "trigger": fired })
    }

    /// Clock messages on screen (cleared with the timer layer).
    pub fn active_clocks(&self) -> Vec<String> {
        let active = crate::util::lock(&self.active);
        crate::util::lock(&self.items).iter().filter(|t| t["typeId"] == "clock-message" && active.contains_key(t["id"].as_str().unwrap_or(""))).filter_map(|t| t["id"].as_str().map(str::to_string)).collect()
    }
}

// ── Segments ────────────────────────────────────────────────────────────────

const DEFAULT_SEGMENTS: [&str; 6] = ["Preservice", "Prayer", "Worship", "The Word", "Testimony", "Ministration"];
const PRESERVICE_2: &str = "Preservice 2";
const PACK: &str = include_str!("../data/announcement_pack.json");

fn is_preservice(name: &str) -> bool {
    let n = name.trim().to_lowercase().replace(' ', "");
    n == "preservice" || n == "pre-service"
}

/// The pre-service announcement loop (scenes and pace).
fn preservice_loop(key: &str) -> Map<String, Value> {
    let pack: Value = serde_json::from_str(PACK).expect("bundled announcement pack");
    let mut m = Map::new();
    m.insert("scenes".into(), pack[key].clone());
    m.insert("scenePace".into(), pack["pace"].clone());
    m
}

pub struct Segments {
    path: PathBuf,
    items: Mutex<Vec<Value>>,
    triggers: Arc<Triggers>,
    pub styles: TimerStyles,
}

/// Saved looks for a timer: the slides (scenes), their pace, and the design under them. The two designs Kairo shipped
/// as "Preservice" and "Preservice 2" are the first two; anything edited can be saved as another and applied to any timer.
/// (id, name, key in the announcement pack): a welcome with invitations, a plain starting-soon countdown, Reach
/// (a raised hand in blue light, with the big words behind it), Highlands (a lake and a mountain path in blue, one
/// serif and one sans) and Ember (warm red, the tree in front of the words).
const BUILT_IN_TEMPLATES: [(&str, &str, &str); 5] = [
    ("builtin-welcome-to-church", "Welcome and invite", "preservice"),
    ("builtin-welcome", "Starting soon", "preservice2"),
    ("builtin-reach", "Reach", "reach"),
    ("builtin-highlands", "Highlands", "highlands"),
    ("builtin-ember", "Ember", "ember"),
];

/// The names of a timer's slides, in order: two designs with the same ones are the same design.
fn scene_names(v: &Value) -> Vec<String> {
    v["scenes"].as_array().into_iter().flatten().map(|s| s["name"].as_str().unwrap_or("").to_string()).collect()
}

pub struct TimerStyles {
    path: PathBuf,
    items: Mutex<Vec<Value>>,
}

impl TimerStyles {
    pub fn open(app_data: &std::path::Path) -> TimerStyles {
        let path = app_data.join("segments").join("timer_styles.json");
        let mut items = load(&path);
        // The built-in templates are Kairo's own: a version that ships replaces an older copy, and one it no longer ships goes.
        let before = items.len();
        items.retain(|s| s["builtIn"] != true || BUILT_IN_TEMPLATES.iter().any(|(id, _, _)| s["id"] == *id));
        let mut changed = items.len() != before;
        for (id, name, key) in BUILT_IN_TEMPLATES {
            let mut style = Value::Object(preservice_loop(key));
            style["id"] = json!(id);
            style["name"] = json!(name);
            style["builtIn"] = json!(true);
            match items.iter_mut().find(|s| s["id"] == id) {
                Some(have) if *have == style => {}
                Some(have) => {
                    *have = style;
                    changed = true;
                }
                None => {
                    items.push(style);
                    changed = true;
                }
            }
        }
        if changed {
            save(&path, &items);
        }
        TimerStyles { path, items: Mutex::new(items) }
    }

    pub fn list(&self) -> Vec<Value> {
        crate::util::lock(&self.items).clone()
    }

    /// Saves `from` (a segment, or any value with scenes / scenePace / themeId / slideStyles) as a style called `name`.
    pub fn add(&self, name: &str, from: &Value) -> Value {
        let style = json!({
            "id": new_id(), "name": name,
            "scenes": from["scenes"].clone(), "scenePace": from["scenePace"].clone(),
            "themeId": from["themeId"].clone(), "slideStyles": if from["slideStyles"].is_object() { from["slideStyles"].clone() } else { json!({}) },
        });
        let mut items = crate::util::lock(&self.items);
        items.push(style.clone());
        save(&self.path, &items);
        style
    }

    pub fn get(&self, id: &str) -> Option<Value> {
        crate::util::lock(&self.items).iter().find(|s| s["id"] == id).cloned()
    }

    /// The two built-in styles stay; any other can be removed.
    pub fn remove(&self, id: &str) -> bool {
        let mut items = crate::util::lock(&self.items);
        let before = items.len();
        items.retain(|s| !(s["id"] == id && s["builtIn"] != true));
        save(&self.path, &items);
        items.len() != before
    }
}

impl Segments {
    pub fn open(app_data: &std::path::Path, triggers: Arc<Triggers>) -> Segments {
        let path = app_data.join("segments").join("segments.json");
        let mut items = load(&path);
        for s in items.iter_mut() {
            for (k, d) in [("themeId", Value::Null), ("slideStyles", json!({})), ("scenes", json!([])), ("scenePace", Value::Null)] {
                if s.get(k).is_none() {
                    s[k] = d;
                }
            }
        }
        let me = Segments { path, items: Mutex::new(items), triggers, styles: TimerStyles::open(app_data) };
        me.seed();
        me
    }

    fn seed(&self) {
        let mut changed = false;
        {
            let mut items = crate::util::lock(&self.items);
            // A segment live when the app last closed isn't running now.
            for s in items.iter_mut() {
                if s["status"] == "live" && !self.triggers.is_active(s["triggerId"].as_str().unwrap_or("")) {
                    s["status"] = json!("pending");
                    changed = true;
                }
            }
            for s in items.iter_mut() {
                let empty = s["scenes"].as_array().is_none_or(|a| a.is_empty()) && s["themeId"].is_null();
                if is_preservice(s["name"].as_str().unwrap_or("")) && s["loopSeeded"] != true && empty {
                    for (k, v) in preservice_loop("preservice") {
                        s[k.as_str()] = v;
                    }
                    s["loopSeeded"] = json!(true);
                    changed = true;
                }
            }
        }
        // "Preservice 2" is a style of the one Preservice timer now. An existing card that isn't running becomes a saved
        // style (so nothing the church edited is lost) and leaves the list.
        let second = crate::util::lock(&self.items).iter()
            .find(|s| s["name"].as_str().unwrap_or("").trim().eq_ignore_ascii_case(PRESERVICE_2) && s["status"] != "live").cloned();
        if let Some(sec) = second {
            // Kept as a template only when it isn't one we already have (a church that edited it, not the stock one).
            if !self.styles.list().iter().any(|t| scene_names(t) == scene_names(&sec)) {
                self.styles.add("Preservice 2", &sec);
            }
            self.triggers.remove(sec["triggerId"].as_str().unwrap_or(""));
            let mut items = crate::util::lock(&self.items);
            items.retain(|x| x["id"] != sec["id"]);
            for (i, seg) in items.iter_mut().enumerate() {
                seg["order"] = json!(i);
            }
            changed = true;
        }
        if changed {
            save(&self.path, &crate::util::lock(&self.items));
        }
        if crate::util::lock(&self.items).is_empty() {
            for name in DEFAULT_SEGMENTS {
                let mut opts = Map::new();
                if is_preservice(name) {
                    opts = preservice_loop("preservice");
                    opts.insert("loopSeeded".into(), json!(true));
                }
                self.add(name, &Value::Object(opts));
            }
        }
    }

    fn with_trigger(&self, s: &Value) -> Value {
        let mut out = s.clone();
        out["trigger"] = self.triggers.list().into_iter().find(|t| t["id"] == s["triggerId"]).unwrap_or(Value::Null);
        out
    }

    pub fn add(&self, name: &str, opts: &Value) -> Value {
        let trigger = self.triggers.add(&json!({ "typeId": "stage-timer", "label": name }));
        let mut items = crate::util::lock(&self.items);
        let or = |k: &str, d: Value| if opts[k].is_null() { d } else { opts[k].clone() };
        let mut seg = json!({
            "id": new_id(), "name": name, "order": items.len(), "triggerId": trigger["id"], "status": "pending",
            "themeId": or("themeId", Value::Null), "slideStyles": or("slideStyles", json!({})),
            "scenes": or("scenes", json!([])), "scenePace": or("scenePace", Value::Null),
        });
        for flag in ["loopSeeded", "preservice2Seeded"] {
            if opts[flag] == true {
                seg[flag] = json!(true);
            }
        }
        items.push(seg.clone());
        save(&self.path, &items);
        drop(items);
        self.with_trigger(&seg)
    }

    pub fn list(&self) -> Vec<Value> {
        let mut items = crate::util::lock(&self.items).clone();
        items.sort_by_key(|s| s["order"].as_i64().unwrap_or(0));
        items.iter().map(|s| self.with_trigger(s)).collect()
    }

    pub fn update(&self, id: &str, r: &Value) -> Option<Value> {
        let mut items = crate::util::lock(&self.items);
        let s = items.iter_mut().find(|s| s["id"] == id)?;
        let trigger_id = s["triggerId"].as_str().unwrap_or("").to_string();
        if let Some(n) = r.get("name") {
            s["name"] = n.clone();
            self.triggers.update(&trigger_id, &json!({ "label": n }));
        }
        if let Some(p) = r.get("params") {
            self.triggers.update(&trigger_id, &json!({ "params": p }));
        }
        for k in ["themeId", "slideStyles", "scenes", "scenePace", "fixedAt", "included", "templateId"] {
            if let Some(v) = r.get(k) {
                s[k] = v.clone();
            }
        }
        let out = s.clone();
        save(&self.path, &items);
        drop(items);
        Some(self.with_trigger(&out))
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut items = crate::util::lock(&self.items);
        let Some(s) = items.iter().find(|s| s["id"] == id).cloned() else { return false };
        self.triggers.remove(s["triggerId"].as_str().unwrap_or(""));
        items.retain(|x| x["id"] != id);
        for (i, seg) in items.iter_mut().enumerate() {
            seg["order"] = json!(i);
        }
        save(&self.path, &items);
        true
    }

    pub fn reorder(&self, ids: &[Value]) -> Vec<Value> {
        {
            let mut items = crate::util::lock(&self.items);
            for (i, id) in ids.iter().enumerate() {
                if let Some(s) = items.iter_mut().find(|s| s["id"] == *id) {
                    s["order"] = json!(i);
                }
            }
            save(&self.path, &items);
        }
        self.list()
    }

    pub fn start(&self, id: &str, end_at_time: Option<&Value>) -> Value {
        let (trigger_id, was_live) = {
            let items = crate::util::lock(&self.items);
            let Some(s) = items.iter().find(|s| s["id"] == id) else { return json!({ "error": "No such segment" }) };
            let was = items.iter().find(|x| x["status"] == "live" && x["id"] != id).map(|x| (x["id"].clone(), x["triggerId"].as_str().unwrap_or("").to_string()));
            (s["triggerId"].as_str().unwrap_or("").to_string(), was)
        };
        if let Some(e) = end_at_time {
            self.triggers.update(&trigger_id, &json!({ "params": { "endAtTime": e } }));
        }
        let result = self.triggers.fire(&trigger_id);
        if result.get("error").is_some() {
            return result;
        }
        if let Some((_, t)) = &was_live {
            self.triggers.stop(t);
        }
        let out = {
            let mut items = crate::util::lock(&self.items);
            if let Some((wid, _)) = &was_live {
                if let Some(w) = items.iter_mut().find(|x| x["id"] == *wid) {
                    w["status"] = json!("done");
                    w["endedAt"] = json!(epoch_ms());
                }
            }
            let s = items.iter_mut().find(|s| s["id"] == id).expect("found above");
            s["status"] = json!("live");
            s["startedAt"] = json!(epoch_ms());
            if let Some(o) = s.as_object_mut() {
                o.remove("endedAt");
                o.remove("extendedSec");
            }
            let out = s.clone();
            save(&self.path, &items);
            out
        };
        json!({ "ok": true, "segment": self.with_trigger(&out) })
    }

    fn set_status(&self, id: &str, status: &str) -> Value {
        let trigger_id = {
            let items = crate::util::lock(&self.items);
            let Some(s) = items.iter().find(|s| s["id"] == id) else { return json!({ "error": "No such segment" }) };
            s["triggerId"].as_str().unwrap_or("").to_string()
        };
        self.triggers.stop(&trigger_id);
        let out = {
            let mut items = crate::util::lock(&self.items);
            let s = items.iter_mut().find(|s| s["id"] == id).expect("found above");
            s["status"] = json!(status);
            // How long it really ran: kept when it is marked done, forgotten when it goes back to waiting.
            if status == "done" {
                s["endedAt"] = json!(epoch_ms());
            } else if let Some(o) = s.as_object_mut() {
                o.remove("startedAt");
                o.remove("endedAt");
                o.remove("extendedSec");
            }
            let out = s.clone();
            save(&self.path, &items);
            out
        };
        json!({ "ok": true, "segment": self.with_trigger(&out) })
    }

    /// Gives a segment a saved style's slides, pace and design (its length, name and times stay).
    pub fn apply_style(&self, id: &str, style_id: &str) -> Value {
        let Some(style) = self.styles.get(style_id) else { return json!({ "error": "No such style" }) };
        let out = {
            let mut items = crate::util::lock(&self.items);
            let Some(s) = items.iter_mut().find(|s| s["id"] == id) else { return json!({ "error": "No such segment" }) };
            for k in ["scenes", "scenePace", "themeId", "slideStyles"] {
                s[k] = if style[k].is_null() && k == "slideStyles" { json!({}) } else { style[k].clone() };
            }
            s["templateId"] = json!(style_id);
            let out = s.clone();
            save(&self.path, &items);
            out
        };
        json!({ "ok": true, "segment": self.with_trigger(&out) })
    }

    /// "Add 5 min": the live segment gets `seconds` more (or fewer, negative), on screen at the next tick.
    pub fn extend(&self, id: &str, seconds: i64) -> Value {
        let trigger_id = {
            let items = crate::util::lock(&self.items);
            let Some(s) = items.iter().find(|s| s["id"] == id) else { return json!({ "error": "No such segment" }) };
            if s["status"] != "live" {
                return json!({ "error": "That segment isn't running" });
            }
            s["triggerId"].as_str().unwrap_or("").to_string()
        };
        if !self.triggers.extend(&trigger_id, seconds * 1000) {
            return json!({ "error": "That segment isn't running" });
        }
        let out = {
            let mut items = crate::util::lock(&self.items);
            let s = items.iter_mut().find(|s| s["id"] == id).expect("found above");
            let so_far = s["extendedSec"].as_i64().unwrap_or(0);
            s["extendedSec"] = json!(so_far + seconds);
            let out = s.clone();
            save(&self.path, &items);
            out
        };
        json!({ "ok": true, "segment": self.with_trigger(&out) })
    }

    pub fn stop(&self, id: &str, mark_done: bool) -> Value {
        self.set_status(id, if mark_done { "done" } else { "pending" })
    }

    pub fn reset(&self, id: &str) -> Value {
        self.set_status(id, "pending")
    }

    pub fn has_live(&self) -> bool {
        crate::util::lock(&self.items).iter().any(|s| s["status"] == "live")
    }

    pub fn stop_live(&self) {
        let live = crate::util::lock(&self.items).iter().find(|s| s["status"] == "live").and_then(|s| s["id"].as_str().map(str::to_string));
        if let Some(id) = live {
            self.stop(&id, true);
        }
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn built_in_templates_are_refreshed_and_keep_their_own_slides() {
        let dir = std::env::temp_dir().join(format!("kairo-templates-{}", new_id()));
        std::fs::create_dir_all(dir.join("segments")).unwrap();
        // An install from before: an old copy of a built-in, one Kairo no longer ships, and one the church saved itself.
        let old = json!([
            {"id": "builtin-welcome", "name": "Starting soon", "builtIn": true, "scenes": []},
            {"id": "builtin-gone", "name": "Gone", "builtIn": true, "scenes": []},
            {"id": "mine", "name": "Mine", "scenes": [{"name": "A"}]}
        ]);
        std::fs::write(dir.join("segments").join("timer_styles.json"), old.to_string()).unwrap();
        let styles = TimerStyles::open(&dir).list();
        let names: Vec<&str> = styles.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Starting soon", "Mine", "Welcome and invite", "Reach", "Highlands", "Ember"]);
        for s in styles.iter().filter(|s| s["builtIn"] == true) {
            let scenes = s["scenes"].as_array().unwrap();
            assert!(scenes.len() >= 3, "{} has {} slides", s["name"], scenes.len());
            for sc in scenes {
                assert!(sc["layers"].as_array().unwrap().iter().any(|l| l["binding"] == "timer"), "{} / {} has no countdown", s["name"], sc["name"]);
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
