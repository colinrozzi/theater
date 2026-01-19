//! Integration test for multiple Theater handlers with Composite runtime.
//!
//! This test verifies that:
//! 1. Multiple handlers can be set up together (runtime, store, supervisor)
//! 2. An actor can call host functions from all handlers
//! 3. The Composite runtime properly routes calls to the correct handler

use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::composite_bridge::{AsyncRuntime, CompositeInstance, Ctx, Value};
use theater::config::actor_manifest::{StoreHandlerConfig, SupervisorHostConfig};
use theater::handler::{Handler, HandlerContext};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use tokio::sync::mpsc;
use tracing::info;

#[tokio::test]
async fn test_multi_handler_composite() {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // Load the test actor WASM
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/multi-handler-test/target/wasm32-unknown-unknown/release/multi_handler_test_actor.wasm"
    );

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            panic!(
                "Failed to read test actor WASM from {}: {}. \
                 Make sure to build it first with: \
                 cd test-actors/multi-handler-test && cargo build --release --target wasm32-unknown-unknown",
                wasm_path, e
            );
        }
    };

    info!("Loaded multi-handler test WASM: {} bytes", wasm_bytes.len());

    // Create the Composite runtime
    let runtime = AsyncRuntime::new();

    // Create actor store components
    let actor_id = TheaterId::generate();
    let (theater_tx, mut _theater_rx) = mpsc::channel::<TheaterCommand>(10);
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id.clone(), theater_tx.clone())));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id.clone(),
        theater_tx.clone(),
        actor_handle.clone(),
        chain.clone(),
    );

    // Create handler instances
    let mut store_handler = StoreHandler::new(StoreHandlerConfig {}, None);
    let mut supervisor_handler = SupervisorHandler::new(SupervisorHostConfig {}, None);

    // Create handler context for tracking satisfied imports
    let mut handler_ctx = HandlerContext::new();

    // Create the CompositeInstance with all handlers
    let result = CompositeInstance::new(
        "multi-handler-test",
        &wasm_bytes,
        &runtime,
        actor_store,
        |builder| {
            // Register runtime handler (log function)
            builder
                .interface("theater:simple/runtime")?
                .func_typed("log", |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    let msg = match input {
                        Value::String(s) => s,
                        _ => format!("{:?}", input),
                    };
                    info!("[ACTOR LOG] {}", msg);
                    Value::Tuple(vec![])
                })?;

            // Register store handler
            store_handler.setup_host_functions_composite(builder, &mut handler_ctx)?;

            // Register supervisor handler
            supervisor_handler.setup_host_functions_composite(builder, &mut handler_ctx)?;

            Ok(())
        },
    )
    .await;

    let mut instance = match result {
        Ok(inst) => inst,
        Err(e) => {
            panic!("Failed to create CompositeInstance: {}", e);
        }
    };

    info!("CompositeInstance created with multiple handlers");

    // Register the init export
    instance.register_export("theater:simple/actor", "init");

    // Call the init function
    let state: Option<Vec<u8>> = None;
    let params: Vec<u8> = vec![];

    info!("Calling init function to test all handlers...");
    info!("========================================");

    let result = instance.call_function("init", state, params).await;

    info!("========================================");

    match result {
        Ok((new_state, _result_bytes)) => {
            info!("Multi-handler test PASSED!");
            info!("Final state: {:?}", new_state);
        }
        Err(e) => {
            panic!("Multi-handler test FAILED: {}", e);
        }
    }
}
