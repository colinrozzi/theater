use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use tracing::debug;

use crate::cli::client::TheaterClient;
use theater::id::TheaterId;
use std::str::FromStr;

#[derive(Debug, Parser)]
pub struct MessageArgs {
    /// ID of the actor to send a message to
    #[arg(required = true)]
    pub actor_id: String,
    
    /// Message to send (as string)
    #[arg(required = true)]
    pub message: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
    
    /// Send as a request (awaits response) instead of a one-way message
    #[arg(short, long, default_value = "true")]
    pub request: bool,
}

pub fn execute(args: &MessageArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Sending message to actor: {}", args.actor_id);
    debug!("Message: {}", args.message);
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
        let message_bytes = args.message.as_bytes().to_vec();
        
        if args.request {
            // Send as a request and wait for response
            let response: Vec<u8> = client.request_actor_message(actor_id.clone(), message_bytes).await?;
            
            // Output the response
            if json {
                let output = serde_json::json!({
                    "actor_id": actor_id.to_string(),
                    "request": args.message,
                    "response": String::from_utf8_lossy(&response).to_string()
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{} Response from actor: {}", 
                    style("✓").green().bold(),
                    style(actor_id.to_string()).cyan());
                
                println!("{}", String::from_utf8_lossy(&response));
            }
        } else {
            // Send as a one-way message
            client.send_actor_message(actor_id.clone(), message_bytes).await?;
            
            // Output the result
            if json {
                let output = serde_json::json!({
                    "success": true,
                    "actor_id": actor_id.to_string(),
                    "message": args.message
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{} Message sent to actor: {}", 
                    style("✓").green().bold(),
                    style(actor_id.to_string()).cyan());
            }
        }
        
        Ok::<(), anyhow::Error>(())
    })?;
    
    Ok(())
}
