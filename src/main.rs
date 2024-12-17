use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use std::path::PathBuf;
use theater::actor_runtime::ActorRuntime;
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
        return Err(anyhow::anyhow!(
            "Manifest file not found: {}",
            args.manifest.display()
        ));
    }

    // Create and initialize the runtime
    let runtime = ActorRuntime::from_file(args.manifest).await?;

    // Start the event server if configured
    if let Some(event_config) = &runtime.config.event_server.clone() {
        tokio::spawn(async move {
            theater::event_server::run_event_server(runtime.config.event_server.unwrap().port)
                .await;
        });
        info!("Event server starting on port {}", event_config.port);
    }

    info!("Actor '{}' initialized successfully!", runtime.config.name);

    // Wait for Ctrl+C
    info!("Actor started at {}", Utc::now());
    tokio::signal::ctrl_c().await?;

    info!("Shutting down...");
    Ok(())
}
