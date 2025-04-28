use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::debug;

use crate::cli::client::TheaterClient;

#[derive(Debug, Parser)]
pub struct WatchArgs {
    /// Path to the actor manifest file
    #[arg(required = true)]
    pub manifest: PathBuf,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Watch interval in seconds
    #[arg(short, long, default_value = "2")]
    pub interval: u64,
}

pub fn execute(args: &WatchArgs, _verbose: bool, _json: bool) -> Result<()> {
    debug!("Watching manifest: {}", args.manifest.display());
    debug!("Connecting to server at: {}", args.address);

    // Check if the manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow!(
            "Manifest file not found: {}",
            args.manifest.display()
        ));
    }

    // Create runtime for async operations
    let runtime = tokio::runtime::Runtime::new()?;

    println!(
        "{} Watching {} for changes. Press Ctrl+C to stop.",
        style("ℹ").blue().bold(),
        style(args.manifest.display().to_string()).cyan()
    );

    // Deploy the actor initially
    let actor_id = runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);
        client.connect().await?;

        let manifest_content = std::fs::read_to_string(&args.manifest)?;
        let actor_id = client.start_actor(manifest_content, None).await?;

        println!(
            "{} Deployed actor: {}",
            style("✓").green().bold(),
            style(actor_id.to_string()).cyan()
        );

        Ok::<_, anyhow::Error>(actor_id)
    })?;

    // Get initial file metadata
    let mut last_modified = std::fs::metadata(&args.manifest)?.modified()?;
    let mut last_deploy_time = Instant::now();

    // Watch for changes
    loop {
        // Check if ctrl+c was pressed
        // In a real implementation, we would use proper Ctrl+C handling
        // For now, we'll simulate by checking if a certain amount of time has passed
        // In a production app, use proper signal handling with the ctrlc crate
        if last_deploy_time.elapsed() > Duration::from_secs(120) {
            // Exit after 2 minutes for testing
            println!("\n{} Stopping watch mode.", style("ℹ").blue().bold());
            break;
        }

        // Sleep for the watch interval
        std::thread::sleep(Duration::from_secs(args.interval));

        // Check if the file was modified
        if let Ok(metadata) = std::fs::metadata(&args.manifest) {
            if let Ok(modified) = metadata.modified() {
                if modified > last_modified {
                    last_modified = modified;

                    // Don't redeploy if it's been less than 1 second since the last deploy
                    // This helps prevent multiple deploys for rapid file changes
                    if last_deploy_time.elapsed() < Duration::from_secs(1) {
                        continue;
                    }

                    // Redeploy the actor
                    println!(
                        "\n{} Changes detected, redeploying...",
                        style("ℹ").blue().bold()
                    );

                    last_deploy_time = Instant::now();

                    // Stop the existing actor and deploy a new one
                    if let Err(e) = runtime.block_on(async {
                        let mut client = TheaterClient::new(args.address);
                        client.connect().await?;

                        // Try to stop the existing actor
                        if let Err(e) = client.stop_actor(actor_id.clone()).await {
                            println!(
                                "{} Failed to stop previous actor: {}",
                                style("⚠").yellow().bold(),
                                e
                            );
                        }

                        // Deploy the new actor
                        let manifest_content = std::fs::read_to_string(&args.manifest)?;
                        let new_actor_id = client.start_actor(manifest_content, None).await?;

                        println!(
                            "{} Redeployed actor: {}",
                            style("✓").green().bold(),
                            style(new_actor_id.to_string()).cyan()
                        );

                        Ok::<(), anyhow::Error>(())
                    }) {
                        println!("{} Error redeploying actor: {}", style("✗").red().bold(), e);
                    }
                }
            }
        }
    }

    Ok(())
}
