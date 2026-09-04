use anyhow::Result;
use clap::{Parser, ValueEnum};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::{error::CliError, CommandContext};
use theater::chain::ChainEvent;
use theater::events::lifecycle::{ActorLifecycleEvent, TerminationCause};
use theater::events::{decode_chain_event_payload, ChainEventPayload};
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::Value;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::{resolve_reference, resolve_reference_cached, ResourceCache};
use theater::ManifestConfig;
use theater::TheaterId;

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

    // Parse the manifest first (needed to check for replay handler)
    let manifest = ManifestConfig::from_toml_str(&manifest_content)
        .map_err(|e| CliError::invalid_manifest(format!("Failed to parse manifest: {}", e)))?;

    // Create the TheaterRuntime in-process
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    // One URL→bytes cache shared across every entry point in this CLI
    // invocation: the top-level wasm fetch below and the supervisor host
    // fn. Lasts until the CLI process exits.
    let resource_cache = Arc::new(ResourceCache::new());
    // The standard handler set lives in the composition root, `theater-stage`,
    // so the CLI, the tests, and any embedder assemble the same node.
    let handler_registry = theater_stage::standard_handlers(
        theater_tx.clone(),
        &theater_stage::StandardHandlers {
            show_actor_logs: !args.no_actor_logs,
            resource_cache: resource_cache.clone(),
        },
    );

    let mut runtime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        handler_registry,
        resource_cache.clone(),
        theater_native::TokioSpawn,
    )
    .await
    .map_err(|e| CliError::server_error(format!("Failed to create runtime: {}", e)))?;

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

    // Load WASM bytes. Honors the manifest's `static_package` flag by
    // routing through the same ResourceCache the supervisor and resume
    // paths use — same-process subsequent fetches of this URL hit.
    let wasm_bytes = if manifest.static_package {
        let (arc, _hit) = resolve_reference_cached(&wasm_path, &resource_cache)
            .await
            .map_err(|e| {
                CliError::server_error(format!("Failed to load WASM from '{}': {}", wasm_path, e))
            })?;
        (*arc).clone()
    } else {
        resolve_reference(&wasm_path).await.map_err(|e| {
            CliError::server_error(format!("Failed to load WASM from '{}': {}", wasm_path, e))
        })?
    };

    // Spawn the actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

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
            subscription_tx: None, // Using global subscription instead
            parent_id: None,
        }
    } else {
        TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
            init_state,
            response_tx,
            subscription_tx: None, // Using global subscription instead
            parent_id: None,
        }
    };

    theater_tx
        .send(cmd)
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
            // Global event subscription (all actors). The root actor's exit is
            // its terminal chain event — decode the cause to print its final
            // result / error, mirroring the old supervisor-notification path.
            event = global_events_rx.recv() => {
                if let Some((event_actor_id, chain_event)) = event {
                    // Output events if --events mode is enabled
                    // (Actor logs are printed directly by SelfHandler, not extracted here)
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

                    // Root actor termination: print its result/error, then exit.
                    if event_actor_id == actor_id && chain_event.event_type == "terminated" {
                        if let Some(ChainEventPayload::Lifecycle(
                            ActorLifecycleEvent::Terminated { cause },
                        )) = decode_chain_event_payload(&chain_event.data)
                        {
                            match cause {
                                TerminationCause::Completed {
                                    final_state: Some(output),
                                } => {
                                    let _ = std::io::stdout().write_all(&output);
                                    let _ = std::io::stdout().flush();
                                }
                                TerminationCause::Completed { final_state: None } => {}
                                TerminationCause::Failed { error } => {
                                    eprintln!("Actor error: {}", error);
                                    std::process::exit(1);
                                }
                                other => debug!("Actor terminated: {:?}", other),
                            }
                        }
                        break;
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
                });

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
