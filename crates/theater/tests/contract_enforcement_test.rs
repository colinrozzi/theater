//! Contract enforcement integration test.
//!
//! State lives inside the module now, and interface compatibility is guaranteed
//! at load time by the runtime's InterfaceHashMismatch gate (the per-call
//! `validate_value_in_type_space` return-type check was retired with the engine
//! migration). This drives the `contract-test` actor through its typed actions
//! and confirms:
//! 1. rich typed values (records, variants, nested types) round-trip across the
//!    boundary intact — the actor's typed returns decode to the expected shapes;
//! 2. the actor's in-module state is inspectable via its `get-state` export and
//!    reflects the mutations its actions make.
//!
//! (The old "wrong *state* type is rejected" tests are gone — state is no longer
//! threaded through the call, so there is no state argument to validate.)

use std::sync::Arc;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::pack_bridge::{
    decode_value, plain_host_fn, CachingPackRuntime, HostImports, PackInstance, Value, WasmEngine,
};
use tokio::sync::mpsc;
use tokio::sync::RwLock as SyncRwLock;
use tracing::info;

mod common;

/// Helper to create a PackInstance from the contract-test actor
async fn create_instance() -> PackInstance {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/contract-test/target/wasm32-unknown-unknown/release/contract_test_actor.wasm"
    );

    let member = std::fs::read(wasm_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read contract-test WASM from {}: {}. \
             Build it first: cd test-actors/contract-test && cargo build --release --target wasm32-unknown-unknown",
            wasm_path, e
        );
    });
    let wasm_bytes = common::helpers::link_self_contained(member);

    let runtime = CachingPackRuntime::new();
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id)));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(actor_id, theater_tx.clone(), actor_handle, chain);

    let mut imports = HostImports::new();
    imports.define(
        "theater:simple/self",
        "log",
        plain_host_fn(|input: Value| async move {
            let msg = match input {
                Value::String(s) => s,
                _ => format!("{:?}", input),
            };
            info!("[ACTOR LOG] {}", msg);
            Value::Tuple(vec![])
        }),
    );

    let wasm_bytes = Arc::new(wasm_bytes);
    let (module, _cache_hit) = runtime
        .compile_cached(&wasm_bytes)
        .await
        .expect("Failed to compile WASM module");
    let instance = runtime
        .engine()
        .instantiate(&module, imports)
        .await
        .expect("Failed to instantiate Pack module");

    PackInstance::new("contract-test", instance, actor_store, wasm_bytes)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_valid_typed_calls() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let mut instance = create_instance().await;

    // init sets the actor's in-module state and returns unit (no threaded state).
    instance
        .call_function("theater:simple/actor.init", vec![])
        .await
        .expect("init should succeed");

    // The state is inspectable via the actor's get-state export — a rich nested
    // record (record containing a record + a variant + a u32) must round-trip.
    let state = instance
        .call_value("theater:simple/actor.get-state", &Value::Tuple(vec![]))
        .await
        .expect("get-state should succeed");
    info!("State after init: {:?}", state);
    match &state {
        Value::Record { type_name, fields } => {
            assert_eq!(type_name, "actor-state");
            for field in ["name", "pos", "status", "step-count"] {
                assert!(
                    fields.iter().any(|(name, _)| name == field),
                    "actor-state missing field {}",
                    field
                );
            }
        }
        _ => panic!("Expected actor-state record, got: {:?}", state),
    }

    // move-to(position) -> status. The typed return decodes to a well-formed
    // status variant, proving the rich value round-trips across the boundary.
    let target = Value::Record {
        type_name: "position".into(),
        fields: vec![
            ("x".into(), Value::F64(10.0)),
            ("y".into(), Value::F64(20.0)),
        ],
    };
    let bytes = instance
        .call_function_with_value(
            "theater:contract-test/actions.move-to",
            Value::Tuple(vec![target]),
        )
        .await
        .expect("move-to should succeed");
    let status = decode_value(&bytes).expect("decode move-to status");
    info!("move-to returned status: {:?}", status);
    match &status {
        Value::Variant { case_name, .. } => assert_eq!(case_name, "moving"),
        _ => panic!("Expected a status variant, got: {:?}", status),
    }

    // get-status() -> status (no params, typed return).
    let bytes = instance
        .call_function_with_value(
            "theater:contract-test/actions.get-status",
            Value::Tuple(vec![]),
        )
        .await
        .expect("get-status should succeed");
    assert!(matches!(
        decode_value(&bytes).expect("decode status"),
        Value::Variant { .. }
    ));

    // set-error(string) -> unit.
    instance
        .call_function_with_value(
            "theater:contract-test/actions.set-error",
            Value::Tuple(vec![Value::String("something went wrong".into())]),
        )
        .await
        .expect("set-error should succeed");

    // The mutation is visible through get-state: status is now the error case
    // and the step-count reflects the earlier move-to.
    let state = instance
        .call_value("theater:simple/actor.get-state", &Value::Tuple(vec![]))
        .await
        .expect("get-state should succeed");
    match &state {
        Value::Record { fields, .. } => {
            let status = fields
                .iter()
                .find(|(n, _)| n == "status")
                .map(|(_, v)| v)
                .expect("status field");
            match status {
                Value::Variant { case_name, .. } => assert_eq!(case_name, "error"),
                _ => panic!("Expected status variant, got {:?}", status),
            }
            let step = fields
                .iter()
                .find(|(n, _)| n == "step-count")
                .map(|(_, v)| v)
                .expect("step-count field");
            assert_eq!(*step, Value::U32(1), "move-to bumped step-count to 1");
        }
        _ => panic!("Expected actor-state record, got: {:?}", state),
    }

    info!("All valid typed calls succeeded, in-module state reflected them!");
}
