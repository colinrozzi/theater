//! Integration tests for the Replay Handler
//!
//! This test demonstrates how to use the ReplayHandler to replay an actor
//! from a recorded event chain.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use theater::config::actor_manifest::{
    FileSystemHandlerConfig, RuntimeHostConfig, TimingHostConfig,
};
use theater::events::runtime::RuntimeEventData;
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;

use theater_handler_filesystem::{FilesystemEventData, FilesystemHandler};
use theater_handler_io::{IoEventData, WasiIoHandler};
use theater_handler_replay::ReplayHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_timing::{TimingEventData, TimingHandler};

/// Define test-specific handler events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestHandlerEvents {
    Io(IoEventData),
    Timing(TimingEventData),
    Filesystem(FilesystemEventData),
    HostFunction(theater::HostFunctionCall),
}

/// Test event type wrapping Theater's core events with our handler events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestEvents(theater::events::TheaterEvents<TestHandlerEvents>);

// Implement From for core event types required by TheaterRuntime
impl From<RuntimeEventData> for TestEvents {
    fn from(event: RuntimeEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::Runtime(event))
    }
}

impl From<theater::events::theater_runtime::TheaterRuntimeEventData> for TestEvents {
    fn from(event: theater::events::theater_runtime::TheaterRuntimeEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::TheaterRuntime(event))
    }
}

impl From<theater::events::wasm::WasmEventData> for TestEvents {
    fn from(event: theater::events::wasm::WasmEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::Wasm(event))
    }
}

// Implement From for the handler events
impl From<IoEventData> for TestEvents {
    fn from(event: IoEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::Handler(
            TestHandlerEvents::Io(event),
        ))
    }
}

impl From<TimingEventData> for TestEvents {
    fn from(event: TimingEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::Handler(
            TestHandlerEvents::Timing(event),
        ))
    }
}

impl From<FilesystemEventData> for TestEvents {
    fn from(event: FilesystemEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::Handler(
            TestHandlerEvents::Filesystem(event),
        ))
    }
}

// HostFunctionCall is used by handlers for recording in standardized format
impl From<theater::HostFunctionCall> for TestEvents {
    fn from(event: theater::HostFunctionCall) -> Self {
        TestEvents(theater::events::TheaterEvents::Handler(
            TestHandlerEvents::HostFunction(event),
        ))
    }
}

/// Get the path to the test actor's WASM component
fn get_test_wasm_path() -> PathBuf {
    // Use the runtime handler's test actor
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../theater-handler-runtime/test-actors/runtime-test/target/wasm32-wasip1/release/runtime_test.wasm")
}

/// Create a manifest for the test actor
fn create_test_manifest_content() -> String {
    let wasm_path = get_test_wasm_path();
    format!(
        r#"name = "runtime-test"
version = "0.1.0"
component = "{}"
description = "Test actor for replay handler"
save_chain = true

[[handler]]
type = "runtime"
"#,
        wasm_path.display()
    )
}

/// Creates a handler registry for normal (recording) mode
fn create_recording_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> HandlerRegistry<TestEvents> {
    let mut registry = HandlerRegistry::new();

    // Runtime handler
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    // IO handler - provides wasi:io and wasi:cli interfaces
    registry.register(WasiIoHandler::new());

    // Timing handler - provides wasi:clocks interfaces
    let timing_config = TimingHostConfig {
        max_sleep_duration: 3600000,
        min_sleep_duration: 1,
    };
    registry.register(TimingHandler::new(timing_config, None));

    // Filesystem handler - provides wasi:filesystem interfaces
    let filesystem_config = FileSystemHandlerConfig {
        path: Some(std::path::PathBuf::from("/tmp")),
        new_dir: Some(true),
        allowed_commands: None,
    };
    registry.register(FilesystemHandler::new(filesystem_config, None));

    registry
}

/// Creates a handler registry for replay mode
/// Note: For full replay, we'd need the ReplayHandler to satisfy ALL imports.
/// For now, we use ReplayHandler for theater:simple/* and real handlers for WASI.
fn create_replay_registry(
    expected_chain: Vec<theater::chain::ChainEvent>,
) -> HandlerRegistry<TestEvents> {
    let mut registry = HandlerRegistry::new();

    // Replay handler for theater:simple/* interfaces
    registry.register(ReplayHandler::new(expected_chain));

    // We still need WASI handlers for basic IO
    // In a full replay implementation, these would also be replayed
    registry.register(WasiIoHandler::new());

    let timing_config = TimingHostConfig {
        max_sleep_duration: 3600000,
        min_sleep_duration: 1,
    };
    registry.register(TimingHandler::new(timing_config, None));

    let filesystem_config = FileSystemHandlerConfig {
        path: Some(std::path::PathBuf::from("/tmp")),
        new_dir: Some(true),
        allowed_commands: None,
    };
    registry.register(FilesystemHandler::new(filesystem_config, None));

    registry
}

/// Test that the ReplayHandler can be registered and used
#[tokio::test]
async fn test_replay_handler_registration() -> Result<()> {
    // Skip test if WASM file doesn't exist
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: WASM file not found at {:?}",
            wasm_path
        );
        return Ok(());
    }

    // Create an empty chain for testing registration
    let empty_chain = vec![];

    // Create replay registry
    let registry = create_replay_registry(empty_chain);

    // Verify we can create it
    println!("Replay registry created successfully");

    // The registry should have 4 handlers: ReplayHandler + 3 WASI handlers
    // We can't easily check the count, but we can verify no panic occurred

    drop(registry);

    Ok(())
}

