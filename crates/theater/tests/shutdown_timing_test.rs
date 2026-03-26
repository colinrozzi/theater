//! Integration test for actor shutdown timing.
//!
//! This test verifies that actors shut down quickly (within 2 seconds),
//! not hitting the 10-second timeout that indicates a bug in shutdown handling.
//!
//! The test:
//! 1. Creates a TheaterRuntime
//! 2. Spawns a minimal actor
//! 3. Stops the actor
//! 4. Asserts shutdown completes within 2 seconds

use std::time::{Duration, Instant};
use theater::config::actor_manifest::{
    HandlerConfig, ManifestConfig, RuntimeHostConfig, StoreHandlerConfig, SupervisorHostConfig,
    TcpHandlerConfig, TimerHandlerConfig,
};
use theater::config::inheritance::HandlerPermissionPolicy;
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater_handler_loop::LoopHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_rpc::RpcHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use theater_handler_tcp::TcpHandler;
use theater_handler_timer::TimerHandler;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

/// Maximum acceptable shutdown time - if it takes longer, we have a bug
const MAX_SHUTDOWN_TIME: Duration = Duration::from_secs(2);

/// Timeout for the entire test
const TEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Helper to create a minimal manifest config for testing
fn create_test_manifest(name: &str, wasm_path: &str) -> ManifestConfig {
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: wasm_path.to_string(),
        description: None,
        long_description: None,
        initial_state: None,
        save_chain: None,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![HandlerConfig::Runtime {
            config: RuntimeHostConfig {},
        }],
    }
}

#[tokio::test]
async fn test_actor_shutdown_timing() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,theater=debug")
        .try_init();

    // Set up THEATER_HOME for any handlers that need it
    let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
    std::env::set_var("THEATER_HOME", temp_dir.path());

    // Load the shutdown-test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/shutdown-test/target/wasm32-unknown-unknown/release/shutdown_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read shutdown-test WASM from {}: {}. \n\
                 Build it first with: \n\
                 cd test-actors/shutdown-test && cargo build --release",
                wasm_path, e
            );
        }
    };

    info!("Loaded shutdown-test WASM: {} bytes", wasm_bytes.len());

    // Create theater runtime channels
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let theater_tx_clone = theater_tx.clone();

    // Create handler registry with runtime handler
    let mut handler_registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    handler_registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    // Create and start the theater runtime
    let runtime_handle = tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            theater_tx_clone,
            theater_rx,
            None,
            handler_registry,
        )
        .await
        .expect("Failed to create runtime");

        runtime.run().await
    });

    // Give runtime a moment to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create manifest for the test actor
    let manifest = create_test_manifest("shutdown-test", wasm_path);

    // Spawn the actor
    info!("=== Spawning actor ===");
    let (spawn_tx, spawn_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: wasm_bytes.clone(),
            name: Some("shutdown-test".to_string()),
            manifest: Some(manifest),
                            init_bytes: None,
            response_tx: spawn_tx,
            supervisor_tx: None,
            subscription_tx: None,
        })
        .await
        .expect("Failed to send spawn command");

    let actor_id = tokio::time::timeout(Duration::from_secs(5), spawn_rx)
        .await
        .expect("Timeout waiting for spawn response")
        .expect("Spawn channel closed")
        .expect("Failed to spawn actor");

    info!("Actor spawned: {:?}", actor_id);

    // Give the actor a moment to fully initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now stop the actor and measure how long it takes
    info!("=== Stopping actor ===");
    let (stop_tx, stop_rx) = oneshot::channel();
    let stop_start = Instant::now();

    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await
        .expect("Failed to send stop command");

    // Wait for stop to complete
    let stop_result = tokio::time::timeout(TEST_TIMEOUT, stop_rx)
        .await
        .expect("Test timeout - actor shutdown took too long")
        .expect("Stop channel closed");

    let shutdown_duration = stop_start.elapsed();

    info!(
        "Actor shutdown completed in {:?} (result: {:?})",
        shutdown_duration, stop_result
    );

    // Assert shutdown was fast
    assert!(
        shutdown_duration < MAX_SHUTDOWN_TIME,
        "Actor shutdown took {:?}, which exceeds the maximum acceptable time of {:?}. \
         This indicates a bug in shutdown handling - likely a handler not responding to shutdown signals.",
        shutdown_duration,
        MAX_SHUTDOWN_TIME
    );

    info!("=== Shutdown timing test PASSED ===");
    info!(
        "Shutdown completed in {:?} (limit: {:?})",
        shutdown_duration, MAX_SHUTDOWN_TIME
    );

    // Clean up - tell runtime to stop
    drop(theater_tx);

    // Wait for runtime to exit
    let _ = tokio::time::timeout(Duration::from_secs(2), runtime_handle).await;
}

