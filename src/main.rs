use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use std::path::PathBuf;
use theater::ActorRuntime;
use tracing::info;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the actor manifest file
    #[arg(short, long)]
    manifest: PathBuf,

    /// Port for the event server
    #[arg(short, long, default_value = "3030")]
    event_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Verify manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow::anyhow!("Manifest file not found: {}", args.manifest.display()));
    }

    // Start the event server
    let event_port = args.event_port;
    tokio::spawn(async move {
        theater::event_server::run_event_server(event_port).await;
    });
    info!("Event server starting on port {}", event_port);

    // Create and initialize the runtime
    let mut runtime = ActorRuntime::from_file(args.manifest).await?;
    info!("Actor '{}' initialized successfully!", runtime.config.name);

    // Wait for Ctrl+C
    info!("Actor started at {}", Utc::now());
    tokio::signal::ctrl_c().await?;

    info!("Shutting down...");
    runtime.shutdown().await?;

    Ok(())
}