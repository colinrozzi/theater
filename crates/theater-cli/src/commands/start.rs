use anyhow::Result;
use clap::{Parser, ValueEnum};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::{error::CliError, CommandContext};
use theater::chain::ChainEvent;
use theater::config::actor_manifest::{
    RuntimeHostConfig, StoreHandlerConfig, SupervisorHostConfig, TcpHandlerConfig,
    TerminalHandlerConfig, TimerHandlerConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::pack_bridge::Value;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater::ManifestConfig;
use theater::TheaterId;
use theater_handler_loop::LoopHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_rpc::RpcHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use theater_handler_tcp::TcpHandler;
use theater_handler_terminal::TerminalHandler;
use theater_handler_timer::TimerHandler;

/// Output format for chain events
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum EventFormat {
    /// JSON format (one JSON object per line)
    Json,
    /// Short format (compact, one line per event)
    #[default]
    Short,
    /// Full format (complete event data, multi-line)
    Full,
}

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Path or URL to the actor manifest file
    #[arg(default_value = "manifest.toml")]
    pub manifest: String,

    /// Output chain events from all actors
    #[arg(long)]
    pub events: bool,

    /// Format for event output (used with --events)
    #[arg(long, value_enum, default_value = "short")]
    pub events_format: EventFormat,

    /// Save the chain to a local directory after the actor exits.
    /// Defaults to `.chains/` in the current directory.
    #[arg(long, default_missing_value = ".chains", num_args = 0..=1)]
    pub save: Option<PathBuf>,

    /// Skip calling the actor's init function after spawning
    #[arg(long)]
    pub no_init: bool,

    /// Disable actor log output to stdout
    #[arg(long)]
    pub no_actor_logs: bool,
}

/// Format a chain event with actor ID prefix using ChainEvent's Display impl (short)
fn format_event_short(event: &ChainEvent, actor_id: &TheaterId) -> String {
    let id_str = actor_id.to_string();
    let short_id = &id_str[..8.min(id_str.len())];
    format!("[{}] {}\n", short_id, event)
}

