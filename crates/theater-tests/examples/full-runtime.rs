//! Theater Runtime Example with Theater-Specific Handlers
//!
//! This example demonstrates how to create a Theater runtime with the
//! Theater-specific handler crates.
//!
//! NOTE: the old WASI handlers (environment, filesystem, http, io, timing,
//! random, process) have been removed. They may be redesigned for the Composite
//! runtime later.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example full-runtime
//! ```

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, Level};

use std::sync::Arc;
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::ResourceCache;

// Import Theater-specific handlers
use theater_handler_message_server::MessageServerHandler;
use theater_handler_runtime::{RuntimeHandler, RuntimeHostConfig};
use theater_handler_self::{SelfHandler, SelfHostConfig};
use theater_handler_store::{StoreHandler, StoreHandlerConfig};

/// Creates a HandlerRegistry with Theater-specific handlers.
fn create_handler_registry(
    theater_tx: tokio::sync::mpsc::UnboundedSender<TheaterCommand>,
) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    info!("Registering Theater-specific handlers...");

    // Self handler - provides the per-actor handle (log, self, shutdown)
    info!("  - Registering self handler");
    let self_config = SelfHostConfig {};
    registry.register(SelfHandler::new(self_config, theater_tx, None));

    // Store handler - provides key-value storage for actors
    info!("  - Registering store handler");
    let store_config = StoreHandlerConfig::default();
    registry.register(StoreHandler::new(store_config, None));

    // Runtime (control) handler - spawn/inspect/drive any actor, capability-gated
    info!("  - Registering runtime (control) handler");
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, None));

    // Message server handler - provides inter-actor messaging
    info!("  - Registering message-server handler");
    let message_router = theater_handler_message_server::MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router));

    info!("Successfully registered 4 Theater-specific handlers!");
    info!("");
    info!("NOTE: the old WASI handlers were removed");

    registry
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    info!("Theater Runtime - Handler Example");
    info!("==================================");
    info!("");
    info!("This example demonstrates a Theater runtime with Theater-specific handlers:");
    info!("  - self            - Per-actor handle (log, self, shutdown)");
    info!("  - store           - Content-addressed storage");
    info!("  - runtime         - Actor spawn/inspect/drive (control plane)");
    info!("  - message-server  - Inter-actor messaging");
    info!("");
    info!("The old WASI handlers (environment, filesystem, http, timing, random,");
    info!("process) have been removed.");
    info!("");

    // Create communication channels
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();

    // Create handler registry with Theater-specific handlers
    let handler_registry = create_handler_registry(theater_tx.clone());

    info!("");
    info!("Creating Theater runtime...");

    // Create the Theater runtime
    let mut runtime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        handler_registry,
        Arc::new(ResourceCache::new()),
        theater_native::TokioSpawn,
    )
    .await?;

    info!("Runtime created successfully!");
    info!("");
    info!("Runtime is ready to accept commands.");
    info!("To spawn actors, send SpawnActor commands via the theater_tx channel.");
    info!("");
    info!("Press Ctrl+C to shutdown...");
    info!("");

    // Run the runtime
    runtime.run().await?;

    info!("Runtime shut down gracefully.");

    Ok(())
}
