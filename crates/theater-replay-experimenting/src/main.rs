//! Theater Replay Experimenting
//!
//! This crate is for experimenting with replay functionality.
//! We can create chains, compare them, and develop the replay system.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use theater::chain::{ChainEvent, StateChain};
use theater::events::ChainEventData;
use theater::id::TheaterId;
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store};

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

// =============================================================================
// ReplayHandler Design Sketch
// =============================================================================

/// The ReplayHandler is a special handler that:
/// 1. Reads a chain to know what functions were called
/// 2. Registers host functions that return recorded outputs
/// 3. Compares the running chain against the expected chain
///
/// Key insight: We extract interface/function info from the component's WIT,
/// then for each call, we look up the next expected event and return its output.
///
/// ```text
/// Component WIT says it imports:
///   - theater:simple/runtime (log, shutdown, get-chain)
///   - theater:simple/timing (now, sleep)
///
/// Chain contains events like:
///   Event 0: { type: "actor.init", data: ... }
///   Event 1: { type: "theater:simple/runtime/log", data: HostFunctionCall { output: [] } }
///   Event 2: { type: "theater:simple/timing/now", data: HostFunctionCall { output: [timestamp bytes] } }
///   ...
///
/// ReplayHandler.setup_host_functions():
///   For each interface the component imports:
///     linker.instance("theater:simple/timing")
///       .func_wrap("now", |ctx| {
///           // Get next expected event from verifier
///           let expected = verifier.next_expected();
///           // Extract HostFunctionCall from expected.data
///           let call: HostFunctionCall = deserialize(expected.data);
///           // Return the recorded output bytes
///           return deserialize_output(call.output);
///       });
/// ```
///
/// The beauty is:
/// - The runtime records events as usual
/// - We compare hashes after each event
/// - If they match, we're on track
/// - If they diverge, we detect it immediately
#[allow(dead_code)]
pub struct ReplayHandler {
    /// The expected chain we're replaying
    expected_chain: Vec<ChainEvent>,
    /// Current position in the chain
    position: std::sync::atomic::AtomicUsize,
}

