use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::cli::client::TheaterClient;

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path to the actor manifest file
    #[arg(required = true)]
    pub manifest: PathBuf,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

pub fn execute(args: &StartArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Starting actor from manifest: {}", args.manifest.display());
    debug!("Connecting to server at: {}", args.address);
    
    // Check if the manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow!("Manifest file not found: {}", args.manifest.display()));
    }
    
    // Read the manifest file
    let manifest_content = std::fs::read_to_string(&args.manifest)?;
    
    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;
    
    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);
        
        // Connect to the server
        client.connect().await?;
        
        // Start the actor
        let actor_id = client.start_actor(manifest_content).await?;
        
        // Output the result
        if !json {
            println!("{} Started actor: {}", 
                style("✓").green().bold(),
                style(actor_id.to_string()).cyan());
        } else {
            let output = serde_json::json!({
                "success": true,
                "actor_id": actor_id.to_string(),
                "manifest": args.manifest.display().to_string()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        
        Ok::<(), anyhow::Error>(())
    })?;
    
    Ok(())
}
