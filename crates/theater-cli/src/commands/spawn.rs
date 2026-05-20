use anyhow::Result;
use clap::{Parser, ValueEnum};
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
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::Value;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater::ManifestConfig;
use theater::TheaterId;
use theater_handler_loop::LoopHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_podman::PodmanHandler;
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

/// Arguments shared by `theater spawn` and `theater setup`.
#[derive(Debug, Parser)]
pub struct SpawnArgs {
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

    /// Disable actor log output to stdout
    #[arg(long)]
    pub no_actor_logs: bool,
}

/// `theater setup` takes the same arguments as `theater spawn`.
pub type SetupArgs = SpawnArgs;

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

/// Create a handler registry with all Theater handlers
fn create_handler_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
    show_actor_logs: bool,
) -> Result<HandlerRegistry, CliError> {
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

    // Podman handler - container management via the podman CLI
    let podman_config = theater::config::actor_manifest::PodmanHandlerConfig::default();
    registry.register(PodmanHandler::new(podman_config));

    Ok(registry)
}

/// `theater spawn manifest.toml` — load the actor, set up its task loops,
/// AND call its `theater:simple/actor.init` export before returning control
/// to the caller. The runtime auto-inits (PR A in ticket #27); the CLI
/// doesn't fire init itself.
pub async fn execute_spawn(args: &SpawnArgs, ctx: &CommandContext) -> Result<(), CliError> {
    run(args, ctx, /* call_init = */ true).await
}

/// `theater setup manifest.toml` — load the actor and set up its task loops,
/// but do NOT call `actor.init`. Used by replay (the replay handler walks
/// the recorded chain and fires init from there) and by callers that want
/// to drive init themselves with custom typed params.
pub async fn execute_setup(args: &SetupArgs, ctx: &CommandContext) -> Result<(), CliError> {
    run(args, ctx, /* call_init = */ false).await
}

/// Shared body for `spawn` and `setup`. Differs only in which
/// `TheaterCommand` variant it dispatches.
async fn run(args: &SpawnArgs, ctx: &CommandContext, call_init: bool) -> Result<(), CliError> {
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

    // (Chain writing is handled by the runtime's ChainWriter via runtime.chain_dir)

    // Parse the manifest first (needed to check for replay handler)
    let manifest = ManifestConfig::from_toml_str(&manifest_content)
        .map_err(|e| CliError::invalid_manifest(format!("Failed to parse manifest: {}", e)))?;

    // Create the TheaterRuntime in-process
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_handler_registry(theater_tx.clone(), !args.no_actor_logs)?;

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

    // The runtime stores `init_state` as the actor's initial state and
    // (for SpawnActor) prepends it to the auto-fired actor.init call.
    // For the CLI, the only place a caller can supply that state is the
    // manifest's `initial_state` field — fall back to it here when set,
    // otherwise use the conventional none sentinel.
    //
    // PR A (#58) moved this resolver out of `spawn_actor` with the intent
    // that each caller does its own resolution; this line is the CLI's.
    let init_state = match manifest.initial_state.as_ref() {
        Some(s) => Value::String(s.clone()),
        None => default_init_state(),
    };

    // SpawnActor: setup + auto-init (the runtime calls actor.init before
    // responding). SetupActor: setup only — caller drives init separately
    // (or a handler like ReplayHandler does it from the chain).
    let cmd = if call_init {
        TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
            init_state,
            response_tx,
            supervisor_tx: Some(supervisor_tx),
            subscription_tx: None, // Using global subscription instead
        }
    } else {
        TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
            init_state,
            response_tx,
            supervisor_tx: Some(supervisor_tx),
            subscription_tx: None, // Using global subscription instead
        }
    };

    theater_tx
        .send(cmd)
        .await
        .map_err(|e| CliError::server_error(format!("Failed to send spawn command: {}", e)))?;

    // Wait for the actor to start (and, for SpawnActor, for init to complete).
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

            // Global event subscription (all actors)
            event = global_events_rx.recv() => {
                if let Some((event_actor_id, event_result)) = event {
                    match event_result {
                        Ok(chain_event) => {
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
