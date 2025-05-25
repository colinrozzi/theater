use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::debug;

use crate::client::TheaterClient;
use std::str::FromStr;
use theater::id::TheaterId;

#[derive(Debug, Parser)]
pub struct MessageArgs {
    /// ID of the actor to send a message to
    #[arg(required = true)]
    pub actor_id: String,

    /// Message to send (as string)
    #[arg(required_unless_present = "file")]
    pub message: Option<String>,

    /// File containing message to send
    #[arg(short, long, conflicts_with = "message")]
    pub file: Option<PathBuf>,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Send as a request (awaits response) instead of a one-way message
    #[arg(short, long, default_value = "false")]
    pub request: bool,
}

pub fn execute(args: &MessageArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Sending message to actor: {}", args.actor_id);
    // Get message content either from direct argument or file
    let message_content = if let Some(message) = &args.message {
        message.clone()
    } else if let Some(file_path) = &args.file {
        debug!("Reading message from file: {:?}", file_path);
        fs::read_to_string(file_path).map_err(|e| anyhow!("Failed to read message file: {}", e))?
    } else {
        return Err(anyhow!("Either message or file must be provided"));
    };

    debug!("Message: {}", message_content);
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

        // Convert message to bytes
        let message_bytes = message_content.as_bytes().to_vec();

        if args.request {
            // Send as a request and wait for response
            let response: Vec<u8> = client
                .request_actor_message(actor_id.clone(), message_bytes)
                .await?;

            // Output the response
            if json {
                let output = serde_json::json!({
                    "actor_id": actor_id.to_string(),
                    "request": message_content,
                    "response": String::from_utf8_lossy(&response).to_string()
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!(
                    "{} Response from actor: {}",
                    style("✓").green().bold(),
                    style(actor_id.to_string()).cyan()
                );

                println!("{}", String::from_utf8_lossy(&response));
            }
        } else {
            // Send as a one-way message
            client
                .send_actor_message(actor_id.clone(), message_bytes)
                .await?;

            // Output the result
            if json {
                let output = serde_json::json!({
                    "success": true,
                    "actor_id": actor_id.to_string(),
                    "message": message_content
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!(
                    "{} Message sent to actor: {}",
                    style("✓").green().bold(),
                    style(actor_id.to_string()).cyan()
                );
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
