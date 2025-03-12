use anyhow::Result;
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use tracing::debug;

use crate::cli::client::TheaterClient;

#[derive(Debug, Parser)]
pub struct ListArgs {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
}

pub fn execute(args: &ListArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Listing actors");
    debug!("Connecting to server at: {}", args.address);
    
    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;
    
    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);
        
        // Connect to the server
        client.connect().await?;
        
        // Get the list of actors
        let actors = client.list_actors().await?;
        
        // Output the result
        if !json {
            println!("{} Running actors: {}", 
                style("ℹ").blue().bold(),
                style(actors.len().to_string()).cyan());
            
            if actors.is_empty() {
                println!("  No actors are currently running.");
            } else {
                for (i, actor_id) in actors.iter().enumerate() {
                    println!("  {}. {}", i + 1, actor_id);
                }
            }
        } else {
            let actor_ids = actors.iter().map(|id| id.to_string()).collect::<Vec<_>>();
            let output = serde_json::json!({
                "count": actors.len(),
                "actors": actor_ids
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        
        Ok::<(), anyhow::Error>(())
    })?;
    
    Ok(())
}
