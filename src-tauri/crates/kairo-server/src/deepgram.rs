//! Deepgram live transcription: microphone audio in, transcripts through the
//! detection funnel. A dropped connection reconnects without ending the
//! session (audio heard meanwhile is replayed); a key, credit or rate-limit
//! refusal ends it with a message saying what to do.

use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use crate::Shared;

const UTTERANCE_END_MS: u32 = 1200;
const KEEPALIVE: Duration = Duration::from_secs(8);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Audio kept while reconnecting: 30 s of 16 kHz 16-bit mono.
const GAP_AUDIO_MAX_BYTES: usize = 16_000 * 2 * 30;
/// int16 peak above which a frame is speech (speech here logs 2000–18000).
const SPEECH_PEAK_MIN: i32 = 1500;
/// This much speech with no transcript back: the connection is stuck.
const STUCK_SPEECH_MS: f64 = 30_000.0;

const MSG_AUTH: &str = "Deepgram rejected the API key. Open Settings → paste a valid key.";
const MSG_CREDITS: &str = "Your Deepgram account is out of credit, so listening stopped. Add credit at console.deepgram.com — or switch the Speech Engine to Offline in Settings to keep going without it.";
const MSG_QUOTA: &str = "Deepgram is turning away requests from this account for now (rate limit). Wait a minute, then press Start again.";
const MSG_NETWORK: &str = "Network error reaching Deepgram. Check your internet connection.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Auth,
    Credits,
    Quota,
    Network,
    Unknown,
}

impl Failure {
    fn classify(code: Option<u16>, text: &str) -> Failure {
        let lc = text.to_lowercase();
        let has = |s: &str| lc.contains(s);
        if matches!(code, Some(401 | 403)) || has("unauthorized") || has("invalid api key") || has("invalid credentials") || has("forbidden") {
            Failure::Auth
        } else if code == Some(402) || has("payment required") || has("asr_payment_required") || has("enough credits") || has("out of credit") || has("insufficient credit") || has("insufficient fund") || has("insufficient balance") {
            Failure::Credits
        } else if code == Some(429) || has("quota") || has("rate limit") || has("insufficient") {
            Failure::Quota
        } else if has("dns") || has("connection refused") || has("timed out") || has("network") || has("failed to lookup") || has("io error") {
            Failure::Network
        } else {
            Failure::Unknown
        }
    }

    fn fatal(self) -> bool {
        matches!(self, Failure::Auth | Failure::Credits | Failure::Quota)
    }

    fn kind(self) -> &'static str {
        match self {
            Failure::Auth => "auth",
            Failure::Credits => "credits",
            Failure::Quota => "quota",
            Failure::Network => "network",
            Failure::Unknown => "unknown",
        }
    }

    fn message(self, raw: &str) -> String {
        match self {
            Failure::Auth => MSG_AUTH.into(),
            Failure::Credits => MSG_CREDITS.into(),
            Failure::Quota => MSG_QUOTA.into(),
            Failure::Network => MSG_NETWORK.into(),
            Failure::Unknown => format!("Deepgram didn't accept the connection ({raw}). Press Start to try again."),
        }
    }
}

