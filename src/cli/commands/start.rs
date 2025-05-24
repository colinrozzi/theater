use anyhow::{anyhow, Result};
use clap::Parser;
use std::net::SocketAddr;

use tracing::debug;

use crate::cli::client::{ManagementResponse, TheaterClient};
use crate::cli::utils::event_display::{display_events_header, display_single_event};
use theater::utils::resolve_reference;

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path or URL to the actor manifest file
    #[arg(required = true)]
    pub manifest: String,

    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// Initial state as JSON string or path to JSON file
    #[arg(short, long)]
    pub initial_state: Option<String>,

    /// Subscribe to actor events
    #[arg(short, long)]
    pub subscribe: bool,

    /// Act as the actor's parent
    #[arg(short, long)]
    pub parent: bool,

    /// Output only the actor ID (useful for piping to other commands)
    #[arg(long)]
    pub id_only: bool,

    /// Event format (pretty, compact, json)
    #[arg(short, long, default_value = "compact")]
    pub format: String,
}

pub fn execute(args: &StartArgs, _verbose: bool, json: bool) -> Result<()> {
    debug!("Starting actor from manifest: {}", args.manifest);
    debug!("Connecting to server at: {}", args.address);

    // Create runtime first to handle both sync and async operations
    let runtime = tokio::runtime::Runtime::new()?;

    // Resolve the manifest reference (could be file path, URL, or store path)
    let manifest_bytes = runtime.block_on(async {
        resolve_reference(&args.manifest).await.map_err(|e| {
            anyhow!(
                "Failed to resolve manifest reference '{}': {}",
                args.manifest,
                e
            )
        })
    })?;

    // Convert bytes to string
    let manifest_content = String::from_utf8(manifest_bytes)
        .map_err(|e| anyhow!("Manifest content is not valid UTF-8: {}", e))?;

    // Handle the initial state parameter
    let initial_state = if let Some(state_str) = &args.initial_state {
        // Try to resolve as reference first (file path, URL, or store path)
        match runtime.block_on(resolve_reference(state_str)) {
            Ok(bytes) => {
                debug!("Resolved initial state from reference: {}", state_str);
                Some(bytes)
            }
            Err(_) => {
                // If resolution fails, assume it's a JSON string
                debug!("Using provided string as JSON initial state");
                Some(state_str.as_bytes().to_vec())
            }
        }
    } else {
        None
    };

    // Connect to the server and start the actor
    runtime.block_on(async {
        let mut client = TheaterClient::new(args.address);

        // Connect to the server
        client.connect().await?;

        // Start the actor with initial state
        client
            .start_actor(manifest_content, initial_state, args.parent, args.subscribe)
            .await?;

        if args.subscribe && !json {
            println!("");
            display_events_header(&args.format);
        }

        loop {
            tokio::select! {
                data = client.next_response() => {
                    if let Some(data) = data {
                    match data {
                        Ok(ManagementResponse::ActorStarted { id }) => {
                            if args.id_only {
                                println!("{}", id);
                                break;
                            } else {
                                println!("-----[actor started]-----------------");
                                println!(
                                    "     {}",
                                    id
                                );
                                println!("-------------------------------------");
                                // if we are not subscribing or acting as a parent, break the loop
                                if !(args.subscribe || args.parent) {
                                    break;
                                }
                            }
                        }
                        Ok(ManagementResponse::ActorEvent { event }) => {
                            if args.subscribe {
                                display_single_event(&event, &args.format, json).expect("Failed to display event");
                            }
                        }
                        Ok(ManagementResponse::ActorError { error }) => {
                            if args.subscribe {
                                println!("-----[actor error]-----------------");
                                println!(
                                    "     {}",
                                    error
                                );
                                println!("-----------------------------------");
                            }
                        }
                        Ok(ManagementResponse::ActorStopped { id }) => {
                            println!("-----[actor stopped]-----------------");
                            println!("{}", id);
                            println!("-------------------------------------");
                            break;
                        }
                        Ok(ManagementResponse::ActorResult(actor_result)) => {
                            if args.parent {
                                println!("-----[actor result]-----------------");
                                println!(
                                    "     {}",
                                    actor_result
                                );
                                println!("------------------------------------");
                            }
                        }
                        Err(e) => {
                            println!("Error receiving data: {:?}", e);
                            break;
                        }
                        _ => {
                            println!("Received unknown data: {:?}", data);
                        }
                    }
                }
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                },
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
