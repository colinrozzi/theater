//! Gate test for the packr 0.10.x **self-contained actor** cutover.
//!
//! This is the one validation bare wasmtime cannot do: it drives a
//! self-contained composite through theater's real runtime + `CallInterceptor`
//! and checks the interceptor/chain-log path end to end.
//!
//! It:
//!   1. links the `replay-test` actor member + the packr **bundled** allocator
//!      (`packr::DEFAULT_ALLOCATOR_WASM`) into a self-contained composite via
//!      `packr::link` (own memory, `__pack_alloc`, host imports only);
//!   2. loads it through `PackInstance::new_with_interceptor` — exercising the
//!      0.10.x self-contained loader (`assert_self_contained`);
//!   3. drives `handle-send`, whose handler logs STATIC string literals, and
//!      asserts those strings survive marshalling into the host boundary intact
//!      (the `.rodata`/static-data path — numeric fixtures hide data bugs);
//!   4. replays the recorded host calls via `ReplayRecordingInterceptor` and
//!      asserts a byte-identical chain head (deterministic replay).
//!
//! Requires the member built first:
//!   cd test-actors/replay-test && cargo build --target wasm32-unknown-unknown --release

use std::sync::{Arc, Mutex};

use packr::abi::ValueType;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::{ChainEvent, StateChain};
use theater::id::TheaterId;
use theater::interceptor::{RecordingInterceptor, ReplayRecordingInterceptor};
use theater::messages::TheaterCommand;
use theater::pack_bridge::{ActorResult, AsyncRuntime, CallInterceptor, Ctx, PackInstance, Value};

use tokio::sync::mpsc;
use tokio::sync::RwLock as SyncRwLock;

/// Read the plain-built `replay-test` actor wasm. As of packr 0.11.0 an actor
/// is a plain cargo build (`setup_guest!()` links the allocator in), so the
/// member is directly loadable — no composition step. The member must be built
/// plain first (packr-guest 0.11.0, no fixed-base recipe):
///   cd test-actors/replay-test && cargo build --target wasm32-unknown-unknown --release
fn link_replay_composite() -> Vec<u8> {
    let member_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/replay-test/target/wasm32-unknown-unknown/release/replay_test_actor.wasm"
    );
    std::fs::read(member_path).unwrap_or_else(|e| {
        panic!(
            "read replay-test member {}: {}. Build it first: \
             cd test-actors/replay-test && cargo build --target wasm32-unknown-unknown --release",
            member_path, e
        )
    })
}

/// Result of one `handle-send` drive.
struct DriveResult {
    /// Strings the `log` host fn received (empty on a replay run — the recorded
    /// output is fed back instead of calling the real host fn).
    logs: Vec<String>,
    /// The chain head hash after the call.
    head: Option<Vec<u8>>,
    /// Chain events emitted during the call (captured via a subscriber).
    events: Vec<ChainEvent>,
}