/// A running session: stop it, and it finalizes what was being said.
pub struct Session {
    stop: Option<oneshot::Sender<oneshot::Sender<()>>>,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Session {
    /// Still listening (Deepgram may have ended it: a refused key, no credit).
    pub fn alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn stop(mut self) {
        if let Some(s) = self.stop.take() {
            let (done_tx, done_rx) = oneshot::channel();
            if s.send(done_tx).is_ok() {
                let _ = tokio::time::timeout(Duration::from_secs(3), done_rx).await;
            }
        }
    }
}

/// The words recognised so far in a segment Deepgram hasn't finalized.
#[derive(Default)]
struct Pending {
    text: String,
    confidence: f32,
}

/// What Deepgram is told about the service.
async fn listen_url(app: &Shared) -> String {
    let lang = app.settings.str("sttLanguage").await.unwrap_or_else(|| "en-US".into());
    let mut q = vec![
        ("model".to_string(), "nova-3".to_string()),
        ("language".into(), lang.clone()),
        ("smart_format".into(), "true".into()),
        ("punctuate".into(), "true".into()),
        ("interim_results".into(), "true".into()),
        ("utterance_end_ms".into(), UTTERANCE_END_MS.to_string()),
        ("endpointing".into(), "300".into()),
        ("encoding".into(), "linear16".into()),
        ("sample_rate".into(), "16000".into()),
        ("channels".into(), "1".into()),
        ("no_delay".into(), "true".into()),
        ("filler_words".into(), "false".into()),
        ("diarize".into(), "false".into()),
    ];
    for term in crate::vocabulary::terms(app, &lang).await {
        q.push(("keyterm".into(), term));
    }
    let query: Vec<String> = q.into_iter().map(|(k, v)| format!("{k}={}", urlencode(&v))).collect();
    format!("wss://api.deepgram.com/v1/listen?{}", query.join("&"))
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn state(app: &Shared, state: &str, error: Option<(String, Failure)>) {
    let mut m = json!({ "type": "connection-state", "state": state });
    if let Some((msg, kind)) = error {
        m["error"] = json!(msg);
        m["errorKind"] = json!(kind.kind());
    }
    app.hub.broadcast(m);
}

/// Starts listening. Resolves once Deepgram has accepted (or refused) the connection.
pub async fn start(app: Shared) -> Result<Session, Value> {
    let key = app.settings.str("deepgramApiKey").await.unwrap_or_default().trim().to_string();
    if key.is_empty() {
        app.hub.broadcast(json!({ "type": "listening-error", "error": "No Deepgram API key yet. Paste one in Settings → Audio, or switch the Speech Engine to Offline." }));
        return Err(json!({ "error": "No Deepgram API key" }));
    }
    let Some((_, detector)) = app.ready() else { return Err(json!({ "error": "The detection engine is still loading" })) };
    detector.send(kairo_detect::engine::Event::Reset);
    let (audio_tx, audio_rx) = mpsc::channel::<Bytes>(512);
    *app.audio.lock().await = Some(audio_tx);
    let (stop_tx, stop_rx) = oneshot::channel();
    let (first_tx, first_rx) = oneshot::channel::<Result<(), Value>>();
    let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = alive.clone();
    tokio::spawn(async move {
        run(app.clone(), key, audio_rx, stop_rx, first_tx).await;
        flag.store(false, std::sync::atomic::Ordering::Relaxed);
    });
    match first_rx.await {
        Ok(Ok(())) => Ok(Session { stop: Some(stop_tx), alive }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(json!({ "error": "Deepgram session ended" })),
    }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(app: &Shared, key: &str) -> Result<Ws, (Failure, String)> {
    let url = listen_url(app).await;
    let mut req = url.into_client_request().map_err(|e| (Failure::Unknown, e.to_string()))?;
    req.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Token {key}")).map_err(|e| (Failure::Auth, e.to_string()))?);
    match tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(req)).await {
        Err(_) => Err((Failure::Network, "timeout".into())),
        Ok(Ok((ws, _))) => Ok(ws),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => {
            let code = resp.status().as_u16();
            let body = resp.body().as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
            let dg_error = resp.headers().get("dg-error").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
            let text = format!("HTTP {code} {dg_error} {body}");
            Err((Failure::classify(Some(code), &text), text))
        }
        Ok(Err(e)) => {
            let text = e.to_string();
            Err((Failure::classify(None, &text), text))
        }
    }
}

async fn run(app: Shared, key: String, mut audio: mpsc::Receiver<Bytes>, mut stop: oneshot::Receiver<oneshot::Sender<()>>, first: oneshot::Sender<Result<(), Value>>) {
    let Some((_, detector)) = app.ready() else { return };
    let detector = detector.clone();
    let mut first = Some(first);
    let mut attempts = 0u32;
    let mut gap: Vec<Bytes> = Vec::new();
    let mut gap_bytes = 0usize;
    let mut pending = Pending::default();
    state(&app, "connecting", None);

    let lock_pending = |pending: &mut Pending, why: &str| {
        let p = std::mem::take(pending);
        if !p.text.trim().is_empty() {
            eprintln!("[Deepgram] {why} — locking the unfinished words: \"{}\"", p.text.chars().take(80).collect::<String>());
            detector.speech(&app.hub, &p.text, true, p.confidence.max(0.8), true, None, false);
        }
    };

    'session: loop {
        let ws = match connect(&app, &key).await {
            Ok(ws) => ws,
            Err((failure, raw)) => {
                eprintln!("[Deepgram] Error ({}): {raw}", failure.kind());
                if failure.fatal() || first.is_some() {
                    let msg = failure.message(&raw);
                    state(&app, "error", Some((msg.clone(), failure)));
                    if let Some(f) = first.take() {
                        let _ = f.send(Err(json!({ "error": msg, "errorKind": failure.kind() })));
                    }
                    break 'session;
                }
                // Mid-session: keep the session, try again shortly.
                let delay = if attempts == 0 { 250 } else { (1000 * attempts as u64).min(2000) };
                attempts += 1;
                state(&app, "reconnecting", None);
                let wait = tokio::time::sleep(Duration::from_millis(delay));
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        _ = &mut wait => continue 'session,
                        Some(chunk) = audio.recv() => buffer_gap(&mut gap, &mut gap_bytes, chunk),
                        done = &mut stop => { finish(&app, done.ok()).await; return; }
                    }
                }
            }
        };
        let (mut tx, mut rx) = ws.split();
        attempts = 0;
        state(&app, "connected", None);
        if let Some(f) = first.take() {
            eprintln!("[Deepgram] Connected.");
            let _ = f.send(Ok(()));
        } else {
            detector.send(kairo_detect::engine::Event::Reconnected);
            if !gap.is_empty() {
                eprintln!("[Deepgram] Reconnected — replaying {:.1}s of audio heard while reconnecting", gap_bytes as f64 / 32_000.0);
            }
            for chunk in gap.drain(..) {
                let _ = tx.send(Message::Binary(chunk)).await;
            }
            gap_bytes = 0;
        }
        let mut keepalive = tokio::time::interval(KEEPALIVE);
        keepalive.tick().await;
        let mut speech_ms_since_transcript = 0.0f64;
        let reason: String = loop {
            tokio::select! {
                Some(chunk) = audio.recv() => {
                    speech_ms_since_transcript += speech_ms(&chunk);
                    if tx.send(Message::Binary(chunk)).await.is_err() {
                        break "send failed".into();
                    }
                    if speech_ms_since_transcript > STUCK_SPEECH_MS {
                        eprintln!("[Deepgram] {:.0}s of speech with no transcript back — connection looks stuck, reconnecting", speech_ms_since_transcript / 1000.0);
                        break "stuck".into();
                    }
                }
                _ = keepalive.tick() => {
                    let _ = tx.send(Message::Text(r#"{"type":"KeepAlive"}"#.into())).await;
                }
                msg = rx.next() => match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&t) {
                            if on_message(&app, &detector, &v, &mut pending) {
                                speech_ms_since_transcript = 0.0;
                            }
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let (code, why) = frame.map(|f| (u16::from(f.code), f.reason.to_string())).unwrap_or((0, String::new()));
                        let failure = Failure::classify(None, &why);
                        if failure == Failure::Auth || failure == Failure::Credits {
                            let msg = failure.message(&why);
                            eprintln!("[Deepgram] Closed ({}): {code} \"{why}\"", failure.kind());
                            state(&app, "error", Some((msg, failure)));
                            break 'session;
                        }
                        break format!("closed (code {code} \"{why}\")");
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break format!("error: {e}"),
                    None => break "closed".into(),
                },
                done = &mut stop => {
                    // Stop: ask for the last words, wait briefly for them, then close.
                    let _ = tx.send(Message::Text(r#"{"type":"Finalize"}"#.into())).await;
                    let deadline = tokio::time::sleep(Duration::from_millis(1500));
                    tokio::pin!(deadline);
                    loop {
                        tokio::select! {
                            _ = &mut deadline => break,
                            msg = rx.next() => match msg {
                                Some(Ok(Message::Text(t))) => {
                                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                                        let is_final = v["is_final"] == true;
                                        on_message(&app, &detector, &v, &mut pending);
                                        if is_final { break; }
                                    }
                                }
                                Some(Ok(_)) => {}
                                _ => break,
                            }
                        }
                    }
                    lock_pending(&mut pending, "stopped");
                    let _ = tx.send(Message::Text(r#"{"type":"CloseStream"}"#.into())).await;
                    let _ = tx.close().await;
                    finish(&app, done.ok()).await;
                    return;
                }
            }
        };
        lock_pending(&mut pending, &format!("connection {reason}"));
        let _ = tx.close().await;
        eprintln!("[Deepgram] Connection {reason} — reconnecting (session stays on)");
        state(&app, "reconnecting", None);
    }
    *app.audio.lock().await = None;
}

async fn finish(app: &Shared, done: Option<oneshot::Sender<()>>) {
    *app.audio.lock().await = None;
    state(app, "disconnected", None);
    if let Some(d) = done {
        let _ = d.send(());
    }
}

fn buffer_gap(gap: &mut Vec<Bytes>, bytes: &mut usize, chunk: Bytes) {
    *bytes += chunk.len();
    gap.push(chunk);
    while *bytes > GAP_AUDIO_MAX_BYTES && !gap.is_empty() {
        *bytes -= gap.remove(0).len();
    }
}

/// Milliseconds of speech in a frame of 16 kHz 16-bit PCM (0 if it's quiet).
fn speech_ms(chunk: &[u8]) -> f64 {
    let samples = chunk.len() / 2;
    let peak = (0..samples).step_by(4).map(|i| (i16::from_le_bytes([chunk[2 * i], chunk[2 * i + 1]]) as i32).abs()).max().unwrap_or(0);
    if peak >= SPEECH_PEAK_MIN { samples as f64 / 16.0 } else { 0.0 }
}

/// One message from Deepgram. Returns whether a transcript came back.
fn on_message(app: &Shared, detector: &crate::detect::Detector, v: &Value, pending: &mut Pending) -> bool {
    match v["type"].as_str() {
        Some("Results") => {
            let alt = &v["channel"]["alternatives"][0];
            let text = alt["transcript"].as_str().unwrap_or("").to_string();
            let confidence = alt["confidence"].as_f64().unwrap_or(0.0) as f32;
            let is_final = v["is_final"] == true;
            if is_final {
                let p = std::mem::take(pending);
                // The segment finalized empty: the words heard in it stand.
                if text.trim().is_empty() && p.text.split_whitespace().count() >= 3 {
                    eprintln!("[Deepgram] segment finalized empty — locking the unfinished words: \"{}\"", p.text.chars().take(80).collect::<String>());
                    detector.speech(&app.hub, &p.text, true, p.confidence.max(0.8), true, None, false);
                    return true;
                }
            }
            if text.trim().is_empty() {
                return false;
            }
            if !is_final {
                *pending = Pending { text: text.clone(), confidence };
            }
            let words = alt.get("words").filter(|w| w.is_array()).cloned();
            detector.speech(&app.hub, &text, is_final, confidence, v["speech_final"] == true, words, false);
            true
        }
        Some("UtteranceEnd") => {
            detector.send(kairo_detect::engine::Event::UtteranceEnd);
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_say_what_to_do() {
        assert_eq!(Failure::classify(Some(401), ""), Failure::Auth);
        assert_eq!(Failure::classify(Some(402), ""), Failure::Credits);
        assert_eq!(Failure::classify(None, "ASR_PAYMENT_REQUIRED"), Failure::Credits);
        assert_eq!(Failure::classify(Some(429), ""), Failure::Quota);
        assert!(!Failure::Network.fatal());
    }

    #[test]
    fn quiet_frames_aren_t_speech() {
        let quiet = vec![0u8; 640];
        assert_eq!(speech_ms(&quiet), 0.0);
        let loud: Vec<u8> = (0..320).flat_map(|_| 5000i16.to_le_bytes()).collect();
        assert_eq!(speech_ms(&loud), 20.0);
    }
}
