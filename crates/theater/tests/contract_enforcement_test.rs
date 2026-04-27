//! Contract enforcement integration test.
//!
//! Tests that the runtime validates type contracts:
//! 1. Valid typed calls succeed (records, variants, nested types)
//! 2. Invalid state types are rejected before entering WASM
//! 3. Return types are validated after the call

use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::pack_bridge::{AsyncRuntime, Ctx, PackInstance, Value};
use tokio::sync::mpsc;
use tracing::info;

/// Helper to create a PackInstance from the contract-test actor
async fn create_instance() -> PackInstance {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/contract-test/target/wasm32-unknown-unknown/release/contract_test_actor.wasm"
    );

    let wasm_bytes = std::fs::read(wasm_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read contract-test WASM from {}: {}. \
             Build it first: cd test-actors/contract-test && cargo build --release --target wasm32-unknown-unknown",
            wasm_path, e
        );
    });

    let runtime = AsyncRuntime::new();
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel::<TheaterCommand>(10);
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(
        actor_id,
        theater_tx.clone(),
    )));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id,
        theater_tx.clone(),
        actor_handle,
        chain,
        Value::Tuple(vec![]),
    );

    let mut instance = PackInstance::new(
        "contract-test",
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

    // Cache types so validation is active
    instance
        .cache_function_types()
        .await
        .expect("Failed to cache function types");

    instance
}

#[tokio::test]
async fn test_valid_typed_calls() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // Init — takes value (anything), returns actor-state
    let state = Value::Tuple(vec![]);
    let (state, _) = instance
        .call_function("theater:simple/actor.init", state, vec![])
        .await
        .expect("init should succeed");

    info!("State after init: {:?}", state);

    // Verify we got a proper actor-state record
    match &state {
        Value::Record { type_name, fields } => {
            assert_eq!(type_name, "actor-state");
            assert!(fields.iter().any(|(name, _)| name == "name"));
            assert!(fields.iter().any(|(name, _)| name == "pos"));
            assert!(fields.iter().any(|(name, _)| name == "status"));
            assert!(fields.iter().any(|(name, _)| name == "step-count"));
        }
        _ => panic!("Expected actor-state record, got: {:?}", state),
    }

    // move-to — takes actor-state + position, returns actor-state + status
    let target = Value::Record {
        type_name: "position".into(),
        fields: vec![
            ("x".into(), Value::F64(10.0)),
            ("y".into(), Value::F64(20.0)),
        ],
    };
    let (state, result_bytes) = instance
        .call_function_with_value(
            "theater:contract-test/actions.move-to",
            state,
            Value::Tuple(vec![target]),
        )
        .await
        .expect("move-to should succeed");

    info!("State after move-to: {:?}", state);
    assert!(
        !result_bytes.is_empty(),
        "Should have return value (status)"
    );

    // get-status — takes actor-state, returns actor-state + status
    let (state, _) = instance
        .call_function_with_value(
            "theater:contract-test/actions.get-status",
            state,
            Value::Tuple(vec![]),
        )
        .await
        .expect("get-status should succeed");

    // set-error — takes actor-state + string, returns actor-state
    let (_state, _) = instance
        .call_function_with_value(
            "theater:contract-test/actions.set-error",
            state,
            Value::Tuple(vec![Value::String("something went wrong".into())]),
        )
        .await
        .expect("set-error should succeed");

    info!("All valid typed calls succeeded!");
}

#[tokio::test]
async fn test_invalid_state_type_rejected() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // First, get a valid state via init
    let state = Value::Tuple(vec![]);
    let (valid_state, _) = instance
        .call_function("theater:simple/actor.init", state, vec![])
        .await
        .expect("init should succeed");

    // Now try to call move-to with WRONG state type (a string instead of actor-state record)
    let wrong_state = Value::String("not a valid state".into());
    let target = Value::Record {
        type_name: "position".into(),
        fields: vec![("x".into(), Value::F64(1.0)), ("y".into(), Value::F64(2.0))],
    };

    let result = instance
        .call_function_with_value(
            "theater:contract-test/actions.move-to",
            wrong_state,
            Value::Tuple(vec![target]),
        )
        .await;

    assert!(result.is_err(), "Should reject wrong state type");
    let err = result.unwrap_err().to_string();
    info!("Correctly rejected invalid state: {}", err);
    assert!(
        err.contains("State type mismatch"),
        "Error should mention state type mismatch: {}",
        err
    );
}

#[tokio::test]
async fn test_missing_record_field_rejected() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // Create a record that's missing the "status" field
    let incomplete_state = Value::Record {
        type_name: "actor-state".into(),
        fields: vec![
            ("name".into(), Value::String("test".into())),
            (
                "pos".into(),
                Value::Record {
                    type_name: "position".into(),
                    fields: vec![("x".into(), Value::F64(0.0)), ("y".into(), Value::F64(0.0))],
                },
            ),
            // missing "status" and "step-count"
        ],
    };

    let result = instance
        .call_function_with_value(
            "theater:contract-test/actions.get-status",
            incomplete_state,
            Value::Tuple(vec![]),
        )
        .await;

    assert!(result.is_err(), "Should reject incomplete record");
    let err = result.unwrap_err().to_string();
    info!("Correctly rejected incomplete record: {}", err);
    assert!(
        err.contains("missing field") || err.contains("MissingField"),
        "Error should mention missing field: {}",
        err
    );
}

#[tokio::test]
async fn test_wrong_field_type_rejected() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // Create a state where "step-count" is a string instead of u32
    let bad_state = Value::Record {
        type_name: "actor-state".into(),
        fields: vec![
            ("name".into(), Value::String("test".into())),
            (
                "pos".into(),
                Value::Record {
                    type_name: "position".into(),
                    fields: vec![("x".into(), Value::F64(0.0)), ("y".into(), Value::F64(0.0))],
                },
            ),
            (
                "status".into(),
                Value::Variant {
                    type_name: "status".into(),
                    case_name: "idle".into(),
                    tag: 0,
                    payload: vec![],
                },
            ),
            ("step-count".into(), Value::String("not a number".into())), // wrong type!
        ],
    };

    let result = instance
        .call_function_with_value(
            "theater:contract-test/actions.get-status",
            bad_state,
            Value::Tuple(vec![]),
        )
        .await;

    assert!(result.is_err(), "Should reject wrong field type");
    let err = result.unwrap_err().to_string();
    info!("Correctly rejected wrong field type: {}", err);
    assert!(
        err.contains("step-count") || err.contains("expected u32"),
        "Error should reference the bad field: {}",
        err
    );
}
