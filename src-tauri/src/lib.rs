// KAIRO — Tauri shell
// Runs the Kairo server (crates/kairo-server) in this process on a local port
// with a per-launch token, opens the app and output windows, and owns the
// native outputs (NDI, Syphon, Spout, DeckLink) and their capture. The offline
// speech engine runs as a second copy of this program (see main.rs).

mod capture;
mod stream;
mod senders;
mod video_bg;
mod decklink;
mod ndi;
#[cfg(target_os = "macos")]
mod syphon;
#[cfg(target_os = "windows")]
mod spout;
mod fonts;

use std::process::Child;
use std::sync::{Arc, Mutex};
use base64::Engine;
use senders::{decode_media, Handles};
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri::menu::{AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_updater::UpdaterExt;

// ── Native app menu ──────────────────────────────────────────────────────
// Previously just Tauri's bare default (File > Close Window and nothing
// else) — no Edit menu at all, which on macOS means Cmd+Z/Cmd+C/Cmd+V/
// Cmd+A don't reliably reach a WKWebView text field: those shortcuts are
// routed through the OS's Edit-menu-backed responder chain (`cut:`/`copy:`/
// `paste:`/`undo:`/`selectAll:` selectors), not just "whatever the browser
// does with the key combo" — an app with no Edit menu items wired to those
// selectors can silently drop them in some inputs. Everything menu-command-
// shaped (New Theme, Import, Export) is dispatched to the frontend as a
// plain window event rather than reimplemented in Rust — app.js already
// owns every one of those flows (file pickers, unsaved-state, toasts), so
// Rust's job here is just "tell the frontend which menu item fired".
fn build_app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let about_metadata = AboutMetadataBuilder::new()
        .name(Some("KAIRO"))
        .version(Some(env!("CARGO_PKG_VERSION")))
        .copyright(Some("© 2026 KAIRO"))
        .website(Some("https://github.com/miide-Adeyemi/Kairo"))
        .website_label(Some("GitHub"))
        .build();

    // macOS's own app menu (titled with the app's real name automatically,
    // "KAIRO" here) — About/Services/Hide/Quit live here by OS convention,
    // not under File/Help the way Windows/Linux menus would put them.
    // Standard macOS placement (Preferences… right after About, Cmd+,) —
    // opens the same in-app Settings panel the toolbar gear icon does, via
    // the "menu-settings" window event (see the frontend listener in
    // app.js) rather than Rust knowing anything about that panel itself.
    let settings_item = MenuItemBuilder::with_id("menu-settings", "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;

    // Check for Updates… sits under the app's own name, as on every Mac app.
    let app_menu = SubmenuBuilder::new(app, "KAIRO")
        .about(Some(about_metadata))
        .text("menu-check-updates", "Check for Updates…")
        .separator()
        .item(&settings_item)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    // File: what an operator makes, brings in and takes away — a playlist, a
    // theme, content to import, and the service's own exports (the toolbar's
    // Export menu) plus the current theme.
    let new_playlist_item = MenuItemBuilder::with_id("menu-new-playlist", "New Playlist")
        .accelerator("CmdOrCtrl+N")
        .build(app)?;
    let import_item = MenuItemBuilder::with_id("menu-import", "Import…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let export_notes_item = MenuItemBuilder::with_id("menu-export-notes", "Sermon Notes (PDF)…")
        .accelerator("CmdOrCtrl+E")
        .build(app)?;
    let export_menu = SubmenuBuilder::new(app, "Export")
        .item(&export_notes_item)
        .text("menu-export-transcript", "Transcript")
        .text("menu-export-verses", "Verses Shown")
        .separator()
        .text("menu-export-theme", "Current Theme…")
        .build()?;
    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&new_playlist_item)
        .text("menu-new-theme", "New Theme")
        .separator()
        .item(&import_item)
        .item(&export_menu)
        .separator()
        .close_window()
        .build()?;

    // Standard Cut/Copy/Paste/Select All/Undo/Redo — see the file-level
    // comment above for why this menu existing at all (not just its
    // contents) is the actual fix.
    // Find… opens the frontend's global search (global_search.js) — the menu
    // accelerator also makes Cmd+F work while focus sits in a text field.
    let find_item = MenuItemBuilder::with_id("menu-find", "Find…")
        .accelerator("CmdOrCtrl+F")
        .build(app)?;
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&find_item)
        .build()?;

    // View: the app's own tabs, one shortcut each (the toolbar's order), and
    // what the Monitor shows.
    let mut view_builder = SubmenuBuilder::new(app, "View");
    for (id, label, keys) in [
        ("menu-view-bible", "Bible", "CmdOrCtrl+1"),
        ("menu-view-slides", "Slides", "CmdOrCtrl+2"),
        ("menu-view-timer", "Timer", "CmdOrCtrl+3"),
        ("menu-view-songs", "Songs", "CmdOrCtrl+4"),
        ("menu-view-media", "Media", "CmdOrCtrl+5"),
        ("menu-view-studio", "Theme Studio", "CmdOrCtrl+6"),
    ] {
        view_builder = view_builder.item(&MenuItemBuilder::with_id(id, label).accelerator(keys).build(app)?);
    }
    let view_menu = view_builder
        .separator()
        .text("menu-monitor-grid", "Show or Hide All Outputs in the Monitor")
        .item(&MenuItemBuilder::with_id("menu-video-director", "Video Director Mode").accelerator("CmdOrCtrl+Shift+D").build(app)?)
        .separator()
        .fullscreen()
        .build()?;

    // Operator-facing live controls — everything here has an existing
    // toolbar button already wired up with the real logic (dedup, WS
    // broadcast, etc.); like File's items above, Rust's job is only to tell
    // the frontend which one fired (see initNativeMenuBridge in
    // app_startup.js), not to reimplement any of it. Accelerators picked to
    // avoid the Edit/Window menus' defaults above.
    let toggle_listening_item = MenuItemBuilder::with_id("menu-toggle-listening", "Start or Stop Listening")
        .accelerator("CmdOrCtrl+L")
        .build(app)?;
    let range_next_item = MenuItemBuilder::with_id("menu-range-next", "Next Verse")
        .accelerator("CmdOrCtrl+Right")
        .build(app)?;
    // Back (undo the last change to the screen) mirrors Next; Clear Bible is
    // what gets cleared most during a sermon, so it owns Cmd+K. Clear Slide
    // (only the slide under a verse since the layered output) has no default.
    let output_back_item = MenuItemBuilder::with_id("menu-output-back", "Back to the Previous Screen")
        .accelerator("CmdOrCtrl+Left")
        .build(app)?;
    let clear_bible_item = MenuItemBuilder::with_id("menu-clear-bible", "Clear Bible")
        .accelerator("CmdOrCtrl+K")
        .build(app)?;
    let clear_all_item = MenuItemBuilder::with_id("menu-clear-all", "Clear All")
        .accelerator("CmdOrCtrl+Shift+K")
        .build(app)?;

    let controls_menu = SubmenuBuilder::new(app, "Controls")
        .item(&toggle_listening_item)
        .separator()
        .item(&range_next_item)
        .item(&output_back_item)
        .text("menu-range-end", "End Range")
        .separator()
        .item(&clear_bible_item)
        .text("menu-clear-slide", "Clear Slide")
        .text("menu-clear-media", "Clear Media")
        // The timer/clock layer's own clear — added once the output grew
        // a third composited layer alongside slide/media (see the Timer
        // tab in service.js and the server's Segments (kairo-server library.rs)).
        .text("menu-clear-timer", "Clear Timer")
        .item(&clear_all_item)
        .build()?;

    // Window: the app's other windows — the Monitor (every output, live) and
    // where outputs are set up. Close Window is under File, as on every Mac app.
    // Live production: streaming, recording, macros. Each item runs the same action as
    // Stream Deck and OSC ("menu-run-<action>", see app_startup.js runRemoteAction).
    let mut macros_menu = SubmenuBuilder::new(app, "Run Macro");
    for n in 1..=9 {
        macros_menu = macros_menu.text(format!("menu-run-macro-{n}"), format!("Macro {n}"));
    }
    let macros_menu = macros_menu.build()?;
    let production_menu = SubmenuBuilder::new(app, "Production")
        .text("menu-run-stream-start", "Go Live")
        .text("menu-run-stream-stop", "Stop Stream")
        .separator()
        .text("menu-run-record-start", "Start Recording")
        .text("menu-run-record-stop", "Stop Recording")
        .separator()
        .item(&macros_menu)
        .separator()
        .text("menu-run-auto-send-on", "Auto-send Verses On")
        .text("menu-run-auto-send-off", "Auto-send Verses Off")
        .build()?;

    // Director mode's switcher (nothing happens while Director mode is off).
    let mut cams_menu = SubmenuBuilder::new(app, "Input to the Screen");
    for n in 1..=9 {
        let mut b = MenuItemBuilder::with_id(format!("menu-run-director-input-{n}"), format!("Input {n}"));
        if n <= 4 { b = b.accelerator(format!("CmdOrCtrl+Alt+{n}")); }
        cams_menu = cams_menu.item(&b.build(app)?);
    }
    let cams_menu = cams_menu.build()?;
    let mut layouts_menu = SubmenuBuilder::new(app, "Layout to Preview");
    for n in 1..=9 {
        layouts_menu = layouts_menu.text(format!("menu-run-director-layout-{n}"), format!("Layout {n}"));
    }
    let layouts_menu = layouts_menu.build()?;
    let director_menu = SubmenuBuilder::new(app, "Director")
        .item(&MenuItemBuilder::with_id("menu-run-director-send", "Send to Screen").accelerator("CmdOrCtrl+Return").build(app)?)
        .item(&MenuItemBuilder::with_id("menu-run-director-fade-to-black", "Fade to Black").accelerator("CmdOrCtrl+Shift+B").build(app)?)
        .text("menu-run-director-undo", "Undo Last Send")
        .separator()
        .item(&cams_menu)
        .item(&layouts_menu)
        .separator()
        .item(&MenuItemBuilder::with_id("menu-run-director-replay", "Instant Replay to Preview").accelerator("CmdOrCtrl+Shift+R").build(app)?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize_with_text("Zoom")
        .separator()
        .text("menu-monitor-window", "Monitor")
        .text("menu-outputs", "Outputs…")
        .separator()
        .bring_all_to_front()
        .build()?;

    let help_menu = SubmenuBuilder::new(app, "Help")
        .text("menu-getting-started", "Getting Started")
        .text("menu-shortcuts", "Keyboard Shortcuts")
        .text("menu-help", "Kairo Help")
        .separator()
        .text("menu-learn-more", "Kairo on GitHub")
        .build()?;

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&controls_menu)
        .item(&director_menu)
        .item(&production_menu)
        .item(&window_menu)
        .item(&help_menu)
        .build()
}