#[tokio::test]
async fn test_multiple_actor_shutdown_timing() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,theater=debug")
        .try_init();

    // Set up THEATER_HOME
    let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
    std::env::set_var("THEATER_HOME", temp_dir.path());

    // Load the shutdown-test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/shutdown-test/target/wasm32-unknown-unknown/release/shutdown_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read shutdown-test WASM from {}: {}",
                wasm_path, e
            );
        }
    };

    // Create theater runtime
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let theater_tx_clone = theater_tx.clone();
    let mut handler_registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    handler_registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    let runtime_handle = tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            theater_tx_clone,
            theater_rx,
            None,
            handler_registry,
        )
        .await
        .expect("Failed to create runtime");

        runtime.run().await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Spawn multiple actors
    let num_actors = 3;
    let mut actor_ids = Vec::new();

    for i in 0..num_actors {
        let manifest = create_test_manifest(&format!("shutdown-test-{}", i), wasm_path);

        let (spawn_tx, spawn_rx) = oneshot::channel();
        theater_tx
            .send(TheaterCommand::SpawnActor {
                wasm_bytes: wasm_bytes.clone(),
                name: Some(format!("shutdown-test-{}", i)),
                manifest: Some(manifest),
                            init_bytes: None,
                response_tx: spawn_tx,
                supervisor_tx: None,
                subscription_tx: None,
            })
            .await
            .expect("Failed to send spawn command");

        let actor_id = tokio::time::timeout(Duration::from_secs(5), spawn_rx)
            .await
            .expect("Timeout waiting for spawn")
            .expect("Channel closed")
            .expect("Failed to spawn");

        info!("Spawned actor {}: {:?}", i, actor_id);
        actor_ids.push(actor_id);
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Stop all actors and measure total time
    info!("=== Stopping {} actors ===", num_actors);
    let total_start = Instant::now();

    for (i, actor_id) in actor_ids.iter().enumerate() {
        let (stop_tx, stop_rx) = oneshot::channel();
        let stop_start = Instant::now();

        theater_tx
            .send(TheaterCommand::StopActor {
                actor_id: actor_id.clone(),
                response_tx: stop_tx,
            })
            .await
            .expect("Failed to send stop command");

        let _ = tokio::time::timeout(TEST_TIMEOUT, stop_rx)
            .await
            .expect("Timeout stopping actor")
            .expect("Channel closed");

        let duration = stop_start.elapsed();
        info!("Actor {} shutdown in {:?}", i, duration);

        assert!(
            duration < MAX_SHUTDOWN_TIME,
            "Actor {} shutdown took {:?}, exceeds {:?}",
            i,
            duration,
            MAX_SHUTDOWN_TIME
        );
    }

    let total_duration = total_start.elapsed();
    info!(
        "=== All {} actors stopped in {:?} ===",
        num_actors, total_duration
    );

    // Total time should be reasonable (not num_actors * 10 seconds)
    let max_total = MAX_SHUTDOWN_TIME * (num_actors as u32) + Duration::from_secs(1);
    assert!(
        total_duration < max_total,
        "Total shutdown time {:?} exceeds expected {:?}",
        total_duration,
        max_total
    );

    drop(theater_tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), runtime_handle).await;
}

/// Test shutdown timing with supervisor handler registered.
/// The supervisor handler has more complex shutdown handling including
/// cloned instances that need to properly respond to shutdown signals.
#[tokio::test]
async fn test_actor_shutdown_with_supervisor_handler() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,theater=debug")
        .try_init();

    // Set up THEATER_HOME
    let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
    std::env::set_var("THEATER_HOME", temp_dir.path());

    // Load the shutdown-test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/shutdown-test/target/wasm32-unknown-unknown/release/shutdown_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read shutdown-test WASM from {}: {}",
                wasm_path, e
            );
        }
    };

    info!("Loaded shutdown-test WASM: {} bytes", wasm_bytes.len());

    // Create theater runtime channels
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let theater_tx_clone = theater_tx.clone();

    // Create handler registry with runtime AND supervisor handlers
    let mut handler_registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    handler_registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

    // Add supervisor handler - this was identified as potentially problematic
    let supervisor_config = SupervisorHostConfig {};
    handler_registry.register(SupervisorHandler::new(supervisor_config, None));

    // Create and start the theater runtime
    let runtime_handle = tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            theater_tx_clone,
            theater_rx,
            None,
            handler_registry,
        )
        .await
        .expect("Failed to create runtime");

        runtime.run().await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create manifest with both runtime and supervisor handlers
    let manifest = ManifestConfig {
        name: "shutdown-test-supervisor".to_string(),
        version: "0.1.0".to_string(),
        package: wasm_path.to_string(),
        description: None,
        long_description: None,
        initial_state: None,
        save_chain: None,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![
            HandlerConfig::Runtime {
                config: RuntimeHostConfig {},
            },
            HandlerConfig::Supervisor {
                config: SupervisorHostConfig {},
            },
        ],
    };

    // Spawn the actor
    info!("=== Spawning actor with supervisor handler ===");
    let (spawn_tx, spawn_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: wasm_bytes.clone(),
            name: Some("shutdown-test-supervisor".to_string()),
            manifest: Some(manifest),
                            init_bytes: None,
            response_tx: spawn_tx,
            supervisor_tx: None,
            subscription_tx: None,
        })
        .await
        .expect("Failed to send spawn command");

    let actor_id = tokio::time::timeout(Duration::from_secs(5), spawn_rx)
        .await
        .expect("Timeout waiting for spawn response")
        .expect("Spawn channel closed")
        .expect("Failed to spawn actor");

    info!("Actor spawned: {:?}", actor_id);

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Now stop the actor and measure how long it takes
    info!("=== Stopping actor with supervisor handler ===");
    let (stop_tx, stop_rx) = oneshot::channel();
    let stop_start = Instant::now();

    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await
        .expect("Failed to send stop command");

    let stop_result = tokio::time::timeout(TEST_TIMEOUT, stop_rx)
        .await
        .expect("Test timeout - actor shutdown took too long")
        .expect("Stop channel closed");

    let shutdown_duration = stop_start.elapsed();

    info!(
        "Actor with supervisor shutdown completed in {:?} (result: {:?})",
        shutdown_duration, stop_result
    );

    assert!(
        shutdown_duration < MAX_SHUTDOWN_TIME,
        "Actor shutdown with supervisor handler took {:?}, exceeds {:?}. \
         This confirms the supervisor handler shutdown bug.",
        shutdown_duration,
        MAX_SHUTDOWN_TIME
    );

    info!("=== Supervisor handler shutdown test PASSED ===");

    drop(theater_tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), runtime_handle).await;
}

