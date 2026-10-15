//! Settings → Streaming "Check": can this computer reach the stream server?
//! It opens a plain connection to the server's RTMP or RTMPS port and closes it
//! again. Nothing is sent and no stream key is involved.

use std::time::Duration;

use axum::Json;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// (host, port) of an rtmp:// or rtmps:// address, or why it is not one.
pub fn parse(url: &str) -> Result<(String, u16), &'static str> {
    let url = url.trim();
    let (rest, default_port) = if let Some(r) = url.strip_prefix("rtmps://").or_else(|| url.strip_prefix("RTMPS://")) {
        (r, 443)
    } else if let Some(r) = url.strip_prefix("rtmp://").or_else(|| url.strip_prefix("RTMP://")) {
        (r, 1935)
    } else {
        return Err("The address must start with rtmp:// or rtmps://");
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.contains(char::is_whitespace) || authority.contains('@') {
        return Err("That address has no server name.");
    }
    // An IPv6 address: [2001:db8::1] or [2001:db8::1]:1935.
    if let Some(inner) = authority.strip_prefix('[') {
        let (host, after) = inner.split_once(']').ok_or("That address has no server name.")?;
        if host.is_empty() { return Err("That address has no server name."); }
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| "The port is not a number.")?,
            None if after.is_empty() => default_port,
            None => return Err("That address has no server name."),
        };
        return Ok((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port: u16 = port.parse().map_err(|_| "The port is not a number.")?;
            Ok((host.to_string(), port))
        }
        Some(_) => Err("That address has no server name."),
        None => Ok((authority.to_string(), default_port)),
    }
}

pub async fn check(Json(body): Json<Value>) -> Json<Value> {
    let url = body.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let (host, port) = match parse(url) {
        Ok(hp) => hp,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    match timeout(Duration::from_secs(4), TcpStream::connect((host.as_str(), port))).await {
        Ok(Ok(_)) => Json(json!({ "ok": true, "host": host, "port": port })),
        Ok(Err(_)) => Json(json!({ "ok": false, "error": format!("Could not reach {host}. Check the address and your internet.") })),
        Err(_) => Json(json!({ "ok": false, "error": format!("{host} did not answer in time.") })),
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn defaults_and_ports() {
        assert_eq!(parse("rtmp://a.rtmp.youtube.com/live2"), Ok(("a.rtmp.youtube.com".into(), 1935)));
        assert_eq!(parse("rtmps://live-api-s.facebook.com:443/rtmp/"), Ok(("live-api-s.facebook.com".into(), 443)));
        assert_eq!(parse("rtmps://example.com/app"), Ok(("example.com".into(), 443)));
        assert_eq!(parse("rtmp://example.com:1936/x"), Ok(("example.com".into(), 1936)));
        assert_eq!(parse("rtmp://[2001:db8::1]/live"), Ok(("2001:db8::1".into(), 1935)));
        assert_eq!(parse("rtmps://[::1]:8443/x"), Ok(("::1".into(), 8443)));
    }

    #[test]
    fn rejects_other_things() {
        assert!(parse("http://example.com").is_err());
        assert!(parse("rtmp://").is_err());
        assert!(parse("rtmp://user@host/app").is_err());
        assert!(parse("rtmp://host:abc/app").is_err());
        assert!(parse("rtmp://:1935/app").is_err());
        assert!(parse("rtmp://[::1/app").is_err());
    }
}