// Shared by both the startup background check and the on-demand "Check for
// Updates…" menu item. `announce_up_to_date` distinguishes them: the silent
// startup check should stay silent unless there's actually something to
// show, while a menu click is a direct request that deserves a response
// either way (a "Check for Updates" button that says nothing when there's
// nothing new reads as broken, not reassuring).
fn check_for_updates(app: AppHandle, announce_up_to_date: bool) {
    tauri::async_runtime::spawn(async move {
        match app.updater() {
            Ok(updater) => match updater.check().await {
                Ok(Some(update)) => {
                    println!("[KAIRO] Update available: {}", update.version);
                    let _ = app.emit(
                        "update-available",
                        serde_json::json!({
                            "version": update.version,
                            "notes":   update.body.unwrap_or_default(),
                        }),
                    );
                }
                Ok(None) => {
                    println!("[KAIRO] App is up to date.");
                    if announce_up_to_date {
                        let _ = app.emit("update-check-result", serde_json::json!({ "upToDate": true }));
                    }
                }
                Err(e) => {
                    eprintln!("[KAIRO] Update check error: {e}");
                    if announce_up_to_date {
                        let _ = app.emit("update-check-result", serde_json::json!({ "error": e.to_string() }));
                    }
                }
            },
            Err(e) => {
                eprintln!("[KAIRO] Updater unavailable: {e}");
                if announce_up_to_date {
                    let _ = app.emit("update-check-result", serde_json::json!({ "error": e.to_string() }));
                }
            }
        }
    });
}

struct ServerProcess(Arc<Mutex<Option<Child>>>);

/// Holds the dynamically-allocated server port and auth token. Generated once
/// at startup, passed to the Node sidecar via env vars, and surfaced to the
/// frontend via Tauri IPC. Allows the server to bind to an OS-assigned free
/// loopback port (no hardcoded 7777, no `kill -9` on collision) and requires
/// a shared secret on every HTTP/WS request to prevent other local processes
/// from controlling the live display.
struct ServerConfig {
    port:  u16,
    token: String,
}

