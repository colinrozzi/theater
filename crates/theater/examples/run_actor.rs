//! Simple actor runner for fast iteration.
//!
//! Usage: cargo run --example run_actor -- path/to/manifest.toml
//!
//! This bypasses the server/client architecture and runs an actor directly
//! using the Theater runtime library.

use anyhow::{Context, Result};
use std::path::PathBuf;
use theater::config::actor_manifest::{RuntimeHostConfig, StoreHandlerConfig, SupervisorHostConfig};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater_handler_message_server::MessageServerHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use tokio::signal;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,theater=debug".to_string()),
        )
        .init();

    // Get manifest path from args
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <manifest.toml>", args[0]);
        std::process::exit(1);
    }

    let manifest_path = PathBuf::from(&args[1])
        .canonicalize()
        .with_context(|| format!("Failed to resolve manifest path: {}", args[1]))?;

    info!("Loading manifest from: {}", manifest_path.display());

    // Verify manifest exists
    if !manifest_path.exists() {
        return Err(anyhow::anyhow!(
            "Manifest not found: {}",
            manifest_path.display()
        ));
    }

    // Create the Theater runtime with handlers
    let (theater_tx, theater_rx) = mpsc::channel(100);
    let handler_registry = create_handler_registry(theater_tx.clone());

    let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry)
        .await
        .context("Failed to create Theater runtime")?;

    // Start the runtime in a background task
    let runtime_handle = tokio::spawn(async move {
        if let Err(e) = runtime.run().await {
            error!("Theater runtime error: {}", e);
        }
    });

    // Give runtime a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Spawn the actor
    info!("Spawning actor...");
    let (response_tx, response_rx) = oneshot::channel();

    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest_path.to_string_lossy().to_string(),
            wasm_bytes: None,
            init_bytes: None,
            response_tx,
            parent_id: None,
            supervisor_tx: None,
            subscription_tx: None,
        })
        .await
        .context("Failed to send spawn command")?;

    // Wait for spawn response
    match response_rx.await {
        Ok(Ok(actor_id)) => {
            info!("Actor spawned successfully: {}", actor_id);
        }
        Ok(Err(e)) => {
            error!("Failed to spawn actor: {}", e);
            return Err(anyhow::anyhow!("Spawn failed: {}", e));
        }
        Err(_) => {
            error!("Spawn channel closed");
            return Err(anyhow::anyhow!("Spawn channel closed"));
        }
    }

    // Wait for Ctrl+C
    info!("Actor running. Press Ctrl+C to stop.");
    signal::ctrl_c().await?;
    info!("Shutting down...");

    // Cleanup
    drop(theater_tx);
    let _ = runtime_handle.await;

    info!("Done.");
    Ok(())
}

fn create_handler_registry(theater_tx: mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    info!("Registering handlers...");

    // Runtime handler - provides actor runtime information and control
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    // Store handler - provides key-value storage for actors
    let store_config = StoreHandlerConfig {};
    registry.register(StoreHandler::new(store_config, None));

    // Supervisor handler - allows actors to spawn and manage child actors
    let supervisor_config = SupervisorHostConfig {};
    registry.register(SupervisorHandler::new(supervisor_config, None));

    // Message server handler - provides inter-actor messaging
    let message_router = theater_handler_message_server::MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router));

    info!("✓ 4 handlers registered");

    registry
}
