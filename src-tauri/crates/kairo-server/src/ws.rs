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
    if let Some(tx) = app.audio.lock().await.as_ref() {
        let _ = tx.try_send(data);
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
    let outgoing = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(m) => {
                    if sink.send(Message::Text(m.into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
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
