use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::{error::CliError, CommandContext};
use theater::config::actor_manifest::{
    RuntimeHostConfig, StoreHandlerConfig, SupervisorHostConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path or URL to the actor manifest file
    #[arg(default_value = "manifest.toml")]
    pub manifest: String,

    /// Initial state as JSON string or path to JSON file
    #[arg(short, long)]
    pub initial_state: Option<String>,

    /// Show verbose output
    #[arg(long)]
    pub verbose: bool,
}

/// Create a handler registry with all Theater handlers
fn create_handler_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    // Runtime handler - provides log, get-chain, shutdown
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    // Store handler - provides content storage
    let store_config = StoreHandlerConfig {};
    registry.register(StoreHandler::new(store_config, None));

    // Supervisor handler - allows spawning/managing child actors
    let supervisor_config = SupervisorHostConfig {};
    registry.register(SupervisorHandler::new(supervisor_config, None));

    // Message server handler - inter-actor messaging
    let message_router = MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router.clone()));

    registry
}

/// Execute the start command - spin up a local runtime and run the actor
pub async fn execute_async(args: &StartArgs, ctx: &CommandContext) -> Result<(), CliError> {
    debug!("Starting actor from manifest: {}", args.manifest);

    // Resolve the manifest reference (file path, URL, or store path)
    let manifest_bytes = resolve_reference(&args.manifest).await.map_err(|e| {
        CliError::invalid_manifest(format!(
            "Failed to resolve manifest reference '{}': {}",
            args.manifest, e
        ))
    })?;

    let manifest_content = String::from_utf8(manifest_bytes).map_err(|e| {
        CliError::invalid_manifest(format!("Manifest content is not valid UTF-8: {}", e))
    })?;

    // Handle initial state
    let initial_state = if let Some(state_str) = &args.initial_state {
        match resolve_reference(state_str).await {
            Ok(bytes) => Some(bytes),
            Err(_) => Some(state_str.as_bytes().to_vec()),
        }
    } else {
        None
    };

    // Create the TheaterRuntime in-process
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_handler_registry(theater_tx.clone());

    let mut runtime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        None, // no channel events forwarding needed
        handler_registry,
    )
    .await
    .map_err(|e| CliError::server_error(format!("Failed to create runtime: {}", e)))?;

    // Spawn the runtime event loop in a background task
    let runtime_handle = tokio::spawn(async move {
        if let Err(e) = runtime.run().await {
            error!("Theater runtime error: {}", e);
        }
    });

    // Spawn the actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    // Set up a supervisor channel so we get notified when the actor exits
    let (supervisor_tx, mut supervisor_rx) = mpsc::channel(32);

    // Set up an event subscription channel for logging
    let (subscription_tx, mut subscription_rx) = mpsc::channel(32);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest_content,
            wasm_bytes: None,
            init_bytes: initial_state,
            response_tx,
            parent_id: None,
            supervisor_tx: Some(supervisor_tx),
            subscription_tx: Some(subscription_tx),
        })
        .await
        .map_err(|e| CliError::server_error(format!("Failed to send spawn command: {}", e)))?;

    // Wait for the actor to start
    let actor_id = match response_rx.await {
        Ok(Ok(id)) => {
            info!("Actor started: {}", id);
            if args.verbose {
                eprintln!("Actor started: {}", id);
            }
            id
        }
        Ok(Err(e)) => {
            return Err(CliError::server_error(format!(
                "Failed to start actor: {}",
                e
            )));
        }
        Err(e) => {
            return Err(CliError::server_error(format!(
                "Failed to receive spawn response: {}",
                e
            )));
        }
    };

    println!("{}", actor_id);

    // Now wait for either:
    // - The actor to exit (supervisor notification)
    // - Ctrl+C
    // - Shutdown token cancellation
    //
    // Forward events to stderr if verbose
    loop {
        tokio::select! {
            // Actor result (exit/error)
            result = supervisor_rx.recv() => {
                match result {
                    Some(actor_result) => {
                        debug!("Actor exited: {:?}", actor_result);
                        match actor_result {
                            theater::messages::ActorResult::Success(success) => {
                                if let Some(output) = success.result {
                                    use std::io::Write;
                                    let _ = std::io::stdout().write_all(&output);
                                    let _ = std::io::stdout().flush();
                                }
                            }
                            theater::messages::ActorResult::Error(err) => {
                                eprintln!("Actor error: {}", err.error);
                                std::process::exit(1);
                            }
                            theater::messages::ActorResult::ExternalStop(_) => {
                                debug!("Actor stopped externally");
                            }
                        }
                        break;
                    }
                    None => {
                        // Supervisor channel closed, actor is done
                        debug!("Supervisor channel closed");
                        break;
                    }
                }
            }

            // Event subscription (for verbose logging)
            event = subscription_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        Ok(chain_event) => {
                            if args.verbose {
                                eprintln!("[{}]", chain_event.event_type);
                            }
                            if chain_event.event_type == "shutdown" {
                                break;
                            }
                        }
                        Err(e) => {
                            debug!("Actor error event: {:?}", e);
                        }
                    }
                }
            }

            // Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                debug!("Received Ctrl+C, stopping actor {}", actor_id);
                eprintln!("\nStopping actor...");

                let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
                let _ = theater_tx.send(TheaterCommand::StopActor {
                    actor_id: actor_id.clone(),
                    response_tx: stop_tx,
                }).await;

                // Wait briefly for graceful shutdown
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(5),
                    stop_rx,
                ).await {
                    Ok(Ok(Ok(()))) => debug!("Actor stopped gracefully"),
                    _ => debug!("Actor stop timed out or failed"),
                }
                break;
            }

            // Shutdown token
            _ = ctx.shutdown_token.cancelled() => {
                debug!("Shutdown token cancelled");
                break;
            }
        }
    }

    // Drop the theater_tx to signal the runtime to stop
    drop(theater_tx);

    // Wait for runtime to finish (with timeout)
    let _ = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        runtime_handle,
    )
    .await;

    Ok(())
}
