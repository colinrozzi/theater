use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use tracing::debug;

use crate::client::TheaterClient;
use std::str::FromStr;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct StopArgs {
    /// ID of the actor to stop
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

pub fn execute(args: &StopArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Stopping actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id)
        .map_err(|_| anyhow!("Invalid actor ID: {}", args.actor_id))?;

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);

        // Connect to the server
        client.connect().await?;

        // Stop the actor
        client.stop_actor(actor_id.clone()).await?;

        // Output the result
        if !json {
            println!(
                "{} Stopped actor: {}",
                style("✓").green().bold(),
                style(actor_id.to_string()).cyan()
            );
        } else {
            let output = serde_json::json!({
                "success": true,
                "actor_id": actor_id.to_string()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