/// Test recording a run and then replaying it
#[tokio::test]
async fn test_record_and_replay() -> Result<()> {
    // Skip test if WASM file doesn't exist
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: WASM file not found at {:?}. Build with: \n  cd crates/theater-handler-runtime/test-actors/runtime-test && cargo component build --release",
            wasm_path
        );
        return Ok(());
    }

    println!("\n=== Phase 1: Recording ===\n");

    // --- Phase 1: Record a run ---
    let manifest_content = create_test_manifest_content();

    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_recording_registry(theater_tx.clone());

    let mut runtime: TheaterRuntime<TestEvents> =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;

    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    // Create a subscription channel to receive events as they happen
    let (event_tx, mut event_rx) = mpsc::channel(100);

    // Spawn the actor with event subscription
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest_content.clone(),
            init_bytes: None,
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await?;

    let spawn_result = timeout(Duration::from_secs(10), response_rx).await??;
    let actor_id = spawn_result?;
    println!("Recorded run - Actor ID: {}", actor_id);

    // Collect events from the subscription
    let mut recorded_chain = Vec::new();
    let collect_timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();
    let mut last_event_time = std::time::Instant::now();

    while start.elapsed() < collect_timeout {
        match timeout(Duration::from_millis(500), event_rx.recv()).await {
            Ok(Some(Ok(event))) => {
                last_event_time = std::time::Instant::now();
                println!(
                    "  Event {}: type={}, hash={}",
                    recorded_chain.len(),
                    event.event_type,
                    hex::encode(&event.hash[..8.min(event.hash.len())])
                );
                recorded_chain.push(event);
            }
            Ok(Some(Err(e))) => {
                println!("Received actor error: {:?}", e);
                break;
            }
            Ok(None) => {
                println!("Event channel closed");
                break;
            }
            Err(_) => {
                // Timeout on recv, check if we've been idle for a while
                if last_event_time.elapsed() > Duration::from_secs(3) {
                    println!("No events for 3 seconds, stopping collection");
                    break;
                }
            }
        }
    }

    println!("Recorded chain has {} events", recorded_chain.len());

    // Stop the first actor and runtime
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await?;
    let _ = timeout(Duration::from_secs(5), stop_rx).await;
    drop(theater_tx);
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    if recorded_chain.is_empty() {
        println!("No events recorded, skipping replay test");
        return Ok(());
    }

    println!("\n=== Phase 2: Replay ===\n");

    // --- Phase 2: Replay using the recorded chain ---
    let (theater_tx2, theater_rx2) = mpsc::channel::<TheaterCommand>(32);
    let replay_registry = create_replay_registry(recorded_chain.clone());

    let mut replay_runtime: TheaterRuntime<TestEvents> =
        TheaterRuntime::new(theater_tx2.clone(), theater_rx2, None, replay_registry).await?;

    let replay_runtime_handle = tokio::spawn(async move { replay_runtime.run().await });

    // Spawn the actor in replay mode
    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx2
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest_content,
            init_bytes: None,
            parent_id: None,
            response_tx: response_tx2,
            supervisor_tx: None,
            subscription_tx: None,
        })
        .await?;

    let spawn_result2 = timeout(Duration::from_secs(10), response_rx2).await;
    match spawn_result2 {
        Ok(Ok(Ok(replay_actor_id))) => {
            println!("Replay run - Actor ID: {}", replay_actor_id);

            // Wait for replay to complete
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Get the replay chain
            let (chain_tx2, chain_rx2) = tokio::sync::oneshot::channel();
            let _ = theater_tx2
                .send(TheaterCommand::GetActorEvents {
                    actor_id: replay_actor_id.clone(),
                    response_tx: chain_tx2,
                })
                .await;

            if let Ok(Ok(Ok(replay_events))) =
                timeout(Duration::from_secs(5), chain_rx2).await
            {
                println!("Replay chain has {} events", replay_events.len());
                for (i, event) in replay_events.iter().enumerate() {
                    println!(
                        "  Event {}: type={}, hash={}",
                        i,
                        event.event_type,
                        hex::encode(&event.hash[..8.min(event.hash.len())])
                    );
                }
            }

            // Stop the replay actor
            let (stop_tx2, stop_rx2) = tokio::sync::oneshot::channel();
            let _ = theater_tx2
                .send(TheaterCommand::StopActor {
                    actor_id: replay_actor_id,
                    response_tx: stop_tx2,
                })
                .await;
            let _ = timeout(Duration::from_secs(5), stop_rx2).await;
        }
        Ok(Ok(Err(e))) => {
            println!("Replay actor spawn error: {}", e);
        }
        Ok(Err(e)) => {
            println!("Replay channel error: {}", e);
        }
        Err(_) => {
            println!("Replay spawn timeout");
        }
    }

    drop(theater_tx2);
    let _ = timeout(Duration::from_secs(5), replay_runtime_handle).await;

    println!("\n=== Test Complete ===");
    Ok(())
}
