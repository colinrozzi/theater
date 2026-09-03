//! Full-runtime replay test: replaying a recorded chain reconstructs in-module state.
//!
//! State lives inside the module now (`docs/in-module-state.md`); the chain of
//! call I/O is the single source of truth, and state is a replayable projection of
//! it. This test proves that guarantee through the **real `TheaterRuntime`**, end
//! to end:
//!
//! 1. spawn the `state-test` actor, drive it (init, increment, increment,
//!    get-count) so the runtime records a chain of `WasmCall`/`WasmResult` events,
//!    and read its live state via `get-actor-state` (which calls the actor's
//!    `get-state` export);
//! 2. persist that chain to a file and spawn a **fresh** actor in replay mode —
//!    the `ReplayHandler` re-runs the recorded calls and verifies chain-hash
//!    equality step by step (a successful replay already proves determinism);
//! 3. read the replayed actor's `get-actor-state` and assert it equals the
//!    recorded one — state reconstructed from nothing but the chain.

use std::sync::Arc;
use std::time::Duration;

use theater::chain::ChainEvent;
use theater::config::actor_manifest::{
    HandlerConfig, ManifestConfig, ReplayHandlerConfig, SelfHostConfig,
};
use theater::config::inheritance::HandlerPermissionPolicy;
use theater::handler::HandlerRegistry;
use theater::id::TheaterId;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::Value;
use theater::utils::ResourceCache;
use theater_handler_self::SelfHandler;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

fn state_test_wasm_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/state-test/target/wasm32-unknown-unknown/release/state_test_actor.wasm"
    )
    .to_string()
}

/// A registry with just the `self` handler — all `state-test` imports is
/// `theater:simple/self.log`.
fn registry(theater_tx: mpsc::UnboundedSender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(SelfHostConfig {}, theater_tx, None));
    registry
}

fn start_runtime() -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let reg = registry(theater_tx.clone());
    tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            tx_for_runtime,
            theater_rx,
            reg,
            Arc::new(ResourceCache::new()),
        )
        .await
        .expect("create runtime");
        runtime.run().await
    });
    theater_tx
}

fn manifest(name: &str, handlers: Vec<HandlerConfig>) -> ManifestConfig {
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: state_test_wasm_path(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers,
    }
}

/// Set up an actor (no auto-init) with an event subscription; returns its id.
async fn setup_actor(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    name: &str,
    manifest: ManifestConfig,
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
) -> TheaterId {
    let wasm_bytes = std::fs::read(state_test_wasm_path()).unwrap_or_else(|e| {
        panic!(
            "read state-test wasm: {}. Build it: \
             cd test-actors/state-test && cargo build --release --target wasm32-unknown-unknown",
            e
        )
    });
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some(name.to_string()),
            manifest: Some(manifest),
            init_state: default_init_state(),
            response_tx: tx,
            subscription_tx: Some(event_tx),
            parent_id: None,
        })
        .expect("send SetupActor");
    timeout(Duration::from_secs(10), rx)
        .await
        .expect("setup timeout")
        .expect("setup channel")
        .expect("setup ok")
}

/// Read an actor's state via `get-actor-state` (calls its `get-state` export).
async fn get_actor_state(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    actor_id: TheaterId,
) -> Value {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorState {
            actor_id,
            response_tx: tx,
        })
        .expect("send GetActorState");
    timeout(Duration::from_secs(5), rx)
        .await
        .expect("get-state timeout")
        .expect("get-state channel")
        .expect("get-state ok")
}

/// Drain chain events until the subscription goes idle.
async fn collect_events(
    rx: &mut mpsc::Receiver<(TheaterId, ChainEvent)>,
    idle: Duration,
    max: Duration,
) -> Vec<ChainEvent> {
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    let mut last = std::time::Instant::now();
    while start.elapsed() < max {
        match timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some((_, ev))) => {
                last = std::time::Instant::now();
                events.push(ev);
            }
            Ok(None) => break,
            Err(_) => {
                if last.elapsed() > idle {
                    break;
                }
            }
        }
    }
    events
}