/// Prefer the historical port 7777 (so `frontendDist` in tauri.conf.json keeps
/// working in the common case and the existing dev workflow is untouched). If
/// 7777 is already in use, fall back to an OS-assigned free port. The window
/// is then navigated to the actual URL after the server comes up — see the
/// post-health-check navigation step in `setup`.
///
/// The probe listener is dropped immediately; Node re-binds a moment later.
/// There is a tiny race window (~ms) but no other process is realistically
/// going to grab that exact port in between.
fn pick_free_port() -> u16 {
    // Dev builds never spawn their own Node sidecar (see the
    // `#[cfg(not(debug_assertions))]` gate around `start_server` in `run`
    // below) — the real, only server is the one `beforeDevCommand` already
    // started (`npm run server`, always port 7777) before this process even
    // launches. That means the 7777-probe below deterministically fails
    // every single dev run (the port is legitimately taken, by the server
    // this app is actually supposed to use), silently falling back to a
    // random port nothing is listening on — every consumer of this value
    // (the health-check poll, the post-splash `navigate()`, and the
    // `get_server_port`/`get_server_config` IPC commands the frontend and
    // the display window both rely on) would then all point at that dead
    // port instead of the real server. Concretely: the window would
    // navigate away from the correctly-loaded page to a connection that
    // refuses, the frontend's JS would never run, and the app would appear
    // to hang on the splash screen or show a blank window — indistinguishable
    // from "not running". Always use 7777 in dev; only probe/fall back to a
    // random free port in a release build, where this process's own sidecar
    // spawn (below) is what's actually claiming that port.
    // KAIRO_SERVER_PORT (the same variable the server reads) lets a second dev
    // copy run beside the first on another port.
    #[cfg(debug_assertions)]
    {
        return std::env::var("KAIRO_SERVER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7777);
    }
    #[cfg(not(debug_assertions))]
    {
        if std::net::TcpListener::bind("127.0.0.1:7777").is_ok() {
            return 7777;
        }
        std::net::TcpListener::bind("127.0.0.1:0")
            .ok()
            .and_then(|l| l.local_addr().ok())
            .map(|a| a.port())
            .unwrap_or(7777)
    }
}

/// 32 bytes of OS entropy → URL-safe base64. ~43 chars, ~256 bits of entropy.
/// KAIRO_SERVER_TOKEN (32+ characters) replaces it, for driving a test run
/// from scripts; whoever can set this process's environment controls it anyway.
fn generate_auth_token() -> String {
    if let Ok(t) = std::env::var("KAIRO_SERVER_TOKEN") {
        if t.len() >= 32 {
            return t;
        }
    }
    let mut bytes = [0u8; 32];
    if getrandom::getrandom(&mut bytes).is_err() {
        // Fallback: never happens on supported platforms, but if the OS RNG
        // is unavailable we'd rather start with a weak token than refuse to boot.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = ((nanos >> (i % 16)) as u8) ^ (i as u8);
        }
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

// Output Looks: multiple independent named NDI/Syphon outputs (e.g. "Atem"
// AND "NDI Output" as separate senders at once) rather than one fixed
// sender each — mirrors ProPresenter's own Screen Configuration list, and
// Kairo's own pre-existing extraDisplays() pattern for External Display.
// Keyed by an id the frontend generates (settings.ndiOutputs/syphonOutputs,
// src/app.js) — each id gets its own independent handle/sender thread the
// first time anything touches it; ndi.rs/syphon.rs themselves are
// completely unaware multiple instances exist, they only ever see one
// Arc<Mutex<Handle>> at a time, same as before this change.
// One set of handles per sender kind (senders.rs), keyed by the output's id.
#[cfg(target_os = "macos")]
type SyphonState = Handles<syphon::SyphonHandle>;
// Windows' equivalent of the macOS Syphon state: the frontend's syphon_* commands (below)
// dispatch to whichever of the two exists on the current platform; there is never a build with both.
#[cfg(target_os = "windows")]
type SyphonState = Handles<spout::SpoutHandle>;
type NdiState = Handles<ndi::NdiHandle>;
type DeckLinkState = Handles<decklink::DeckLinkHandle>;

// ── The server ────────────────────────────────────────────────────────────

/// Starts Kairo's server inside this process (release builds; dev runs it
/// separately via `beforeDevCommand`): the UI files, the Bible data and
/// models from the bundle's resources, settings and downloads in app data.
#[cfg_attr(debug_assertions, allow(dead_code))]
fn start_server(app: &AppHandle, port: u16, token: &str) {
    let cwd = || std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let resource_dir = app.path().resource_dir().unwrap_or_else(|e| {
        eprintln!("[KAIRO] WARNING: Tauri resource dir unavailable ({e}) — falling back to cwd-relative paths.");
        cwd()
    });
    let pick = |rel: &str| {
        let bundled = resource_dir.join(rel);
        if bundled.exists() { bundled } else { cwd().join(rel) }
    };
    let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("kairo"));
    std::fs::create_dir_all(&app_data_dir).ok();
    let cfg = kairo_server::Config {
        port,
        token: token.to_string(),
        db_dir: pick("databases/bibles"),
        models_dir: app_data_dir.join("models"),
        app_data_dir,
        ui_dir: pick("src"),
    };
    println!("[KAIRO] databases:  {}", cfg.db_dir.display());
    println!("[KAIRO] ui:         {}", cfg.ui_dir.display());
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = kairo_server::serve(cfg).await {
            eprintln!("[KAIRO] Server failed: {e}");
            // Without its server Kairo can't show anything: say why instead of opening a window
            // that never loads.
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            app2.dialog()
                .message(format!("Kairo couldn't start its built-in server, so it can't run.\n\n{e}\n\nQuit Kairo and open it again. If this keeps happening, restart the computer."))
                .title("Kairo couldn't start")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    });
}

// ── Tauri commands ────────────────────────────────────────────────────────

#[tauri::command]
fn get_server_port(state: tauri::State<'_, ServerConfig>) -> u16 {
    state.port
}

#[tauri::command]
fn get_server_token(state: tauri::State<'_, ServerConfig>) -> String {
    state.token.clone()
}

#[tauri::command]
fn get_server_config(state: tauri::State<'_, ServerConfig>) -> serde_json::Value {
    serde_json::json!({ "port": state.port, "token": state.token })
}

