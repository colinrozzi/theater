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

    // Setup graceful shutdown handling
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = shutdown_tx.send(());
    });

    // Run the CLI
    run(cli, config, shutdown_rx).await
}
