//! Replay Experimenting
//!
//! This crate experiments with the replay functionality in Theater.
//! It demonstrates recording an actor run and then replaying it using
//! manifest-based configuration.
//!
//! ## Usage
//!
//! ```bash
//! # Build the test actor first
//! cd test-actors/replay-test && cargo build --release
//!
//! # Run the replay experiment
//! cargo run -p theater-replay-experimenting
//!
//! # Run as a test
//! cargo test -p theater-replay-experimenting
//! ```

use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use theater::chain::ChainEvent;
use theater::config::actor_manifest::{RuntimeHostConfig, SupervisorHostConfig};
use theater::handler::HandlerRegistry;
use theater::messages::{ActorMessage, ActorRequest, ActorSend, MessageCommand, TheaterCommand};
use theater::pack_bridge::Value;
use theater::theater_runtime::TheaterRuntime;
use theater::utils::resolve_reference;
use theater::ActorError;
use theater::ManifestConfig;

use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_runtime::RuntimeHandler;
use theater_handler_supervisor::SupervisorHandler;
use theater_handler_tcp::TcpHandler;

/// Result of replay verification
#[derive(Debug)]
pub struct ReplayVerificationResult {
    /// Original chain events from recording
    pub original_chain: Vec<ChainEvent>,
    /// Replay chain events
    pub replay_chain: Vec<ChainEvent>,
    /// Number of hash mismatches
    pub mismatches: usize,
    /// Whether verification passed
    pub passed: bool,
}

impl ReplayVerificationResult {
    /// Check if the chains have the same length
    pub fn same_length(&self) -> bool {
        self.original_chain.len() == self.replay_chain.len()
    }

    /// Get a detailed comparison of the chains
    pub fn comparison_details(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "Original events: {}\nReplay events: {}\n",
            self.original_chain.len(),
            self.replay_chain.len()
        ));

        let max_len = self.original_chain.len().max(self.replay_chain.len());
        for i in 0..max_len {
            let orig_hash = self
                .original_chain
                .get(i)
                .map(|e| hex::encode(&e.hash))
                .unwrap_or_else(|| "-".to_string());
            let replay_hash = self
                .replay_chain
                .get(i)
                .map(|e| hex::encode(&e.hash))
                .unwrap_or_else(|| "-".to_string());
            let event_type = self
                .original_chain
                .get(i)
                .map(|e| e.event_type.clone())
                .or_else(|| self.replay_chain.get(i).map(|e| e.event_type.clone()))
                .unwrap_or_else(|| "-".to_string());

            let matches = orig_hash == replay_hash;
            let indicator = if matches { "✓" } else { "✗" };

            output.push_str(&format!(
                "{}: {} vs {} [{}] {}\n",
                i,
                &orig_hash[..orig_hash.len().min(16)],
                &replay_hash[..replay_hash.len().min(16)],
                event_type,
                indicator
            ));
        }

        output
    }
}

/// Get the path to the test actor's WASM package (Pack/Graph ABI)
pub fn get_test_wasm_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../test-actors/replay-test/target/wasm32-unknown-unknown/release/replay_test_actor.wasm")
}

/// Create a manifest for recording mode
pub fn create_recording_manifest(wasm_path: &PathBuf) -> String {
    format!(
        r#"name = "replay-test"
version = "0.1.0"
package = "{}"
description = "Test actor for replay - recording mode"

[[handler]]
type = "runtime"

[[handler]]
type = "message-server"
"#,
        wasm_path.display()
    )
}

/// Create a manifest for replay mode using the recorded chain
pub fn create_replay_manifest(wasm_path: &PathBuf, chain_path: &str) -> String {
    format!(
        r#"name = "replay-test-replay"
version = "0.1.0"
package = "{}"
description = "Test actor for replay - replay mode"

[[handler]]
type = "replay"
chain = "{}"

[[handler]]
type = "runtime"

[[handler]]
type = "message-server"
"#,
        wasm_path.display(),
        chain_path
    )
}

/// Helper to load manifest and wasm bytes from a manifest string
async fn load_manifest_and_wasm(manifest_str: &str) -> Result<(ManifestConfig, Vec<u8>)> {
    let manifest = ManifestConfig::from_toml_str(manifest_str)?;
    let wasm_bytes = resolve_reference(&manifest.package).await?;
    Ok((manifest, wasm_bytes))
}

/// Creates a handler registry with RuntimeHandler and MessageServerHandler.
/// Returns both the registry and the MessageRouter for sending messages.
pub fn create_base_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> (HandlerRegistry, MessageRouter) {
    let mut registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    let message_router = MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router.clone()));

    (registry, message_router)
}

/// Get the path to the supervisor test actor's WASM package
pub fn get_supervisor_test_wasm_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../test-actors/supervisor-replay-test/target/wasm32-unknown-unknown/release/supervisor_replay_test_actor.wasm")
}

/// Create a manifest for the child actor (runtime only)
pub fn create_child_manifest(child_wasm_path: &PathBuf) -> String {
    format!(
        r#"name = "replay-test-child"
version = "0.1.0"
package = "{}"

[[handler]]
type = "runtime"
"#,
        child_wasm_path.display()
    )
}

/// Create a recording manifest with runtime + message-server + supervisor
pub fn create_supervisor_recording_manifest(wasm_path: &PathBuf) -> String {
    format!(
        r#"name = "supervisor-replay-test"
version = "0.1.0"
package = "{}"

[[handler]]
type = "runtime"

[[handler]]
type = "message-server"

[[handler]]
type = "supervisor"
"#,
        wasm_path.display()
    )
}

/// Create a replay manifest with supervisor
pub fn create_supervisor_replay_manifest(wasm_path: &PathBuf, chain_path: &str) -> String {
    format!(
        r#"name = "supervisor-replay-test-replay"
version = "0.1.0"
package = "{}"

[[handler]]
type = "replay"
chain = "{}"

[[handler]]
type = "runtime"

[[handler]]
type = "message-server"

[[handler]]
type = "supervisor"
"#,
        wasm_path.display(),
        chain_path
    )
}

