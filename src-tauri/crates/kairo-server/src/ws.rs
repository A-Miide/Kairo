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

async fn forward_audio(app: &Shared, data: bytes::Bytes) {
    crate::audio_stats::note_frame(&data);
    if let Some(tx) = app.audio.lock().await.as_ref() {
        if tx.try_send(data).is_err() {
            crate::audio_stats::STATS.dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
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
    let animation = app.settings.get("outputAnimation").await.unwrap_or_else(|| "fade".into());
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
                    let animation = app_out.settings.get("outputAnimation").await.unwrap_or_else(|| "fade".into());
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