/// Real, OS-level connected-display enumeration for the External Display /
/// extra-output pickers. The frontend previously relied ENTIRELY on the
/// browser's Window Management API (`getScreenDetails()` / `screen.
/// isExtended`) to detect connected monitors — that API is Chromium-only;
/// WebKit (the engine behind Tauri's WKWebView on macOS) has never
/// implemented it, so `getScreenDetails` is simply `undefined` and
/// `isExtended` is unsupported too. On macOS this meant a newly connected
/// display was silently never detected — not a permission issue, not a
/// timing issue, the API the whole feature depended on doesn't exist in
/// this webview at all. Tauri's own monitor APIs go through the OS
/// directly (winit → Cocoa NSScreen on macOS, Win32 on Windows), so they
/// work regardless of what the webview engine exposes to JS. Same field
/// shape the frontend's Window-Management-API path already produces
/// (index/width/height/left/top/isPrimary) so both paths are interchangeable.
// macOS's NSScreen (what tao's available_monitors() reads on this platform)
// only reflects a hotplugged display once the run loop has actually
// processed NSApplicationDidChangeScreenParametersNotification — a query
// issued off the main thread (which is where #[tauri::command] handlers
// run by default) can read a stale, cached screen count indefinitely, even
// polled repeatedly, even minutes after the display was connected. Real
// incident this caused: a genuinely connected second monitor never showed
// up no matter how often the frontend re-polled this command, because the
// polling itself was never the problem — every single call was reading
// the same stale off-main-thread snapshot. run_on_main_thread forces the
// actual query onto the thread where the run loop (and therefore NSScreen's
// notification-driven cache) is live.
#[tauri::command]
fn list_monitors(app: AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let app_for_thread = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| -> Result<Vec<serde_json::Value>, String> {
            let win = app_for_thread
                .get_webview_window("main")
                .ok_or_else(|| "main window not available".to_string())?;
            let monitors = win.available_monitors().map_err(|e| e.to_string())?;
            let primary_pos = win
                .primary_monitor()
                .ok()
                .flatten()
                .map(|m| *m.position());
            Ok(monitors
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let pos = m.position();
                    let size = m.size();
                    // available_monitors()/Monitor::size()/position() are PHYSICAL
                    // pixels — but WebviewWindowBuilder's x/y/width/height (what
                    // openDisplayOutput/openDisplayWindow feed straight from this
                    // command's output) are LOGICAL pixels. On any monitor with a
                    // scale factor != 1 (any Retina display), using physical values
                    // unconverted places the window at the wrong coordinates
                    // entirely — on a 3-monitor span this can easily land off
                    // every actual screen, so the window opens and content renders
                    // into it correctly, just somewhere nothing is ever watching.
                    // Real incident this caused: "I see the displays but nothing
                    // is sending" — detection was working and content WAS being
                    // pushed to the window, it was just invisible off-screen.
                    let scale = m.scale_factor();
                    let logical_size = size.to_logical::<i32>(scale);
                    let logical_pos  = pos.to_logical::<i32>(scale);
                    serde_json::json!({
                        "index": i,
                        "width": logical_size.width,
                        "height": logical_size.height,
                        "left": logical_pos.x,
                        "top": logical_pos.y,
                        "isPrimary": primary_pos.map(|p| p == *pos).unwrap_or(i == 0),
                    })
                })
                .collect())
        })();
        // Channel send failing just means the caller already gave up
        // waiting (e.g. dropped rx) — nothing to do about that here.
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// Called by the frontend once it has actually painted the real app (auth
/// token loaded, styles applied) — this is the signal to reveal the main
/// window. Showing the window immediately after dispatching `navigate()`
/// (the old approach) raced the new page's own load/paint: the window
/// could become visible while WebKit was still mid-navigation, showing its
/// default white document background before our dark CSS applied — a
/// visible white flash. Letting the frontend decide "ready" removes the
/// race entirely; see the health-check thread's fallback timer in `run()`
/// for what happens if this is never called (JS error, etc).
#[tauri::command]
fn signal_main_ready(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Called from the frontend when the user clicks "Update & Restart".
/// Re-fetches the update (already confirmed available) and installs it.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(update) = update {
        update
            .download_and_install(|_chunk, _total| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

// ── NDI commands ──────────────────────────────────────────────────────────
// The frontend calls these to drive the native NDI sender. We don't bundle
// libndi; the user's NDI Tools install (or NDI SDK) provides it.

#[tauri::command]
fn ndi_available() -> bool {
    ndi::is_libndi_available()
}

#[tauri::command]
fn ndi_start(app: AppHandle, id: String, source_name: String, width: Option<i32>, height: Option<i32>) -> Result<(), String> {
    let state = app.state::<NdiState>();
    let handle = state.handle_for(&id)?;
    {
        // Check-and-reserve happens under one lock acquisition so two
        // near-simultaneous calls can't both pass the check before either's
        // background thread has actually installed its sender — see
        // try_reserve_start's doc comment.
        let mut h = handle.lock().map_err(|e| e.to_string())?;
        if !h.try_reserve_start() {
            return Ok(()); // idempotent — already broadcasting or starting
        }
    }
    let result = ndi::start(&source_name, width.unwrap_or(0), height.unwrap_or(0), handle.clone());
    if result.is_err() {
        // start() failed before ever reaching the point where it would clear
        // the reservation itself — clear it here so a retry isn't blocked.
        if let Ok(mut h) = handle.lock() { h.clear_starting(); }
    }
    result
}

#[tauri::command]
fn ndi_stop(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<NdiState>();
    let handle = state.handle_for(&id)?;
    let mut h = handle.lock().map_err(|e| e.to_string())?;
    h.stop();
    Ok(())
}

#[tauri::command]
fn ndi_update(app: AppHandle, id: String, verse: String, reference: String) -> Result<(), String> {
    let state = app.state::<NdiState>();
    let handle = state.handle_for(&id)?;
    let h = handle.lock().map_err(|e| e.to_string())?;
    h.update(verse, reference);
    Ok(())
}

// Output Looks — Media/Timer layer parity for the native NDI sender (see
// ndi.rs's own render_frame). `image_base64: None` clears the Media layer
// (mirrors display.html's renderMediaStage(null)); Some(b64) is standard
// (non-URL-safe) base64-encoded PNG/JPEG bytes, the same encoding this app
// already uses for media data URIs elsewhere (e.g. the server's import
// route's dataBase64 field) — decoded here, not on the frontend, so the
// render thread only ever receives raw bytes to hand to `image::load_from_memory`.
#[tauri::command]
fn ndi_update_media(app: AppHandle, id: String, image_base64: Option<String>) -> Result<(), String> {
    let state = app.state::<NdiState>();
    let handle = state.handle_for(&id)?;
    let h = handle.lock().map_err(|e| e.to_string())?;
    h.update_media(decode_media(image_base64)?);
    Ok(())
}

#[tauri::command]
fn ndi_update_timer(app: AppHandle, id: String, text: String) -> Result<(), String> {
    let state = app.state::<NdiState>();
    let handle = state.handle_for(&id)?;
    let h = handle.lock().map_err(|e| e.to_string())?;
    h.update_timer(text);
    Ok(())
}

// ── Output capture (capture.rs) ───────────────────────────────────────────
// An NDI / Syphon output's hidden display window (`kairo-cap-<id>`,
// display.html?capture=1) calls this whenever what it shows has changed; the
// frame goes to that output's senders. A change while a capture is running
// is coalesced into one more capture after it.
#[derive(Default)]
struct CaptureState {
    busy: Mutex<std::collections::HashMap<String, Option<u32>>>,
    /// Consecutive failures per output and stage ("capture" / "syphon"), and whether the window was told.
    trouble: Mutex<std::collections::HashMap<(String, &'static str), (u32, bool)>>,
}

/// A failed capture or Syphon update used to go to stderr only, so the output's graphics froze
/// with nothing said. After a few failures in a row the main window is told
/// ("output-capture-trouble" { id, stage, error }), and told again with error null once it works.
const CAPTURE_TROUBLE_AFTER: u32 = 3;
fn note_capture(app: &AppHandle, id: &str, stage: &'static str, r: Result<(), String>) {
    let tell = {
        let state = app.state::<CaptureState>();
        let Ok(mut t) = state.trouble.lock() else { return };
        match r {
            Ok(()) => match t.remove(&(id.to_string(), stage)) {
                Some((_, true)) => Some(None),
                _ => None,
            },
            Err(e) => {
                eprintln!("[Capture] {id} ({stage}): {e}");
                let entry = t.entry((id.to_string(), stage)).or_insert((0, false));
                entry.0 += 1;
                if entry.0 >= CAPTURE_TROUBLE_AFTER && !entry.1 {
                    entry.1 = true;
                    Some(Some(e))
                } else {
                    None
                }
            }
        }
    };
    if let Some(error) = tell {
        let _ = app.emit("output-capture-trouble", serde_json::json!({ "id": id, "stage": stage, "error": error }));
    }
}

#[tauri::command]
fn output_capture_supported() -> bool {
    capture::supported()
}

#[tauri::command]
fn output_capture(app: AppHandle, id: String, width: u32, height: u32, fade_ms: Option<u32>) -> Result<(), String> {
    let fade = fade_ms.unwrap_or(0);
    {
        let state = app.state::<CaptureState>();
        let mut busy = state.busy.lock().map_err(|e| e.to_string())?;
        if let Some(pending) = busy.get_mut(&id) {
            *pending = Some(pending.unwrap_or(0).max(fade));
            return Ok(());
        }
        busy.insert(id.clone(), None);
    }
    run_capture(app, id, width.clamp(16, 7680), height.clamp(16, 4320), fade);
    Ok(())
}

fn run_capture(app: AppHandle, id: String, width: u32, height: u32, fade: u32) {
    let Some(window) = app.get_webview_window(&format!("kairo-cap-{id}")) else {
        if let Ok(mut b) = app.state::<CaptureState>().busy.lock() { b.remove(&id); }
        return;
    };
    let (app2, id2) = (app.clone(), id.clone());
    let _ = capture::snapshot_alpha(&window, width, height, move |r| {
        match r {
            Ok(frame) => {
                // Diagnostics: KAIRO_CAPTURE_DUMP=<dir> saves each frame as a PNG.
                if let Some(dir) = std::env::var_os("KAIRO_CAPTURE_DUMP") {
                    let path = std::path::Path::new(&dir).join(format!("{id2}-{}.png", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())));
                    let rgba: Vec<u8> = frame.unpremultiplied().chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
                    if let Some(img) = image::RgbaImage::from_raw(frame.width, frame.height, rgba) { let _ = img.save(&path); }
                }
                note_capture(&app2, &id2, "capture", Ok(()));
                let frame = Arc::new(frame);
                // A moving background playing under this output: the
                // snapshot is what sits on top of it (video_bg.rs mixes).
                let playing = app2.state::<VideoBgState>().0.lock().ok().and_then(|m| m.get(&id2).map(|v| v.set_overlay(frame.clone(), fade)));
                if playing.is_none() {
                    push_frame(&app2, &id2, frame, fade);
                }
            }
            Err(e) => note_capture(&app2, &id2, "capture", Err(e.to_string())),
        }
        // One more if something changed meanwhile.
        let next = app2.state::<CaptureState>().busy.lock().ok().and_then(|mut b| match b.get(&id2) {
            Some(Some(f)) => { let f = *f; b.insert(id2.clone(), None); Some(f) }
            _ => { b.remove(&id2); None }
        });
        if let Some(f) = next {
            run_capture(app2, id2, width, height, f);
        }
    });
}

/// A finished frame to every sender of output `id`: the stream and the recording for a
/// `stream-` capture, else each native sender kind (senders.rs).
fn push_frame(app: &AppHandle, id: &str, frame: Arc<capture::Frame>, fade: u32) {
    if id.starts_with("stream-") {
        app.state::<stream::Engines>().push_frame(id, frame, fade);
        return;
    }
    for sender in senders::all() {
        let sent = sender.send(app, id, frame.clone(), fade);
        if let Some(stage) = sender.stage() {
            note_capture(app, id, stage, sent);
        }
    }
}

// Full-frame video backgrounds under NDI / Syphon / DeckLink (video_bg.rs):
// the output's capture window reports the video it stopped drawing (its URL
// on the local server, and how it was fitted), or that there's none now.
#[derive(Default)]
struct VideoBgState(Mutex<std::collections::HashMap<String, video_bg::VideoBg>>);

#[tauri::command]
async fn output_video(app: AppHandle, id: String, url: Option<String>, fit: Option<String>, width: u32, height: u32) -> Result<(), String> {
    let Some(url) = url.filter(|u| !u.is_empty()) else {
        if let Some(v) = app.state::<VideoBgState>().0.lock().map_err(|e| e.to_string())?.remove(&id) { v.stop(); }
        return Ok(());
    };
    let fit = fit.unwrap_or_else(|| "contain".into());
    // The file behind the address, and the bundled ffmpeg, from the server.
    let (port, token) = { let c = app.state::<ServerConfig>(); (c.port, c.token.clone()) };
    let resolved: serde_json::Value = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/api/media/resolve"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "url": url }))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    let (Some(file), Some(ffmpeg)) = (resolved["path"].as_str(), resolved["ffmpeg"].as_str()) else {
        return Err(resolved["error"].as_str().unwrap_or("video not found").to_string());
    };
    let file = std::path::PathBuf::from(file);
    let state = app.state::<VideoBgState>();
    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    if map.get(&id).is_some_and(|v| v.file == file && v.fit == fit) {
        return Ok(());
    }
    if let Some(old) = map.remove(&id) { old.stop(); }
    let (app2, id2) = (app.clone(), id.clone());
    let (w, h) = (width.clamp(16, 7680), height.clamp(16, 4320));
    let v = video_bg::VideoBg::start(std::path::Path::new(ffmpeg), file, &fit, w, h, 30, move |frame| push_frame(&app2, &id2, frame, 0))?;
    map.insert(id, v);
    Ok(())
}

// ── Live streaming (stream.rs) ────────────────────────────────────────────
// Settings → Streaming's Go live: one FFmpeg pipeline per streamed output, fed by that
// output's capture window (`kairo-cap-stream-<output>`), the Director's cameras and the
// audio input. Status goes to every window as a 'stream-status' event.
#[tauri::command]
async fn stream_start(app: AppHandle, pipelines: Vec<stream::PipelineConfig>) -> Result<(), String> {
    launch(&app, stream::Kind::Stream, pipelines).await
}

/// Starts the engine of `kind`: finds an FFmpeg, then runs the pipelines on it with their
/// status going out as that kind's event. Streaming and recording both start through here.
async fn launch(app: &AppHandle, kind: stream::Kind, pipelines: Vec<stream::PipelineConfig>) -> Result<(), String> {
    let ffmpeg = stream_ffmpeg(app, &pipelines).await?;
    let app2 = app.clone();
    let on_status: stream::OnStatus = Arc::new(move |s| { let _ = app2.emit(kind.event(), s); });
    let app3 = app.clone();
    tauri::async_runtime::spawn_blocking(move || app3.state::<stream::Engines>().of(kind).start(ffmpeg, pipelines, on_status)).await.map_err(|e| e.to_string())?
}

/// The FFmpeg that streams and records: Kairo's own when it can, else one on this computer.
async fn stream_ffmpeg(app: &AppHandle, pipelines: &[stream::PipelineConfig]) -> Result<std::path::PathBuf, String> {
    let (port, token) = { let c = app.state::<ServerConfig>(); (c.port, c.token.clone()) };
    let found: serde_json::Value = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/api/stream/ffmpeg"))
        .bearer_auth(&token)
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    let own = found["path"].as_str().map(std::path::PathBuf::from);
    let need_tls = pipelines.iter().flat_map(|p| &p.destinations).any(|d| d.url.trim().to_lowercase().starts_with("rtmps://"));
    tauri::async_runtime::spawn_blocking(move || stream::streaming_ffmpeg(own, need_tls)).await.map_err(|e| e.to_string())?
        .ok_or_else(|| "No copy of FFmpeg that can stream or record was found on this computer.".to_string())
}

#[tauri::command]
fn stream_stop(app: AppHandle) {
    app.state::<stream::Engines>().of(stream::Kind::Stream).stop();
}

#[tauri::command]
fn stream_status(app: AppHandle) -> Vec<stream::PipelineStatus> {
    app.state::<stream::Engines>().of(stream::Kind::Stream).status()
}

#[tauri::command]
fn stream_program(app: AppHandle, output: Option<String>, program: Option<String>, zone: Option<stream::Zone>) {
    app.state::<stream::Engines>().set_program(output.as_deref(), program, zone);
}

/// The cameras set up in Director changed while streaming or recording: the engines say which
/// are not in the picture yet (they are used from the next start).
#[tauri::command]
fn stream_cameras(app: AppHandle, cameras: Vec<stream::Camera>) {
    app.state::<stream::Engines>().set_cameras(cameras);
}

/// The sound mixer's levels and mutes changed: applied live to the stream and the recording.
#[tauri::command]
fn stream_mix(app: AppHandle, mix: Vec<stream::AudioIn>) {
    app.state::<stream::Engines>().set_mix(mix);
}

// ── Recording (the stream engine writing a file) ─────────────────────────
// The top bar's Record: the same picture, cameras and sound as a stream, written to an MP4
// in the folder chosen under Settings → Streaming (Movies/Kairo by default). Separate from
// streaming, so either starts and stops without touching the other. Status: 'record-status'.
/// A file name part from the page: letters, digits, spaces and - _ only.
fn safe_name(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_alphanumeric() || " -_".contains(*c)).take(80).collect();
    let t = t.trim().to_string();
    if t.is_empty() { "Kairo".into() } else { t }
}

