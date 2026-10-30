//! The WebSocket on `/`: UI windows receive every broadcast; a socket opened
//! with `?audio=1` only sends microphone audio (16 kHz PCM frames), and the
//! main window's own socket may send audio too.

use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;

use crate::{query_param, token_from, Shared};

/// `/`: the UI's page, or the WebSocket when it's an upgrade.
pub async fn root(State(app): State<Shared>, uri: Uri, headers: HeaderMap, ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>) -> Response {
    let Ok(ws) = ws else {
        return crate::routes::index(&app).await;
    };
    if app.cfg.auth_required() && token_from(&headers, &uri).as_deref() != Some(app.cfg.token.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let audio_only = query_param(uri.query().unwrap_or(""), "audio").as_deref() == Some("1");
    ws.on_upgrade(move |socket| async move {
        if audio_only {
            audio(app, socket).await
        } else {
            client(app, socket).await
        }
    })
}

pub(crate) async fn forward_audio(app: &Shared, data: bytes::Bytes) {
    // The server is capturing the input itself: a window streaming the same input too would feed
    // the engine every sound twice, interleaved, and it hears almost nothing. That happened when
    // listening was started from outside the window (OSC, a second window): the window "resumed"
    // its own capture before the server's was registered (dev build, 2026-10-20).
    if crate::util::lock(&app.capture).is_some() {
        return;
    }
    if let Some(tx) = note_audio(app, &data).await {
        if tx.try_send(data).is_err() {
            crate::audio_stats::STATS.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// The same, for the server's own capture (native_audio.rs): it waits for room in the engine's
/// queue instead of dropping the frame, so no audio (and no words) is lost.
pub(crate) async fn forward_audio_lossless(app: &Shared, data: bytes::Bytes) {
    if let Some(tx) = note_audio(app, &data).await {
        if tx.send(data).await.is_err() {
            crate::audio_stats::STATS.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

async fn note_audio(app: &Shared, data: &bytes::Bytes) -> Option<tokio::sync::mpsc::Sender<bytes::Bytes>> {
    crate::audio_stats::note_frame(data);
    let samples: Vec<f32> = data.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0).collect();
    if let Some(change) = crate::interpretation::with(|i| i.audio(&samples)) {
        crate::detect::announce_interpretation(&app.hub, change);
    }
    crate::music::audio(&samples);
    app.audio.lock().await.clone()
}

async fn audio(app: Shared, mut socket: WebSocket) {
    while let Some(Ok(m)) = socket.next().await {
        if let Message::Binary(b) = m {
            forward_audio(&app, b).await;
        }
    }
}

async fn client(app: Shared, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = app.hub.subscribe();
    let animation = app.settings.get("outputAnimation").await.unwrap_or_else(|| "cut".into());
    let speed = app.settings.get("outputAnimationSpeed").await.unwrap_or_else(|| 1.into());
    for m in app.hub.catch_up(app.ready().is_some(), animation, speed) {
        if sink.send(Message::Text(m.into())).await.is_err() {
            return;
        }
    }
    let app_out = app.clone();
    let outgoing = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(m) => {
                    if sink.send(Message::Text(m.into())).await.is_err() {
                        break;
                    }
                }
                // This window fell behind (a busy or sleeping display) and missed messages, maybe
                // a send or a clear: it gets what is on screen now, as a newly opened one does.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    crate::routes::write_debug_global("server:ws-lagged", serde_json::json!({ "missed": n }));
                    let animation = app_out.settings.get("outputAnimation").await.unwrap_or_else(|| "cut".into());
                    let speed = app_out.settings.get("outputAnimationSpeed").await.unwrap_or_else(|| 1.into());
                    for m in app_out.hub.catch_up(app_out.ready().is_some(), animation, speed) {
                        if sink.send(Message::Text(m.into())).await.is_err() {
                            return;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
    while let Some(Ok(m)) = stream.next().await {
        match m {
            Message::Binary(b) => forward_audio(&app, b).await,
            Message::Text(t) => {
                // A display window reporting its media playback state back up.
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if v["type"] == "media-status" {
                        app.hub.broadcast(v);
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    outgoing.abort();
}
