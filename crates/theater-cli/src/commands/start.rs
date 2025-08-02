use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;

use tracing::debug;

use crate::client::ManagementResponse;
use crate::utils::event_display::{display_structured_event, parse_event_fields};
use crate::{error::CliError, output::formatters::ActorStarted, CommandContext};
use theater::messages::ActorResult;
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

    /// Enable Unix-style signal handling (SIGINT/SIGKILL)
    #[arg(short, long, default_value_t = false)]
    pub unix_signals: bool,

    /// Show detailed startup information instead of just actor ID
    #[arg(long)]
    pub verbose: bool,

    /// Event fields to include (comma-separated: hash,parent,type,timestamp,description,data,data_size)
    #[arg(long, default_value = "hash,parent,type,timestamp,description,data_size,data")]
    pub event_fields: String,
}

/// Execute the start command with single-select-branch signal handling
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
        match resolve_reference(state_str).await {
            Ok(bytes) => {
                debug!("Resolved initial state from reference: {}", state_str);
                Some(bytes)
            }
            Err(_) => {
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

    // Start the actor
    client
        .start_actor(manifest_content, initial_state, args.parent, args.subscribe)
        .await
        .map_err(|e| CliError::actor_not_found(format!("Failed to start actor: {}", e)))?;

    // Parse event fields
    let event_fields = if args.subscribe {
        parse_event_fields(&args.event_fields)
    } else {
        vec![]
    };

    let mut actor_started = false;
    let mut actor_result: Option<ActorResult> = None;
    let timeout_duration = tokio::time::Duration::from_secs(30);
    let mut actor_id: Option<String> = None;

    debug!("Entering response loop");

    loop {
        tokio::select! {
            data = client.next_response() => {
                debug!("Received response: {:?}", data);
                if let Ok(data) = data {
                    match data {
                        ManagementResponse::ActorStarted { id } => {
                            debug!("Actor started: {}", id);
                            actor_started = true;
                            let id_str = id.to_string();
                            actor_id = Some(id_str.clone());

                            if !(args.subscribe || args.parent) {
                                println!("{}", id);
                            }

                            if args.verbose {
                                let result = ActorStarted {
                                    actor_id: id_str.clone(),
                                    manifest_path: args.manifest.clone(),
                                    address: address.to_string(),
                                    subscribing: args.subscribe,
                                    acting_as_parent: args.parent,
                                    unix_signals: args.unix_signals,
                                };
                                ctx.output.output(&result, None)?;
                            }

                            if !(args.subscribe || args.parent) {
                                debug!("Exiting after startup");
                                break;
                            }
                        }
                        ManagementResponse::ActorEvent { event } => {
                            if args.subscribe {
                                display_structured_event(&event, &event_fields)
                                    .map_err(|e| CliError::invalid_input("event_display", "event", e.to_string()))?;
                                if event.event_type == "shutdown" {
                                    break;
                                }
                            }
                        }
                        ManagementResponse::ActorResult(result) => {
                            if args.parent {
                                match args.subscribe {
                                    true => actor_result = Some(result),
                                    false => write_actor_result(result),
                                }
                            }
                        }
                        ManagementResponse::Error { error } => {
                            return Err(CliError::management_error(error));
                        }
                        _ => {}
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                if !actor_started {
                    return Err(CliError::operation_timeout("Actor startup", timeout_duration.as_secs()));
                }
            }
            
            // Unix signal handling - conditional signal handling
            signal = async {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{SignalKind, signal};
                    
                    // Initialize signals once
                    static mut SIGINT_HANDLE: Option<tokio::signal::unix::Signal> = None;
                    static mut SIGTERM_HANDLE: Option<tokio::signal::unix::Signal> = None;
                    static INIT: std::sync::Once = std::sync::Once::new();
                    
                    INIT.call_once(|| {
                        if let Ok(sig) = signal(SignalKind::interrupt()) {
                            unsafe { SIGINT_HANDLE = Some(sig) };
                        }
                        if let Ok(sig) = signal(SignalKind::terminate()) {
                            unsafe { SIGTERM_HANDLE = Some(sig) };
                        }
                    });
                    
                    unsafe {
                        let mut sigint = SIGINT_HANDLE.take();
                        let mut sigterm = SIGTERM_HANDLE.take();
                        
                        let sigint_recv = sigint.as_mut().map(|s| s.recv()).unwrap_or(futures::future::pending());
                        let sigterm_recv = sigterm.as_mut().map(|s| s.recv()).unwrap_or(futures::future::pending());
                        
                        tokio::select! {
                            _ = sigint_recv => "SIGINT",
                            _ = sigterm_recv => "SIGTERM",
                            _ = tokio::signal::ctrl_c() => "SIGINT",
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    "SIGINT"
                }
            } => {
                match signal {
                    "SIGINT" | "SIGTERM" => {
                        let sig_type = if signal == "SIGINT" { "SIGINT" } else { "SIGTERM" };
                        if let Some(actor_id) = &actor_id {
                            debug!("{} received, {} actor {}", sig_type, 
                                  if sig_type == "SIGINT" { "gracefully stopping" } else { "terminating" }, actor_id);
                            eprintln!("\nReceived {}, {} actor...", sig_type, 
                                    if sig_type == "SIGINT" { "gracefully stopping" } else { "terminating" });
                            
                            if sig_type == "SIGINT" {
                                let _ = client.stop_actor(actor_id).await;
                            } else {
                                let _ = client.terminate_actor(actor_id).await;
                            }
                        }
                        break;
                    }
                    _ => break,
                }
            }
            _ = tokio::signal::ctrl_c() => {
                debug!("Received Ctrl-C, stopping");
                if args.verbose {
                    eprintln!("Interrupted by user");
                }
                break;
            }
            _ = ctx.shutdown_token.cancelled() => {
                break;
            }
        }
    }

    if args.parent && args.subscribe {
        match actor_result {
            Some(result) => {
                debug!("Actor result received");
                println!("OUTPUT");
                write_actor_result(result);
            }
            None => {
                eprintln!("No actor result received");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

fn write_actor_result(actor_result: ActorResult) {
    use std::io::{self, Write};

    match actor_result {
        ActorResult::Success(result) => {
            if let Some(output) = result.result {
                let _ = io::stdout().write_all(&output);
                let _ = io::stdout().flush();
                std::process::exit(0);
            }
        }
        ActorResult::Error(error) => {
            let error_message = format!("Error: {}", error.error);
            let _ = io::stdout().write_all(error_message.as_bytes());
            let _ = io::stdout().flush();
            std::process::exit(1);
        }
        ActorResult::ExternalStop(_) => {
            let _ = io::stdout().write_all(b"Actor stopped externally");
            let _ = io::stdout().flush();
            std::process::exit(1);
        }
    }
}