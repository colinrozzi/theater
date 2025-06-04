use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use tracing::debug;

use crate::client::ManagementResponse;
use crate::utils::event_display::{display_events_header, display_single_event};
use crate::{error::CliError, output::formatters::ActorStarted, CommandContext};
use theater::utils::resolve_reference;

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path or URL to the actor manifest file
    #[arg(required = true)]
    pub manifest: String,

    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,

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

/// Execute the start command asynchronously (modernized)
pub async fn execute_async(args: &StartArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Starting actor from manifest: {}", args.manifest);

    // Get server address from args or config
    let address = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", address);

    // Resolve the manifest reference (could be file path, URL, or store path)
    let manifest_bytes = resolve_reference(&args.manifest).await.map_err(|e| {
        CliError::invalid_manifest(format!(
            "Failed to resolve manifest reference '{}': {}",
            args.manifest, e
        ))
    })?;

    // Convert bytes to string
    let manifest_content = String::from_utf8(manifest_bytes).map_err(|e| {
        CliError::invalid_manifest(format!("Manifest content is not valid UTF-8: {}", e))
    })?;

    // Handle the initial state parameter
    let initial_state = if let Some(state_str) = &args.initial_state {
        // Try to resolve as reference first (file path, URL, or store path)
        match resolve_reference(state_str).await {
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

    // Create client and connect
    let client = ctx.create_client();
    client
        .connect()
        .await
        .map_err(|e| CliError::connection_failed(address, e))?;

    // Start the actor with initial state
    client
        .start_actor(manifest_content, initial_state, args.parent, args.subscribe)
        .await
        .map_err(|e| CliError::actor_not_found(format!("Failed to start actor: {}", e)))?;

    if args.subscribe && !ctx.json {
        println!("");
        display_events_header(&args.format);
    }

    let mut is_running = true;
    let mut actor_started = false;

    // Add a timeout for actor startup
    let timeout_duration = tokio::time::Duration::from_secs(30);
    
    while is_running {
        tokio::select! {
            data = client.next_response() => {
                if let Ok(Some(data)) = data {
                    match data {
                        ManagementResponse::ActorStarted { id } => {
                            actor_started = true;
                            
                            if args.id_only {
                                println!("{}", id);
                                is_running = false;
                            } else {
                                let result = ActorStarted {
                                    actor_id: id.to_string(),
                                    manifest_path: args.manifest.clone(),
                                    address: address.to_string(),
                                    subscribing: args.subscribe,
                                    acting_as_parent: args.parent,
                                };
                                ctx.output.output(&result, None)?;

                                // if we are not subscribing or acting as a parent, break the loop
                                if !(args.subscribe || args.parent) {
                                    is_running = false;
                                }
                            }
                        }
                        ManagementResponse::ActorEvent { event } => {
                            if args.subscribe {
                                display_single_event(&event, &args.format, ctx.json)
                                    .map_err(|e| CliError::invalid_input("event_display", "event", e.to_string()))?;
                            }
                        }
                        ManagementResponse::ActorError { error } => {
                            if args.subscribe {
                                println!("-----[actor error]-----------------");
                                println!("     {}", error);
                                println!("-----------------------------------");
                            }
                        }
                        ManagementResponse::ActorStopped { id } => {
                            println!("-----[actor stopped]-----------------");
                            println!("{}", id);
                            println!("-------------------------------------");
                            is_running = false;
                        }
                        ManagementResponse::ActorResult(actor_result) => {
                            if args.parent {
                                println!("-----[actor result]-----------------");
                                println!("     {}", actor_result);
                                println!("------------------------------------");
                            }
                        }
                        _ => {
                            println!("Unknown response received");
                            is_running = false;
                        }
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                if !actor_started {
                    return Err(CliError::operation_timeout("Actor startup", timeout_duration.as_secs()));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                debug!("Received Ctrl-C, stopping");
                if !ctx.json {
                    println!("\n{}\n", "Interrupted by user");
                }
                is_running = false;
            }
        }
    }

    Ok(())
}
