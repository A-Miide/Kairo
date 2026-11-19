// KAIRO — Tauri entry point
// A window program on Windows, with no console of its own: without this the app opened a black
// console window beside it, with the server's log lines in it, and closing it ended the app. The
// programs it starts (kairo-server proc.rs) open none either.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started as the offline speech engine (kairo-server offline.rs): be only that.
    if let Some(code) = kairo_server::offline::worker_main() {
        std::process::exit(code);
    }
    kairo_lib::run();
}
