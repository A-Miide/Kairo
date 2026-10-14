//! OSC remote control: Companion / Stream Deck, lighting desks or another
//! show controller send `/kairo/<action>` over UDP and Kairo does what the
//! matching button does (the same actions as the Controls menu and hotkeys).
//!
//! Off until the operator turns it on (Settings → Remote control): anything
//! on the network can send to it, so it only ever runs the named actions.

use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::net::UdpSocket;

use crate::Shared;

pub const DEFAULT_PORT: u16 = 8765;
/// Every action a remote may run; the UI maps each to its button.
pub const ACTIONS: &[&str] = &[
    "toggle-listening",
    "listen-start",
    "listen-stop",
    "slide-next",
    "slide-prev",
    "range-next",
    "range-end",
    "output-back",
    "clear-bible",
    "clear-slide",
    "clear-media",
    "clear-timer",
    "clear-all",
];
const PREFIX: &str = "/kairo/";
const SETTINGS_EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default)]
struct Status {
    listening_on: Option<u16>,
    error: Option<String>,
    last: Option<String>,
}

static STATUS: Mutex<Status> = Mutex::new(Status { listening_on: None, error: None, last: None });

pub fn status() -> Value {
    let s = STATUS.lock().map(|s| s.clone()).unwrap_or_default();
    json!({ "listening": s.listening_on.is_some(), "port": s.listening_on, "error": s.error, "last": s.last, "actions": ACTIONS })
}

fn set_status(f: impl FnOnce(&mut Status)) {
    if let Ok(mut s) = STATUS.lock() {
        f(&mut s);
    }
}

/// Runs for the life of the server, following the settings.
pub async fn run(app: Shared) {
    let mut socket: Option<(u16, UdpSocket)> = None;
    let mut buf = vec![0u8; 8192];
    loop {
        let enabled = app.settings.bool("oscEnabled").await.unwrap_or(false);
        let port = app.settings.get("oscPort").await.and_then(|v| v.as_u64()).and_then(|p| u16::try_from(p).ok()).filter(|p| *p >= 1024).unwrap_or(DEFAULT_PORT);
        if !enabled {
            if socket.take().is_some() {
                eprintln!("[OSC] Off");
            }
            set_status(|s| *s = Status::default());
            tokio::time::sleep(SETTINGS_EVERY).await;
            continue;
        }
        if socket.as_ref().is_none_or(|(p, _)| *p != port) {
            socket = None;
            match UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
                Ok(s) => {
                    eprintln!("[OSC] Listening on UDP port {port}");
                    set_status(|st| {
                        st.listening_on = Some(port);
                        st.error = None;
                    });
                    socket = Some((port, s));
                }
                Err(e) => {
                    let msg = format!("Port {port} is in use or unavailable ({e}). Pick another port.");
                    eprintln!("[OSC] {msg}");
                    set_status(|st| {
                        st.listening_on = None;
                        st.error = Some(msg);
                    });
                    tokio::time::sleep(SETTINGS_EVERY).await;
                    continue;
                }
            }
        }
        let Some((_, sock)) = socket.as_ref() else { continue };
        match tokio::time::timeout(SETTINGS_EVERY, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                for (address, args) in messages(&buf[..n]) {
                    let Some(action) = action_for(&address, &args) else { continue };
                    eprintln!("[OSC] {address} from {from} → {action}");
                    set_status(|s| s.last = Some(address.clone()));
                    app.hub.broadcast(json!({ "type": "remote", "action": action }));
                }
            }
            Ok(Err(e)) => eprintln!("[OSC] Receive failed: {e}"),
            Err(_) => {} // no message: look at the settings again
        }
    }
}

/// The action an address names, unless its argument says "released"
/// (a button's 0 / false), so press-and-release sends act once.
fn action_for(address: &str, args: &[Arg]) -> Option<&'static str> {
    let name = address.strip_prefix(PREFIX)?.trim_end_matches('/').replace('/', "-").to_ascii_lowercase();
    let released = match args.first() {
        Some(Arg::Int(0)) | Some(Arg::Bool(false)) => true,
        Some(Arg::Float(f)) => *f == 0.0,
        _ => false,
    };
    if released {
        return None;
    }
    ACTIONS.iter().copied().find(|a| *a == name)
}

