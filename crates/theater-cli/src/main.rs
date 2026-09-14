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

    // Initialize logging based on --log-level
    let log_level: tracing::Level = cli.log_level.into();

    let registry = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(log_level.into()));

    // For now, use simple formatting regardless of structured setting
    registry
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // Setup graceful shutdown handling with immediate response to Ctrl+C
    let shutdown_token = tokio_util::sync::CancellationToken::new();
    let shutdown_token_clone = shutdown_token.clone();

    // Handle termination signals — SIGINT (Ctrl-C) AND SIGTERM (systemd
    // stop/restart), so a service stop drives the same graceful shutdown.
    tokio::spawn(async move {
        let sig = theater_cli::utils::shutdown_signal().await;
        println!("\nReceived {}, shutting down...", sig);
        shutdown_token_clone.cancel();
    });

    // Run the CLI with cancellation support. On signal, the token is cancelled;
    // rather than hard-exit immediately (which would preempt a graceful drain —
    // the spawn command drains the whole runtime, delivering wait_for_shutdown
    // to every actor), we give the running command a bounded grace period to
    // finish, and only force-exit if it overruns.
    let run_fut = run(cli, config, shutdown_token.clone());
    tokio::pin!(run_fut);

    let result = tokio::select! {
        result = &mut run_fut => result,
        _ = shutdown_token.cancelled() => {
            const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(15);
            match tokio::time::timeout(SHUTDOWN_GRACE, &mut run_fut).await {
                Ok(result) => result,
                Err(_) => {
                    eprintln!(
                        "Shutdown grace period ({:?}) elapsed; forcing exit.",
                        SHUTDOWN_GRACE
                    );
                    std::process::exit(130); // Standard exit code for Ctrl+C
                }
            }
        }
    };

    result
}
