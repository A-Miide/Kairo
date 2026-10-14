// KAIRO — Tauri entry point
fn main() {
    // Started as the offline speech engine (kairo-server offline.rs): be only that.
    if let Some(code) = kairo_server::offline::worker_main() {
        std::process::exit(code);
    }
    kairo_lib::run();
}
