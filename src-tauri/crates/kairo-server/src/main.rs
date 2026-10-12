//! The server on its own, for development: `cargo run -p kairo-server`
//! (port 7777, no token, the project's own data and UI files).

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    kairo_server::serve(kairo_server::Config::from_env(root.canonicalize()?)).await
}
