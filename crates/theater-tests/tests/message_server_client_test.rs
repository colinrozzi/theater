//! Full-runtime message-server client-interface guard.
//!
//! Nothing in CI previously spawned a real `#[derive(State)]` actor that exports
//! a *client* interface, nor round-tripped a request through the message-server
//! handler. That blind spot let a whole class of staleness ship/regress unseen —
//! the state-slot pact bug (#196), the dual-`__pack_alloc` link bug (#198), and
//! stale manifest `[[handler]]` types (#199). `state-test` compiles
//! stateless-client-less, so it exercised none of it.
//!
//! This test spawns the `state-client-test` actor (a `#[derive(State)]` actor
//! exporting `message-server-client`) through the real `TheaterRuntime` and drives
//! a request end to end:
//!   - **build** the actor to wasm (catches the dual-packr link conflict),
//!   - **spawn** it with `self` + `message-server` handlers (catches a stale
//!     manifest handler-type and any import-hash mismatch),
//!   - route a real `Request` → `handle-request` → the handler's no-state
//!     `parse_request_response` → assert the response bytes.

use std::sync::Arc;
use std::time::Duration;

use theater::config::actor_manifest::{HandlerConfig, ManifestConfig};
use theater::config::inheritance::HandlerPermissionPolicy;
use theater::handler::HandlerRegistry;
use theater::messages::{
    default_init_state, ActorMessage, ActorRequest, MessageCommand, TheaterCommand,
};
use theater::pack_bridge::Value;
use theater::utils::ResourceCache;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_self::{SelfHandler, SelfHostConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

fn wasm_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/state-client-test/target/wasm32-unknown-unknown/release/state_client_test_actor.wasm"
    )
    .to_string()
}

fn start_runtime(router: MessageRouter) -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    registry.register(MessageServerHandler::new(None, router));
    tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            tx_for_runtime,
            theater_rx,
            registry,
            Arc::new(ResourceCache::new()),
            theater_native::TokioSpawn,
        )
        .await
        .expect("create runtime");
        runtime.run().await
    });
    theater_tx
}

fn manifest(name: &str) -> ManifestConfig {
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: wasm_path(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![
            HandlerConfig::unit("self"),
            HandlerConfig::unit("message-server"),
        ],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn message_server_client_request_roundtrips_through_handler() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    let router = MessageRouter::new();
    let theater_tx = start_runtime(router.clone());

    // ---- spawn the actor ----
    let wasm_bytes = std::fs::read(wasm_path()).unwrap_or_else(|e| {
        panic!(
            "read state-client-test wasm: {}. Build it: \
             cd test-actors/state-client-test && cargo build --release --target wasm32-unknown-unknown",
            e
        )
    });
    let (setup_tx, setup_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some("state-client-test".to_string()),
            manifest: Some(manifest("state-client-test")),
            init_state: default_init_state(),
            response_tx: setup_tx,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SetupActor");
    let actor_id = timeout(Duration::from_secs(10), setup_rx)
        .await
        .expect("setup timeout")
        .expect("setup channel")
        .expect("setup ok");

    // ---- run init so the actor calls register() and joins the message router ----
    let (h_tx, h_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: actor_id.clone(),
            response_tx: h_tx,
        })
        .expect("send GetActorHandle");
    let handle = timeout(Duration::from_secs(5), h_rx)
        .await
        .expect("handle timeout")
        .expect("handle channel")
        .expect("handle present");
    handle
        .call_function(
            "theater:simple/actor.init".to_string(),
            Value::Tuple(vec![]),
        )
        .await
        .expect("init call");

    // register() runs inside init; give the router a moment to record the mailbox.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ---- route a real Request through the message-server handler ----
    let (resp_tx, resp_rx) = oneshot::channel::<Vec<u8>>();
    let (ack_tx, ack_rx) = oneshot::channel();
    router
        .route_message(MessageCommand::SendMessage {
            target_id: actor_id.clone(),
            message: ActorMessage::Request(ActorRequest {
                response_tx: resp_tx,
                data: b"ping".to_vec(),
            }),
            response_tx: ack_tx,
        })
        .await
        .expect("route request");
    let _ = timeout(Duration::from_secs(5), ack_rx).await;

    let response = timeout(Duration::from_secs(5), resp_rx)
        .await
        .expect("response timeout")
        .expect("response channel");

    // handle-request returns ok(Some("response:" + data)) — bare-ok, no state slot;
    // the handler's parse_request_response unwraps it to exactly these bytes.
    assert_eq!(
        response,
        b"response:ping".to_vec(),
        "the routed request must round-trip through handle-request + the no-state \
         response parser"
    );
}
