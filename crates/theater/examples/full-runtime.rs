//! Theater Runtime Example with Theater-Specific Handlers
//!
//! This example demonstrates how to create a Theater runtime with the
//! Theater-specific handler crates.
//!
//! NOTE: WASI handlers (environment, filesystem, http, io, timing, random, process)
//! have been deprecated and moved to crates/deprecated/. They will be redesigned
//! for Composite runtime support later.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example full-runtime
//! ```

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, Level};
use tracing_subscriber;

use theater::config::actor_manifest::{
    RuntimeHostConfig, StoreHandlerConfig, SupervisorHostConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;

// Import Theater-specific handlers
use theater_handler_message_server::MessageServerHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;

/// Creates a HandlerRegistry with Theater-specific handlers.
fn create_handler_registry(theater_tx: tokio::sync::mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    info!("Registering Theater-specific handlers...");

    // Runtime handler - provides actor runtime information and control
    info!("  - Registering runtime handler");
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    // Store handler - provides key-value storage for actors
    info!("  - Registering store handler");
    let store_config = StoreHandlerConfig {};
    registry.register(StoreHandler::new(store_config, None));

    // Supervisor handler - allows actors to spawn and manage child actors
    info!("  - Registering supervisor handler");
    let supervisor_config = SupervisorHostConfig {};
    registry.register(SupervisorHandler::new(supervisor_config, None));

    // Message server handler - provides inter-actor messaging
    info!("  - Registering message-server handler");
    let message_router = theater_handler_message_server::MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router));

    info!("Successfully registered 4 Theater-specific handlers!");
    info!("");
    info!("NOTE: WASI handlers are deprecated - see crates/deprecated/");

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
    info!("  - runtime         - Runtime functions (log, get-state, shutdown)");
    info!("  - store           - Content-addressed storage");
    info!("  - supervisor      - Actor supervision");
    info!("  - message-server  - Inter-actor messaging");
    info!("");
    info!("WASI handlers (environment, filesystem, http, timing, random, process)");
    info!("have been deprecated and moved to crates/deprecated/.");
    info!("");

    // Create communication channels
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);

    // Optional: Create channel for runtime events
    let (channel_events_tx, _channel_events_rx) = mpsc::channel(32);

    // Create handler registry with Theater-specific handlers
    let handler_registry = create_handler_registry(theater_tx.clone());

    info!("");
    info!("Creating Theater runtime...");

    // Create the Theater runtime
    let mut runtime: TheaterRuntime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        Some(channel_events_tx),
        handler_registry,
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
