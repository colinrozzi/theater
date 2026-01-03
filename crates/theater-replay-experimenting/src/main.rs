//! Replay Experimenting
//!
//! This crate experiments with the replay functionality in Theater.
//! It demonstrates recording an actor run and then replaying it.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use theater::config::actor_manifest::RuntimeHostConfig;
use theater::events::runtime::RuntimeEventData;
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater::ReplayHandler;

use theater_handler_runtime::RuntimeHandler;

/// No custom handler events needed for runtime-test actor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestHandlerEvents {}

/// Test event type wrapping Theater's core events
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

/// Get the path to the test actor's WASM component
fn get_test_wasm_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Use the runtime-test actor which uses theater:simple/runtime
    // Built with cargo component, target is wasm32-unknown-unknown
    manifest_dir.join("../theater-handler-runtime/test-actors/runtime-test/target/wasm32-unknown-unknown/release/runtime_test.wasm")
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

    // Runtime handler - provides theater:simple/runtime interface
    // This is the only handler needed for the runtime-test actor
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    registry
}

/// Creates a handler registry for replay mode
fn create_replay_registry(
    expected_chain: Vec<theater::chain::ChainEvent>,
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> HandlerRegistry<TestEvents> {
    let mut registry = HandlerRegistry::new();

    // Replay handler - intercepts all imports and returns recorded outputs
    // Must be registered FIRST so it intercepts imports before RuntimeHandler
    registry.register(ReplayHandler::new(expected_chain));

    // RuntimeHandler is still needed to register export functions like
    // theater:simple/actor.init - the ReplayHandler intercepts the imports
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    registry
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("theater_replay_experimenting=info".parse()?)
                .add_directive("theater=info".parse()?),
        )
        .init();

    // Check for WASM file
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        eprintln!("WASM file not found at {:?}", wasm_path);
        eprintln!("Build with: cd crates/theater-handler-runtime/test-actors/runtime-test && cargo component build --release");
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

    // Create a subscription channel to receive events
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
                if last_event_time.elapsed() > Duration::from_secs(3) {
                    println!("No events for 3 seconds, stopping collection");
                    break;
                }
            }
        }
    }

    println!("\nRecorded chain has {} events", recorded_chain.len());

    // Save chain to file for later use
    let chain_json = serde_json::to_string_pretty(&recorded_chain)?;
    let chain_path = "/tmp/recorded_chain.json";
    std::fs::write(chain_path, &chain_json)?;
    println!("Saved chain to {}", chain_path);

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
    let replay_registry = create_replay_registry(recorded_chain.clone(), theater_tx2.clone());

    let mut replay_runtime: TheaterRuntime<TestEvents> =
        TheaterRuntime::new(theater_tx2.clone(), theater_rx2, None, replay_registry).await?;

    let replay_runtime_handle = tokio::spawn(async move { replay_runtime.run().await });

    // Create subscription for replay events
    let (replay_event_tx, mut replay_event_rx) = mpsc::channel(100);

    // Spawn the actor in replay mode
    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx2
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest_content,
            init_bytes: None,
            parent_id: None,
            response_tx: response_tx2,
            supervisor_tx: None,
            subscription_tx: Some(replay_event_tx),
        })
        .await?;

    let spawn_result2 = timeout(Duration::from_secs(10), response_rx2).await;
    match spawn_result2 {
        Ok(Ok(Ok(replay_actor_id))) => {
            println!("Replay run - Actor ID: {}", replay_actor_id);

            // Collect replay events
            let mut replay_chain = Vec::new();
            let start = std::time::Instant::now();
            let mut last_event_time = std::time::Instant::now();

            while start.elapsed() < Duration::from_secs(10) {
                match timeout(Duration::from_millis(500), replay_event_rx.recv()).await {
                    Ok(Some(Ok(event))) => {
                        last_event_time = std::time::Instant::now();
                        println!(
                            "  Replay Event {}: type={}, hash={}",
                            replay_chain.len(),
                            event.event_type,
                            hex::encode(&event.hash[..8.min(event.hash.len())])
                        );
                        replay_chain.push(event);
                    }
                    Ok(Some(Err(e))) => {
                        println!("Replay error: {:?}", e);
                        break;
                    }
                    Ok(None) => {
                        println!("Replay channel closed");
                        break;
                    }
                    Err(_) => {
                        if last_event_time.elapsed() > Duration::from_secs(3) {
                            println!("No replay events for 3 seconds, stopping");
                            break;
                        }
                    }
                }
            }

            println!("\nReplay chain has {} events", replay_chain.len());

            // Compare chains
            println!("\n=== Comparison ===\n");
            println!("Original events: {}", recorded_chain.len());
            println!("Replay events:   {}", replay_chain.len());

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

    println!("\n=== Experiment Complete ===");
    Ok(())
}
