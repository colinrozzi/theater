//! Theater Replay Experimenting
//!
//! This crate is for experimenting with replay functionality.
//! We can create chains, compare them, and develop the replay system.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use theater::chain::{ChainEvent, StateChain};
use theater::events::ChainEventData;
use theater::id::TheaterId;
use tokio::sync::mpsc;

/// A simple event for testing - no timestamps, deterministic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimpleEvent {
    Log { message: String },
    FunctionCall { name: String, input: Vec<u8>, output: Vec<u8> },
    StateChange { old: Vec<u8>, new: Vec<u8> },
}

/// Create a chain event without timestamp dependency
fn create_event(event_type: &str, data: SimpleEvent, description: Option<String>) -> ChainEventData<SimpleEvent> {
    ChainEventData {
        event_type: event_type.to_string(),
        data,
        timestamp: 0, // Fixed timestamp for deterministic hashing
        description,
    }
}

/// Run a simulated actor execution and return the chain
/// Returns the chain and keeps the receiver alive to avoid channel panics
async fn simulate_actor_run(actor_id: TheaterId) -> Result<(StateChain<SimpleEvent>, mpsc::Receiver<theater::messages::TheaterCommand>)> {
    let (tx, rx) = mpsc::channel(10);
    let mut chain: StateChain<SimpleEvent> = StateChain::new(actor_id, tx);

    // Simulate: Actor init
    chain.add_typed_event(create_event(
        "actor.init",
        SimpleEvent::Log { message: "Actor starting".to_string() },
        Some("Actor initialization".to_string()),
    ))?;

    // Simulate: Log call
    chain.add_typed_event(create_event(
        "runtime.log",
        SimpleEvent::FunctionCall {
            name: "log".to_string(),
            input: b"Hello from actor".to_vec(),
            output: vec![],
        },
        Some("Actor log: Hello from actor".to_string()),
    ))?;

    // Simulate: Another log
    chain.add_typed_event(create_event(
        "runtime.log",
        SimpleEvent::FunctionCall {
            name: "log".to_string(),
            input: b"Second message".to_vec(),
            output: vec![],
        },
        Some("Actor log: Second message".to_string()),
    ))?;

    // Simulate: Actor complete
    chain.add_typed_event(create_event(
        "actor.complete",
        SimpleEvent::Log { message: "Actor done".to_string() },
        Some("Actor completed".to_string()),
    ))?;

    Ok((chain, rx))
}

/// Compare two chains and report differences
fn compare_chains(chain1: &StateChain<SimpleEvent>, chain2: &StateChain<SimpleEvent>) {
    let events1 = chain1.get_events();
    let events2 = chain2.get_events();

    println!("\n=== Chain Comparison ===");
    println!("Chain 1: {} events", events1.len());
    println!("Chain 2: {} events", events2.len());

    if events1.len() != events2.len() {
        println!("MISMATCH: Different number of events!");
        return;
    }

    let mut all_match = true;
    for (i, (e1, e2)) in events1.iter().zip(events2.iter()).enumerate() {
        let hash_match = e1.hash == e2.hash;
        let type_match = e1.event_type == e2.event_type;
        let data_match = e1.data == e2.data;

        if hash_match && type_match && data_match {
            println!("Event {}: MATCH ({})", i, e1.event_type);
        } else {
            all_match = false;
            println!("Event {}: MISMATCH", i);
            if !type_match {
                println!("  - type: '{}' vs '{}'", e1.event_type, e2.event_type);
            }
            if !data_match {
                println!("  - data differs");
            }
            if !hash_match {
                println!("  - hash: {} vs {}", hex::encode(&e1.hash), hex::encode(&e2.hash));
            }
        }
    }

    if all_match {
        println!("\nAll events match! Chains are identical.");
    } else {
        println!("\nChains have differences.");
    }
}

// =============================================================================
// Replay Verifier - compares chains as we go
// =============================================================================

/// ReplayVerifier compares a running chain against an expected chain.
///
/// As the actor runs and creates events, we compare each new event's hash
/// against the expected chain. If they match, we continue. If they don't,
/// we've diverged.
///
/// The next unverified event in the expected chain tells us what output
/// to return for the next host function call.
pub struct ReplayVerifier {
    /// The expected chain we're replaying against
    expected: Vec<ChainEvent>,
    /// How many events have been verified so far
    verified_count: usize,
}

impl ReplayVerifier {
    /// Create a new verifier from an expected chain
    pub fn new(expected: Vec<ChainEvent>) -> Self {
        Self {
            expected,
            verified_count: 0,
        }
    }

    /// Verify that the running chain matches the expected chain so far.
    /// Returns Ok(()) if all events match, or Err with the divergence point.
    pub fn verify_against(&mut self, running: &[ChainEvent]) -> Result<()> {
        // Check each event in the running chain against expected
        for (i, running_event) in running.iter().enumerate() {
            if i >= self.expected.len() {
                return Err(anyhow!(
                    "Running chain has more events ({}) than expected ({})",
                    running.len(),
                    self.expected.len()
                ));
            }

            let expected_event = &self.expected[i];

            if running_event.hash != expected_event.hash {
                return Err(anyhow!(
                    "Chain diverged at event {}: expected hash {}, got {}",
                    i,
                    hex::encode(&expected_event.hash),
                    hex::encode(&running_event.hash)
                ));
            }
        }

        self.verified_count = running.len();
        Ok(())
    }