/// Creates a handler registry with RuntimeHandler, MessageServerHandler, and SupervisorHandler.
pub fn create_supervisor_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> (HandlerRegistry, MessageRouter) {
    let mut registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));

    let message_router = MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router.clone()));

    let supervisor_config = SupervisorHostConfig {};
    registry.register(SupervisorHandler::new(supervisor_config, None));

    (registry, message_router)
}

/// Collect events from a channel with timeout
async fn collect_events(
    event_rx: &mut mpsc::Receiver<Result<ChainEvent, ActorError>>,
    idle_timeout: Duration,
    max_timeout: Duration,
) -> Vec<ChainEvent> {
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    let mut last_event_time = std::time::Instant::now();

    while start.elapsed() < max_timeout {
        match timeout(Duration::from_millis(100), event_rx.recv()).await {
            Ok(Some(Ok(event))) => {
                last_event_time = std::time::Instant::now();
                events.push(event);
            }
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => {
                if last_event_time.elapsed() > idle_timeout {
                    break;
                }
            }
        }
    }

    events
}

/// Run a complete replay verification test
///
/// This function:
/// 1. Records an actor run and collects its events
/// 2. Replays the actor using the recorded chain
/// 3. Compares the event hashes
///
/// Returns a ReplayVerificationResult with the comparison details.
pub async fn run_replay_verification(
    chain_path: &str,
    verbose: bool,
) -> Result<ReplayVerificationResult> {
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        return Err(anyhow::anyhow!(
            "WASM file not found at {:?}. Build with: cd test-actors/replay-test && cargo build --release",
            wasm_path
        ));
    }

    if verbose {
        println!("\n=== Phase 1: Recording ===\n");
    }

    // --- Phase 1: Record a run ---
    let recording_manifest_str = create_recording_manifest(&wasm_path);
    let (recording_manifest, wasm_bytes) = load_manifest_and_wasm(&recording_manifest_str).await?;

    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let (handler_registry, message_router) = create_base_registry(theater_tx.clone());

    let mut runtime =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;

    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    // Create a subscription channel to receive events
    let (event_tx, mut event_rx) = mpsc::channel(100);

    // Spawn the actor with event subscription
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(recording_manifest.name.clone()),
            manifest: Some(recording_manifest),
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await?;

    let spawn_result = timeout(Duration::from_secs(10), response_rx).await??;
    let actor_id = spawn_result?;

    if verbose {
        println!("Recorded run - Actor ID: {}", actor_id);
    }

    // Get actor handle and call init
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: actor_id.clone(),
            response_tx: handle_tx,
        })
        .await?;
    let actor_handle = timeout(Duration::from_secs(5), handle_rx)
        .await??
        .ok_or_else(|| anyhow::anyhow!("Actor handle not found"))?;

    // Call init to start the actor
    if verbose {
        println!("Calling init...");
    }
    actor_handle
        .call_function("theater:simple/actor.init".to_string(), Value::Tuple(vec![]))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to call init: {:?}", e))?;

    // Collect init events (short idle timeout so we move on quickly)
    let mut recorded_chain = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;

    if verbose {
        println!(
            "After init: {} events recorded, sending messages...",
            recorded_chain.len()
        );
    }

    // Send two messages to the actor via the message router
    for i in 0..2 {
        let msg_data = format!("test message {}", i).into_bytes();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        message_router
            .route_message(MessageCommand::SendMessage {
                target_id: actor_id.clone(),
                message: ActorMessage::Send(ActorSend { data: msg_data }),
                response_tx,
            })
            .await?;
        // Wait for routing to complete
        let _ = timeout(Duration::from_secs(5), response_rx).await;
    }

    // Collect message-handling events
    let msg_events = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;
    recorded_chain.extend(msg_events);

    if verbose {
        println!("Recorded chain has {} events", recorded_chain.len());
        for (i, event) in recorded_chain.iter().enumerate() {
            println!(
                "  Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Save chain to file for replay
    let chain_json = serde_json::to_string_pretty(&recorded_chain)?;
    std::fs::write(chain_path, &chain_json)?;

    if verbose {
        println!("Saved chain to {}", chain_path);
    }

    // Stop the first actor
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await?;
    let _ = timeout(Duration::from_secs(5), stop_rx).await;

    if recorded_chain.is_empty() {
        drop(theater_tx);
        let _ = timeout(Duration::from_secs(5), runtime_handle).await;
        return Err(anyhow::anyhow!("No events recorded"));
    }

    if verbose {
        println!("\n=== Phase 2: Replay ===\n");
    }

    // --- Phase 2: Replay ---
    let replay_manifest_str = create_replay_manifest(&wasm_path, chain_path);
    let (replay_manifest, replay_wasm_bytes) = load_manifest_and_wasm(&replay_manifest_str).await?;

    // Create subscription for replay events
    let (replay_event_tx, mut replay_event_rx) = mpsc::channel(100);

    // Spawn the replay actor
    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: replay_wasm_bytes,
            name: Some(replay_manifest.name.clone()),
            manifest: Some(replay_manifest),
            parent_id: None,
            response_tx: response_tx2,
            supervisor_tx: None,
            subscription_tx: Some(replay_event_tx),
        })
        .await?;

    let spawn_result2 = timeout(Duration::from_secs(10), response_rx2).await??;
    let replay_actor_id = spawn_result2?;

    if verbose {
        println!("Replay run - Actor ID: {}", replay_actor_id);
    }

    // Collect replay events
    let replay_chain = collect_events(
        &mut replay_event_rx,
        Duration::from_secs(2),
        Duration::from_secs(10),
    )
    .await;

    if verbose {
        println!("Replay chain has {} events", replay_chain.len());
        for (i, event) in replay_chain.iter().enumerate() {
            println!(
                "  Replay Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Stop the replay actor
    let (stop_tx2, stop_rx2) = tokio::sync::oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: replay_actor_id,
            response_tx: stop_tx2,
        })
        .await;
    let _ = timeout(Duration::from_secs(5), stop_rx2).await;

    // Shutdown the runtime
    drop(theater_tx);
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    // Compare chains
    let mut mismatches = 0;
    let max_len = recorded_chain.len().max(replay_chain.len());
    for i in 0..max_len {
        let orig_hash = recorded_chain.get(i).map(|e| &e.hash);
        let replay_hash = replay_chain.get(i).map(|e| &e.hash);
        if orig_hash != replay_hash {
            mismatches += 1;
        }
    }

    let same_length = recorded_chain.len() == replay_chain.len();
    let passed = mismatches == 0 && same_length;

    Ok(ReplayVerificationResult {
        original_chain: recorded_chain,
        replay_chain,
        mismatches,
        passed,
    })
}

/// Run a replay verification test that exercises handle-request (with response).
///
/// This function:
/// 1. Records an actor run with a Send message followed by a Request message
/// 2. Verifies the request response contains "response:" + original data
/// 3. Replays the actor using the recorded chain
/// 4. Compares event hashes for determinism
pub async fn run_request_replay_verification(
    chain_path: &str,
    verbose: bool,
) -> Result<ReplayVerificationResult> {
    let wasm_path = get_test_wasm_path();
    if !wasm_path.exists() {
        return Err(anyhow::anyhow!(
            "WASM file not found at {:?}. Build with: cd test-actors/replay-test && cargo build --release",
            wasm_path
        ));
    }

    if verbose {
        println!("\n=== Phase 1: Recording (with request) ===\n");
    }

    // --- Phase 1: Record a run ---
    let recording_manifest_str = create_recording_manifest(&wasm_path);
    let (recording_manifest, wasm_bytes) = load_manifest_and_wasm(&recording_manifest_str).await?;

    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let (handler_registry, message_router) = create_base_registry(theater_tx.clone());

    let mut runtime =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;

    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    let (event_tx, mut event_rx) = mpsc::channel(100);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(recording_manifest.name.clone()),
            manifest: Some(recording_manifest),
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await?;

    let spawn_result = timeout(Duration::from_secs(10), response_rx).await??;
    let actor_id = spawn_result?;

    if verbose {
        println!("Recorded run - Actor ID: {}", actor_id);
    }

    // Get actor handle and call init
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: actor_id.clone(),
            response_tx: handle_tx,
        })
        .await?;
    let actor_handle = timeout(Duration::from_secs(5), handle_rx)
        .await??
        .ok_or_else(|| anyhow::anyhow!("Actor handle not found"))?;

    // Call init to start the actor
    if verbose {
        println!("Calling init...");
    }
    actor_handle
        .call_function("theater:simple/actor.init".to_string(), Value::Tuple(vec![]))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to call init: {:?}", e))?;

    // Collect init events
    let mut recorded_chain = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;

    if verbose {
        println!(
            "After init: {} events recorded, sending messages...",
            recorded_chain.len()
        );
    }

    // Send one Send message
    {
        let msg_data = b"hello from send".to_vec();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        message_router
            .route_message(MessageCommand::SendMessage {
                target_id: actor_id.clone(),
                message: ActorMessage::Send(ActorSend { data: msg_data }),
                response_tx,
            })
            .await?;
        let _ = timeout(Duration::from_secs(5), response_rx).await;
    }

    // Collect send-handling events
    let send_events = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;
    recorded_chain.extend(send_events);

    // Send one Request message and verify the response
    let request_data = b"test request data".to_vec();
    let expected_response = b"response:test request data".to_vec();
    {
        let (actor_response_tx, actor_response_rx) = tokio::sync::oneshot::channel();
        let (cmd_response_tx, cmd_response_rx) = tokio::sync::oneshot::channel();
        message_router
            .route_message(MessageCommand::SendMessage {
                target_id: actor_id.clone(),
                message: ActorMessage::Request(ActorRequest {
                    data: request_data,
                    response_tx: actor_response_tx,
                }),
                response_tx: cmd_response_tx,
            })
            .await?;
        // Wait for routing
        let _ = timeout(Duration::from_secs(5), cmd_response_rx).await;
        // Wait for the actual response from the actor
        let response = timeout(Duration::from_secs(5), actor_response_rx).await??;

        if verbose {
            println!(
                "Request response: {:?}",
                String::from_utf8_lossy(&response)
            );
        }

        assert_eq!(
            response, expected_response,
            "Request response should be 'response:' + original data"
        );
    }

    // Collect request-handling events
    let req_events = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;
    recorded_chain.extend(req_events);

    if verbose {
        println!("Recorded chain has {} events", recorded_chain.len());
        for (i, event) in recorded_chain.iter().enumerate() {
            println!(
                "  Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Save chain to file for replay
    let chain_json = serde_json::to_string_pretty(&recorded_chain)?;
    std::fs::write(chain_path, &chain_json)?;

    if verbose {
        println!("Saved chain to {}", chain_path);
    }

    // Stop the first actor
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await?;
    let _ = timeout(Duration::from_secs(5), stop_rx).await;

    if recorded_chain.is_empty() {
        drop(theater_tx);
        let _ = timeout(Duration::from_secs(5), runtime_handle).await;
        return Err(anyhow::anyhow!("No events recorded"));
    }

    if verbose {
        println!("\n=== Phase 2: Replay (with request) ===\n");
    }

    // --- Phase 2: Replay ---
    let replay_manifest_str = create_replay_manifest(&wasm_path, chain_path);
    let (replay_manifest, replay_wasm_bytes) = load_manifest_and_wasm(&replay_manifest_str).await?;

    let (replay_event_tx, mut replay_event_rx) = mpsc::channel(100);

    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: replay_wasm_bytes,
            name: Some(replay_manifest.name.clone()),
            manifest: Some(replay_manifest),
            parent_id: None,
            response_tx: response_tx2,
            supervisor_tx: None,
            subscription_tx: Some(replay_event_tx),
        })
        .await?;

    let spawn_result2 = timeout(Duration::from_secs(10), response_rx2).await??;
    let replay_actor_id = spawn_result2?;

    if verbose {
        println!("Replay run - Actor ID: {}", replay_actor_id);
    }

    // Collect replay events
    let replay_chain = collect_events(
        &mut replay_event_rx,
        Duration::from_secs(2),
        Duration::from_secs(10),
    )
    .await;

    if verbose {
        println!("Replay chain has {} events", replay_chain.len());
        for (i, event) in replay_chain.iter().enumerate() {
            println!(
                "  Replay Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Stop the replay actor
    let (stop_tx2, stop_rx2) = tokio::sync::oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: replay_actor_id,
            response_tx: stop_tx2,
        })
        .await;
    let _ = timeout(Duration::from_secs(5), stop_rx2).await;

    // Shutdown the runtime
    drop(theater_tx);
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    // Compare chains
    let mut mismatches = 0;
    let max_len = recorded_chain.len().max(replay_chain.len());
    for i in 0..max_len {
        let orig_hash = recorded_chain.get(i).map(|e| &e.hash);
        let replay_hash = replay_chain.get(i).map(|e| &e.hash);
        if orig_hash != replay_hash {
            mismatches += 1;
        }
    }

    let same_length = recorded_chain.len() == replay_chain.len();
    let passed = mismatches == 0 && same_length;

    Ok(ReplayVerificationResult {
        original_chain: recorded_chain,
        replay_chain,
        mismatches,
        passed,
    })
}

/// Run a supervisor replay verification test.
///
/// This function:
/// 1. Spawns a parent supervisor actor
/// 2. Sends spawn, list, stop commands and waits for the external-stop callback
/// 3. Records the event chain
/// 4. Replays and verifies hash determinism
pub async fn run_supervisor_replay_verification(
    chain_path: &str,
    verbose: bool,
) -> Result<ReplayVerificationResult> {
    let wasm_path = get_supervisor_test_wasm_path();
    if !wasm_path.exists() {
        return Err(anyhow::anyhow!(
            "Supervisor test WASM not found at {:?}. Build with: cd test-actors/supervisor-replay-test && cargo build --release",
            wasm_path
        ));
    }

    let child_wasm_path = get_test_wasm_path();
    if !child_wasm_path.exists() {
        return Err(anyhow::anyhow!(
            "Child test WASM not found at {:?}. Build with: cd test-actors/replay-test && cargo build --release",
            child_wasm_path
        ));
    }

    // Canonicalize so the child manifest has an absolute path
    let child_wasm_path = child_wasm_path.canonicalize()?;

    // Write child manifest to a temp file
    let child_manifest_path = format!(
        "/tmp/supervisor_test_child_manifest_{}.toml",
        std::process::id()
    );
    let child_manifest_content = create_child_manifest(&child_wasm_path);
    std::fs::write(&child_manifest_path, &child_manifest_content)?;

    if verbose {
        println!("\n=== Supervisor Phase 1: Recording ===\n");
        println!("Child manifest written to: {}", child_manifest_path);
    }

    // --- Phase 1: Record ---
    let recording_manifest_str = create_supervisor_recording_manifest(&wasm_path);
    let (recording_manifest, wasm_bytes) = load_manifest_and_wasm(&recording_manifest_str).await?;

    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let (handler_registry, message_router) = create_supervisor_registry(theater_tx.clone());

    let mut runtime =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;
    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    let (event_tx, mut event_rx) = mpsc::channel(100);

    // Spawn the parent supervisor actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(recording_manifest.name.clone()),
            manifest: Some(recording_manifest),
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await?;

    let spawn_result = timeout(Duration::from_secs(10), response_rx).await??;
    let actor_id = spawn_result?;

    if verbose {
        println!("Parent actor ID: {}", actor_id);
    }

    // Get actor handle and call init
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: actor_id.clone(),
            response_tx: handle_tx,
        })
        .await?;
    let actor_handle = timeout(Duration::from_secs(5), handle_rx)
        .await??
        .ok_or_else(|| anyhow::anyhow!("Actor handle not found"))?;

    // Call init to start the actor
    if verbose {
        println!("Calling init...");
    }
    actor_handle
        .call_function("theater:simple/actor.init".to_string(), Value::Tuple(vec![]))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to call init: {:?}", e))?;

    // Collect init events
    let mut recorded_chain = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;

    if verbose {
        println!("After init: {} events", recorded_chain.len());
    }

    // Send "spawn:<child_manifest_path>"
    {
        let msg = format!("spawn:{}", child_manifest_path);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        message_router
            .route_message(MessageCommand::SendMessage {
                target_id: actor_id.clone(),
                message: ActorMessage::Send(ActorSend {
                    data: msg.into_bytes(),
                }),
                response_tx,
            })
            .await?;
        let _ = timeout(Duration::from_secs(5), response_rx).await;
    }

    let spawn_events = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;
    recorded_chain.extend(spawn_events);

    if verbose {
        println!("After spawn: {} total events", recorded_chain.len());
    }

    // Send "list"
    {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        message_router
            .route_message(MessageCommand::SendMessage {
                target_id: actor_id.clone(),
                message: ActorMessage::Send(ActorSend {
                    data: b"list".to_vec(),
                }),
                response_tx,
            })
            .await?;
        let _ = timeout(Duration::from_secs(5), response_rx).await;
    }

    let list_events = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;
    recorded_chain.extend(list_events);

    if verbose {
        println!("After list: {} total events", recorded_chain.len());
    }

    // Send "stop"
    {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        message_router
            .route_message(MessageCommand::SendMessage {
                target_id: actor_id.clone(),
                message: ActorMessage::Send(ActorSend {
                    data: b"stop".to_vec(),
                }),
                response_tx,
            })
            .await?;
        let _ = timeout(Duration::from_secs(5), response_rx).await;
    }

    // Wait longer for the callback event (handle-child-external-stop)
    let stop_events = collect_events(
        &mut event_rx,
        Duration::from_secs(2),
        Duration::from_secs(15),
    )
    .await;
    recorded_chain.extend(stop_events);

    if verbose {
        println!(
            "After stop (incl. callback): {} total events",
            recorded_chain.len()
        );
        for (i, event) in recorded_chain.iter().enumerate() {
            println!(
                "  Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Save chain
    let chain_json = serde_json::to_string_pretty(&recorded_chain)?;
    std::fs::write(chain_path, &chain_json)?;

    if verbose {
        println!("Saved chain to {}", chain_path);
    }

    // Stop the parent actor
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: actor_id.clone(),
            response_tx: stop_tx,
        })
        .await?;
    let _ = timeout(Duration::from_secs(5), stop_rx).await;

    if recorded_chain.is_empty() {
        drop(theater_tx);
        let _ = timeout(Duration::from_secs(5), runtime_handle).await;
        let _ = std::fs::remove_file(&child_manifest_path);
        return Err(anyhow::anyhow!("No events recorded"));
    }

    if verbose {
        println!("\n=== Supervisor Phase 2: Replay ===\n");
    }

    // --- Phase 2: Replay ---
    let replay_manifest_str = create_supervisor_replay_manifest(&wasm_path, chain_path);
    let (replay_manifest, replay_wasm_bytes) = load_manifest_and_wasm(&replay_manifest_str).await?;

    let (replay_event_tx, mut replay_event_rx) = mpsc::channel(100);

    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: replay_wasm_bytes,
            name: Some(replay_manifest.name.clone()),
            manifest: Some(replay_manifest),
            parent_id: None,
            response_tx: response_tx2,
            supervisor_tx: None,
            subscription_tx: Some(replay_event_tx),
        })
        .await?;

    let spawn_result2 = timeout(Duration::from_secs(10), response_rx2).await??;
    let replay_actor_id = spawn_result2?;

    if verbose {
        println!("Replay actor ID: {}", replay_actor_id);
    }

    // Collect replay events
    let replay_chain = collect_events(
        &mut replay_event_rx,
        Duration::from_secs(2),
        Duration::from_secs(15),
    )
    .await;

    if verbose {
        println!("Replay chain has {} events", replay_chain.len());
        for (i, event) in replay_chain.iter().enumerate() {
            println!(
                "  Replay Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Stop replay actor
    let (stop_tx2, stop_rx2) = tokio::sync::oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: replay_actor_id,
            response_tx: stop_tx2,
        })
        .await;
    let _ = timeout(Duration::from_secs(5), stop_rx2).await;

    // Shutdown runtime
    drop(theater_tx);
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    // Clean up temp file
    let _ = std::fs::remove_file(&child_manifest_path);

    // Compare chains
    let mut mismatches = 0;
    let max_len = recorded_chain.len().max(replay_chain.len());
    for i in 0..max_len {
        let orig_hash = recorded_chain.get(i).map(|e| &e.hash);
        let replay_hash = replay_chain.get(i).map(|e| &e.hash);
        if orig_hash != replay_hash {
            mismatches += 1;
        }
    }

    let same_length = recorded_chain.len() == replay_chain.len();
    let passed = mismatches == 0 && same_length;

    Ok(ReplayVerificationResult {
        original_chain: recorded_chain,
        replay_chain,
        mismatches,
        passed,
    })
}

