//! End-to-end test: a packr **multi-memory composite** runs as a real theater
//! actor.
//!
//! Unlike `composite_integration_test.rs` (which links a single member via the
//! 0.10.x self-contained loader), this loads a composite produced by `packr
//! compose` — two ISOLATED components (each with its own memory) fused into one
//! module by a bridging shim. The entry component exports
//! `theater:simple/actor.init` and imports `math.double` from the provider
//! component; the composite's only residual import is `theater:simple/runtime.log`.
//!
//! `init` calls `double(21)` across the component gap and stores the result in
//! state. A green run proves theater's own loader (`PackInstance` →
//! `assert_self_contained` → instantiate) accepts a multi-memory composite,
//! satisfies its residual host import, and runs a cross-component call inside the
//! actor lifecycle — returning state `{ doubled: 42 }`.
//!
//! The fixture is pre-built (see `test-actors/comp-composite/README.md`) because
//! `packr compose` is unreleased tooling theater's pinned packr does not expose.

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composed_multimemory_actor_runs_under_theater() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    // The pre-built composite (packr compose of comp-actor + math-real).
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/comp-composite/comp_actor_composite.wasm"
    );
    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => panic!(
            "Failed to read composite fixture from {}: {}. \
             Regenerate it per test-actors/comp-composite/README.md.",
            wasm_path, e
        ),
    };
    // Note: NO link_self_contained() — the composite is already self-contained
    // (it exports its own memory + allocator and imports only host functions).
    info!("Loaded composite: {} bytes", wasm_bytes.len());

    let runtime = AsyncRuntime::new();

    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel::<TheaterCommand>(10);
    let (operation_tx, _operation_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (control_tx, _control_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id)));
    let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

    let actor_store = ActorStore::new(
        actor_id,
        theater_tx.clone(),
        actor_handle,
        chain,
        Value::Tuple(vec![]),
    );

    // Instantiate through theater's own loader, providing the residual host
    // import `theater:simple/runtime.log`.
    let mut instance = PackInstance::new(
        "composed-actor-e2e",
        &wasm_bytes,
        &runtime,
        actor_store,
        |builder| {
            builder.interface("theater:simple/runtime")?.func_typed(
                "log",
                |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    let msg = match input {
                        Value::String(s) => s,
                        other => format!("{:?}", other),
                    };
                    info!("[ACTOR LOG] {}", msg);
                    Value::Tuple(vec![])
                },
            )?;
            Ok(())
        },
    )
    .await
    .expect(
        "PackInstance::new failed — theater's loader rejected the multi-memory \
         composite, or a residual host import was unsatisfied",
    );

    info!("Composite instantiated under theater");

    // Drive the actor lifecycle. init calls double(21) across the component gap.
    let (new_state, _result_bytes) = instance
        .call_function("theater:simple/actor.init", Value::Tuple(vec![]), vec![])
        .await
        .expect("theater:simple/actor.init failed");

    info!("init returned state: {:?}", new_state);

    // The cross-component call must have produced double(21) = 42 in state.
    let doubled = match &new_state {
        Value::Record { fields, .. } => fields
            .iter()
            .find(|(k, _)| k == "doubled")
            .map(|(_, v)| v.clone())
            .expect("init state must carry a `doubled` field"),
        other => panic!("init must return a state record, got {other:?}"),
    };
    assert_eq!(
        doubled,
        Value::S64(42),
        "init must compute double(21)=42 via the cross-component (shim) call — \
         proof the composite's two memories bridged correctly under theater"
    );
}
