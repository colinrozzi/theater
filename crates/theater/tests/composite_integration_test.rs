//! Integration test for Composite runtime with Theater.
//!
//! This test verifies that:
//! 1. PackInstance can load a WASM module
//! 2. Host functions (like log) can be called from the actor
//! 3. Export functions (like init) can be called from the host

use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::pack_bridge::{AsyncRuntime, PackInstance, Ctx, Value};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use tokio::sync::mpsc;
use tracing::info;

#[tokio::test]
async fn test_composite_instance_basic() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // Load the test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/composite-test/target/wasm32-unknown-unknown/release/composite_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read test actor WASM from {}: {}. \
                 Make sure to build it first with: \
                 cd test-actors/composite-test && cargo build --release --target wasm32-unknown-unknown",
                wasm_path, e
            );
        }
    };

    info!("Loaded WASM bytes: {} bytes", wasm_bytes.len());

    // Create the Composite runtime
    let runtime = AsyncRuntime::new();

    // Create actor store components
    let actor_id = TheaterId::generate();
    let (theater_tx, mut theater_rx) = mpsc::channel::<TheaterCommand>(10);
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id.clone(), theater_tx.clone())));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id.clone(),
        theater_tx.clone(),
        actor_handle,
        chain,
    );

    // Create the PackInstance with host functions
    let result = PackInstance::new(
        "composite-test",
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

    // Register the init export
    instance.register_export("theater:simple/actor", "init");

    // Call the init function
    // The init function expects: Tuple(Option<List<u8>>, List<u8>)
    let state: Option<Vec<u8>> = None;
    let params: Vec<u8> = vec![];

    info!("Calling init function...");

    let result = instance.call_function("init", state, params).await;

    match result {
        Ok((new_state, result_bytes)) => {
            info!("init succeeded!");
            info!("New state: {:?}", new_state);
            info!("Result bytes: {:?}", result_bytes);
        }
        Err(e) => {
            panic!("init failed: {}", e);
        }
    }

    // Check if any TheaterCommands were sent (like shutdown)
    if let Ok(cmd) = theater_rx.try_recv() {
        info!("Received theater command: {:?}", cmd);
    }

    info!("Composite integration test passed!");
}
