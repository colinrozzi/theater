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
pub struct ProcessArgs {
    /// Path or URL to the actor manifest file
    #[arg(default_value = "manifest.toml")]
    pub manifest: String,

    /// Address of the theater server
    #[arg(short, long)]
    pub address: Option<SocketAddr>,

    /// Initial state as JSON string or path to JSON file
    #[arg(short, long)]
    pub initial_state: Option<String>,

    /// Disable event subscription (process mode enables this by default)
    #[arg(long, default_value_t = false)]
    pub no_subscribe: bool,

    /// Don't act as parent (process mode enables this by default)
    #[arg(long, default_value_t = false)]
    pub no_parent: bool,

    /// Enable Unix-style signal handling (enabled by default in process mode)
    #[arg(short, long, default_value_t = true)]
    pub unix_signals: bool,

    /// Show detailed startup information
    #[arg(long)]
    pub verbose: bool,

    /// Event fields to include (comma-separated: hash,parent,type,timestamp,description,data,data_size)
    #[arg(
        long,
        default_value = "hash,parent,type,timestamp,description,data_size,data"
    )]
    pub event_fields: String,

    /// Timeout in seconds after which to terminate the process (0 = no timeout)
    #[arg(long, default_value_t = 0)]
    pub timeout: u64,

    /// Restart the actor if it fails (supervision policy)
    #[arg(long, default_value_t = false)]
    pub restart_on_failure: bool,
}