/// Format a chain event with full data (multi-line, complete)
fn format_event_full(event: &ChainEvent, actor_id: &TheaterId) -> String {
    let id_str = actor_id.to_string();
    let short_id = &id_str[..8.min(id_str.len())];
    let hash_hex = hex::encode(&event.hash);
    let parent_hex = event
        .parent_hash
        .as_ref()
        .map(hex::encode)
        .unwrap_or_else(|| "none".to_string());
    let data_str = String::from_utf8_lossy(&event.data);

    format!(
        "EVENT [{}] {}\nparent: {}\ntype: {}\nsize: {}\n{}\n\n",
        short_id,
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
        "data": format!("{} bytes (pack-encoded)", event.data.len())
    });
    serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string())
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
        let file = self.files.entry(*actor_id).or_insert_with(|| {
            let path = self.dir.join(format!("{}.chain", actor_id));
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect("Failed to open chain file")
        });

        let block = theater::chain::format::format_event(event);
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
    show_actor_logs: bool,
) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    // Runtime handler - provides log, get-chain, shutdown
    let runtime_config = RuntimeHostConfig {};
    registry.register(
        RuntimeHandler::new(runtime_config, theater_tx.clone(), None)
            .with_show_logs(show_actor_logs),
    );

    // Store handler - provides content storage
    let store_config = StoreHandlerConfig::default();
    registry.register(StoreHandler::new(store_config, None));

    // Supervisor handler - allows spawning/managing child actors
    let supervisor_config = SupervisorHostConfig {};
    registry.register(SupervisorHandler::new(supervisor_config, None));

    // Message server handler - inter-actor messaging
    let message_router = MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router.clone()));

    // RPC handler - direct actor-to-actor function calls
    registry.register(RpcHandler::new(theater_tx.clone()));

    // TCP handler - TCP server/client functionality
    let tcp_config = TcpHandlerConfig {
        listen: None,
        max_connections: None,
        ..Default::default()
    };
    registry.register(TcpHandler::new(tcp_config));

    // Terminal handler - stdin/stdout/stderr for interactive CLI apps
    let terminal_config = TerminalHandlerConfig::default();
    registry.register(TerminalHandler::new(terminal_config));

    // Timer handler - periodic tick callbacks for game loops, polling, etc.
    let timer_config = TimerHandlerConfig::default();
    registry.register(TimerHandler::new(timer_config));

    // Loop handler - cooperative looping with yield points
    registry.register(LoopHandler::new());

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

    // Set up chain file manager if --save is specified
    let mut chain_file_manager = if let Some(ref dir) = args.save {
        Some(ChainFileManager::new(dir.clone())?)
    } else {
        None
    };

    // Create the TheaterRuntime in-process
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_handler_registry(theater_tx.clone(), !args.no_actor_logs);

    let mut runtime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        None, // no channel events forwarding needed
        handler_registry,
    )
    .await
    .map_err(|e| CliError::server_error(format!("Failed to create runtime: {}", e)))?;

    // Set chain output directory if --save is specified
    if let Some(ref dir) = args.save {
        runtime.chain_dir = Some(dir.clone());
    }

    // Set up global event subscription (receives events from ALL actors)
    let (global_events_tx, mut global_events_rx) = mpsc::channel(256);
    runtime.add_global_subscription(global_events_tx);

    // Spawn the runtime event loop in a background task
    let runtime_handle = tokio::spawn(async move {
        if let Err(e) = runtime.run().await {
            error!("Theater runtime error: {}", e);
        }
    });

    // Parse the manifest
    let manifest = ManifestConfig::from_toml_str(&manifest_content)
        .map_err(|e| CliError::invalid_manifest(format!("Failed to parse manifest: {}", e)))?;

    // Resolve WASM path relative to manifest directory
    let wasm_path = if manifest.package.starts_with('/') || manifest.package.contains("://") {
        // Absolute path or URL - use as is
        manifest.package.clone()
    } else {
        // Relative path - resolve relative to manifest's directory
        let manifest_path = std::path::Path::new(&args.manifest);
        if let Some(manifest_dir) = manifest_path.parent() {
            manifest_dir
                .join(&manifest.package)
                .to_string_lossy()
                .to_string()
        } else {
            manifest.package.clone()
        }
    };

    // Load WASM bytes
    let wasm_bytes = resolve_reference(&wasm_path).await.map_err(|e| {
        CliError::server_error(format!("Failed to load WASM from '{}': {}", wasm_path, e))
    })?;

    // Spawn the actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    // Set up a supervisor channel so we get notified when the actor exits
    let (supervisor_tx, mut supervisor_rx) = mpsc::channel(32);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
            init_bytes: None,
            response_tx,
            supervisor_tx: Some(supervisor_tx),
            subscription_tx: None, // Using global subscription instead
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

    // Call init unless --no-init flag is set
    if !args.no_init {
        // Get the actor handle
        let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
        theater_tx
            .send(TheaterCommand::GetActorHandle {
                actor_id,
                response_tx: handle_tx,
            })
            .await
            .map_err(|e| CliError::server_error(format!("Failed to get actor handle: {}", e)))?;

        let actor_handle = match handle_rx.await {
            Ok(Some(handle)) => handle,
            Ok(None) => {
                return Err(CliError::server_error("Actor handle not found".to_string()));
            }
            Err(e) => {
                return Err(CliError::server_error(format!(
                    "Failed to receive actor handle: {}",
                    e
                )));
            }
        };

        // Call init — state is injected by execute_call_pack from the actor store
        let init_params = Value::Tuple(vec![]);
        debug!("Calling init on actor {}", actor_id);
        let _init_result = actor_handle
            .call_function("theater:simple/actor.init".to_string(), init_params)
            .await
            .map_err(|e| CliError::server_error(format!("Failed to call init: {}", e)))?;
        debug!("Init completed");
    }

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

            // Global event subscription (all actors)
            event = global_events_rx.recv() => {
                if let Some((event_actor_id, event_result)) = event {
                    match event_result {
                        Ok(chain_event) => {
                            // Persist to chain file if enabled
                            if let Some(ref mut manager) = chain_file_manager {
                                if let Err(e) = manager.write_event(&event_actor_id, &chain_event) {
                                    eprintln!("Warning: failed to write chain event: {}", e);
                                }
                            }

                            // Output events if --events mode is enabled
                            // (Actor logs are printed directly by RuntimeHandler, not extracted here)
                            if args.events {
                                match args.events_format {
                                    EventFormat::Json => {
                                        println!("{}", format_event_json(&chain_event, &event_actor_id));
                                    }
                                    EventFormat::Short => {
                                        print!("{}", format_event_short(&chain_event, &event_actor_id));
                                    }
                                    EventFormat::Full => {
                                        print!("{}", format_event_full(&chain_event, &event_actor_id));
                                    }
                                }
                            }

                            // Check for root actor shutdown
                            if event_actor_id == actor_id && chain_event.event_type == "shutdown" {
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
                    actor_id,
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
    let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), runtime_handle).await;

    Ok(())
}