async fn stop_actor(theater_tx: &mpsc::UnboundedSender<TheaterCommand>, actor_id: TheaterId) {
    let (tx, rx) = oneshot::channel();
    let _ = theater_tx.send(TheaterCommand::StopActor {
        actor_id,
        response_tx: tx,
    });
    let _ = timeout(Duration::from_secs(5), rx).await;
}

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
            .expect("state has a count field"),
        other => panic!("expected an actor-state record, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replay_reconstructs_in_module_state_full_runtime() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    let theater_tx = start_runtime();
    let chain_path = std::env::temp_dir().join(format!(
        "theater_replay_state_{}_{}.json",
        std::process::id(),
        TheaterId::generate()
    ));

    // ---- Record run: drive the actor through the full runtime ----
    let (rec_ev_tx, mut rec_ev_rx) = mpsc::channel(256);
    let record_id = setup_actor(
        &theater_tx,
        "state-test-record",
        manifest("state-test-record", vec![HandlerConfig::unit("self")]),
        rec_ev_tx,
    )
    .await;

    // Grab the actor handle and drive: init, increment x2, get-count. Each call
    // records WasmCall/WasmResult events on the actor's chain.
    let (h_tx, h_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: record_id,
            response_tx: h_tx,
        })
        .expect("send GetActorHandle");
    let handle = timeout(Duration::from_secs(5), h_rx)
        .await
        .expect("handle timeout")
        .expect("handle channel")
        .expect("handle present");

    for func in [
        "theater:simple/actor.init",
        "theater:simple/state-test.increment",
        "theater:simple/state-test.increment",
        "theater:simple/state-test.get-count",
    ] {
        handle
            .call_function(func.to_string(), Value::Tuple(vec![]))
            .await
            .unwrap_or_else(|e| panic!("record call {} failed: {:?}", func, e));
    }

    let recorded_chain = collect_events(
        &mut rec_ev_rx,
        Duration::from_millis(500),
        Duration::from_secs(10),
    )
    .await;
    assert!(
        !recorded_chain.is_empty(),
        "the driven calls must record a chain"
    );

    let recorded_state = get_actor_state(&theater_tx, record_id).await;
    assert_eq!(
        count_of(&recorded_state),
        2,
        "two increments leave the in-module count at 2"
    );

    std::fs::write(
        &chain_path,
        serde_json::to_string(&recorded_chain).expect("serialize chain"),
    )
    .expect("write chain file");

    stop_actor(&theater_tx, record_id).await;

    // ---- Replay run: a fresh actor in replay mode, driven by the chain ----
    let (rep_ev_tx, mut rep_ev_rx) = mpsc::channel(256);
    let replay_id = setup_actor(
        &theater_tx,
        "state-test-replay",
        manifest(
            "state-test-replay",
            vec![
                HandlerConfig::new(
                    "replay",
                    ReplayHandlerConfig {
                        chain: chain_path.clone(),
                    },
                ),
                HandlerConfig::unit("self"),
            ],
        ),
        rep_ev_tx,
    )
    .await;

    // The ReplayHandler re-runs the recorded calls (verifying chain-hash equality
    // as it goes). Wait for the replay chain to settle.
    let replay_chain = collect_events(
        &mut rep_ev_rx,
        Duration::from_secs(2),
        Duration::from_secs(15),
    )
    .await;

    // The core guarantee: the replayed actor's state — reconstructed purely by
    // re-running the recorded chain — matches the original.
    let replayed_state = get_actor_state(&theater_tx, replay_id).await;
    assert_eq!(
        replayed_state,
        recorded_state,
        "replaying the recorded chain reconstructs the same in-module state \
         (recorded count={}, replayed count={})",
        count_of(&recorded_state),
        count_of(&replayed_state)
    );

    // Determinism: the replay reproduced the recorded chain, event for event.
    assert_eq!(
        recorded_chain.len(),
        replay_chain.len(),
        "replay must reproduce the same number of chain events"
    );
    for (i, (orig, rep)) in recorded_chain.iter().zip(replay_chain.iter()).enumerate() {
        assert_eq!(
            orig.hash, rep.hash,
            "chain event {} hash must match on replay (deterministic)",
            i
        );
    }

    stop_actor(&theater_tx, replay_id).await;
    let _ = std::fs::remove_file(&chain_path);
}
