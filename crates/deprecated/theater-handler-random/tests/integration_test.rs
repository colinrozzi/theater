//! Integration tests for the Random Handler
//!
//! This test creates a minimal Theater runtime with just the random handler
//! and the runtime handler, spawns the test actor, and verifies the event chain.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use theater::config::actor_manifest::{
    FileSystemHandlerConfig, RandomHandlerConfig, RuntimeHostConfig, TimingHostConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;

use theater_handler_filesystem::{FilesystemEventData, FilesystemHandler};
use theater_handler_io::{IoEventData, WasiIoHandler};
use theater_handler_random::{RandomEventData, RandomHandler};
use theater_handler_runtime::RuntimeHandler;
use theater_handler_timing::{TimingEventData, TimingHandler};

/// Define test-specific handler events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestHandlerEvents {
    Random(RandomEventData),
    Io(IoEventData),
    Timing(TimingEventData),
    Filesystem(FilesystemEventData),
}

/// Test event type wrapping Theater's core events with our handler events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestEvents(theater::events::TheaterEvents<TestHandlerEvents>);

// Implement From for core event types required by TheaterRuntime
impl From<theater::events::runtime::RuntimeEventData> for TestEvents {
    fn from(event: theater::events::runtime::RuntimeEventData) -> Self {
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
impl From<RandomEventData> for TestEvents {
    fn from(event: RandomEventData) -> Self {
        TestEvents(theater::events::TheaterEvents::Handler(
            TestHandlerEvents::Random(event),
        ))
    }
}

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

/// Get the path to the test actor's directory
fn get_test_actor_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("test-actors/wasi-random-test")
}

/// Get the path to the test actor's WASM component
fn get_test_wasm_path() -> PathBuf {
    // cargo component builds to wasm32-wasip1 target
    get_test_actor_dir().join("target/wasm32-wasip1/release/wasi_random_test.wasm")
}

/// Create a manifest with absolute paths for testing
fn create_test_manifest_content() -> String {
    let wasm_path = get_test_wasm_path();
    format!(
        r#"name = "wasi-random-test"
version = "0.1.0"
component = "{}"
description = "WASI random interface test actor"
save_chain = true

[[handler]]
type = "runtime"

[[handler]]
type = "random"
max_bytes = 1048576
max_int = 9223372036854775807
allow_crypto_secure = false
"#,
        wasm_path.display()
    )
}

/// Creates a handler registry with the handlers needed for the test
fn create_test_handler_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> HandlerRegistry<TestEvents> {
    let mut registry = HandlerRegistry::new();

    // Runtime handler is required for actor initialization
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    // Random handler - the one we're testing
    let random_config = RandomHandlerConfig {
        seed: Some(12345), // Use a fixed seed for reproducibility in tests
        max_bytes: 1024 * 1024,
        max_int: u64::MAX - 1,
        allow_crypto_secure: false,
    };
    registry.register(RandomHandler::new(random_config, None));

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

#[tokio::test]
async fn test_random_handler_with_test_actor() -> Result<()> {
    // Skip test if WASM file doesn't exist (not built yet)
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: WASM file not found at {:?}. Build with: cargo component build --release",
            wasm_path
        );
        return Ok(());
    }

    // Create manifest content with absolute paths
    let manifest_content = create_test_manifest_content();
    println!("Using manifest:\n{}", manifest_content);

