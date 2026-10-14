//! The server on its own, for development: `cargo run -p kairo-server`
//! (port 7777, no token, the project's own data and UI files).

fn main() -> anyhow::Result<()> {
    // Started as the offline speech engine (offline.rs): be only that.
    if let Some(code) = kairo_server::offline::worker_main() {
        std::process::exit(code);
    }
    tokio::runtime::Runtime::new()?.block_on(serve())
}

async fn serve() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    kairo_server::serve(kairo_server::Config::from_env(root.canonicalize()?)).await
}