/// Test shutdown timing with ALL handlers registered.
/// This is a comprehensive test to ensure all handlers properly respond to shutdown.
#[tokio::test]
async fn test_actor_shutdown_with_all_handlers() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,theater=debug")
        .try_init();

    // Set up THEATER_HOME
    let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
    std::env::set_var("THEATER_HOME", temp_dir.path());

    // Load the shutdown-test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/shutdown-test/target/wasm32-unknown-unknown/release/shutdown_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read shutdown-test WASM from {}: {}",
                wasm_path, e
            );
        }
    };

    info!("Loaded shutdown-test WASM: {} bytes", wasm_bytes.len());

    // Create theater runtime channels
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let theater_tx_clone = theater_tx.clone();

    // Create handler registry with ALL handlers
    let mut handler_registry = HandlerRegistry::new();

    // Runtime handler
    handler_registry.register(RuntimeHandler::new(
        RuntimeHostConfig {},
        theater_tx.clone(),
        None,
    ));

    // Store handler
    handler_registry.register(StoreHandler::new(StoreHandlerConfig::default(), None));

    // Supervisor handler
    handler_registry.register(SupervisorHandler::new(SupervisorHostConfig {}, None));

    // Message server handler
    let message_router = MessageRouter::new();
    handler_registry.register(MessageServerHandler::new(None, message_router));

    // RPC handler
    handler_registry.register(RpcHandler::new(theater_tx.clone()));

    // TCP handler
    handler_registry.register(TcpHandler::new(TcpHandlerConfig::default()));

    // Timer handler
    handler_registry.register(TimerHandler::new(TimerHandlerConfig::default()));

    // Loop handler
    handler_registry.register(LoopHandler::new());

    info!("Registered 8 handlers");

    // Create and start the theater runtime
    let runtime_handle = tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            theater_tx_clone,
            theater_rx,
            None,
            handler_registry,
        )
        .await
        .expect("Failed to create runtime");

        runtime.run().await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Create manifest with runtime handler only (the actor only needs runtime)
    // but all handlers are available in the registry
    let manifest = create_test_manifest("shutdown-test-all-handlers", wasm_path);

    // Spawn the actor
    info!("=== Spawning actor with all handlers available ===");
    let (spawn_tx, spawn_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: wasm_bytes.clone(),
            name: Some("shutdown-test-all-handlers".to_string()),
            manifest: Some(manifest),
                            init_bytes: None,
            response_tx: spawn_tx,
            supervisor_tx: None,
            subscription_tx: None,
        })
        .await
        .expect("Failed to send spawn command");

    let actor_id = tokio::time::timeout(Duration::from_secs(5), spawn_rx)
        .await
        .expect("Timeout waiting for spawn response")
        .expect("Spawn channel closed")
        .expect("Failed to spawn actor");

    info!("Actor spawned: {:?}", actor_id);

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Stop the actor and measure
    info!("=== Stopping actor with all handlers ===");
    let (stop_tx, stop_rx) = oneshot::channel();
    let stop_start = Instant::now();

    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await
        .expect("Failed to send stop command");

    let stop_result = tokio::time::timeout(TEST_TIMEOUT, stop_rx)
        .await
        .expect("Test timeout - actor shutdown took too long")
        .expect("Stop channel closed");

    let shutdown_duration = stop_start.elapsed();

    info!(
        "Actor with all handlers shutdown completed in {:?} (result: {:?})",
        shutdown_duration, stop_result
    );

    assert!(
        shutdown_duration < MAX_SHUTDOWN_TIME,
        "Actor shutdown with all handlers took {:?}, exceeds {:?}",
        shutdown_duration,
        MAX_SHUTDOWN_TIME
    );

    info!("=== All handlers shutdown test PASSED ===");

    drop(theater_tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), runtime_handle).await;
}