/// Get the path to the TCP echo actor's WASM package
pub fn get_tcp_echo_wasm_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../crates/theater-handler-tcp/examples/tcp-echo/target/wasm32-unknown-unknown/release/tcp_echo_actor.wasm")
}

/// Create a recording manifest with runtime + tcp handlers
pub fn create_tcp_recording_manifest(wasm_path: &PathBuf, listen_addr: &str) -> String {
    // Pass listen address via initial_state so the actor can call listen()
    let initial_state = format!(r#"{{"listen": "{}"}}"#, listen_addr);
    format!(
        r#"name = "tcp-echo-replay-test"
version = "0.1.0"
package = "{}"
initial_state = '{}'

[[handler]]
type = "runtime"

[[handler]]
type = "tcp"
"#,
        wasm_path.display(),
        initial_state
    )
}

/// Create a replay manifest for TCP actor
pub fn create_tcp_replay_manifest(wasm_path: &PathBuf, chain_path: &str, listen_addr: &str) -> String {
    let initial_state = format!(r#"{{"listen": "{}"}}"#, listen_addr);
    format!(
        r#"name = "tcp-echo-replay-test-replay"
version = "0.1.0"
package = "{}"
initial_state = '{}'

[[handler]]
type = "replay"
chain = "{}"

[[handler]]
type = "runtime"

[[handler]]
type = "tcp"
"#,
        wasm_path.display(),
        initial_state,
        chain_path
    )
}

/// Creates a handler registry with RuntimeHandler and TcpHandler.
pub fn create_tcp_registry(
    theater_tx: mpsc::Sender<TheaterCommand>,
) -> HandlerRegistry {
    use theater::config::actor_manifest::TcpHandlerConfig;
    let mut registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));
    registry.register(TcpHandler::new(TcpHandlerConfig { listen: None, max_connections: None }));
    registry
}

