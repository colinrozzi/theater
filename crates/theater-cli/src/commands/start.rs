use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tracing::debug;

use crate::client::ManagementResponse;
use crate::tui;
use crate::utils::event_display::{display_structured_event, parse_event_fields};
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
    #[arg(short, long, default_value_t = false)]
    pub subscribe: bool,

    /// Act as the actor's parent
    #[arg(short, long, default_value_t = false)]
    pub parent: bool,

    /// Show detailed startup information instead of just actor ID
    #[arg(long)]
    pub verbose: bool,

    /// Event fields to include (comma-separated: hash,parent,type,timestamp,description,data)
    #[arg(long, default_value = "hash,parent,type,timestamp,description,data")]
    pub event_fields: String,
}

/// Execute the start command with new Unix-friendly behavior
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
    debug!("Calling start actor on client");
    client
        .start_actor(manifest_content, initial_state, args.parent, args.subscribe)
        .await
        .map_err(|e| CliError::actor_not_found(format!("Failed to start actor: {}", e)))?;
    debug!("Actor start request sent successfully");

    // Check if we should use TUI mode (only when both parent and subscribe, and not in JSON mode)
    let use_tui = args.subscribe && args.parent && !ctx.json && !args.verbose;

    if use_tui {
        // Use TUI mode
        return run_with_tui(args, client).await;
    }

    // Parse event fields for structured output
    let event_fields = if args.subscribe {
        parse_event_fields(&args.event_fields)
    } else {
        vec![]
    };

    let mut actor_started = false;
    let mut actor_result: Option<String> = None;
    let timeout_duration = tokio::time::Duration::from_secs(30);

    debug!("Entering response loop, waiting for actor start confirmation or events");
    loop {
        tokio::select! {
            data = client.next_response() => {
                debug!("Received response from client");
                debug!("Response data: {:?}", data);
                if let Ok(data) = data {
                    match data {
                        ManagementResponse::ActorStarted { id } => {
                            debug!("Management response received: Actor started with ID: {}", id);
                            actor_started = true;

                            // Determine output behavior based on flags
                            if !args.subscribe && !args.parent {
                                // Default behavior: just output actor ID
                                println!("{}", id);
                                break;
                            } else if args.verbose {
                                // Verbose mode: show detailed startup info
                                let result = ActorStarted {
                                    actor_id: id.to_string(),
                                    manifest_path: args.manifest.clone(),
                                    address: address.to_string(),
                                    subscribing: args.subscribe,
                                    acting_as_parent: args.parent,
                                };
                                ctx.output.output(&result, None)?;
                                
                                if !args.subscribe && !args.parent {
                                    break;
                                }
                            }
                            // For subscribe/parent modes, continue processing events
                        }
                        ManagementResponse::ActorEvent { event } => {
                            if args.subscribe {
                                // Use structured output format
                                display_structured_event(&event, &event_fields)
                                    .map_err(|e| CliError::invalid_input("event_display", "event", e.to_string()))?;
                            }
                        }
                        ManagementResponse::ActorError { error } => {
                            // Actor errors go to stderr and we exit with error code
                            eprintln!("Actor error: {}", error);
                            std::process::exit(1);
                        }
                        ManagementResponse::ActorStopped { .. } => {
                            // Actor stopped, break the loop to output final result
                            break;
                        }
                        ManagementResponse::ActorResult(result) => {
                            if args.parent {
                                // Store the result to output at the end
                                actor_result = Some(result.to_string());
                            }
                        }
                        ManagementResponse::Error { error } => {
                            return Err(CliError::management_error(error));
                        }
                        _ => {
                            debug!("Unknown response received");
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
                if args.verbose {
                    eprintln!("Interrupted by user");
                }
                break;
            }
        }
    }

    // Output final actor result if we're acting as parent
    if args.parent {
        if let Some(result) = actor_result {
            println!("OUTPUT\n\n{}", result);
        }
    }

    Ok(())
}

/// Run the start command with TUI interface
async fn run_with_tui(
    args: &StartArgs,
    client: crate::client::TheaterClient,
) -> Result<(), CliError> {
    debug!("Starting TUI mode for actor monitoring");

    // Create channel for communication with TUI
    let (response_tx, response_rx) = mpsc::unbounded_channel();

    // We'll need to get the actor ID from the first ActorStarted response
    let mut _actor_id: Option<String> = None;
    let mut tui_started = false;
    let mut tui_completed = false;

    // Add a timeout for actor startup
    let timeout_duration = tokio::time::Duration::from_secs(30);

    // Start TUI task early and wait for first actor started event
    let mut tui_handle = {
        let manifest_path = args.manifest.clone();
        tokio::spawn(async move {
            // Use a placeholder actor ID initially
            if let Err(e) =
                tui::run_tui("Starting...".to_string(), manifest_path, response_rx).await
            {
                eprintln!("TUI error: {}", e);
            }
        })
    };

    loop {
        tokio::select! {
            data = client.next_response() => {
                if let Ok(response) = data {
                    match &response {
                        ManagementResponse::ActorStarted { id } => {
                            _actor_id = Some(id.to_string());
                            debug!("Actor started with ID: {}", id);
                            tui_started = true;
                        }
                        ManagementResponse::ActorStopped { .. } => {
                            // Send to TUI and break
                            let _ = response_tx.send(response);
                            break;
                        }
                        _ => {}
                    }

                    // Send all responses to TUI
                    if let Err(_) = response_tx.send(response) {
                        // TUI channel closed, probably user quit
                        debug!("TUI channel closed, stopping");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                if !tui_started {
                    return Err(CliError::operation_timeout("Actor startup", timeout_duration.as_secs()));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                debug!("Received Ctrl-C, stopping TUI mode");
                break;
            }
            result = &mut tui_handle, if !tui_completed => {
                match result {
                    Ok(_) => debug!("TUI task completed"),
                    Err(e) => debug!("TUI task error: {}", e),
                }
                tui_completed = true;
                break;
            }
        }
    }

    // Clean up - wait for TUI to finish if it hasn't already
    if !tui_completed {
        let _ = tokio::time::timeout(tokio::time::Duration::from_millis(500), tui_handle).await;
    }

    Ok(())
}
