//! BeemFlow CLI - workflow orchestration runtime
//!
//! Run with: cargo run --bin flow -- <command>
//! Or after build: ./target/release/flow <command>

#[tokio::main]
async fn main() {
    // Load .env file as early as possible (like Go version)
    // This loads environment variables for API keys, secrets, etc.
    let _ = dotenvy::dotenv();

    // Initialize logging
    beemflow::init_logging();

    // Run CLI (delegates to operations)
    if let Err(e) = beemflow::cli::run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