/// Run a TCP echo replay verification test.
///
/// This function:
/// 1. Spawns a TCP echo actor that listens on a port
/// 2. Connects a test client and sends/receives data
/// 3. Records the event chain
/// 4. Replays and verifies hash determinism
pub async fn run_tcp_replay_verification(
    chain_path: &str,
    verbose: bool,
) -> Result<ReplayVerificationResult> {
    let wasm_path = get_tcp_echo_wasm_path();
    if !wasm_path.exists() {
        return Err(anyhow::anyhow!(
            "TCP echo WASM not found at {:?}. Build with: cd crates/theater-handler-tcp/examples/tcp-echo && cargo build --release --target wasm32-unknown-unknown",
            wasm_path
        ));
    }

    // Find an available port
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let listen_addr = listener.local_addr()?;
    drop(listener); // Release the port so the actor can use it
    let listen_addr_str = listen_addr.to_string();

    if verbose {
        println!("\n=== TCP Phase 1: Recording ===\n");
        println!("Listen address: {}", listen_addr_str);
    }

    // --- Phase 1: Record ---
    let recording_manifest_str = create_tcp_recording_manifest(&wasm_path, &listen_addr_str);
    let (recording_manifest, wasm_bytes) = load_manifest_and_wasm(&recording_manifest_str).await?;

    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_tcp_registry(theater_tx.clone());

    let mut runtime =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;
    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    let (event_tx, mut event_rx) = mpsc::channel(100);

    // Spawn the TCP echo actor
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(recording_manifest.name.clone()),
            manifest: Some(recording_manifest),
            parent_id: None,
            response_tx,
            supervisor_tx: None,
            subscription_tx: Some(event_tx),
        })
        .await?;

    let spawn_result = timeout(Duration::from_secs(10), response_rx).await??;
    let actor_id = spawn_result?;

    if verbose {
        println!("Actor ID: {}", actor_id);
    }

    // Get actor handle and call init
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: actor_id.clone(),
            response_tx: handle_tx,
        })
        .await?;
    let actor_handle = timeout(Duration::from_secs(5), handle_rx)
        .await??
        .ok_or_else(|| anyhow::anyhow!("Actor handle not found"))?;

    // Call init to start the listener
    if verbose {
        println!("Calling init (which starts TCP listener)...");
    }

    // Build initial state as option<list<u8>>
    // This is passed to the actor as params, but the actor's init function
    // receives the store state (from manifest initial_state) as the first tuple element.
    // For now, we pass an empty tuple as params since init doesn't need extra params.
    let state_json = format!(r#"{{"listen": "{}"}}"#, listen_addr_str);
    let state_bytes: Vec<pack::abi::Value> = state_json.bytes().map(pack::abi::Value::U8).collect();
    let init_state = pack::abi::Value::Option {
        inner_type: pack::abi::ValueType::List(Box::new(pack::abi::ValueType::U8)),
        value: Some(Box::new(pack::abi::Value::List {
            elem_type: pack::abi::ValueType::U8,
            items: state_bytes,
        })),
    };

    actor_handle
        .call_function("theater:simple/actor.init".to_string(), Value::Tuple(vec![init_state]))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to call init: {:?}", e))?;

    // Collect init events
    let mut recorded_chain = collect_events(
        &mut event_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;

    if verbose {
        println!("After init: {} events", recorded_chain.len());
    }

    // Connect a test client with retry logic
    if verbose {
        println!("Connecting test client...");
    }

    let mut tcp_client = None;
    for attempt in 0..10 {
        match tokio::net::TcpStream::connect(&listen_addr).await {
            Ok(stream) => {
                tcp_client = Some(stream);
                break;
            }
            Err(e) if attempt < 9 => {
                if verbose {
                    println!("  Connection attempt {} failed: {}, retrying...", attempt + 1, e);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => return Err(anyhow::anyhow!("Failed to connect after 10 attempts: {}", e)),
        }
    }
    let mut tcp_client = tcp_client.unwrap();
    let test_data = b"Hello from TCP test!";

    // Send data
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tcp_client.write_all(test_data).await?;

    // Read echoed data
    let mut buf = vec![0u8; 1024];
    let n = tcp_client.read(&mut buf).await?;
    let echoed = &buf[..n];

    if verbose {
        println!("Sent: {:?}", String::from_utf8_lossy(test_data));
        println!("Received: {:?}", String::from_utf8_lossy(echoed));
    }

    assert_eq!(echoed, test_data, "Echoed data should match sent data");

    // Collect connection handling events
    let conn_events = collect_events(
        &mut event_rx,
        Duration::from_secs(1),
        Duration::from_secs(10),
    )
    .await;
    recorded_chain.extend(conn_events);

    if verbose {
        println!("After connection: {} total events", recorded_chain.len());
        for (i, event) in recorded_chain.iter().enumerate() {
            println!(
                "  Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Save chain
    let chain_json = serde_json::to_string_pretty(&recorded_chain)?;
    std::fs::write(chain_path, &chain_json)?;

    if verbose {
        println!("Saved chain to {}", chain_path);
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

    if recorded_chain.is_empty() {
        drop(theater_tx);
        let _ = timeout(Duration::from_secs(5), runtime_handle).await;
        return Err(anyhow::anyhow!("No events recorded"));
    }

    if verbose {
        println!("\n=== TCP Phase 2: Replay ===\n");
    }

    // --- Phase 2: Replay ---
    let replay_manifest_str = create_tcp_replay_manifest(&wasm_path, chain_path, &listen_addr_str);
    let (replay_manifest, replay_wasm_bytes) = load_manifest_and_wasm(&replay_manifest_str).await?;

    let (replay_event_tx, mut replay_event_rx) = mpsc::channel(100);

    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: replay_wasm_bytes,
            name: Some(replay_manifest.name.clone()),
            manifest: Some(replay_manifest),
            parent_id: None,
            response_tx: response_tx2,
            supervisor_tx: None,
            subscription_tx: Some(replay_event_tx),
        })
        .await?;

    let spawn_result2 = timeout(Duration::from_secs(10), response_rx2).await??;
    let replay_actor_id = spawn_result2?;

    if verbose {
        println!("Replay actor ID: {}", replay_actor_id);
    }

    // Collect replay events
    let replay_chain = collect_events(
        &mut replay_event_rx,
        Duration::from_secs(2),
        Duration::from_secs(15),
    )
    .await;

    if verbose {
        println!("Replay chain has {} events", replay_chain.len());
        for (i, event) in replay_chain.iter().enumerate() {
            println!(
                "  Replay Event {}: type={}, hash={}",
                i,
                event.event_type,
                hex::encode(&event.hash[..8.min(event.hash.len())])
            );
        }
    }

    // Stop replay actor
    let (stop_tx2, stop_rx2) = tokio::sync::oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: replay_actor_id,
            response_tx: stop_tx2,
        })
        .await;
    let _ = timeout(Duration::from_secs(5), stop_rx2).await;

    // Shutdown runtime
    drop(theater_tx);
    let _ = timeout(Duration::from_secs(5), runtime_handle).await;

    // Compare chains
    let mut mismatches = 0;
    let max_len = recorded_chain.len().max(replay_chain.len());
    for i in 0..max_len {
        let orig_hash = recorded_chain.get(i).map(|e| &e.hash);
        let replay_hash = replay_chain.get(i).map(|e| &e.hash);
        if orig_hash != replay_hash {
            mismatches += 1;
        }
    }

    let same_length = recorded_chain.len() == replay_chain.len();
    let passed = mismatches == 0 && same_length;

    Ok(ReplayVerificationResult {
        original_chain: recorded_chain,
        replay_chain,
        mismatches,
        passed,
    })
}

/// Format event data as readable string
fn format_event_data(event: &ChainEvent) -> String {
    if event.data.is_empty() {
        return "<empty>".to_string();
    }

    match String::from_utf8(event.data.clone()) {
        Ok(s) => s,
        Err(_) => format!("<{} bytes binary>", event.data.len()),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("theater_replay_experimenting=info".parse()?)
                .add_directive("theater=warn".parse()?),
        )
        .init();

    let chain_path = "/tmp/recorded_chain.json";
    let request_chain_path = "/tmp/recorded_request_chain.json";
    let verbose = true;

    let result = run_replay_verification(chain_path, verbose).await?;

    // Print comparison
    println!("\n=== Comparison ===\n");
    println!("Original events: {}", result.original_chain.len());
    println!("Replay events:   {}", result.replay_chain.len());

    println!("\n=== Hash Comparison ===\n");
    println!(
        "{:<4} {:<40} {:<40} {:<30}",
        "#", "Original Hash", "Replay Hash", "Event Type"
    );
    println!("{}", "-".repeat(120));

    let max_len = result.original_chain.len().max(result.replay_chain.len());
    for i in 0..max_len {
        let orig_hash = result
            .original_chain
            .get(i)
            .map(|e| hex::encode(&e.hash))
            .unwrap_or_else(|| "-".to_string());
        let replay_hash = result
            .replay_chain
            .get(i)
            .map(|e| hex::encode(&e.hash))
            .unwrap_or_else(|| "-".to_string());
        let event_type = result
            .original_chain
            .get(i)
            .map(|e| e.event_type.clone())
            .or_else(|| result.replay_chain.get(i).map(|e| e.event_type.clone()))
            .unwrap_or_else(|| "-".to_string());

        let matches = orig_hash == replay_hash;
        let indicator = if matches { "✓" } else { "✗" };

        println!(
            "{:<4} {:<40} {:<40} {:<30} {}",
            i,
            &orig_hash[..orig_hash.len().min(38)],
            &replay_hash[..replay_hash.len().min(38)],
            &event_type[..event_type.len().min(28)],
            indicator
        );
    }

    // Verification summary
    println!("\n=== Verification Summary ===\n");

    if result.same_length() {
        println!(
            "✓ Chain lengths match: {} events",
            result.original_chain.len()
        );
    } else {
        println!(
            "✗ Chain lengths differ: original={}, replay={}",
            result.original_chain.len(),
            result.replay_chain.len()
        );
    }

    if result.mismatches == 0 {
        println!("✓ All hashes match");
    } else {
        println!("✗ {} hash mismatches found", result.mismatches);
    }

    if result.passed {
        println!("\n🎉 REPLAY VERIFICATION PASSED! 🎉");
        println!("The replayed actor produced an identical event chain.");
    } else {
        println!("\n❌ REPLAY VERIFICATION FAILED ❌");
        println!("The replayed actor produced a different event chain.");

        // Print chain contents on failure
        println!("\n=== Original Chain ===\n");
        for (i, event) in result.original_chain.iter().enumerate() {
            println!("--- Event {} ---", i);
            println!("Type: {}", event.event_type);
            println!("Data: {}", format_event_data(event));
            println!();
        }

        println!("\n=== Replay Chain ===\n");
        for (i, event) in result.replay_chain.iter().enumerate() {
            println!("--- Event {} ---", i);
            println!("Type: {}", event.event_type);
            println!("Data: {}", format_event_data(event));
            println!();
        }

        return Err(anyhow::anyhow!(
            "Replay verification failed: {} mismatches",
            result.mismatches
        ));
    }

    println!("\n=== Send Experiment Complete ===");

    // --- Request replay verification ---
    println!("\n\n{}", "=".repeat(60));
    println!("=== Request Replay Verification ===\n");

    let request_result =
        run_request_replay_verification(request_chain_path, verbose).await?;

    println!("\n=== Request Comparison ===\n");
    println!("Original events: {}", request_result.original_chain.len());
    println!("Replay events:   {}", request_result.replay_chain.len());

    if request_result.passed {
        println!("\n🎉 REQUEST REPLAY VERIFICATION PASSED! 🎉");
    } else {
        println!("\n❌ REQUEST REPLAY VERIFICATION FAILED ❌");
        println!("{}", request_result.comparison_details());
        return Err(anyhow::anyhow!(
            "Request replay verification failed: {} mismatches",
            request_result.mismatches
        ));
    }

    // --- Supervisor replay verification ---
    println!("\n\n{}", "=".repeat(60));
    println!("=== Supervisor Replay Verification ===\n");

    let supervisor_chain_path = "/tmp/recorded_supervisor_chain.json";
    let supervisor_result =
        run_supervisor_replay_verification(supervisor_chain_path, verbose).await?;

    println!("\n=== Supervisor Comparison ===\n");
    println!(
        "Original events: {}",
        supervisor_result.original_chain.len()
    );
    println!("Replay events:   {}", supervisor_result.replay_chain.len());

    if supervisor_result.passed {
        println!("\n🎉 SUPERVISOR REPLAY VERIFICATION PASSED! 🎉");
    } else {
        println!("\n❌ SUPERVISOR REPLAY VERIFICATION FAILED ❌");
        println!("{}", supervisor_result.comparison_details());
        return Err(anyhow::anyhow!(
            "Supervisor replay verification failed: {} mismatches",
            supervisor_result.mismatches
        ));
    }

    // --- TCP replay verification ---
    println!("\n\n{}", "=".repeat(60));
    println!("=== TCP Replay Verification ===\n");

    let tcp_chain_path = "/tmp/recorded_tcp_chain.json";
    let tcp_result = run_tcp_replay_verification(tcp_chain_path, verbose).await?;

    println!("\n=== TCP Comparison ===\n");
    println!("Original events: {}", tcp_result.original_chain.len());
    println!("Replay events:   {}", tcp_result.replay_chain.len());

    if tcp_result.passed {
        println!("\n🎉 TCP REPLAY VERIFICATION PASSED! 🎉");
    } else {
        println!("\n❌ TCP REPLAY VERIFICATION FAILED ❌");
        println!("{}", tcp_result.comparison_details());
        return Err(anyhow::anyhow!(
            "TCP replay verification failed: {} mismatches",
            tcp_result.mismatches
        ));
    }

    println!("\n=== All Experiments Complete ===");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that replay produces identical chain hashes
    ///
    /// This test:
    /// 1. Runs an actor and records its event chain
    /// 2. Replays the actor using the recorded chain
    /// 3. Verifies that all event hashes match
    #[tokio::test]
    async fn test_replay_verification() {
        // Use a unique temp file for this test
        let chain_path = format!("/tmp/test_replay_chain_{}.json", std::process::id());

        let result = run_replay_verification(&chain_path, false)
            .await
            .expect("Replay verification should complete");

        // Clean up temp file
        let _ = std::fs::remove_file(&chain_path);

        // Assertions
        assert!(
            !result.original_chain.is_empty(),
            "Original chain should have events"
        );
        assert!(
            !result.replay_chain.is_empty(),
            "Replay chain should have events"
        );
        assert!(
            result.same_length(),
            "Chain lengths should match: original={}, replay={}",
            result.original_chain.len(),
            result.replay_chain.len()
        );
        assert_eq!(
            result.mismatches, 0,
            "All hashes should match. Comparison:\n{}",
            result.comparison_details()
        );
        assert!(
            result.passed,
            "Replay verification should pass. Details:\n{}",
            result.comparison_details()
        );
    }

    /// Test that request replay produces identical chain hashes
    ///
    /// This test exercises handle-request (with a non-trivial return value)
    /// to validate the call_function(Value) -> Value path:
    /// 1. Runs an actor, sends a Send + Request message, verifies the response
    /// 2. Replays the actor using the recorded chain
    /// 3. Verifies that all event hashes match
    #[tokio::test]
    async fn test_request_replay_verification() {
        let chain_path = format!("/tmp/test_request_replay_chain_{}.json", std::process::id());

        let result = run_request_replay_verification(&chain_path, true)
            .await
            .expect("Request replay verification should complete");

        // Clean up temp file
        let _ = std::fs::remove_file(&chain_path);

        // Assertions
        assert!(
            !result.original_chain.is_empty(),
            "Original chain should have events"
        );
        assert!(
            !result.replay_chain.is_empty(),
            "Replay chain should have events"
        );
        assert!(
            result.same_length(),
            "Chain lengths should match: original={}, replay={}",
            result.original_chain.len(),
            result.replay_chain.len()
        );
        assert_eq!(
            result.mismatches, 0,
            "All hashes should match. Comparison:\n{}",
            result.comparison_details()
        );
        assert!(
            result.passed,
            "Request replay verification should pass. Details:\n{}",
            result.comparison_details()
        );
    }

    /// Test that the chain has expected event types
    #[tokio::test]
    async fn test_replay_event_types() {
        let chain_path = format!("/tmp/test_replay_types_{}.json", std::process::id());

        let result = run_replay_verification(&chain_path, false)
            .await
            .expect("Replay verification should complete");

        // Clean up temp file
        let _ = std::fs::remove_file(&chain_path);

        // Check that we have the expected event types
        let event_types: Vec<&str> = result
            .original_chain
            .iter()
            .map(|e| e.event_type.as_str())
            .collect();

        // Should have wasm events and runtime/log events
        assert!(
            event_types.iter().any(|t| *t == "wasm"),
            "Should have wasm events"
        );
        assert!(
            event_types
                .iter()
                .any(|t| t.contains("theater:simple/runtime/log")),
            "Should have runtime/log events"
        );
    }

    /// Test that supervisor replay produces identical chain hashes
    ///
    /// This test:
    /// 1. Spawns a supervisor actor, sends spawn/list/stop commands
    /// 2. Waits for the handle-child-external-stop callback
    /// 3. Replays the actor using the recorded chain
    /// 4. Verifies that all event hashes match
    #[tokio::test]
    async fn test_supervisor_replay_verification() {
        let chain_path = format!(
            "/tmp/test_supervisor_replay_chain_{}.json",
            std::process::id()
        );

        let result = run_supervisor_replay_verification(&chain_path, false)
            .await
            .expect("Supervisor replay verification should complete");

        // Clean up temp file
        let _ = std::fs::remove_file(&chain_path);

        // Assertions
        assert!(
            !result.original_chain.is_empty(),
            "Original chain should have events"
        );
        assert!(
            !result.replay_chain.is_empty(),
            "Replay chain should have events"
        );
        assert!(
            result.same_length(),
            "Chain lengths should match: original={}, replay={}",
            result.original_chain.len(),
            result.replay_chain.len()
        );
        assert_eq!(
            result.mismatches, 0,
            "All hashes should match. Comparison:\n{}",
            result.comparison_details()
        );
        assert!(
            result.passed,
            "Supervisor replay verification should pass. Details:\n{}",
            result.comparison_details()
        );
    }

    /// Test that TCP echo replay produces identical chain hashes
    ///
    /// This test:
    /// 1. Spawns a TCP echo actor that listens on a port
    /// 2. Connects a test client and sends/receives data
    /// 3. Records the event chain
    /// 4. Replays and verifies hash determinism
    ///
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_tcp_replay_verification() {
        let chain_path = format!(
            "/tmp/test_tcp_replay_chain_{}.json",
            std::process::id()
        );

        let result = run_tcp_replay_verification(&chain_path, true)
            .await
            .expect("TCP replay verification should complete");

        // Clean up temp file
        let _ = std::fs::remove_file(&chain_path);

        // Assertions
        assert!(
            !result.original_chain.is_empty(),
            "Original chain should have events"
        );
        assert!(
            !result.replay_chain.is_empty(),
            "Replay chain should have events"
        );
        assert!(
            result.same_length(),
            "Chain lengths should match: original={}, replay={}",
            result.original_chain.len(),
            result.replay_chain.len()
        );
        assert_eq!(
            result.mismatches, 0,
            "All hashes should match. Comparison:\n{}",
            result.comparison_details()
        );
        assert!(
            result.passed,
            "TCP replay verification should pass. Details:\n{}",
            result.comparison_details()
        );
    }
}
