use anyhow::Result;
use clap::{Parser, ValueEnum};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, error};
use tracing_subscriber::EnvFilter;

use crate::{error::CliError, CommandContext};
use theater::chain::ChainEvent;
use theater::config::actor_manifest::{
    RuntimeHostConfig, StoreHandlerConfig, SupervisorHostConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater::TheaterId;
use theater::ManifestConfig;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;

/// Log level for runtime/system logs
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum LogLevel {
    /// Show error logs only
    Error,
    /// Show warning and error logs
    Warn,
    /// Show info, warning, and error logs
    #[default]
    Info,
    /// Show debug and above
    Debug,
    /// Show all logs including trace
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "error"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Trace => write!(f, "trace"),
        }
    }
}

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path or URL to the actor manifest file
    #[arg(default_value = "manifest.toml")]
    pub manifest: String,

    /// Initial state as JSON string or path to JSON file
    #[arg(short, long)]
    pub initial_state: Option<String>,

    /// Output all chain events as JSON (not just logs)
    #[arg(long)]
    pub events: bool,

    /// Directory to persist chain events (one file per actor)
    #[arg(long)]
    pub chain_dir: Option<PathBuf>,

    /// Runtime log level (for system/tracing logs)
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevel>,

    /// Show verbose output (deprecated, use --events or --log-level)
    #[arg(long, hide = true)]
    pub verbose: bool,
}

/// Extract log message from a chain event if it's a runtime log event
fn extract_log_message(event: &ChainEvent) -> Option<String> {
    if event.event_type != "theater:simple/runtime/log" {
        return None;
    }

    // Parse the event data to extract the log message
    // The data is a serialized ChainEventPayload::HostFunction(HostFunctionCall)
    // where input contains the log message
    let data_str = std::str::from_utf8(&event.data).ok()?;
    let payload: serde_json::Value = serde_json::from_str(data_str).ok()?;

    // Navigate to the input field which contains the log message
    // Structure: {"category":"HostFunction","interface":"...","function":"log","input":{"String":"message"},...}
    let input = payload.get("input")?;

    // The input is a Pack Value, which for a string is {"String": "the message"}
    if let Some(msg) = input.get("String") {
        return msg.as_str().map(|s| s.to_string());
    }

    // Fallback: try to get it as a direct string
    input.as_str().map(|s| s.to_string())
}

/// Format a chain event in the custom block format for file persistence
fn format_event_block(event: &ChainEvent) -> String {
    let hash_hex = hex::encode(&event.hash);
    let parent_hex = event
        .parent_hash
        .as_ref()
        .map(|h| hex::encode(h))
        .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string());

    let data_str = String::from_utf8_lossy(&event.data);

    format!(
        "EVENT {}\n{}\n{}\n{}\n\n{}\n\n",
        hash_hex,
        parent_hex,
        event.event_type,
        event.data.len(),
        data_str
    )
}

/// Format a chain event as JSON for stdout
fn format_event_json(event: &ChainEvent, actor_id: &TheaterId) -> String {
    let json = serde_json::json!({
        "actor_id": actor_id.to_string(),
        "hash": hex::encode(&event.hash),
        "parent_hash": event.parent_hash.as_ref().map(hex::encode),
        "event_type": event.event_type,
        "data": serde_json::from_slice::<serde_json::Value>(&event.data).ok()
    });
    serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string())
}

/// Short actor ID for display (first 8 chars)
fn short_id(id: &TheaterId) -> String {
    let s = id.to_string();
    if s.len() > 8 {
        s[..8].to_string()
    } else {
        s
    }
}

/// Manages chain file writers for multiple actors
struct ChainFileManager {
    dir: PathBuf,
    files: HashMap<TheaterId, std::fs::File>,
}

impl ChainFileManager {
    fn new(dir: PathBuf) -> Result<Self, CliError> {
        fs::create_dir_all(&dir).map_err(|e| {
            CliError::file_operation_failed("create directory", dir.display().to_string(), e)
        })?;
        Ok(Self {
            dir,
            files: HashMap::new(),
        })
    }

    fn write_event(&mut self, actor_id: &TheaterId, event: &ChainEvent) -> Result<(), CliError> {
        let file = self.files.entry(actor_id.clone()).or_insert_with(|| {
            let path = self.dir.join(format!("{}.chain", actor_id));
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect("Failed to open chain file")
        });

        let block = format_event_block(event);
        file.write_all(block.as_bytes()).map_err(|e| {
            CliError::file_operation_failed("write event", format!("{}.chain", actor_id), e)
        })?;
        file.flush().map_err(|e| {
            CliError::file_operation_failed("flush", format!("{}.chain", actor_id), e)
        })?;
        Ok(())
    }
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
    // Set up tracing based on --log-level
    if let Some(level) = &args.log_level {
        let filter = EnvFilter::try_new(format!("theater={},theater_handler={}", level, level))
            .unwrap_or_else(|_| EnvFilter::new("info"));
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_writer(std::io::stderr)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    }

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

    // Set up chain file manager if --chain-dir is specified
    let mut chain_file_manager = if let Some(ref dir) = args.chain_dir {
        Some(ChainFileManager::new(dir.clone())?)
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

    // Parse the manifest
    let manifest = ManifestConfig::from_toml_str(&manifest_content).map_err(|e| {
        CliError::invalid_manifest(format!("Failed to parse manifest: {}", e))
    })?;

    // Load WASM bytes from manifest.package
    let wasm_bytes = resolve_reference(&manifest.package).await.map_err(|e| {
        CliError::server_error(format!(
            "Failed to load WASM from '{}': {}",
            manifest.package, e
        ))
    })?;

    // Spawn the actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    // Set up a supervisor channel so we get notified when the actor exits
    let (supervisor_tx, mut supervisor_rx) = mpsc::channel(32);

    // Set up an event subscription channel for logging
    let (subscription_tx, mut subscription_rx) = mpsc::channel(32);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
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
            debug!("Actor started: {}", id);
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

    // Now wait for either:
    // - The actor to exit (supervisor notification)
    // - Ctrl+C
    // - Shutdown token cancellation
    //
    // Output modes:
    // - Default: print only log messages as [actor-id] message
    // - --events: print all chain events as JSON
    // - --chain-dir: also persist events to files
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
                                    // Write actor result to stdout
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

            // Event subscription
            event = subscription_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        Ok(chain_event) => {
                            // Persist to chain file if enabled
                            if let Some(ref mut manager) = chain_file_manager {
                                if let Err(e) = manager.write_event(&actor_id, &chain_event) {
                                    eprintln!("Warning: failed to write chain event: {}", e);
                                }
                            }

                            // Output to stdout based on mode
                            if args.events {
                                // JSON mode: output all events as JSON
                                println!("{}", format_event_json(&chain_event, &actor_id));
                            } else {
                                // Default mode: only show log messages
                                if let Some(msg) = extract_log_message(&chain_event) {
                                    println!("[{}] {}", short_id(&actor_id), msg);
                                }
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
