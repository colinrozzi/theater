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
    let (theater_tx, _) = mpsc::unbounded_channel::<TheaterCommand>();
    let (operation_tx, _) = mpsc::channel(10);
    let (info_tx, _) = mpsc::channel(10);
    let (control_tx, _) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id)));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(actor_id, theater_tx.clone(), actor_handle, chain);

    let mut instance = PackInstance::new(
        "pact-contract-test",
        &wasm_bytes,
        &runtime,
        actor_store,
        |builder| {
            builder.interface("theater:simple/self")?.func_typed(
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

/// Count the todo items in a `list` return value or an `actor-state` record.
fn todo_items(value: &Value) -> Vec<Value> {
    match value {
        // `list` returns list<todo-item> directly.
        Value::List { items, .. } => items.clone(),
        // `actor-state` holds them under the `items` field.
        Value::Record { fields, .. } => fields
            .iter()
            .find(|(n, _)| n == "items")
            .and_then(|(_, v)| match v {
                Value::List { items, .. } => Some(items.clone()),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn item_done(item: &Value) -> bool {
    match item {
        Value::Record { fields, .. } => fields
            .iter()
            .find(|(n, _)| n == "done")
            .map(|(_, v)| matches!(v, Value::Bool(true)))
            .unwrap_or(false),
        _ => false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pact_file_todo_actor() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // init sets the actor's in-module todo-list state; returns unit.
    instance
        .call_function("theater:simple/actor.init", vec![])
        .await
        .expect("init should succeed");

    // add(string) -> todo-item (types defined in the external types.pact).
    let bytes = instance
        .call_function_with_value(
            "theater:todo/actions.add",
            Value::Tuple(vec![Value::String("Buy milk".into())]),
        )
        .await
        .expect("add should succeed");
    let added = packr::abi::decode(&bytes).expect("decode todo-item");
    match &added {
        Value::Record { type_name, .. } => assert_eq!(type_name, "todo-item"),
        _ => panic!("add should return a todo-item, got {:?}", added),
    }

    instance
        .call_function_with_value(
            "theater:todo/actions.add",
            Value::Tuple(vec![Value::String("Write tests".into())]),
        )
        .await
        .expect("add should succeed");

    // list() -> list<todo-item>: both items present.
    let bytes = instance
        .call_function_with_value("theater:todo/actions.list", Value::Tuple(vec![]))
        .await
        .expect("list should succeed");
    let listed = packr::abi::decode(&bytes).expect("decode list");
    assert_eq!(todo_items(&listed).len(), 2, "two todos after two adds");

    // toggle(id=1), then confirm item 1 is done — both via list and get-state.
    instance
        .call_function_with_value(
            "theater:todo/actions.toggle",
            Value::Tuple(vec![Value::U32(1)]),
        )
        .await
        .expect("toggle should succeed");

    let bytes = instance
        .call_function_with_value("theater:todo/actions.list", Value::Tuple(vec![]))
        .await
        .expect("list should succeed after toggle");
    let listed = packr::abi::decode(&bytes).expect("decode list");
    let items = todo_items(&listed);
    assert!(items.iter().any(item_done), "toggled item should be done");

    // The in-module state agrees with the action returns.
    let state = instance
        .call_value("theater:simple/actor.get-state", &Value::Tuple(vec![]))
        .await
        .expect("get-state should succeed");
    match &state {
        Value::Record { type_name, .. } => assert_eq!(type_name, "actor-state"),
        _ => panic!("Expected actor-state record, got {:?}", state),
    }
    assert_eq!(
        todo_items(&state).len(),
        2,
        "get-state reflects the two adds"
    );

    info!("Pact file contract test passed!");
}
