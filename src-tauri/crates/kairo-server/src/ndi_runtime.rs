//! Fetching the NDI® runtime the first time an NDI output is turned on, so
//! nobody has to install NDI Tools. Kairo never ships NDI's files itself: it
//! downloads NDI's own redistributable from NDI and checks it is signed by
//! NDI (NewTek, team W8U66ET244) before using it.
//!
//! - macOS: NDI's package (≈4.6 MB) is unpacked — not installed — and its
//!   `libndi.dylib` kept in Kairo's app data (`ndi/`), where the app's NDI
//!   sender looks first (src-tauri ndi.rs). No password, nothing system-wide.
//! - Windows: NDI's runtime installer runs silently; Windows asks once for
//!   permission (the runtime installs system-wide there).
//! - Linux: NDI publishes no runtime download — installed by hand.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::Shared;

const MAC_URL: &str = "https://ndi.link/NDIRedistV6Apple";
const WIN_URL: &str = "https://ndi.link/NDIRedistV6";
/// NDI's (NewTek's) Apple developer team.
const NDI_TEAM: &str = "W8U66ET244";

#[derive(Clone, Default)]
struct Status {
    running: bool,
    percent: Option<u8>,
    error: Option<String>,
    done: bool,
}
static STATUS: Mutex<Status> = Mutex::new(Status { running: false, percent: None, error: None, done: false });

fn set(f: impl FnOnce(&mut Status)) {
    if let Ok(mut s) = STATUS.lock() {
        f(&mut s);
    }
}

pub fn dir(app: &Shared) -> PathBuf {
    app.cfg.app_data_dir.join("ndi")
}

/// GET /api/ndi/runtime — whether Kairo has its own copy, and any download running.
pub async fn status_route(State(app): State<Shared>) -> Json<Value> {
    let s = STATUS.lock().map(|s| s.clone()).unwrap_or_default();
    let have = dir(&app).join("libndi.dylib").exists();
    Json(json!({
        "available": cfg!(any(target_os = "macos", windows)),
        "installed": have,
        "running": s.running,
        "percent": s.percent,
        "error": s.error,
        "done": s.done,
        "platform": std::env::consts::OS,
    }))
}

/// POST /api/ndi/runtime/install — the operator accepted NDI's licence.
pub async fn install_route(State(app): State<Shared>) -> Json<Value> {
    {
        let Ok(mut s) = STATUS.lock() else { return Json(json!({ "ok": false })) };
        if s.running {
            return Json(json!({ "ok": true, "running": true }));
        }
        *s = Status { running: true, percent: Some(0), error: None, done: false };
    }
    tokio::spawn(async move {
        let r = install(&app).await;
        if let Err(e) = &r {
            eprintln!("[NDI] Runtime download failed: {e}");
        }
        set(|s| {
            s.running = false;
            s.done = r.is_ok();
            s.error = r.err();
        });
    });
    Json(json!({ "ok": true, "running": true }))
}

async fn install(app: &Shared) -> Result<(), String> {
    let out = dir(app);
    tokio::fs::create_dir_all(&out).await.map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(5)).build().map_err(|e| e.to_string())?;
    let (url, name) = if cfg!(target_os = "macos") { (MAC_URL, "libNDI_for_Mac.pkg") } else if cfg!(windows) { (WIN_URL, "NDI Runtime.exe") } else { return Err("NDI publishes no runtime download for Linux: install NDI from ndi.video.".into()) };
    let file = out.join(name);
    let _ = tokio::fs::remove_file(&file).await;
    let on = |got: u64, total: u64| {
        if total > 0 {
            set(|s| s.percent = Some(((got * 100) / total).min(100) as u8));
        }
    };
    crate::installer::download_plain(&client, url, &file, &on).await?;
    let r = if cfg!(target_os = "macos") { unpack_mac(&file, &out).await } else { run_windows(&file).await };
    let _ = tokio::fs::remove_file(&file).await;
    r
}

async fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let o = tokio::process::Command::new(cmd).args(args).output().await.map_err(|e| format!("{cmd}: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    if o.status.success() { Ok(text) } else { Err(format!("{cmd} failed: {}", text.trim())) }
}

/// Checks NDI's signature and Apple's notarization, then keeps only the library.
async fn unpack_mac(pkg: &Path, out: &Path) -> Result<(), String> {
    let p = pkg.to_string_lossy().to_string();
    let sig = run("pkgutil", &["--check-signature", &p]).await?;
    if !sig.contains(&format!("({NDI_TEAM})")) || !sig.contains("trusted by the Apple notary service") {
        return Err("the NDI download isn't signed by NDI — not used".into());
    }
    let tmp = out.join("unpack");
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    run("pkgutil", &["--expand-full", &p, &tmp.to_string_lossy()]).await?;
    let found = find(&tmp, "libndi.dylib").ok_or("no libndi.dylib in NDI's package")?;
    let lib = found.to_string_lossy().to_string();
    run("codesign", &["--verify", "--strict", &lib]).await?;
    let detail = run("codesign", &["-dv", &lib]).await?;
    if !detail.contains(&format!("TeamIdentifier={NDI_TEAM}")) {
        return Err("libndi.dylib isn't signed by NDI — not used".into());
    }
    tokio::fs::copy(&found, out.join("libndi.dylib")).await.map_err(|e| e.to_string())?;
    if let Some(lic) = find(&tmp, "libndi_licenses.txt") {
        let _ = tokio::fs::copy(lic, out.join("libndi_licenses.txt")).await;
    }
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    eprintln!("[NDI] Runtime ready: {}", out.join("libndi.dylib").display());
    Ok(())
}

/// Checks NDI's signature, then runs its installer quietly (Windows asks
/// for permission once).
async fn run_windows(exe: &Path) -> Result<(), String> {
    let p = exe.to_string_lossy().to_string();
    let check = format!("(Get-AuthenticodeSignature -LiteralPath '{}').SignerCertificate.Subject + '|' + (Get-AuthenticodeSignature -LiteralPath '{}').Status", p.replace('\'', "''"), p.replace('\'', "''"));
    let sig = run("powershell", &["-NoProfile", "-Command", &check]).await?;
    let ok = sig.contains("|Valid") && (sig.contains("NewTek") || sig.contains("Vizrt") || sig.contains("NDI"));
    if !ok {
        return Err("the NDI download isn't signed by NDI — not used".into());
    }
    let start = format!("Start-Process -FilePath '{}' -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Verb RunAs -Wait", p.replace('\'', "''"));
    run("powershell", &["-NoProfile", "-Command", &start]).await?;
    Ok(())
}

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}
