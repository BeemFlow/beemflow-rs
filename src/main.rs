//! BeemFlow CLI - workflow orchestration runtime
//!
//! Run with: cargo run --bin flow -- <command>
//! Or after build: ./target/release/flow <command>

#[tokio::main]
async fn main() {
    // Initialize logging
    beemflow::init_logging();

    // Note: .env file loading now happens automatically when EnvSecretsProvider
    // is created via Config::create_secrets_provider()

    // Run CLI (delegates to operations)
    if let Err(e) = beemflow::cli::run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
