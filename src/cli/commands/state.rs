use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use tracing::debug;

use crate::cli::client::TheaterClient;
use theater::id::TheaterId;
use std::str::FromStr;

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
        let mut client = TheaterClient::new(args.address);
        
        // Connect to the server
        client.connect().await?;
        
        // Get the actor state
        let state = client.get_actor_state(actor_id.clone()).await?;
        
        // Output the result based on format
        match state {
            Some(state_bytes) => {
                if json {
                    let output = serde_json::json!({
                        "actor_id": actor_id.to_string(),
                        "state": hex::encode(&state_bytes),
                        "size": state_bytes.len()
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    match args.format.as_str() {
                        "raw" => {
                            // Output the raw bytes
                            print!("{}", String::from_utf8_lossy(&state_bytes));
                        }
                        "json" => {
                            // Try to parse as JSON and output pretty-printed
                            match serde_json::from_slice::<serde_json::Value>(&state_bytes) {
                                Ok(json_value) => {
                                    println!("{}", serde_json::to_string_pretty(&json_value)?);
                                }
                                Err(_) => {
                                    println!("{} (non-JSON data, displaying as hex)", 
                                        style("⚠ Failed to parse state as JSON").yellow());
                                    println!("{}", hex::encode(&state_bytes));
                                }
                            }
                        }
                        "pretty" | _ => {
                            // Try to parse as JSON, fallback to safe display
                            println!("{} Actor State:", style("ℹ").blue().bold());
                            match serde_json::from_slice::<serde_json::Value>(&state_bytes) {
                                Ok(json_value) => {
                                    println!("{}", serde_json::to_string_pretty(&json_value)?);
                                }
                                Err(_) => {
                                    // Try to display as UTF-8 string
                                    if let Ok(string_value) = String::from_utf8(state_bytes.clone()) {
                                        println!("{}", string_value);
                                    } else {
                                        println!("Binary data ({} bytes):", state_bytes.len());
                                        println!("{}", hex::encode(&state_bytes));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            None => {
                if json {
                    let output = serde_json::json!({
                        "actor_id": actor_id.to_string(),
                        "state": null
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    println!("{} Actor has no state", style("ℹ").blue().bold());
                }
            }
        }
        
        Ok::<(), anyhow::Error>(())
    })?;
    
    Ok(())
}
