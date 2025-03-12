use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;

use tracing::debug;

use crate::cli::client::TheaterClient;
use theater::id::TheaterId;
use std::str::FromStr;

#[derive(Debug, Parser)]
pub struct EventsArgs {
    /// ID of the actor to get events from
    #[arg(required = true)]
    pub actor_id: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
    
    /// Number of events to show (0 for all)
    #[arg(short, long, default_value = "10")]
    pub limit: usize,
}

pub fn execute(args: &EventsArgs, verbose: bool, json: bool) -> Result<()> {
    debug!("Getting events for actor: {}", args.actor_id);
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
        
        // Get the actor events
        let mut events = client.get_actor_events(actor_id.clone()).await?;
        
        // Limit the number of events if requested
        if args.limit > 0 && events.len() > args.limit {
            events = events.into_iter().take(args.limit).collect();
        }
        
        // Output the result
        if json {
            let output = serde_json::json!({
                "actor_id": actor_id.to_string(),
                "events": events,
                "count": events.len()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("{} Events for actor: {}", 
                style("ℹ").blue().bold(),
                style(actor_id.to_string()).cyan());
            
            if events.is_empty() {
                println!("  No events found.");
            } else {
                for (i, event) in events.iter().enumerate() {
                    println!("{}. {}", i + 1, event.event_type);
                    println!("   Time: {}", event.timestamp);
                    println!("   Hash: {}", event.hash);
                    if let Some(parent) = &event.parent_hash {
                        println!("   Parent: {}", parent);
                    }
                    println!("");
                }
                
                if args.limit > 0 && events.len() == args.limit {
                    println!("(Showing {} of many events. Use --limit to see more.)", events.len());
                }
            }
        }
        
        Ok::<(), anyhow::Error>(())
    })?;
    
    Ok(())
}