#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Int(i32),
    Float(f32),
    Str(String),
    Bool(bool),
    Other,
}

/// Every message in a packet (bundles opened, nested ones too).
fn messages(packet: &[u8]) -> Vec<(String, Vec<Arg>)> {
    let mut out = Vec::new();
    collect(packet, &mut out, 0);
    out
}

fn collect(p: &[u8], out: &mut Vec<(String, Vec<Arg>)>, depth: usize) {
    if depth > 4 {
        return;
    }
    if p.starts_with(b"#bundle\0") {
        let mut i = 16; // "#bundle\0" + 8-byte time tag
        while i + 4 <= p.len() {
            let size = u32::from_be_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]) as usize;
            i += 4;
            if size == 0 || i + size > p.len() {
                return;
            }
            collect(&p[i..i + size], out, depth + 1);
            i += size;
        }
        return;
    }
    if let Some(m) = message(p) {
        out.push(m);
    }
}

fn message(p: &[u8]) -> Option<(String, Vec<Arg>)> {
    let (address, mut i) = osc_string(p, 0)?;
    if !address.starts_with('/') {
        return None;
    }
    let mut args = Vec::new();
    if i >= p.len() {
        return Some((address, args));
    }
    let (tags, next) = osc_string(p, i)?;
    i = next;
    for t in tags.strip_prefix(',')?.chars() {
        let arg = match t {
            'i' => {
                let b = p.get(i..i + 4)?;
                i += 4;
                Arg::Int(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            }
            'f' => {
                let b = p.get(i..i + 4)?;
                i += 4;
                Arg::Float(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            }
            's' => {
                let (s, next) = osc_string(p, i)?;
                i = next;
                Arg::Str(s)
            }
            'T' => Arg::Bool(true),
            'F' => Arg::Bool(false),
            _ => {
                // Unknown sizes: stop reading, keep what was understood.
                args.push(Arg::Other);
                break;
            }
        };
        args.push(arg);
    }
    Some((address, args))
}

/// A null-terminated string padded to 4 bytes; returns it and the next offset.
fn osc_string(p: &[u8], start: usize) -> Option<(String, usize)> {
    let end = start + p.get(start..)?.iter().position(|b| *b == 0)?;
    let s = std::str::from_utf8(&p[start..end]).ok()?.to_string();
    let next = (end + 4) & !3;
    Some((s, next.min(p.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        while v.len() % 4 != 0 {
            v.push(0);
        }
        v
    }

    fn msg(address: &str, tags: &str, args: &[u8]) -> Vec<u8> {
        let mut v = pad(address);
        if !tags.is_empty() {
            v.extend(pad(tags));
            v.extend_from_slice(args);
        }
        v
    }

    #[test]
    fn plain_messages_name_actions() {
        let m = messages(&msg("/kairo/slide-next", "", &[]));
        assert_eq!(action_for(&m[0].0, &m[0].1), Some("slide-next"));
        let m = messages(&msg("/kairo/slide/prev", ",i", &1i32.to_be_bytes()));
        assert_eq!(action_for(&m[0].0, &m[0].1), Some("slide-prev"));
    }

    #[test]
    fn a_release_or_an_unknown_address_does_nothing() {
        let m = messages(&msg("/kairo/clear-all", ",i", &0i32.to_be_bytes()));
        assert_eq!(action_for(&m[0].0, &m[0].1), None);
        let m = messages(&msg("/kairo/delete-everything", "", &[]));
        assert_eq!(action_for(&m[0].0, &m[0].1), None);
        let m = messages(&msg("/other/slide-next", "", &[]));
        assert_eq!(action_for(&m[0].0, &m[0].1), None);
    }

    #[test]
    fn bundles_are_opened() {
        let inner = msg("/kairo/clear-bible", ",f", &1.0f32.to_be_bytes());
        let mut b = pad("#bundle");
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        b.extend_from_slice(&(inner.len() as u32).to_be_bytes());
        b.extend(inner);
        let m = messages(&b);
        assert_eq!(m.len(), 1);
        assert_eq!(action_for(&m[0].0, &m[0].1), Some("clear-bible"));
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(messages(&[0xff, 0x00, 0x13]).is_empty());
        assert!(messages(b"#bundle\0\0\0\0\0\0\0\0\0\xff\xff\xff\xff").is_empty());
    }
}