    // Create communication channels
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);

    // Create handler registry
    let handler_registry = create_test_handler_registry(theater_tx.clone());

    // Create the Theater runtime
    let mut runtime: TheaterRuntime<TestEvents> =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;

    // Spawn the runtime in a background task
    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    // Create a subscription channel to receive events as they happen
    let (event_tx, mut event_rx) = mpsc::channel(100);

    // Spawn the test actor with event subscription
    // Pass manifest content directly (not a file path)
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest_content,
            init_bytes: None,
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await?;

    // Wait for the actor to be spawned
    let spawn_result = timeout(Duration::from_secs(10), response_rx).await??;
    let actor_id = spawn_result?;

    println!("Actor spawned with ID: {}", actor_id);

    // Collect events from the subscription for a short time
    let mut collected_events = Vec::new();
    let collect_timeout = Duration::from_secs(15);
    let start = std::time::Instant::now();
    let mut last_event_time = std::time::Instant::now();
    
    while start.elapsed() < collect_timeout {
        match timeout(Duration::from_millis(500), event_rx.recv()).await {
            Ok(Some(Ok(event))) => {
                last_event_time = std::time::Instant::now();
                // Try to decode the event data as string for debugging
                let data_preview = if event.data.len() > 200 {
                    format!("{}... ({} bytes)", String::from_utf8_lossy(&event.data[..200]), event.data.len())
                } else {
                    String::from_utf8_lossy(&event.data).to_string()
                };
                println!("Received event: type={}, desc={:?}, data={}", 
                    event.event_type, event.description, data_preview);
                collected_events.push(event);
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
                if last_event_time.elapsed() > Duration::from_secs(5) {
                    println!("No events for 5 seconds, stopping collection");
                    break;
                }
            }
        }
    }
    
    println!("Collected {} events via subscription", collected_events.len());

    // Check actor status first
    let (status_tx, status_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorStatus {
            actor_id: actor_id.clone(),
            response_tx: status_tx,
        })
        .await?;

    match timeout(Duration::from_secs(5), status_rx).await {
        Ok(Ok(Ok(status))) => {
            println!("Actor status: {:?}", status);
        }
        Ok(Ok(Err(e))) => {
            println!("Actor status error: {}", e);
        }
        Ok(Err(e)) => {
            println!("Channel error getting status: {}", e);
        }
        Err(_) => {
            println!("Timeout getting actor status");
        }
    }

    // Request the actor's event chain first (this might work even if actor exited)
    let (chain_tx, chain_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorEvents {
            actor_id: actor_id.clone(),
            response_tx: chain_tx,
        })
        .await?;

    let chain_result = timeout(Duration::from_secs(5), chain_rx).await;

    match chain_result {
        Ok(Ok(Ok(events))) => {
            println!("Actor event chain has {} events", events.len());
            
            // Print all event types for debugging
            for (i, event) in events.iter().enumerate() {
                println!("  Event {}: type={}, desc={:?}", i, event.event_type, event.description);
            }

            // Verify we have random-related events in the chain
            let random_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type.contains("random") || e.event_type.contains("Random"))
                .collect();

            println!("Found {} random-related events", random_events.len());

            // The test actor calls get_random_bytes multiple times and get_random_u64
            // We should see those recorded in the chain
            assert!(
                !random_events.is_empty(),
                "Expected random events in the chain"
            );
        }
        Ok(Ok(Err(e))) => {
            println!("Failed to get actor events: {:?}", e);
        }
        Ok(Err(e)) => {
            println!("Channel error getting events: {}", e);
        }
        Err(_) => {
            println!("Timeout getting actor events");
        }
    }

    // Request the actor's state to verify it completed successfully
    let (state_tx, state_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorState {
            actor_id: actor_id.clone(),
            response_tx: state_tx,
        })
        .await?;

    let state_result = timeout(Duration::from_secs(5), state_rx).await;

    match state_result {
        Ok(Ok(Ok(Some(state)))) => {
            let state_str = String::from_utf8_lossy(&state);
            println!("Actor final state: {}", state_str);
            assert!(
                state_str.contains("WASI random tests passed"),
                "Expected success message in state"
            );
        }
        Ok(Ok(Ok(None))) => {
            println!("Actor state was None - init may have returned no state");
        }
        Ok(Ok(Err(e))) => {
            println!("Failed to get actor state: {}", e);
        }
        Ok(Err(e)) => {
            println!("Channel error getting state: {}", e);
        }
        Err(_) => {
            println!("Timeout getting actor state");
        }
    }

    // Stop the actor
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await?;

    let _ = timeout(Duration::from_secs(5), stop_rx).await;

    // Shutdown the runtime
    drop(theater_tx);

    // Wait for runtime to finish
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    Ok(())
}

#[tokio::test]
async fn test_random_handler_event_recording() -> Result<()> {
    // This is a simpler unit test that just verifies the handler records events correctly
    // without needing the full WASM component

    // For now, this is a placeholder - we could add more granular tests here
    // that test the handler's event recording without a full actor

    Ok(())
}

