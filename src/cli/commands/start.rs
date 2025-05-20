use anyhow::{anyhow, Result};
use clap::Parser;
use console::style;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::debug;

use crate::cli::client::{ManagementResponse, TheaterClient};
use crate::cli::utils::event_display::{display_events_header, display_single_event};

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path to the actor manifest file
    #[arg(required = true)]
    pub manifest: PathBuf,

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
    debug!("Starting actor from manifest: {}", args.manifest.display());
    debug!("Connecting to server at: {}", args.address);

    // Check if the manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow!(
            "Manifest file not found: {}",
            args.manifest.display()
        ));
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
