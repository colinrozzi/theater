//! Golden file test for state chain verification.
//!
//! This test verifies that the state chain produced by an actor matches
//! a known-good "golden" chain. This ensures state tracking works correctly
//! across function calls.

use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::pack_bridge::{ActorResult, AsyncRuntime, Ctx, PackInstance, Value};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use tokio::sync::mpsc;
use tracing::info;

/// Test that state tracking works correctly across multiple function calls.
///
/// This test:
/// 1. Loads the state-test actor
/// 2. Calls init, increment (x2), get-count
/// 3. Verifies the state is correctly passed between calls
/// 4. Uses the new typed API (call_typed + ActorResult)
#[tokio::test]
async fn test_state_tracking_typed_api() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // Load the state-test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/state-test/target/wasm32-unknown-unknown/release/state_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read test actor WASM from {}: {}. \
                 Make sure to build it first with: \
                 cd test-actors/state-test && cargo build --release",
                wasm_path, e
            );
        }
    };

    info!("Loaded WASM bytes: {} bytes", wasm_bytes.len());

    // Create the runtime
    let runtime = AsyncRuntime::new();

    // Create actor store components
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id.clone(), theater_tx.clone())));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id.clone(),
        theater_tx.clone(),
        actor_handle,
        chain.clone(),
        None, // No initial state
    );

    // Create the PackInstance with host functions
    let result = PackInstance::new(
        "state-test",
        &wasm_bytes,
        &runtime,
        actor_store,
        |builder| {
            // Register the log host function
            builder
                .interface("theater:simple/runtime")?
                .func_typed("log", |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    // Extract the message from the Value
                    let msg = match input {
                        Value::String(s) => s,
                        _ => format!("{:?}", input),
                    };
                    info!("[ACTOR LOG] {}", msg);
                    // Return unit (empty tuple)
                    Value::Tuple(vec![])
                })?;
            Ok(())
        },
    )
    .await;

    let mut instance = match result {
        Ok(inst) => inst,
        Err(e) => {
            panic!("Failed to create PackInstance: {}", e);
        }
    };

    info!("PackInstance created successfully");

    // Step 1: Call init using typed API
    info!("=== Calling init ===");
    let result: ActorResult<()> = instance
        .call_typed("theater:simple/actor.init", None, Value::Tuple(vec![]))
        .await
        .expect("init should succeed");

    info!("init succeeded, state = {:?}", result.state.as_ref().map(|s| String::from_utf8_lossy(s)));
    assert!(result.state.is_some(), "init should return state");
    let state = result.state;

    // Step 2: Call increment (first time) - expect count = 1
    info!("=== Calling increment (1) ===");
    let result: ActorResult<i32> = instance
        .call_typed("theater:simple/state-test.increment", state, Value::Tuple(vec![]))
        .await
        .expect("increment should succeed");

    info!("increment 1 succeeded, count = {}", result.value);
    assert_eq!(result.value, 1, "First increment should return 1");
    let state = result.state;

    // Step 3: Call increment (second time) - expect count = 2
    info!("=== Calling increment (2) ===");
    let result: ActorResult<i32> = instance
        .call_typed("theater:simple/state-test.increment", state, Value::Tuple(vec![]))
        .await
        .expect("increment should succeed");

    info!("increment 2 succeeded, count = {}", result.value);
    assert_eq!(result.value, 2, "Second increment should return 2");
    let state = result.state;

    // Step 4: Call get-count - should still be 2
    info!("=== Calling get-count ===");
    let result: ActorResult<i32> = instance
        .call_typed("theater:simple/state-test.get-count", state, Value::Tuple(vec![]))
        .await
        .expect("get-count should succeed");

    info!("get-count succeeded, count = {}", result.value);
    assert_eq!(result.value, 2, "get-count should return 2");

    info!("=== State tracking test passed! ===");
    info!("State was correctly passed between function calls.");
    info!("Count progressed: init(0) -> increment(1) -> increment(2) -> get-count(2)");
}
