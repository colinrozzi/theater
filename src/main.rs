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
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Verify manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow::anyhow!("Manifest file not found: {}", args.manifest.display()));
    }

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