/// Execute the process command - run an actor as a supervised process
pub async fn execute_async(args: &ProcessArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Running actor as process from manifest: {}", args.manifest);

    // Process mode defaults: subscribe and parent are enabled unless explicitly disabled
    let subscribe = !args.no_subscribe;
    let parent = !args.no_parent;

    // Get server address from args or config
    let address = ctx.server_address(args.address);
    debug!("Connecting to server at: {}", address);

    // Resolve the manifest reference
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
            Ok(bytes) => Some(bytes),
            Err(_) => Some(state_str.as_bytes().to_vec()),
        }
    } else {
        None
    };

    // Run with restart loop if restart_on_failure is enabled
    loop {
        let result = run_actor_process(
            &manifest_content,
            initial_state.clone(),
            subscribe,
            parent,
            args,
            ctx,
            address,
        )
        .await;

        match result {
            Ok(()) => {
                debug!("Actor process completed successfully");
                break;
            }
            Err(e) => {
                if args.restart_on_failure {
                    eprintln!("Actor failed: {}, restarting...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }

    Ok(())
}

/// Internal function to run a single actor process
async fn run_actor_process(
    manifest_content: &str,
    initial_state: Option<Vec<u8>>,
    subscribe: bool,
    parent: bool,
    args: &ProcessArgs,
    ctx: &CommandContext,
    address: SocketAddr,
) -> Result<(), CliError> {
    // Create client and connect
    let client = ctx.create_client();
    client
        .connect()
        .await
        .map_err(|e| CliError::connection_failed(address, e))?;

    // Start the actor
    client
        .start_actor(
            manifest_content.to_string(),
            initial_state,
            parent,
            subscribe,
        )
        .await
        .map_err(|e| CliError::actor_not_found(format!("Failed to start actor: {}", e)))?;

    // Parse event fields
    let event_fields = if subscribe {
        parse_event_fields(&args.event_fields)
    } else {
        vec![]
    };

    let mut actor_started = false;
    let mut actor_result: Option<ActorResult> = None;
    let startup_timeout_duration = tokio::time::Duration::from_secs(30);
    let process_timeout_duration = if args.timeout > 0 {
        Some(tokio::time::Duration::from_secs(args.timeout))
    } else {
        None
    };
    let mut actor_id: Option<String> = None;

    debug!("Entering process supervision loop");

    // Set up timeout tracking - compute deadline once, not on every iteration
    let startup_deadline = tokio::time::Instant::now() + startup_timeout_duration;
    let startup_sleep = tokio::time::sleep_until(startup_deadline);
    tokio::pin!(startup_sleep);

    // Process timeout will be set once actor starts
    // Use a far-future deadline initially (we'll reset it when actor starts)
    let far_future = tokio::time::Instant::now() + tokio::time::Duration::from_secs(86400 * 365);
    let process_sleep = tokio::time::sleep_until(far_future);
    tokio::pin!(process_sleep);
    let mut process_timeout_active = false;

    // Set up signal handlers ONCE before the loop (not on every iteration)
    #[cfg(unix)]
    let mut sigterm = if args.unix_signals {
        use tokio::signal::unix::{signal, SignalKind};
        Some(signal(SignalKind::terminate()).ok()).flatten()
    } else {
        None
    };

    #[cfg(not(unix))]
    let sigterm: Option<()> = None;

    // For ctrl_c, we need to handle it differently since it's a future, not a stream
    let ctrl_c_enabled = args.unix_signals;

    loop {
        tokio::select! {
            data = client.next_response() => {
                let data = data.map_err(|err| {
                    eprintln!("Process connection error while waiting for events: {}", err);
                    err
                })?;
                match data {
                    ManagementResponse::ActorStarted { id } => {
                        debug!("Actor process started: {}", id);
                        actor_started = true;
                        let id_str = id.to_string();
                        actor_id = Some(id_str.clone());

                            // Set process timeout now that actor has started
                            if let Some(timeout_duration) = process_timeout_duration {
                                let deadline = tokio::time::Instant::now() + timeout_duration;
                                process_sleep.as_mut().reset(deadline);
                                process_timeout_active = true;
                            }

                            // In process mode, we always show some indication of startup
                            if args.verbose {
                                let result = ActorStarted {
                                    actor_id: id_str.clone(),
                                    manifest_path: args.manifest.clone(),
                                    address: address.to_string(),
                                    subscribing: subscribe,
                                    acting_as_parent: parent,
                                    unix_signals: args.unix_signals,
                                };
                                ctx.output.output(&result, None)?;
                            } else {
                                println!("Process started: {}", id_str);
                            }
                    }
                    ManagementResponse::ActorEvent { event } => {
                        if subscribe {
                            display_structured_event(&event, &event_fields)
                                .map_err(|e| CliError::invalid_input("event_display", "event", e.to_string()))?;
                            if event.event_type == "shutdown" {
                                break;
                            }
                        }
                    }
                    ManagementResponse::ActorResult(result) => {
                        if parent {
                            match subscribe {
                                true => actor_result = Some(result),
                                false => {
                                    write_actor_result(result);
                                    break;
                                }
                            }
                        }
                    }
                    ManagementResponse::Error { error } => {
                        return Err(CliError::management_error(error));
                    }
                    _ => {}
                }
            }

            // Startup timeout - only active before actor starts
            _ = &mut startup_sleep, if !actor_started => {
                return Err(CliError::operation_timeout("Actor startup", startup_timeout_duration.as_secs()));
            }

            // Process timeout - only active after actor starts and if timeout was specified
            _ = &mut process_sleep, if process_timeout_active => {
                eprintln!("Process timeout reached, terminating actor...");
                if let Some(actor_id) = &actor_id {
                    let _ = client.terminate_actor(actor_id).await;
                }
                return Err(CliError::operation_timeout("Process execution", args.timeout));
            }

            // Handle Ctrl+C (SIGINT)
            _ = tokio::signal::ctrl_c(), if ctrl_c_enabled => {
                if let Some(actor_id) = &actor_id {
                    debug!("SIGINT received, gracefully stopping actor {}", actor_id);
                    eprintln!("\nReceived SIGINT, gracefully stopping process...");
                    let _ = client.stop_actor(actor_id).await;
                }
                break;
            }

            // Handle SIGTERM (Unix only)
            _ = async {
                #[cfg(unix)]
                {
                    if let Some(ref mut sig) = sigterm {
                        sig.recv().await
                    } else {
                        futures::future::pending::<Option<()>>().await
                    }
                }
                #[cfg(not(unix))]
                {
                    futures::future::pending::<()>().await
                }
            } => {
                if let Some(actor_id) = &actor_id {
                    debug!("SIGTERM received, terminating actor {}", actor_id);
                    eprintln!("\nReceived SIGTERM, terminating process...");
                    let _ = client.terminate_actor(actor_id).await;
                }
                break;
            }

            _ = ctx.shutdown_token.cancelled() => {
                break;
            }
        }
    }

    if parent && subscribe {
        match actor_result {
            Some(result) => {
                debug!("Process result received");
                write_actor_result(result);
            }
            None => {
                // In process mode, we don't exit with error if no result - the process might have been stopped
                if args.verbose {
                    eprintln!("Process completed without result");
                }
            }
        }
    }

    Ok(())
}

/// Write actor result to stdout (same as start command)
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
            let _ = io::stdout().write_all(b"Process stopped externally");
            let _ = io::stdout().flush();
            std::process::exit(1);
        }
    }
}
