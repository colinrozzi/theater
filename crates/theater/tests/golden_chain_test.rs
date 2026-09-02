//! Replay reconstruction test — the load-bearing guarantee of in-module state.
//!
//! State lives inside the module and is never stored by the runtime; the chain
//! of recorded calls is the single source of truth. The whole model rests on one
//! property: **replaying the recorded sequence of calls reconstructs the exact
//! same state.** This test proves it end to end:
//!
//! 1. drive the `state-test` actor through a sequence of calls (init, increment,
//!    increment, get-count), recording each `(function, params)` — this stands in
//!    for the chain's `WasmCall` log — and asserting the live outputs;
//! 2. read the live in-module state via the actor's `get-state` export;
//! 3. replay the recorded sequence into a *fresh* instance and assert (a) every
//!    replayed output equals the recorded output (determinism ⇒ output-equality)
//!    and (b) the replayed `get-state` equals the live one (state reconstructed).
//!
//! (Recording `WasmCall`/`WasmResult` into the chain itself happens in the actor
//! runtime's `execute_call_pack`, not on a bare `PackInstance`; a full-runtime
//! replay test that drives from the persisted chain lives in `theater-tests`.)

use std::sync::Arc;
use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::StateChain;
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::pack_bridge::{decode_value, AsyncRuntime, Ctx, PackInstance, Value};
use tokio::sync::mpsc;
use tokio::sync::RwLock as SyncRwLock;
use tracing::info;

mod common;

/// Build a fresh `state-test` `PackInstance` (own store + chain + host `log`).
async fn fresh_state_test_instance(wasm_bytes: &[u8]) -> PackInstance {
    let runtime = AsyncRuntime::new();
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id)));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);
    let actor_store = ActorStore::new(actor_id, theater_tx, actor_handle, chain);

    PackInstance::new("state-test", wasm_bytes, &runtime, actor_store, |builder| {
        builder.interface("theater:simple/self")?.func_typed(
            "log",
            |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                if let Value::String(s) = &input {
                    info!("[ACTOR LOG] {}", s);
                }
                Value::Tuple(vec![])
            },
        )?;
        Ok(())
    })
    .await
    .expect("Failed to create PackInstance")
}

/// Extract the `count` field from the actor's `get-state` record.
fn count_of(state: &Value) -> i32 {
    match state {
        Value::Record { fields, .. } => fields
            .iter()
            .find(|(n, _)| n == "count")
            .and_then(|(_, v)| match v {
                Value::S32(n) => Some(*n),
                Value::S64(n) => Some(*n as i32),
                Value::U32(n) => Some(*n as i32),
                _ => None,
            })
            .expect("state should carry a `count` field"),
        _ => panic!("get-state should return a record, got {:?}", state),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replay_reconstructs_in_module_state() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/state-test/target/wasm32-unknown-unknown/release/state_test_actor.wasm"
    );
    let member = std::fs::read(wasm_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read state-test WASM from {}: {}. Build it first: \
             cd test-actors/state-test && cargo build --release --target wasm32-unknown-unknown",
            wasm_path, e
        );
    });
    let wasm_bytes = common::helpers::link_self_contained(member);

    // The recorded sequence of calls — the stand-in for the chain's WasmCall log.
    // (get-count is a pure read; it still belongs in the replay to prove reads are
    // faithfully reproduced.)
    let recorded_calls: Vec<(&str, Value)> = vec![
        ("theater:simple/actor.init", Value::Tuple(vec![])),
        ("theater:simple/state-test.increment", Value::Tuple(vec![])),
        ("theater:simple/state-test.increment", Value::Tuple(vec![])),
        ("theater:simple/state-test.get-count", Value::Tuple(vec![])),
    ];

    // ---- Record run: drive the actor, capture outputs + live state ----
    let mut recorder = fresh_state_test_instance(&wasm_bytes).await;
    let mut recorded_outputs: Vec<Value> = Vec::new();
    for (name, params) in &recorded_calls {
        let bytes = recorder
            .call_function_with_value(name, params.clone())
            .await
            .unwrap_or_else(|e| panic!("record call {} failed: {:#}", name, e));
        recorded_outputs.push(decode_value(&bytes).unwrap_or(Value::Tuple(vec![])));
    }

    // In-module state progressed 0 -> 1 -> 2, and the two increments + get-count
    // all returned the running count.
    assert_eq!(
        recorded_outputs[1],
        Value::S32(1),
        "first increment returns 1"
    );
    assert_eq!(
        recorded_outputs[2],
        Value::S32(2),
        "second increment returns 2"
    );
    assert_eq!(recorded_outputs[3], Value::S32(2), "get-count returns 2");

    let live_state = recorder
        .call_value("theater:simple/actor.get-state", &Value::Tuple(vec![]))
        .await
        .expect("get-state should succeed");
    assert_eq!(count_of(&live_state), 2, "live in-module count is 2");
    info!("recorded: live state count = {}", count_of(&live_state));

    // ---- Replay run: re-run the recorded sequence into a FRESH instance ----
    let mut replay = fresh_state_test_instance(&wasm_bytes).await;
    let mut replay_outputs: Vec<Value> = Vec::new();
    for (name, params) in &recorded_calls {
        let bytes = replay
            .call_function_with_value(name, params.clone())
            .await
            .unwrap_or_else(|e| panic!("replay call {} failed: {:#}", name, e));
        replay_outputs.push(decode_value(&bytes).unwrap_or(Value::Tuple(vec![])));
    }

    // Determinism: replayed outputs equal recorded outputs, call for call.
    assert_eq!(
        replay_outputs, recorded_outputs,
        "replayed outputs must equal recorded outputs (deterministic replay)"
    );

    // The core guarantee: replaying the recorded chain reconstructs the exact
    // same in-module state.
    let replayed_state = replay
        .call_value("theater:simple/actor.get-state", &Value::Tuple(vec![]))
        .await
        .expect("get-state should succeed");
    assert_eq!(
        replayed_state, live_state,
        "replaying the recorded calls must reconstruct byte-identical in-module state"
    );
    info!(
        "replay: reconstructed state count = {} (== live)",
        count_of(&replayed_state)
    );
}
