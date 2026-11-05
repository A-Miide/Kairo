//! OBS (obs-websocket v5): the verse on screen written into a text source.

use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_URL: &str = "ws://localhost:4455";

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Obs {
    conn: Mutex<Option<Ws>>,
    next_id: std::sync::atomic::AtomicU64,
}

fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let secret = b64.encode(Sha256::digest(format!("{password}{salt}").as_bytes()));
    b64.encode(Sha256::digest(format!("{secret}{challenge}").as_bytes()))
}

/// Connects and identifies (op 0 Hello → op 1 Identify → op 2 Identified).
async fn open(url: &str, password: &str) -> Result<Ws, String> {
    let (mut ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| format!("OBS connect timed out after {}ms", CONNECT_TIMEOUT.as_millis()))?
        .map_err(|e| e.to_string())?;
    let hello = next_json(&mut ws).await?;
    let mut identify = json!({ "op": 1, "d": { "rpcVersion": 1 } });
    if let Some(a) = hello["d"]["authentication"].as_object() {
        let (Some(salt), Some(challenge)) = (a.get("salt").and_then(Value::as_str), a.get("challenge").and_then(Value::as_str)) else { return Err("bad hello".into()) };
        identify["d"]["authentication"] = json!(auth_string(password, salt, challenge));
    }
    ws.send(Message::Text(identify.to_string().into())).await.map_err(|e| e.to_string())?;
    let id = next_json(&mut ws).await?;
    if id["op"] != 2 {
        return Err("Authentication failed".into());
    }
    Ok(ws)
}

async fn next_json(ws: &mut Ws) -> Result<Value, String> {
    loop {
        match tokio::time::timeout(CONNECT_TIMEOUT, ws.next()).await {
            Err(_) => return Err("OBS didn't answer".into()),
            Ok(Some(Ok(Message::Text(t)))) => return serde_json::from_str(&t).map_err(|e| e.to_string()),
            Ok(Some(Ok(Message::Close(f)))) => return Err(f.map(|f| f.reason.to_string()).filter(|r| !r.is_empty()).unwrap_or_else(|| "connection closed".into())),
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(e.to_string()),
            Ok(None) => return Err("connection closed".into()),
        }
    }
}

async fn request(ws: &mut Ws, id: u64, kind: &str, data: Value) -> Result<Value, String> {
    let rid = id.to_string();
    ws.send(Message::Text(json!({ "op": 6, "d": { "requestType": kind, "requestId": rid, "requestData": data } }).to_string().into())).await.map_err(|e| e.to_string())?;
    loop {
        let m = next_json(ws).await?;
        if m["op"] == 7 && m["d"]["requestId"] == rid.as_str() {
            if m["d"]["requestStatus"]["result"] == true {
                return Ok(m["d"]["responseData"].clone());
            }
            return Err(m["d"]["requestStatus"]["comment"].as_str().unwrap_or("request failed").to_string());
        }
    }
}

impl Obs {
    pub fn new() -> Obs {
        Obs { conn: Mutex::new(None), next_id: 1.into() }
    }

    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn connected(&self) -> bool {
        self.conn.lock().await.is_some()
    }

    /// (Re)connects when enabled in settings; disconnects otherwise.
    pub async fn connect(&self, enabled: bool, url: Option<String>, password: Option<String>) {
        let mut c = self.conn.lock().await;
        if let Some(mut ws) = c.take() {
            let _ = ws.close(None).await;
        }
        if !enabled {
            return;
        }
        let url = url.filter(|u| !u.is_empty()).unwrap_or_else(|| DEFAULT_URL.into());
        match open(&url, password.as_deref().unwrap_or("")).await {
            Ok(ws) => {
                eprintln!("[OBS] Connected to {url}");
                *c = Some(ws);
            }
            Err(e) => eprintln!("[OBS] Connection failed: {e}"),
        }
    }

    /// Tries a connection without keeping it.
    pub async fn test(url: Option<String>, password: Option<String>) -> Value {
        let url = url.filter(|u| !u.is_empty()).unwrap_or_else(|| DEFAULT_URL.into());
        match open(&url, password.as_deref().unwrap_or("")).await {
            Ok(mut ws) => {
                let v = request(&mut ws, 1, "GetVersion", json!({})).await;
                let _ = ws.close(None).await;
                match v {
                    Ok(v) => json!({ "success": true, "version": v["obsVersion"] }),
                    Err(e) => json!({ "success": false, "error": e }),
                }
            }
            Err(e) => json!({ "success": false, "error": e }),
        }
    }

    async fn call(&self, kind: &str, data: Value) -> Result<Value, String> {
        let id = self.id();
        let mut c = self.conn.lock().await;
        let Some(ws) = c.as_mut() else { return Err("not connected".into()) };
        let r = request(ws, id, kind, data).await;
        if let Err(e) = &r {
            if e.contains("closed") {
                *c = None;
            }
        }
        r
    }

    pub async fn set_text(&self, source: &str, text: &str) {
        if let Err(e) = self.call("SetInputSettings", json!({ "inputName": source, "inputSettings": { "text": text } })).await {
            if e != "not connected" {
                eprintln!("[OBS] Send failed: {e}");
            }
        }
    }

    /// Text sources OBS has (Windows' GDI+ kind, else the macOS/Linux kind).
    pub async fn text_sources(&self) -> Vec<String> {
        let names = |v: &Value| v["inputs"].as_array().map(|a| a.iter().filter_map(|i| i["inputName"].as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
        let gdi = self.call("GetInputList", json!({ "inputKind": "text_gdiplus_v3" })).await.map(|v| names(&v)).unwrap_or_default();
        if !gdi.is_empty() {
            return gdi;
        }
        self.call("GetInputList", json!({ "inputKind": "text_ft2_source_v2" })).await.map(|v| names(&v)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn auth_matches_the_protocol_example() {
        // obs-websocket's documented example: password "supersecretpassword".
        let a = super::auth_string("supersecretpassword", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=");
        assert_eq!(a, "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
    }
}
