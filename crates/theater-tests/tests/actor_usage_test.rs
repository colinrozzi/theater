//! `GetActorUsage`: what an actor uses, read from the runtime without asking
//! the actor. After two calls into `panic-child` (its healthy `init`), the
//! runtime reports two calls, some time spent in them, and the guest's linear
//! memory (wasm pages, so a nonzero multiple of 64 KiB). An actor that isn't
//! running reports `None`.
//!
//! Like `panic_yields_failed_test`, this needs `panic-child` built to wasm32,
//! so it runs in CI (locally: `cargo check -p theater-tests --test
//! actor_usage_test`).

use std::sync::Arc;
use std::time::Duration;

use theater::actor::usage::ActorUsage;
use theater::config::actor_manifest::{HandlerConfig, ManifestConfig};
use theater::config::inheritance::HandlerPermissionPolicy;
use theater::handler::HandlerRegistry;
use theater::id::TheaterId;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::Value;
use theater::utils::ResourceCache;
use theater_handler_self::{SelfHandler, SelfHostConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

const OP_TIMEOUT: Duration = Duration::from_secs(10);

fn start_runtime() -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            tx_for_runtime,
            theater_rx,
            registry,
            Arc::new(ResourceCache::new()),
            theater_native::TokioSpawn,
        )
        .await
        .expect("Failed to create runtime");
        runtime.run().await
    });
    theater_tx
}

async fn usage(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    id: TheaterId,
) -> Option<ActorUsage> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorUsage {
            actor_id: id,
            response_tx: tx,
        })
        .expect("send GetActorUsage");
    timeout(OP_TIMEOUT, rx)
        .await
        .expect("usage timed out")
        .expect("usage channel closed")
}

#[tokio::test]
async fn usage_counts_calls_their_time_and_memory() {
    let theater_tx = start_runtime();
    let path = format!(
        "{}/../../test-actors/panic-child/target/wasm32-unknown-unknown/release/panic_child_actor.wasm",
        env!("CARGO_MANIFEST_DIR")
    );
    let wasm_bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!("Failed to read panic-child wasm at {path}: {e}. Build it: cd test-actors/panic-child && cargo build --release --target wasm32-unknown-unknown")
    });
    let (setup_tx, setup_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some("usage-child".to_string()),
            manifest: Some(ManifestConfig {
                name: "usage-child".to_string(),
                version: "0.1.0".to_string(),
                package: path.clone(),
                description: None,
                long_description: None,
                initial_state: None,
                static_package: false,
                permission_policy: HandlerPermissionPolicy::default(),
                handlers: vec![HandlerConfig::unit("self")],
            }),
            init_state: default_init_state(),
            response_tx: setup_tx,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SetupActor");
    let id = timeout(OP_TIMEOUT, setup_rx)
        .await
        .expect("setup timed out")
        .expect("setup channel closed")
        .expect("setup failed");

    // Set up, not yet called: its memory is known, no calls.
    let before = usage(&theater_tx, id)
        .await
        .expect("a running actor has usage");
    assert_eq!(before.calls, 0, "{before:?}");
    assert!(
        before.memory_bytes > 0 && before.memory_bytes % 65536 == 0,
        "{before:?}"
    );

    let (h_tx, h_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: id,
            response_tx: h_tx,
        })
        .expect("send GetActorHandle");
    let handle = timeout(OP_TIMEOUT, h_rx)
        .await
        .unwrap()
        .unwrap()
        .expect("handle present");
    for _ in 0..2 {
        handle
            .call_function(
                "theater:simple/actor.init".to_string(),
                Value::Tuple(vec![]),
            )
            .await
            .expect("init");
    }

    let after = usage(&theater_tx, id)
        .await
        .expect("a running actor has usage");
    assert_eq!(after.calls, 2, "{after:?}");
    assert!(after.call_time > Duration::ZERO, "{after:?}");
    assert!(after.memory_bytes >= before.memory_bytes, "{after:?}");

    assert_eq!(usage(&theater_tx, TheaterId::generate()).await, None);
}