/// Load the composite with the given interceptor, drive one `handle-send`, and
/// return the captured logs + chain head + emitted events.
async fn drive_handle_send(
    composite: &[u8],
    make_interceptor: impl FnOnce(Arc<SyncRwLock<StateChain>>) -> Arc<dyn CallInterceptor>,
) -> DriveResult {
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let runtime = AsyncRuntime::new();
    let actor_id = TheaterId::generate();
    let (theater_tx, _theater_rx) = mpsc::channel::<TheaterCommand>(100);
    let (op_tx, _op_rx) = mpsc::channel(10);
    let (info_tx, _info_rx) = mpsc::channel(10);
    let (ctl_tx, _ctl_rx) = mpsc::channel(10);
    let chain = Arc::new(SyncRwLock::new(StateChain::new(actor_id)));

    // Capture emitted chain events (StateChain retains nothing; it dispatches).
    let (ev_tx, mut ev_rx) = mpsc::channel::<(TheaterId, ChainEvent)>(1024);
    chain.write().await.add_subscriber(ev_tx);

    let interceptor = make_interceptor(chain.clone());
    let handle = ActorHandle::new(op_tx, info_tx, ctl_tx);
    let store = ActorStore::new(
        actor_id,
        theater_tx,
        handle,
        chain.clone(),
        Value::Tuple(vec![]),
    );

    let cap = captured.clone();
    let mut instance = PackInstance::new_with_interceptor(
        "replay-test-self-contained",
        composite,
        &runtime,
        store,
        Some(interceptor),
        move |builder| {
            let cap_log = cap.clone();
            builder.interface("theater:simple/runtime")?.func_typed(
                "log",
                move |_ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                    if let Value::String(s) = &input {
                        cap_log.lock().unwrap().push(s.clone());
                    }
                    Value::Tuple(vec![])
                },
            )?;
            // The member also imports message-server-host.register as a residual
            // host import. handle-send never calls it, but the import must resolve
            // at instantiate. Stub it returning Ok(()).
            builder
                .interface("theater:simple/message-server-host")?
                .func_typed(
                    "register",
                    move |_ctx: &mut Ctx<'_, ActorStore>, _input: Value| Value::Result {
                        ok_type: ValueType::Tuple(vec![]),
                        err_type: ValueType::String,
                        value: Ok(Box::new(Value::Tuple(vec![]))),
                    },
                )?;
            Ok(())
        },
    )
    .await
    .expect("self-contained composite must load via the 0.10.x self-contained loader");

    // `handle-send(state: option<list<u8>>, params: tuple<string, list<u8>>)`.
    // The handler ignores params and echoes state; pass a well-typed none-state.
    let none_state = Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: None,
    };
    let params = Value::Tuple(vec![
        Value::String("test-sender".into()),
        Value::List {
            elem_type: ValueType::U8,
            items: vec![],
        },
    ]);
    let _res: ActorResult<()> = instance
        .call_typed(
            "theater:simple/message-server-client.handle-send",
            none_state,
            params,
        )
        .await
        .expect("handle-send must succeed on the self-contained composite");

    // Drain the emitted events (all dispatched synchronously during the call).
    let mut events = Vec::new();
    while let Ok((_, ev)) = ev_rx.try_recv() {
        events.push(ev);
    }

    let head = chain.read().await.head_hash().map(|h| h.to_vec());
    let logs = captured.lock().unwrap().clone();
    DriveResult { logs, head, events }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_contained_actor_records_static_strings_and_replays_deterministically() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    let composite = link_replay_composite();

    // ---- Record run: real host calls, recorded by the interceptor ----
    let rec = drive_handle_send(&composite, |chain| {
        Arc::new(RecordingInterceptor::new(chain)) as Arc<dyn CallInterceptor>
    })
    .await;

    // The handler's STATIC string literals must survive marshalling into the
    // host boundary intact — the .rodata/static-data path end to end.
    assert!(
        rec.logs
            .iter()
            .any(|m| m == "Replay test actor: handle-send called"),
        "static log string must survive marshalling; got {:?}",
        rec.logs
    );
    assert!(
        rec.logs
            .iter()
            .any(|m| m == "Replay test actor: processing message"),
        "second static log string must survive; got {:?}",
        rec.logs
    );
    // A .rodata-strip regression would yield right-length but blank/zeroed strings.
    assert!(
        rec.logs
            .iter()
            .all(|m| !m.is_empty() && m.bytes().any(|b| b != 0)),
        "no log string may be blank/zeroed: {:?}",
        rec.logs
    );

    // The interceptor recorded the host calls to the chain.
    assert!(
        rec.head.is_some() && !rec.events.is_empty(),
        "the interceptor must record host calls to the chain (head={:?}, {} events)",
        rec.head,
        rec.events.len()
    );

    // ---- Replay run: feed the recorded events back; assert identical chain head ----
    let recorded = rec.events.clone();
    let replay = drive_handle_send(&composite, move |chain| {
        Arc::new(ReplayRecordingInterceptor::new(recorded, chain)) as Arc<dyn CallInterceptor>
    })
    .await;

    assert_eq!(
        rec.head, replay.head,
        "replay must reproduce a byte-identical chain head (deterministic host-call record)"
    );
}