    /// Get the next expected event (the one we haven't verified yet).
    /// This is what we use to get the output bytes for replay.
    pub fn next_expected(&self) -> Option<&ChainEvent> {
        self.expected.get(self.verified_count)
    }

    /// Get the output bytes from the next expected host function call.
    /// Assumes the next event contains a HostFunctionCall in its data.
    pub fn next_output_bytes(&self) -> Option<Vec<u8>> {
        let event = self.next_expected()?;

        // The event.data contains the serialized SimpleEvent (or in real usage, the event payload)
        // For a HostFunctionCall, we'd extract the output field
        // For now, just return the raw data
        Some(event.data.clone())
    }

    /// Check if replay is complete (all events verified)
    pub fn is_complete(&self) -> bool {
        self.verified_count >= self.expected.len()
    }

    /// Get verification progress
    pub fn progress(&self) -> (usize, usize) {
        (self.verified_count, self.expected.len())
    }
}

/// Demonstrate the replay verifier
async fn demo_replay_verifier() -> Result<()> {
    println!("\n=== Replay Verifier Demo ===\n");

    let actor_id = TheaterId::generate();

    // Create the "expected" chain (first run)
    println!("Creating expected chain...");
    let (expected_chain, _rx1) = simulate_actor_run(actor_id.clone()).await?;
    let expected_events = expected_chain.get_events().to_vec();
    println!("Expected chain has {} events", expected_events.len());

    // Create a verifier
    let mut verifier = ReplayVerifier::new(expected_events);

    // Simulate replay: create events one by one and verify
    println!("\nSimulating replay...");
    let (tx, _rx2) = mpsc::channel(10);
    let mut running_chain: StateChain<SimpleEvent> = StateChain::new(actor_id.clone(), tx);

    // Add first event
    running_chain.add_typed_event(create_event(
        "actor.init",
        SimpleEvent::Log { message: "Actor starting".to_string() },
        Some("Actor initialization".to_string()),
    ))?;

    let running_events = running_chain.get_events();
    match verifier.verify_against(running_events) {
        Ok(()) => println!("  Event 1: verified ✓"),
        Err(e) => println!("  Event 1: FAILED - {}", e),
    }

    // Add second event
    running_chain.add_typed_event(create_event(
        "runtime.log",
        SimpleEvent::FunctionCall {
            name: "log".to_string(),
            input: b"Hello from actor".to_vec(),
            output: vec![],
        },
        Some("Actor log: Hello from actor".to_string()),
    ))?;

    let running_events = running_chain.get_events();
    match verifier.verify_against(running_events) {
        Ok(()) => println!("  Event 2: verified ✓"),
        Err(e) => println!("  Event 2: FAILED - {}", e),
    }

    // Add third event (let's intentionally make it different to see divergence)
    running_chain.add_typed_event(create_event(
        "runtime.log",
        SimpleEvent::FunctionCall {
            name: "log".to_string(),
            input: b"DIFFERENT MESSAGE".to_vec(),  // Different from expected!
            output: vec![],
        },
        Some("Actor log: DIFFERENT MESSAGE".to_string()),
    ))?;

    let running_events = running_chain.get_events();
    match verifier.verify_against(running_events) {
        Ok(()) => println!("  Event 3: verified ✓"),
        Err(e) => println!("  Event 3: DIVERGED - {}", e),
    }

    let (verified, total) = verifier.progress();
    println!("\nProgress: {}/{} events verified", verified, total);

    Ok(())
}

/// Print a chain in a readable format
fn print_chain(chain: &StateChain<SimpleEvent>) {
    println!("\n=== Chain Events ===");
    for (i, event) in chain.get_events().iter().enumerate() {
        println!(
            "{}: [{}] {} - {:?}",
            i,
            hex::encode(&event.hash[..8.min(event.hash.len())]),
            event.event_type,
            event.description.as_deref().unwrap_or("no description")
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("=== Theater Replay Experimenting ===\n");

    // Use the same actor ID for both runs to simulate replay
    let actor_id = TheaterId::generate();
    println!("Actor ID: {}", actor_id);

    // Run 1
    println!("\n--- Run 1 ---");
    let (chain1, _rx1) = simulate_actor_run(actor_id.clone()).await?;
    print_chain(&chain1);

    // Run 2 (should produce identical chain)
    println!("\n--- Run 2 ---");
    let (chain2, _rx2) = simulate_actor_run(actor_id.clone()).await?;
    print_chain(&chain2);

    // Compare
    compare_chains(&chain1, &chain2);

    // Verify both chains
    println!("\n=== Verification ===");
    println!("Chain 1 valid: {}", chain1.verify());
    println!("Chain 2 valid: {}", chain2.verify());

    // Small delay to let background tasks complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Demo the replay verifier
    demo_replay_verifier().await?;

    // Small delay to let background tasks complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(())
}
