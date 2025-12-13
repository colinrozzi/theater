//! Full Theater Runtime Example with All Migrated Handlers
//!
//! This example demonstrates how to create a Theater runtime with the migrated
//! handler crates. Note that some handlers require runtime-specific dependencies
//! and are typically configured through the TheaterServer or custom runtime setup.
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

use theater::chain::ChainEvent;
use theater::config::actor_manifest::{
    EnvironmentHandlerConfig, FileSystemHandlerConfig, HttpClientHandlerConfig,
    ProcessHostConfig, RandomHandlerConfig, RuntimeHostConfig, StoreHandlerConfig,
    SupervisorHostConfig, TimingHostConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;

// Import migrated handlers that can be created independently
use theater_handler_environment::EnvironmentHandler;
use theater_handler_filesystem::FilesystemHandler;
use theater_handler_http_client::HttpClientHandler;
use theater_handler_http_framework::HttpFrameworkHandler;
use theater_handler_message_server::MessageServerHandler;
use theater_handler_process::ProcessHandler;
use theater_handler_random::RandomHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use theater_handler_timing::TimingHandler;

/// Creates a HandlerRegistry with all migrated handlers.
///
/// All 11 handlers can now be registered at runtime creation!
fn create_handler_registry(theater_tx: tokio::sync::mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    info!("Registering migrated handlers...");

    // Phase 1: Simple Handlers
    info!("  ✓ Registering environment handler");
    let env_config = EnvironmentHandlerConfig {
        allowed_vars: None,              // Allow all environment variables
        denied_vars: Some(vec![          // Deny sensitive variables
            "AWS_SECRET_ACCESS_KEY".to_string(),
            "DATABASE_PASSWORD".to_string(),
        ]),
        allow_list_all: false,           // Don't allow listing all vars
        allowed_prefixes: None,
    };
    registry.register(EnvironmentHandler::new(env_config, None));

    info!("  ✓ Registering random handler");
    let random_config = RandomHandlerConfig {
        seed: None,                      // Use OS entropy (not reproducible)
        max_bytes: 1024 * 1024,         // 1MB max
        max_int: u64::MAX - 1,
        allow_crypto_secure: false,
    };
    registry.register(RandomHandler::new(random_config, None));

    info!("  ✓ Registering timing handler");
    let timing_config = TimingHostConfig {
        max_sleep_duration: 3600000,     // 1 hour max
        min_sleep_duration: 1,           // 1ms min
    };
    registry.register(TimingHandler::new(timing_config, None));

    info!("  ✓ Registering runtime handler");
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    // Phase 2: Medium Complexity Handlers
    info!("  ✓ Registering http-client handler");
    let http_client_config = HttpClientHandlerConfig {};
    registry.register(HttpClientHandler::new(http_client_config, None));

    info!("  ✓ Registering filesystem handler");
    let filesystem_config = FileSystemHandlerConfig {
        path: None,                      // No path restrictions
        new_dir: Some(true),            // Allow creating directories
        allowed_commands: None,          // No command restrictions
    };
    registry.register(FilesystemHandler::new(filesystem_config, None));

    // Phase 3: Complex Handlers
    info!("  ✓ Registering process handler");
    let process_config = ProcessHostConfig {
        max_processes: 10,
        max_output_buffer: 1024 * 1024,  // 1MB
        allowed_programs: None,           // No restrictions
        allowed_paths: None,              // No restrictions
    };
    registry.register(ProcessHandler::new(process_config, None));

    info!("  ✓ Registering store handler");
    let store_config = StoreHandlerConfig {};
    registry.register(StoreHandler::new(store_config, None));

    info!("  ✓ Registering supervisor handler");
    let supervisor_config = SupervisorHostConfig {};
    registry.register(SupervisorHandler::new(supervisor_config, None));

    // Phase 4: Framework Handlers
    info!("  ✓ Registering message-server handler");
    let message_router = theater_handler_message_server::MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router));

    info!("  ✓ Registering http-framework handler");
    registry.register(HttpFrameworkHandler::new(None));

    info!("Successfully registered all 11 handlers! 🎉");
    info!("");
    info!("All migrated handlers are now fully integrated!");

    registry
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    info!("🎭 Theater Runtime - Migrated Handlers Example");
    info!("============================================");
    info!("");
    info!("This example demonstrates a Theater runtime with ALL migrated handlers:");
    info!("  ✓ environment     - Environment variable access");
    info!("  ✓ random          - Random value generation");
    info!("  ✓ timing          - Delays and timeouts");
    info!("  ✓ runtime         - Runtime functions (log, get-state, shutdown)");
    info!("  ✓ http-client     - HTTP request capabilities");
    info!("  ✓ filesystem      - File system operations");
    info!("  ✓ process         - OS process spawning and management");
    info!("  ✓ store           - Content-addressed storage");
    info!("  ✓ supervisor      - Actor supervision");
    info!("  ✓ message-server  - Inter-actor messaging");
    info!("  ✓ http-framework  - HTTP/HTTPS server framework");
    info!("");

    // Create communication channels
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);

    // Optional: Create channel for runtime events
    let (channel_events_tx, _channel_events_rx) = mpsc::channel(32);

    // Create handler registry with migrated handlers
    // Pass theater_tx so RuntimeHandler can send shutdown commands
    let handler_registry = create_handler_registry(theater_tx.clone());

    info!("");
    info!("Creating Theater runtime...");

    // Create the Theater runtime with ChainEvent as the event type
    let mut runtime: TheaterRuntime<ChainEvent> = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        Some(channel_events_tx),
        handler_registry,
    )
    .await?;

    info!("✓ Runtime created successfully!");
    info!("");
    info!("Runtime is ready to accept commands.");
    info!("To spawn actors, send SpawnActor commands via the theater_tx channel.");
    info!("");
    info!("Example workflow:");
    info!("  1. Create an actor manifest (manifest.toml)");
    info!("  2. Build your actor WASM component");
    info!("  3. Send SpawnActor command with manifest path");
    info!("  4. The runtime will inject handlers based on actor's WIT imports");
    info!("");
    info!("For a complete server with all handlers, see:");
    info!("  cargo run -p theater-server-cli");
    info!("");
    info!("Press Ctrl+C to shutdown...");
    info!("");

    // Run the runtime
    // In a real application, you would:
    // 1. Spawn this in a separate task
    // 2. Use theater_tx to send commands (SpawnActor, StopActor, etc.)
    // 3. Implement graceful shutdown handling

    runtime.run().await?;

    info!("Runtime shut down gracefully.");

    Ok(())
}
