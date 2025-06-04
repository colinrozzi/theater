use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use tracing::debug;

use crate::client::TheaterClient;
use std::str::FromStr;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct StateArgs {
    /// ID of the actor to get state from
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Output format (raw, json, pretty)
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
}

pub fn execute(args: &StateArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Getting state for actor: {}", args.actor_id);
    debug!("Connecting to server at: {}", args.address);

    // Parse the actor ID
    let actor_id = TheaterId::from_str(&args.actor_id)
        .map_err(|_| anyhow!("Invalid actor ID: {}", args.actor_id))?;

    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let client = TheaterClient::new(args.address, config);

        // Connect to the server
        client.connect().await?;

        // Get the actor state
        let state = client.get_actor_state(&actor_id.to_string()).await?;

        // Output the result based on format
        if json {
            let output = serde_json::json!({
                "actor_id": actor_id.to_string(),
                "state": state
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            match args.format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&state)?);
                }
                "pretty" => {
                    println!("{} Actor State:", style("ℹ").blue().bold());
                    println!("Actor: {}", actor_id);
                    println!("State:");
                    println!("{}", serde_json::to_string_pretty(&state)?);
                }
                _ => {
                    println!("{}", state);
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