#[tauri::command]
async fn record_start(app: AppHandle, mut pipelines: Vec<stream::PipelineConfig>, folder: Option<String>, name: String) -> Result<Vec<String>, String> {
    let dir = match folder.filter(|f| !f.trim().is_empty()) {
        Some(f) => std::path::PathBuf::from(f),
        None => app.path().video_dir().map_err(|e| e.to_string())?.join("Kairo"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't use the recordings folder {}: {e}", dir.display()))?;
    let several = pipelines.len() > 1;
    let mut files = Vec::new();
    for p in &mut pipelines {
        p.destinations.clear();
        let stem = if several { format!("{} {}", safe_name(&name), safe_name(&p.output)) } else { safe_name(&name) };
        let file = dir.join(format!("{stem}.mp4")).to_string_lossy().to_string();
        files.push(file.clone());
        p.record_to = Some(file);
    }
    launch(&app, stream::Kind::Record, pipelines).await?;
    Ok(files)
}

#[tauri::command]
fn record_stop(app: AppHandle) {
    app.state::<stream::Engines>().of(stream::Kind::Record).stop();
}

#[tauri::command]
fn record_status(app: AppHandle) -> Vec<stream::PipelineStatus> {
    app.state::<stream::Engines>().of(stream::Kind::Record).status()
}

/// Shows a recording in Finder / Explorer.
#[tauri::command]
fn record_reveal(path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    if p.extension().and_then(|e| e.to_str()) != Some("mp4") || !p.is_file() { return Err("That recording isn't there any more.".into()); }
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(&p).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(p.parent().unwrap_or(&p)).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

// ── DeckLink (decklink.rs) ────────────────────────────────────────────────
// Cards and their modes; start returns the frame size the card plays, which
// the output's capture window is then made at.
#[tauri::command]
fn decklink_list() -> serde_json::Value {
    decklink::list()
}

#[tauri::command]
fn decklink_start(app: AppHandle, id: String, device: i32, mode: u32, keying: bool, fps: f64) -> Result<serde_json::Value, String> {
    let handle = app.state::<DeckLinkState>().handle_for(&id)?;
    if let Ok(mut h) = handle.lock() {
        h.stop();
    }
    let (w, h) = decklink::start(device, mode, keying, fps, handle)?;
    Ok(serde_json::json!({ "width": w, "height": h }))
}

#[tauri::command]
fn decklink_stop(app: AppHandle, id: String) -> Result<(), String> {
    let handle = app.state::<DeckLinkState>().handle_for(&id)?;
    let mut h = handle.lock().map_err(|e| e.to_string())?;
    h.stop();
    Ok(())
}

// ── Syphon commands (macOS only) ──────────────────────────────────────────
// Syphon.framework is bundled with the app, so unlike NDI there's no SDK to
// install. The framework requires a CGL context — the syphon module handles
// that internally.
#[cfg(target_os = "macos")]
#[tauri::command]
fn syphon_available() -> bool {
    syphon::is_syphon_available()
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn syphon_start(app: AppHandle, id: String, source_name: String, width: Option<u32>, height: Option<u32>) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    syphon::start(&source_name, width.unwrap_or(0), height.unwrap_or(0), state.handle_for(&id)?)
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn syphon_stop(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    syphon::stop(state.handle_for(&id)?)
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn syphon_update(app: AppHandle, id: String, verse: String, reference: String) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    syphon::update(&verse, &reference, state.handle_for(&id)?)
}

// Output Looks — Media/Timer layer parity for the native Syphon sender.
// Same base64 handling and clear-on-None convention as ndi_update_media —
// see its own comment.
#[cfg(target_os = "macos")]
#[tauri::command]
fn syphon_update_media(app: AppHandle, id: String, image_base64: Option<String>) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    let bytes = decode_media(image_base64)?;
    syphon::update_media(bytes, state.handle_for(&id)?)
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn syphon_update_timer(app: AppHandle, id: String, text: String) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    syphon::update_timer(&text, state.handle_for(&id)?)
}

// ── Spout commands (Windows only) ─────────────────────────────────────────
// Same command names as the macOS block above (syphon_*, not spout_*) —
// the frontend treats "same-machine native sender" as one concept with a
// platform-dependent label (see app.js's nativeSenderLabel/outputTypeLabel)
// rather than two separate integrations, so it only ever calls syphon_*
// regardless of OS. spout2-rs's DX11 Sender owns its own D3D11 device, so
// unlike Syphon there's no separate is_spout_available() capability check
// worth exposing — see spout::is_spout_available's own comment.
#[cfg(target_os = "windows")]
#[tauri::command]
fn syphon_available() -> bool {
    spout::is_spout_available()
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn syphon_start(app: AppHandle, id: String, source_name: String, width: Option<u32>, height: Option<u32>) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    spout::start(&source_name, width.unwrap_or(0), height.unwrap_or(0), state.handle_for(&id)?)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn syphon_stop(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    spout::stop(state.handle_for(&id)?)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn syphon_update(app: AppHandle, id: String, verse: String, reference: String) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    spout::update(&verse, &reference, state.handle_for(&id)?)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn syphon_update_media(app: AppHandle, id: String, image_base64: Option<String>) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    let bytes = decode_media(image_base64)?;
    spout::update_media(bytes, state.handle_for(&id)?)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn syphon_update_timer(app: AppHandle, id: String, text: String) -> Result<(), String> {
    let state = app.state::<SyphonState>();
    spout::update_timer(&text, state.handle_for(&id)?)
}

// Stubs for every other platform (Linux) — frontend calls these
// unconditionally and expects a clean `false` / no-op rather than an
// "unknown command" error.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
fn syphon_available() -> bool { false }
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
fn syphon_start(_id: String, _source_name: String, _width: Option<u32>, _height: Option<u32>) -> Result<(), String> {
    Err("Syphon/Spout aren't available on this platform".into())
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
fn syphon_stop(_id: String) -> Result<(), String> { Ok(()) }
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
fn syphon_update(_id: String, _verse: String, _reference: String) -> Result<(), String> { Ok(()) }
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
fn syphon_update_media(_id: String, _image_base64: Option<String>) -> Result<(), String> { Ok(()) }
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
fn syphon_update_timer(_id: String, _text: String) -> Result<(), String> { Ok(()) }

// ── App entry ─────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Allocate the server's port + auth token ONCE, up front, so the same
    // values are visible to the Node sidecar (via env) and to the frontend
    // (via the get_server_config IPC command).
    let server_config = ServerConfig {
        port:  pick_free_port(),
        token: generate_auth_token(),
    };
    println!("[KAIRO] Server will bind to 127.0.0.1:{}", server_config.port);

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // "Paste from clipboard" imports (service.js's quickImportClipboard)
        // — see the Cargo.toml comment next to this dependency.
        .plugin(tauri_plugin_clipboard_manager::init())
        // See the Cargo.toml comment next to this dependency: only the
        // native right-click menu is suppressed, so the app's own
        // JS-based context menus (Timer/Slides/Songs/Media cards) are
        // what the operator actually sees on right-click, in dev builds
        // too — not just release builds where WRY's devtools menu never
        // gets compiled in to begin with.
        .plugin(
            tauri_plugin_prevent_default::Builder::new()
                .with_flags(tauri_plugin_prevent_default::Flags::CONTEXT_MENU)
                .build(),
        )
        .manage(ServerProcess(Arc::new(Mutex::new(None))))
        .manage(server_config)
        .manage(NdiState::default())
        .manage(CaptureState::default())
        .manage(stream::Engines::default())
        .manage(VideoBgState::default())
        // An output's capture window closing ends its video background.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(id) = window.label().strip_prefix("kairo-cap-") {
                    if let Some(v) = window.app_handle().state::<VideoBgState>().0.lock().ok().and_then(|mut m| m.remove(id)) {
                        v.stop();
                    }
                }
            }
        })
        .manage(DeckLinkState::default());

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        builder = builder.manage(SyphonState::default());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            get_server_port,
            get_server_token,
            get_server_config,
            list_monitors,
            signal_main_ready,
            fonts::list_system_fonts,
            install_update,
            ndi_available,
            ndi_start,
            ndi_stop,
            ndi_update,
            ndi_update_media,
            ndi_update_timer,
            output_capture,
            stream_start,
            stream_stop,
            stream_status,
            stream_program,
            stream_mix,
            stream_cameras,
            record_start,
            record_stop,
            record_status,
            record_reveal,
            output_video,
            decklink_list,
            decklink_start,
            decklink_stop,
            output_capture_supported,
            syphon_available,
            syphon_start,
            syphon_stop,
            syphon_update,
            syphon_update_media,
            syphon_update_timer,
        ])
        .menu(build_app_menu)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            // Everything here just forwards to the frontend as a plain window
            // event — app.js already owns New Theme/Import/Export (file
            // pickers, unsaved-state handling, toasts) via their existing
            // toolbar buttons, so the menu should trigger the exact same
            // code path rather than a second, divergent implementation in
            // Rust. "Check for Updates…"/"Learn More" are the only two menu
            // items handled entirely natively, since neither has (or needs)
            // a frontend counterpart.
            match id {
                "menu-check-updates" => check_for_updates(app.clone(), true),
                "menu-learn-more" => { let _ = app.shell().open("https://github.com/miide-Adeyemi/Kairo", None); }
                // Every other item of ours ("menu-…") is the frontend's to
                // carry out (see initNativeMenuBridge in app_startup.js).
                other if other.starts_with("menu-") => {
                    let _ = app.emit(other, ());
                }
                _ => {}
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();

            // Owner: "can we remove this splash?" — the dedicated splash
            // window (branded loading screen shown while the Node sidecar
            // boots, ~2-4s cold start) is gone. The main window already only
            // reveals itself once the frontend calls signal_main_ready() —
            // real paint confirmed, so no white-flash regression from that —
            // it just means there's a real few-second gap on a cold launch
            // where NOTHING is visible yet (no splash, no window) before the
            // main window appears. Accepted tradeoff per the owner's explicit
            // ask, not an oversight.

            // Start the server, inside this process.
            // In dev, the server is started by `beforeDevCommand` in tauri.conf.json
            // so we don't spawn a second one (which would collide on the same port).
            #[cfg(not(debug_assertions))]
            {
                let cfg = app.state::<ServerConfig>();
                start_server(&handle, cfg.port, &cfg.token);
            }
            #[cfg(debug_assertions)]
            {
                println!("[KAIRO] Dev mode — server started externally via beforeDevCommand.");
                let _ = &handle; // keep handle alive for use below
            }

            // Poll /health until the server responds, then show the main
            // window. Max wait: 20 seconds (40 × 500ms).
            let handle2 = handle.clone();
            let health_url = {
                let cfg = handle.state::<ServerConfig>();
                format!("http://127.0.0.1:{}/health", cfg.port)
            };
            std::thread::spawn(move || {
                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(1))
                    .build()
                    .unwrap_or_default();

                // A dev run starts the server through `cargo run --release`, which
                // rebuilds it after any code change (about 90 s) — far past 20 s, and
                // giving up then left a white window that never recovered. Dev waits
                // up to 5 minutes; an installed app's server starts in seconds.
                let max_attempts = if cfg!(debug_assertions) { 600 } else { 40 };
                let mut server_ready = false;
                for attempt in 0..max_attempts {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    // `/health` answers as soon as Express binds (~500ms), but the
                    // detection worker (map load, anchor trie, fingerprints) needs a
                    // couple more seconds. Wait for `workerBasicReady` to flip true
                    // so the user never lands on a half-initialised app.
                    if let Ok(resp) = client.get(&health_url).send() {
                        let body = resp.text().unwrap_or_default();
                        if body.contains("\"workerBasicReady\":true") {
                            println!("[KAIRO] Server ready after ~{}ms", attempt * 500);
                            server_ready = true;
                            break;
                        }
                    }
                }
                if !server_ready {
                    eprintln!("[KAIRO] Server/worker not ready within {}s — showing main window anyway.", max_attempts / 2);
                }

                // Navigate to the real server URL, but do NOT show the window
                // yet — showing it here raced the new page's own load/paint
                // (navigate() only dispatches the load; it doesn't wait for
                // it), so the window could become visible while WebKit was
                // still mid-navigation and briefly show its default white
                // background before our dark CSS applied. The frontend now
                // calls `signal_main_ready` once it has actually painted, and
                // THAT reveals the window instead. This fallback timer just
                // guarantees the window isn't stuck invisible forever if that
                // signal never arrives (JS error, non-Tauri edge case, etc).
                if let Some(win) = handle2.get_webview_window("main") {
                    // ALWAYS (re)navigate now that the server is confirmed up.
                    // The window auto-loads `frontendDist` (localhost:7777) at
                    // creation time — which is at app launch, BEFORE the Node
                    // sidecar is listening — so that first navigation fails
                    // (connection refused → blank page) and must be retried.
                    //
                    // We use the NATIVE `navigate()` (Rust → webview command)
                    // rather than `eval("location.replace(...)")`. After a
                    // failed initial load the page has no live JS context, so
                    // an in-page `eval` silently no-ops and leaves a white
                    // screen — exactly what happened on a cold /Applications
                    // launch where the server takes a few seconds to bind.
                    // `navigate()` reloads the webview regardless of its state.
                    let cfg = handle2.state::<ServerConfig>();
                    match format!("http://127.0.0.1:{}/", cfg.port).parse::<tauri::Url>() {
                        Ok(url) => { let _ = win.navigate(url); }
                        Err(e)  => eprintln!("[KAIRO] invalid frontend URL: {e}"),
                    }
                }
                let handle_fallback = handle2.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(12));
                    if let Some(win) = handle_fallback.get_webview_window("main") {
                        if !win.is_visible().unwrap_or(false) {
                            eprintln!("[KAIRO] Frontend never signalled ready within 12s — showing main window anyway.");
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                });

                // Check for updates in the background after the window is visible.
                // Only runs in release builds — updater endpoint won't resolve in dev.
                // Silent unless something's actually found (announce_up_to_date:
                // false) — unlike the menu's on-demand "Check for Updates…", a
                // background check the user didn't ask for shouldn't ever pop up
                // just to say "you're fine".
                #[cfg(not(debug_assertions))]
                {
                    let handle3 = handle2.clone();
                    tauri::async_runtime::spawn(async move {
                        // Small delay so the UI settles before we show a banner.
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        check_for_updates(handle3, false);
                    });
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Error building KAIRO")
        .run(|app, event| {
            if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
                app.state::<stream::Engines>().kill_now();
                // Recover from a poisoned mutex instead of panicking — this is
                // shutdown-time child-process cleanup; a panic here would
                // skip it entirely and leak the Node sidecar.
                if let Some(mut child) = app
                    .state::<ServerProcess>()
                    .0
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take()
                {
                    let _ = child.kill();
                    let _ = child.wait();
                    println!("[KAIRO] Server stopped.");
                }
                // NDI/Syphon hold native resources (CGL context, GL texture,
                // background sender thread) that were previously never
                // released on quit — only the Node child was cleaned up.
                // stop_and_join (not plain stop) actually waits for the
                // background thread's NDIlib_send_destroy/NDIlib_destroy to
                // run, bounded so a hung native call can't hang shutdown.
                // Now a keyed collection (Output Looks: multiple independent
                // NDI outputs) — stop every instance that was ever touched,
                // not just a single fixed sender.
                {
                    app.state::<NdiState>().each(|handle| {
                        handle.lock().unwrap_or_else(|p| p.into_inner())
                            .stop_and_join(std::time::Duration::from_millis(500));
                    });
                }
                // DeckLink: disable the card's output and keyer before exit.
                {
                    app.state::<DeckLinkState>().each(|handle| {
                        handle.lock().unwrap_or_else(|p| p.into_inner()).stop();
                    });
                }
                #[cfg(target_os = "macos")]
                {
                    app.state::<SyphonState>().each(|handle| {
                        let _ = syphon::stop(handle.clone());
                    });
                }
                #[cfg(target_os = "windows")]
                {
                    app.state::<SyphonState>().each(|handle| {
                        let _ = spout::stop(handle.clone());
                    });
                }
            }
        });
}
