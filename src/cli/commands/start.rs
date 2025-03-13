use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::debug;

use crate::cli::client::{ManagementResponse, TheaterClient};

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path to the actor manifest file
    #[arg(required = true)]
    pub manifest: PathBuf,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,
    
    /// Wait for actor to start
    #[arg(short, long, default_value = "true")]
    pub wait: bool,

    /// Initial state as JSON string or path to JSON file
    #[arg(short, long)]
    pub initial_state: Option<String>,
    
    /// Monitor actor events after starting
    #[arg(long)]
    pub monitor: bool,
    
    /// Output only the actor ID (useful for piping to other commands)
    #[arg(long)]
    pub id_only: bool,
}

pub fn execute(args: &StartArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Starting actor from manifest: {}", args.manifest.display());
    debug!("Connecting to server at: {}", args.address);
    
    // Check if the manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow!("Manifest file not found: {}", args.manifest.display()));
    }
    
    // Read the manifest file
    let manifest_content = std::fs::read_to_string(&args.manifest)?;
    
    // Handle the initial state parameter
    let initial_state = if let Some(state_str) = &args.initial_state {
        // Check if it's a file path
        if std::path::Path::new(state_str).exists() {
            debug!("Reading initial state from file: {}", state_str);
            Some(std::fs::read(state_str)?)
        } else {
            // Assume it's a JSON string
            debug!("Using provided JSON string as initial state");
            Some(state_str.as_bytes().to_vec())
        }
    } else {
        None
    };
    
    // Create runtime and connect to the server
    let runtime = tokio::runtime::Runtime::new()?;
    
    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);
        
        // Connect to the server
        client.connect().await?;
        
        // Start the actor with initial state
        let actor_id = client.start_actor(manifest_content, initial_state).await?;
        
        // Output the result
        if args.id_only {
            // Just print the actor ID for piping to other commands
            println!("{}", actor_id);
        } else if !json {
            println!("{} Actor started successfully: {}", 
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
        
        // If monitor flag is set, subscribe to and monitor events from the actor
        if args.monitor && !json {
            // Set up Ctrl+C handler
            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();
            
            let ctrl_c_handler = tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                r.store(false, Ordering::SeqCst);
            });
            
            // Subscribe to actor events
            println!("{} Monitoring events for actor: {}", 
                style("ℹ").blue().bold(),
                style(actor_id.to_string()).cyan());
            println!("{} Waiting for events (Press Ctrl+C to stop monitoring)", 
                style("i").dim());
            println!("{} Note: You may need to trigger actions in the actor to generate events", 
                style("i").dim());
            
            // Subscribe to actor events
            let subscription_id = client.subscribe_to_actor(actor_id.clone()).await?;
            debug!("Subscribed to actor events, subscription ID: {}", subscription_id);
            
            // Setup to receive events
            let mut event_count = 0;
            let mut heartbeat_counter = 0;
            
            // Keep receiving events until Ctrl+C is pressed
            while running.load(Ordering::SeqCst) {
                // Try to receive a response with a timeout to avoid blocking forever
                if let Ok(response) = tokio::time::timeout(
                    std::time::Duration::from_secs(1), 
                    client.receive_response()
                ).await {
                    match response {
                        Ok(ManagementResponse::ActorEvent { id, event }) => {
                            if id == actor_id {
                                event_count += 1;
                                println!("{}. {}", event_count, event.event_type);
                                println!("   Time: {}", event.timestamp);
                                println!("   Hash: {}", hex::encode(&event.hash));
                                if let Some(parent) = &event.parent_hash {
                                    println!("   Parent: {}", hex::encode(parent));
                                }
                                println!();
                            }
                        },
                        Ok(other) => {
                            // Log other response types for debugging
                            debug!("Received non-event response: {:?}", other);
                        },
                        Err(e) => {
                            // Connection error or other issue
                            debug!("Error receiving event: {:?}", e);
                            break;
                        }
                    }
                } else {
                    // Timeout occurred, but that's okay in this case
                    debug!("Timeout waiting for events, still alive");
                    
                    // Increment heartbeat counter and show a heartbeat message every ~30 seconds
                    heartbeat_counter += 1;
                    if heartbeat_counter >= 30 {
                        println!("{} Still monitoring for events from actor: {}", 
                            style("⟳").dim(),
                            style(&actor_id.to_string()[..8]).dim());
                        heartbeat_counter = 0;
                    }
                }
                // Small delay to prevent CPU spinning, slightly longer
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            
            // Clean up subscription before exiting
            if let Err(e) = client.unsubscribe_from_actor(actor_id.clone(), subscription_id).await {
                debug!("Error unsubscribing from actor events: {:?}", e);
            }
            
            println!("\n{} Stopped monitoring actor: {}", 
                style("✓").green().bold(),
                style(actor_id.to_string()).cyan());
            
            // Cancel the Ctrl+C handler
            ctrl_c_handler.abort();
        }
        
        Ok::<(), anyhow::Error>(())
    })?;
    
    Ok(())
}
