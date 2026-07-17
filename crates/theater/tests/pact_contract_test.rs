//! Test for contract enforcement with types defined in an external .pact file.
//!
//! The pact-contract-test actor defines a todo list with types in types.pact.
//! This test verifies the full flow: init, add items, toggle, list.

use std::sync::Arc;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::pack_bridge::{AsyncRuntime, Ctx, PackInstance, Value};
use tokio::sync::mpsc;
use tokio::sync::RwLock as SyncRwLock;
use tracing::info;

mod common;

async fn create_instance() -> PackInstance {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/pact-contract-test/target/wasm32-unknown-unknown/release/pact_contract_test_actor.wasm"
    );

    let member = std::fs::read(wasm_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read WASM from {}: {}. \
             Build first: cd test-actors/pact-contract-test && cargo build --release --target wasm32-unknown-unknown",
            wasm_path, e
        );
    });
    let wasm_bytes = common::helpers::link_self_contained(member);

    let runtime = AsyncRuntime::new();
    let actor_id = TheaterId::generate();
    let (theater_tx, _) = mpsc::channel::<TheaterCommand>(10);
    let (operation_tx, _) = mpsc::channel(10);
    let (info_tx, _) = mpsc::channel(10);
    let (control_tx, _) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id)));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id,
        theater_tx.clone(),
        actor_handle,
        chain,
        Value::Tuple(vec![]),
    );

    let mut instance = PackInstance::new(
        "pact-contract-test",
        &wasm_bytes,
        &runtime,
        actor_store,
        |builder| {
            builder.interface("theater:simple/runtime")?.func_typed(
                "log",
                |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    let msg = match input {
                        Value::String(s) => s,
                        _ => format!("{:?}", input),
                    };
                    info!("[ACTOR LOG] {}", msg);
                    Value::Tuple(vec![])
                },
            )?;
            Ok(())
        },
    )
    .await
    .expect("Failed to create PackInstance");

    instance
        .cache_function_types()
        .await
        .expect("Failed to cache function types");
    instance
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pact_file_todo_actor() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // Init
    let state = Value::Tuple(vec![]);
    let (state, _) = instance
        .call_function("theater:simple/actor.init", state, vec![])
        .await
        .expect("init should succeed");

    info!("State after init: {:?}", state);
    match &state {
        Value::Record { type_name, .. } => assert_eq!(type_name, "actor-state"),
        _ => panic!("Expected actor-state record"),
    }

    // Add first todo
    let (state, result_bytes) = instance
        .call_function_with_value(
            "theater:todo/actions.add",
            state,
            Value::Tuple(vec![Value::String("Buy milk".into())]),
        )
        .await
        .expect("add should succeed");

    assert!(!result_bytes.is_empty(), "Should return the new todo item");
    info!("State after add 'Buy milk': {:?}", state);

    // Add second todo
    let (state, _) = instance
        .call_function_with_value(
            "theater:todo/actions.add",
            state,
            Value::Tuple(vec![Value::String("Write tests".into())]),
        )
        .await
        .expect("add should succeed");

    // List todos
    let (state, result_bytes) = instance
        .call_function_with_value("theater:todo/actions.list", state, Value::Tuple(vec![]))
        .await
        .expect("list should succeed");

    // Decode and check the list
    let result_value = packr::abi::decode(&result_bytes).expect("decode result");
    info!("Todo list: {:?}", result_value);

    // Toggle first todo
    let (state, _) = instance
        .call_function_with_value(
            "theater:todo/actions.toggle",
            state,
            Value::Tuple(vec![Value::U32(1)]),
        )
        .await
        .expect("toggle should succeed");

    // List again to verify toggle
    let (_state, result_bytes) = instance
        .call_function_with_value("theater:todo/actions.list", state, Value::Tuple(vec![]))
        .await
        .expect("list should succeed after toggle");

    let result_value = packr::abi::decode(&result_bytes).expect("decode result");
    info!("Todo list after toggle: {:?}", result_value);

    info!("Pact file contract test passed!");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pact_file_rejects_wrong_types() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // Get valid state
    let state = Value::Tuple(vec![]);
    let (state, _) = instance
        .call_function("theater:simple/actor.init", state, vec![])
        .await
        .expect("init should succeed");

    // Try to call add with wrong param type (u32 instead of string)
    let result = instance
        .call_function_with_value(
            "theater:todo/actions.toggle",
            Value::String("not a state".into()), // wrong state type
            Value::Tuple(vec![Value::U32(1)]),
        )
        .await;

    assert!(result.is_err(), "Should reject wrong state type");
    let err = result.unwrap_err().to_string();
    info!("Correctly rejected: {}", err);
    assert!(err.contains("State type mismatch"));
}
