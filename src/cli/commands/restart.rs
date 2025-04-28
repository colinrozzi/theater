use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use tracing::debug;

use crate::cli::client::TheaterClient;
use std::str::FromStr;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct RestartArgs {
    /// ID of the actor to restart
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

pub fn execute(args: &RestartArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Restarting actor: {}", args.actor_id);
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

        // Restart the actor
        client.restart_actor(actor_id.clone()).await?;

        // Output the result
        if !json {
            println!(
                "{} Restarted actor: {}",
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
