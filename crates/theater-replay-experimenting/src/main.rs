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
//! cd crates/theater-handler-runtime/test-actors/runtime-test && cargo component build --release
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
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::handler::HandlerRegistry;
use theater::messages::{ActorMessage, ActorSend, MessageCommand, TheaterCommand};
use theater::theater_runtime::TheaterRuntime;
use theater::ActorError;

use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_runtime::RuntimeHandler;

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
"#,
        wasm_path.display(),
        chain_path
    )
}

/// Creates a handler registry with RuntimeHandler
pub fn create_base_registry(theater_tx: mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    let runtime_config = RuntimeHostConfig {};
    registry.register(RuntimeHandler::new(runtime_config, theater_tx, None));
    registry
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
    let recording_manifest = create_recording_manifest(&wasm_path);

    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let handler_registry = create_base_registry(theater_tx.clone());

    let mut runtime =
        TheaterRuntime::new(theater_tx.clone(), theater_rx, None, handler_registry).await?;

    let runtime_handle = tokio::spawn(async move { runtime.run().await });

    // Create a subscription channel to receive events
    let (event_tx, mut event_rx) = mpsc::channel(100);

    // Spawn the actor with event subscription
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: recording_manifest,
            init_bytes: None,
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

    // Collect events
    let recorded_chain = collect_events(
        &mut event_rx,
        Duration::from_secs(2),
        Duration::from_secs(10),
    )
    .await;

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
    let replay_manifest = create_replay_manifest(&wasm_path, chain_path);

    // Create subscription for replay events
    let (replay_event_tx, mut replay_event_rx) = mpsc::channel(100);

    // Spawn the replay actor
    let (response_tx2, response_rx2) = tokio::sync::oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: replay_manifest,
            init_bytes: None,
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

    println!("\n=== Experiment Complete ===");
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
}