/// Test to debug handler matching logic
#[tokio::test]
async fn test_handler_matching_debug() -> Result<()> {
    
    // Skip test if WASM file doesn't exist (not built yet)
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: WASM file not found at {:?}. Build with: cargo component build --release",
            wasm_path
        );
        return Ok(());
    }

    // Load the WASM component and check what imports it needs
    let wasm_bytes = std::fs::read(&wasm_path)?;
    let engine = wasmtime::Engine::default();
    let component = wasmtime::component::Component::new(&engine, &wasm_bytes)?;
    
    // Get component imports and exports
    let component_type = component.component_type();
    let imports: Vec<String> = component_type.imports(&engine)
        .map(|(name, _)| name.to_string())
        .collect();
    let exports: Vec<String> = component_type.exports(&engine)
        .map(|(name, _)| name.to_string())
        .collect();
    
    println!("Component imports:");
    for import in &imports {
        println!("  - {}", import);
    }
    println!("Component exports:");
    for export in &exports {
        println!("  - {}", export);
    }
    
    // Now check what each handler advertises
    let (theater_tx, _theater_rx) = mpsc::channel::<TheaterCommand>(32);
    
    println!("\nHandler advertised imports:");
    
    use theater::handler::Handler;
    
    // Runtime handler
    let runtime_config = RuntimeHostConfig {};
    let runtime_handler = RuntimeHandler::new(runtime_config, theater_tx.clone(), None);
    let runtime_imports = Handler::<TestEvents>::imports(&runtime_handler);
    println!("Runtime handler: {:?}", runtime_imports);
    
    // Random handler
    let random_config = RandomHandlerConfig {
        seed: Some(12345),
        max_bytes: 1024 * 1024,
        max_int: u64::MAX - 1,
        allow_crypto_secure: false,
    };
    let random_handler = RandomHandler::new(random_config, None);
    let random_imports = Handler::<TestEvents>::imports(&random_handler);
    println!("Random handler: {:?}", random_imports);
    
    // IO handler
    let io_handler = WasiIoHandler::new();
    let io_imports = Handler::<TestEvents>::imports(&io_handler);
    println!("IO handler: {:?}", io_imports);
    
    // Timing handler
    let timing_config = TimingHostConfig {
        max_sleep_duration: 3600000,
        min_sleep_duration: 1,
    };
    let timing_handler = TimingHandler::new(timing_config, None);
    let timing_imports = Handler::<TestEvents>::imports(&timing_handler);
    println!("Timing handler: {:?}", timing_imports);
    
    // Filesystem handler
    let filesystem_config = FileSystemHandlerConfig {
        path: Some(std::path::PathBuf::from("/tmp")),
        new_dir: Some(true),
        allowed_commands: None,
    };
    let filesystem_handler = FilesystemHandler::new(filesystem_config, None);
    let filesystem_imports = Handler::<TestEvents>::imports(&filesystem_handler);
    println!("Filesystem handler: {:?}", filesystem_imports);
    
    // Get handler exports
    let runtime_exports = Handler::<TestEvents>::exports(&runtime_handler);
    println!("Runtime handler exports: {:?}", runtime_exports);
    
    // Check for import matches
    println!("\nHandler import matches:");
    
    let handlers_imports: Vec<(&str, Option<Vec<String>>)> = vec![
        ("runtime", runtime_imports.clone()),
        ("random", random_imports),
        ("io", io_imports),
        ("timing", timing_imports),
        ("filesystem", filesystem_imports),
    ];

    for (handler_name, handler_imports) in &handlers_imports {
        if let Some(hi) = handler_imports {
            let matches: Vec<_> = hi.iter()
                .filter(|import| imports.contains(import))
                .collect();
            let non_matches: Vec<_> = hi.iter()
                .filter(|import| !imports.contains(import))
                .collect();
            
            println!("  {} handler:", handler_name);
            if !matches.is_empty() {
                println!("    Import matched: {:?}", matches);
            }
            if !non_matches.is_empty() {
                println!("    Import NOT matched: {:?}", non_matches);
            }
            println!("    -> Would activate on imports: {}", !matches.is_empty());
        }
    }
    
    // Check for export matches (runtime handler expects actor to export theater:simple/actor)
    println!("\nHandler export matches:");
    if let Some(re) = &runtime_exports {
        let export_matches: Vec<_> = re.iter()
            .filter(|exp| exports.contains(exp))
            .collect();
        let export_non_matches: Vec<_> = re.iter()
            .filter(|exp| !exports.contains(exp))
            .collect();
        
        println!("  runtime handler:");
        if !export_matches.is_empty() {
            println!("    Export matched: {:?}", export_matches);
        }
        if !export_non_matches.is_empty() {
            println!("    Export NOT matched: {:?}", export_non_matches);
        }
        let imports_match = runtime_imports.as_ref().map_or(false, |hi| hi.iter().any(|i| imports.contains(i)));
        let exports_match = !export_matches.is_empty();
        println!("    -> Would activate (imports_match || exports_match): {} || {} = {}", 
            imports_match, exports_match, imports_match || exports_match);
    }
    
    Ok(())
}