#[allow(dead_code)]
impl ReplayHandler {
    pub fn new(expected_chain: Vec<ChainEvent>) -> Self {
        Self {
            expected_chain,
            position: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Get the next expected event
    pub fn next_expected(&self) -> Option<&ChainEvent> {
        let pos = self.position.load(std::sync::atomic::Ordering::SeqCst);
        self.expected_chain.get(pos)
    }

    /// Advance to the next event after verification
    pub fn advance(&self) {
        self.position.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Get current position
    pub fn current_position(&self) -> usize {
        self.position.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Verify current event matches expected
    pub fn verify_event(&self, actual: &ChainEvent) -> Result<()> {
        let pos = self.current_position();
        let expected = self.expected_chain.get(pos)
            .ok_or_else(|| anyhow!("No more expected events at position {}", pos))?;

        if actual.hash != expected.hash {
            return Err(anyhow!(
                "Hash mismatch at position {}: expected {}, got {}",
                pos,
                hex::encode(&expected.hash),
                hex::encode(&actual.hash)
            ));
        }

        Ok(())
    }
}

/// Example of what a replay host function would look like.
/// This is pseudocode showing the concept:
///
/// ```ignore
/// // In setup_host_functions for ReplayHandler:
/// interface.func_wrap("now", move |ctx: StoreContextMut<'_, ActorStore<E>>| {
///     // 1. Get the next expected HostFunctionCall from the chain
///     let expected_event = replay_handler.next_expected()
///         .ok_or_else(|| anyhow!("Unexpected call to 'now' - no more events"))?;
///
///     // 2. Parse the HostFunctionCall from the event data
///     let host_call: HostFunctionCall = serde_json::from_slice(&expected_event.data)?;
///
///     // 3. Verify this is the right function
///     if host_call.function != "now" {
///         return Err(anyhow!("Expected call to '{}', got 'now'", host_call.function));
///     }
///
///     // 4. Deserialize the recorded output
///     let output: u64 = serde_json::from_slice(&host_call.output)?;
///
///     // 5. Record an event (the runtime does this, creating a new chain event)
///     ctx.data_mut().record_event(...);
///
///     // 6. After event is recorded, verify the hash matches
///     let running_chain = ctx.data().get_chain();
///     replay_handler.verify_event(running_chain.last())?;
///
///     // 7. Advance position and return the recorded output
///     replay_handler.advance();
///     Ok(output)
/// });
/// ```
#[allow(dead_code)]
fn _example_replay_function() {
    // This function exists just for documentation
}

// =============================================================================
// Component Introspection - Extract imports from WASM components
// =============================================================================

/// Information about a function imported by a component
#[derive(Debug, Clone)]
pub struct ImportedFunction {
    /// Interface name (e.g., "theater:simple/runtime")
    pub interface: String,
    /// Function name (e.g., "log")
    pub function: String,
    /// Number of parameters
    pub param_count: usize,
    /// Number of results
    pub result_count: usize,
}

/// Information about all imports of a component
#[derive(Debug)]
pub struct ComponentImports {
    /// All interfaces imported by the component
    pub interfaces: Vec<String>,
    /// All functions imported by the component
    pub functions: Vec<ImportedFunction>,
}

/// Load a WASM component and extract its imports
pub fn extract_component_imports(component_path: &Path) -> Result<ComponentImports> {
    // Create engine with component model support
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config)?;

    // Load the component
    let wasm_bytes = std::fs::read(component_path)?;
    let component = Component::new(&engine, &wasm_bytes)?;

    // Get component type for introspection
    let component_type = component.component_type();

    let mut interfaces = Vec::new();
    let mut functions = Vec::new();

    // Iterate over all imports
    for (import_name, import_item) in component_type.imports(&engine) {
        match import_item {
            ComponentItem::ComponentInstance(instance_type) => {
                // This is an interface import (like "theater:simple/runtime")
                interfaces.push(import_name.to_string());

                // Iterate over exports of this instance (the functions it provides)
                for (func_name, export_item) in instance_type.exports(&engine) {
                    if let ComponentItem::ComponentFunc(func_type) = export_item {
                        functions.push(ImportedFunction {
                            interface: import_name.to_string(),
                            function: func_name.to_string(),
                            param_count: func_type.params().len(),
                            result_count: func_type.results().len(),
                        });
                    }
                }
            }
            ComponentItem::ComponentFunc(func_type) => {
                // Direct function import (less common)
                functions.push(ImportedFunction {
                    interface: "".to_string(),
                    function: import_name.to_string(),
                    param_count: func_type.params().len(),
                    result_count: func_type.results().len(),
                });
            }
            _ => {
                // Other import types (resources, types, etc.)
                println!("  Other import: {} ({:?})", import_name, import_item);
            }
        }
    }

    Ok(ComponentImports {
        interfaces,
        functions,
    })
}

/// Demo: Load a component and show its imports
async fn demo_component_introspection() -> Result<()> {
    println!("\n=== Component Introspection Demo ===\n");

    // Try to find the runtime-test component
    let component_path = Path::new("/Users/colinrozzi/work/theater/crates/theater-handler-runtime/test-actors/runtime-test/target/wasm32-wasip1/release/runtime_test.wasm");

    if !component_path.exists() {
        println!("Component not found at: {}", component_path.display());
        println!("Skipping introspection demo.");
        return Ok(());
    }

    println!("Loading component: {}", component_path.display());

    let imports = extract_component_imports(component_path)?;

    println!("\nImported interfaces ({}):", imports.interfaces.len());
    for interface in &imports.interfaces {
        println!("  - {}", interface);
    }

    println!("\nImported functions ({}):", imports.functions.len());
    for func in &imports.functions {
        println!(
            "  - {}::{} (params: {}, results: {})",
            func.interface, func.function, func.param_count, func.result_count
        );
    }

    // Now let's show how we'd register stub functions for replay
    println!("\n--- Replay Stub Registration (conceptual) ---");
    println!("For each imported function, we would register a stub that:");
    println!("1. Looks up the next expected event in the chain");
    println!("2. Extracts the recorded output bytes");
    println!("3. Deserializes and returns that output");
    println!("4. Verifies the hash matches after the event is recorded");

    Ok(())
}

// =============================================================================
// Dynamic Stub Registration - Register replay functions for a component
// =============================================================================

/// Recorded output from a host function call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionCall {
    /// Interface name (e.g., "theater:simple/runtime")
    pub interface: String,
    /// Function name (e.g., "log")
    pub function: String,
    /// Input parameters as serialized bytes
    pub input: Vec<u8>,
    /// Output result as serialized bytes
    pub output: Vec<u8>,
}

/// State for the replay - tracks position in the chain and provides outputs
#[derive(Clone)]
pub struct ReplayState {
    /// The expected chain events
    events: Arc<Vec<ChainEvent>>,
    /// Current position in the chain
    position: Arc<Mutex<usize>>,
}

impl ReplayState {
    pub fn new(events: Vec<ChainEvent>) -> Self {
        Self {
            events: Arc::new(events),
            position: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the current event and what function it represents
    pub fn current_event(&self) -> Option<ChainEvent> {
        let pos = *self.position.lock().unwrap();
        self.events.get(pos).cloned()
    }

    /// Get the output bytes for the current event (assuming it's a HostFunctionCall)
    pub fn current_output(&self) -> Option<Vec<u8>> {
        let event = self.current_event()?;
        // Try to parse as HostFunctionCall and extract output
        if let Ok(call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
            Some(call.output)
        } else {
            // Fallback: return raw data
            Some(event.data)
        }
    }

    /// Advance to the next event
    pub fn advance(&self) {
        let mut pos = self.position.lock().unwrap();
        *pos += 1;
    }

    /// Get current position
    pub fn current_position(&self) -> usize {
        *self.position.lock().unwrap()
    }

    /// Check if we've processed all events
    pub fn is_complete(&self) -> bool {
        self.current_position() >= self.events.len()
    }
}

/// Demo: Set up a linker with stub functions for a component
async fn demo_stub_registration() -> Result<()> {
    println!("\n=== Dynamic Stub Registration Demo ===\n");

    // Create engine
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config)?;

    // Load the runtime-test component
    let component_path = Path::new("/Users/colinrozzi/work/theater/crates/theater-handler-runtime/test-actors/runtime-test/target/wasm32-wasip1/release/runtime_test.wasm");

    if !component_path.exists() {
        println!("Component not found, skipping demo.");
        return Ok(());
    }

    let wasm_bytes = std::fs::read(component_path)?;
    let component = Component::new(&engine, &wasm_bytes)?;
    let component_type = component.component_type();

    // Create a linker
    let mut linker: Linker<ReplayState> = Linker::new(&engine);

    // Create a fake chain with some recorded events
    let fake_chain = create_fake_chain_events();
    println!("Created fake chain with {} events", fake_chain.len());

    // Create replay state
    let replay_state = ReplayState::new(fake_chain);

    // Now register stub functions for each imported interface
    println!("\nRegistering stub functions...");

    for (import_name, import_item) in component_type.imports(&engine) {
        if let ComponentItem::ComponentInstance(instance_type) = import_item {
            println!("\n  Interface: {}", import_name);

            // Get or create the interface in the linker
            let mut interface = match linker.instance(&import_name) {
                Ok(i) => i,
                Err(_) => {
                    println!("    (creating new instance)");
                    linker.instance(&import_name)?
                }
            };

            // Register each function in the interface
            for (func_name, export_item) in instance_type.exports(&engine) {
                if let ComponentItem::ComponentFunc(func_type) = export_item {
                    let full_name = format!("{}::{}", import_name, func_name);
                    let result_types: Vec<_> = func_type.results().collect();
                    let result_count = result_types.len();

                    println!(
                        "    - {} (params: {}, results: {})",
                        func_name,
                        func_type.params().len(),
                        result_count
                    );

                    // Clone values for the closure
                    let func_name_clone = func_name.to_string();
                    let full_name_clone = full_name.clone();

                    // Register a stub function that returns default values
                    // In real replay, we'd extract the actual output from the chain
                    interface.func_new_async(
                        &func_name,
                        move |_ctx, _params, results| {
                            let func_name = func_name_clone.clone();
                            let full_name = full_name_clone.clone();
                            Box::new(async move {
                                // In real replay, we would:
                                // 1. Get the current event from replay_state
                                // 2. Parse the HostFunctionCall from event.data
                                // 3. Deserialize output bytes to the correct types
                                // 4. Advance replay_state position

                                // For now, just log and return empty results
                                println!("      [REPLAY] {} called", full_name);

                                // Fill results with defaults (this is a simplification)
                                // Real implementation would deserialize from chain
                                for i in 0..results.len() {
                                    // Default to unit for now - real impl would check types
                                    results[i] = Val::Bool(false);
                                }

                                Ok(())
                            })
                        },
                    )?;
                }
            }
        }
    }

    println!("\n✓ All stub functions registered!");
    println!("\nNote: In a real replay scenario, each stub would:");
    println!("  1. Look up the next expected event in the chain");
    println!("  2. Verify it matches the expected function call");
    println!("  3. Deserialize the recorded output bytes");
    println!("  4. Return those bytes as the function result");

    // We can't actually run the component yet because we haven't implemented
    // all the WASI functions properly. But we've demonstrated the pattern!

    Ok(())
}

/// Create some fake chain events for testing
fn create_fake_chain_events() -> Vec<ChainEvent> {
    vec![
        ChainEvent {
            hash: vec![1, 2, 3, 4],
            parent_hash: None,
            event_type: "actor.init".to_string(),
            data: serde_json::to_vec(&HostFunctionCall {
                interface: "".to_string(),
                function: "init".to_string(),
                input: vec![],
                output: vec![],
            })
            .unwrap(),
            timestamp: 0,
            description: Some("Actor initialization".to_string()),
        },
        ChainEvent {
            hash: vec![5, 6, 7, 8],
            parent_hash: Some(vec![1, 2, 3, 4]),
            event_type: "theater:simple/runtime/log".to_string(),
            data: serde_json::to_vec(&HostFunctionCall {
                interface: "theater:simple/runtime".to_string(),
                function: "log".to_string(),
                input: b"Hello from replay!".to_vec(),
                output: vec![],
            })
            .unwrap(),
            timestamp: 0,
            description: Some("Log call".to_string()),
        },
    ]
}

// =============================================================================
// ReplayHandler - A Handler implementation for replaying from chains
// =============================================================================

/// ReplayHandler implements the Handler trait to provide replay functionality.
///
/// ## How It Works
///
/// 1. **Initialization**: Given a chain of events, the ReplayHandler knows what
///    host function calls were made and what they returned.
///
/// 2. **Dynamic Import Discovery**: When `setup_host_functions` is called,
///    it inspects the component to find what interfaces it imports.
///
/// 3. **Stub Registration**: For each imported function, it registers a stub
///    that returns the recorded output from the chain.
///
/// 4. **Verification**: As the component runs, each event is compared against
///    the expected chain. Hash mismatches indicate divergence.
///
/// ## Event Type Naming Convention
///
/// Chain events use a naming convention to identify which function was called:
/// - `{interface}/{function}` for theater interfaces
/// - e.g., `theater:simple/runtime/log`
/// - e.g., `theater:simple/timing/now`
///
/// ## Integration Pattern
///
/// ```ignore
/// // Create handler with expected chain
/// let replay_handler = ReplayHandler::new(expected_chain);
///
/// // Register with HandlerRegistry (replaces normal handlers)
/// registry.register(replay_handler);
///
/// // Actor runs using ReplayHandler instead of real handlers
/// // Each host call returns recorded output instead of actually executing
/// ```
pub struct FullReplayHandler {
    /// The expected chain we're replaying against
    expected_chain: Vec<ChainEvent>,
    /// Interfaces this handler will satisfy (discovered from chain)
    interfaces: Vec<String>,
    /// Current position in the chain (shared across all stub functions)
    position: Arc<Mutex<usize>>,
}

impl FullReplayHandler {
    /// Create a new ReplayHandler from a chain of events
    pub fn new(expected_chain: Vec<ChainEvent>) -> Self {
        // Extract unique interfaces from the chain events
        let interfaces: Vec<String> = expected_chain
            .iter()
            .filter_map(|event| {
                // Parse event_type to extract interface
                // Format: "interface/function" or just "event.type"
                if event.event_type.contains('/') {
                    // Split off the function name, keep interface
                    let parts: Vec<&str> = event.event_type.rsplitn(2, '/').collect();
                    if parts.len() == 2 {
                        Some(parts[1].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        Self {
            expected_chain,
            interfaces,
            position: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the next expected event
    pub fn next_expected(&self) -> Option<&ChainEvent> {
        let pos = *self.position.lock().unwrap();
        self.expected_chain.get(pos)
    }

    /// Get output bytes for the current event
    pub fn current_output(&self) -> Option<Vec<u8>> {
        let event = self.next_expected()?;
        if let Ok(call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
            Some(call.output)
        } else {
            Some(event.data.clone())
        }
    }

    /// Advance to the next event
    pub fn advance(&self) {
        let mut pos = self.position.lock().unwrap();
        *pos += 1;
    }

    /// Verify that an actual event matches the expected event
    pub fn verify_event(&self, actual_hash: &[u8]) -> Result<()> {
        let pos = *self.position.lock().unwrap();
        let expected = self.expected_chain.get(pos)
            .ok_or_else(|| anyhow!("No expected event at position {}", pos))?;

        if actual_hash != expected.hash {
            return Err(anyhow!(
                "Hash mismatch at position {}: expected {}, got {}",
                pos,
                hex::encode(&expected.hash),
                hex::encode(actual_hash)
            ));
        }

        Ok(())
    }

    /// Get progress
    pub fn progress(&self) -> (usize, usize) {
        let pos = *self.position.lock().unwrap();
        (pos, self.expected_chain.len())
    }
}

/// Demo showing how FullReplayHandler would work
async fn demo_full_replay_handler() -> Result<()> {
    println!("\n=== Full ReplayHandler Demo ===\n");

    // Create a chain from a real run (simulated here)
    println!("Step 1: Create chain from original run");
    let actor_id = TheaterId::generate();
    let (original_chain, _rx) = simulate_actor_run(actor_id.clone()).await?;
    let events = original_chain.get_events().to_vec();
    println!("  Original chain has {} events", events.len());

    // Create the replay handler
    println!("\nStep 2: Create ReplayHandler from chain");
    let replay_handler = FullReplayHandler::new(events);
    println!("  Discovered interfaces: {:?}", replay_handler.interfaces);

    // Simulate replay by stepping through events
    println!("\nStep 3: Simulate replay verification");
    for i in 0..4 {
        if let Some(expected) = replay_handler.next_expected() {
            println!("  Event {}: {} (hash: {})",
                i,
                expected.event_type,
                hex::encode(&expected.hash[..8.min(expected.hash.len())])
            );

            // In real replay:
            // 1. Component calls host function
            // 2. Stub returns recorded output from chain
            // 3. Runtime records new event
            // 4. We compare new event hash with expected
            // 5. Advance if match, error if diverge

            replay_handler.advance();
        }
    }

    let (verified, total) = replay_handler.progress();
    println!("\n  Verified {}/{} events", verified, total);

    // Show how this would integrate with Handler trait
    println!("\n--- Handler Trait Integration ---");
    println!("The FullReplayHandler would implement Handler<E>:");
    println!("  - setup_host_functions(): Register stubs for all imported interfaces");
    println!("  - start(): No-op (replay doesn't need background tasks)");
    println!("  - imports(): Return list of interfaces discovered from chain");
    println!("  - exports(): None (replay doesn't export anything)");

    Ok(())
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

    // Demo component introspection
    demo_component_introspection().await?;

    // Demo stub registration
    demo_stub_registration().await?;

    // Demo full replay handler
    demo_full_replay_handler().await?;

    Ok(())
}
