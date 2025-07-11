use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use theater_cli::{config::Config, run};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI early to check for verbose flag
    let cli = theater_cli::Cli::parse();

    // Load configuration
    let config = Config::load().unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load config, using defaults: {}", e);
        Config::default()
    });

    // Initialize logging based on verbose flag and config
    let log_level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        config.logging.level.parse().unwrap_or(tracing::Level::WARN)
    };

    let registry = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(log_level.into()));

    // For now, use simple formatting regardless of structured setting
    registry
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // Setup graceful shutdown handling with immediate response to Ctrl+C
    let shutdown_token = tokio_util::sync::CancellationToken::new();
    let shutdown_token_clone = shutdown_token.clone();

    // Handle Ctrl+C and other termination signals
    tokio::spawn(async move {
        // Use a simpler approach for cross-platform compatibility
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                println!("\nReceived interrupt signal, shutting down...");
                shutdown_token_clone.cancel();
            }
            Err(err) => {
                eprintln!("Unable to listen for shutdown signal: {}", err);
                // We also shut down in this case
                shutdown_token_clone.cancel();
            }
        }
    });

    // Run the CLI with cancellation support
    let result = tokio::select! {
        result = run(cli, config, shutdown_token.clone()) => result,
        _ = shutdown_token.cancelled() => {
            println!("Operation cancelled by user");
            std::process::exit(130); // Standard exit code for Ctrl+C
        }
    };

    result
}
