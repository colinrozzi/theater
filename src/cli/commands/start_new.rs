use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, warn, info};

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
        if !json {
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
            
            // Try to get all existing events first to avoid missing anything
            match client.get_actor_events(actor_id.clone()).await {
                Ok(events) => {
                    if !events.is_empty() {
                        info!("Found {} existing events", events.len());
                        for (i, event) in events.iter().enumerate() {
                            println!("{}. {} {}", 
                                i + 1,
                                style("[Init]").yellow(),
                                style(&event.event_type).cyan());
                            println!("   Time: {}", event.timestamp);
                            println!("   Hash: {}", hex::encode(&event.hash));
                            if let Some(parent) = &event.parent_hash {
                                println!("   Parent: {}", hex::encode(parent));
                            }
                            println!();
                        }
                    }
                },
                Err(e) => {
                    warn!("Could not retrieve existing events: {}", e);
                }
            }
            
            // Subscribe to actor events with retry mechanism
            let mut subscription_id = None;
            for attempt in 1..=5 {
                match client.subscribe_to_actor(actor_id.clone()).await {
                    Ok(sub_id) => {
                        subscription_id = Some(sub_id);
                        debug!("Subscribed to actor events, subscription ID: {}", sub_id);
                        break;
                    },
                    Err(e) => {
                        if attempt < 5 {
                            warn!("Failed to subscribe to actor events (attempt {}): {}", attempt, e);
                            tokio::time::sleep(std::time::Duration::from_millis(300 * attempt)).await;
                        } else {
                            error!("Failed all attempts to subscribe to actor events: {}", e);
                            return Err(anyhow!("Could not subscribe to actor events: {}", e));
                        }
                    }
                }
            }
            
            // Make sure we got a subscription ID
            let subscription_id = if let Some(id) = subscription_id {
                id
            } else {
                return Err(anyhow!("Failed to obtain subscription ID"));
            };
            
            // Setup to receive events
            let mut event_count = 0;
            let mut heartbeat_counter = 0;
            let mut consecutive_errors = 0;
            let mut reconnect_attempts = 0;
            
            // Keep receiving events until Ctrl+C is pressed
            while running.load(Ordering::SeqCst) {
                // First, try to flush any pending events by polling
                let mut had_events = false;
                for _ in 0..5 {
                    match client.receive_response_nonblocking() {
                        Ok(Ok(ManagementResponse::ActorEvent { id, event })) => {
                            if id == actor_id {
                                had_events = true;
                                consecutive_errors = 0;
                                event_count += 1;
                                println!("{}. {} {}", 
                                    event_count,
                                    style("[Event]").green(),
                                    style(&event.event_type).cyan());
                                println!("   Time: {}", event.timestamp);
                                println!("   Hash: {}", hex::encode(&event.hash));
                                if let Some(parent) = &event.parent_hash {
                                    println!("   Parent: {}", hex::encode(parent));
                                }
                                println!();
                            }
                        },
                        Err(_) => break,
                        _ => {}
                    }
                }
                
                // If we're not getting events for a while, try a ping
                if !had_events && heartbeat_counter % 20 == 10 {
                    debug!("No events received recently, checking connection with ping");
                    match client.ping().await {
                        Ok(_) => {
                            debug!("Ping successful, connection is alive");
                            consecutive_errors = 0;
                        },
                        Err(e) => {
                            warn!("Ping failed: {}", e);
                            consecutive_errors += 1;
                        }
                    }
                }
                
                // Try to receive a response with a timeout to avoid blocking forever
                match tokio::time::timeout(
                    std::time::Duration::from_millis(500), 
                    client.receive_response_with_timeout(std::time::Duration::from_millis(200))
                ).await {
                    Ok(Ok(ManagementResponse::ActorEvent { id, event })) => {
                        if id == actor_id {
                            had_events = true;
                            consecutive_errors = 0;
                            event_count += 1;
                            println!("{}. {} {}", 
                                event_count,
                                style("[Event]").green(),
                                style(&event.event_type).cyan());
                            println!("   Time: {}", event.timestamp);
                            println!("   Hash: {}", hex::encode(&event.hash));
                            if let Some(parent) = &event.parent_hash {
                                println!("   Parent: {}", hex::encode(parent));
                            }
                            println!();
                        }
                    },
                    Ok(Ok(_)) => {
                        // Ignore other response types
                    },
                    Ok(Err(e)) => {
                        // Connection error
                        consecutive_errors += 1;
                        warn!("Error receiving event (error {}): {:?}", consecutive_errors, e);
                        
                        // If we've had several consecutive errors, try to reconnect
                        if consecutive_errors >= 5 {
                            warn!("Multiple consecutive errors, attempting to reconnect...");
                            
                            // Try to reconnect
                            reconnect_attempts += 1;
                            if reconnect_attempts <= 3 {
                                match client.connect().await {
                                    Ok(_) => {
                                        info!("Successfully reconnected to server");
                                        
                                        // Re-subscribe to actor events
                                        match client.subscribe_to_actor(actor_id.clone()).await {
                                            Ok(new_sub_id) => {
                                                info!("Successfully re-subscribed to actor events");
                                                consecutive_errors = 0;
                                            },
                                            Err(e) => {
                                                error!("Failed to re-subscribe to actor events: {}", e);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        error!("Failed to reconnect to server: {}", e);
                                    }
                                }
                            } else {
                                error!("Too many reconnection attempts, giving up");
                                break;
                            }
                        }
                    },
                    Err(_) => {
                        // Timeout occurred, that's fine
                    }
                }
                
                // Increment heartbeat counter and show a heartbeat message every ~10 seconds
                heartbeat_counter += 1;
                if heartbeat_counter >= 20 {
                    println!("{} Still monitoring for events from actor: {}", 
                        style("⟳").dim(),
                        style(&actor_id.to_string()[..8]).dim());
                    heartbeat_counter = 0;
                }
                
                // Small delay to prevent CPU spinning
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
