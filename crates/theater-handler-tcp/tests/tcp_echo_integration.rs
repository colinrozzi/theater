//! Integration test: spin up a TheaterRuntime in-process, spawn the tcp-echo
//! actor, exercise it with real TCP traffic, then pull the chain out and inspect it.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use theater::config::actor_manifest::{RuntimeHostConfig, TcpHandlerConfig};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_tcp::TcpHandler;

/// Build a minimal handler registry with just runtime + tcp handlers.
fn create_handler_registry(theater_tx: mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    registry.register(RuntimeHandler::new(
        RuntimeHostConfig {},
        theater_tx,
        None,
    ));

    registry.register(TcpHandler::new(TcpHandlerConfig {
        listen: None,
        max_connections: None,
    }));

    registry
}

/// Build a manifest TOML string with an absolute path to the pre-built WASM.
fn make_manifest(wasm_path: &str, listen_addr: &str) -> String {
    format!(
        r#"
name = "tcp-echo-test"
version = "0.1.0"
package = "{wasm_path}"

[[handler]]
type = "runtime"

[[handler]]
type = "tcp"
listen = "{listen_addr}"
"#
    )
}

/// Resolve the absolute path to the example actor WASM.
fn wasm_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!(
        "{}/examples/tcp-echo/target/wasm32-unknown-unknown/release/tcp_echo_actor.wasm",
        manifest_dir
    )
}

#[tokio::test]
async fn test_tcp_echo_and_chain() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // ── 0. Check that the WASM binary exists ─────────────────────────────
    let wasm = wasm_path();
    if !std::path::Path::new(&wasm).exists() {
        panic!(
            "WASM not found at {}. Build it first:\n  \
             cd crates/theater-handler-tcp/examples/tcp-echo && cargo build --release",
            wasm
        );
    }

    // Use a high port to avoid conflicts
    let listen_addr = "127.0.0.1:19823";

    // ── 1. Stand up the runtime ──────────────────────────────────────────
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
    let registry = create_handler_registry(theater_tx.clone());

    let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None, registry)
        .await
        .expect("failed to create runtime");

    let runtime_handle = tokio::spawn(async move {
        if let Err(e) = runtime.run().await {
            eprintln!("runtime error: {}", e);
        }
    });

    // ── 2. Spawn the tcp-echo actor ──────────────────────────────────────
    let manifest = make_manifest(&wasm, listen_addr);
    let (response_tx, response_rx) = oneshot::channel();
    let (subscription_tx, mut subscription_rx) = mpsc::channel(64);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: manifest,
            init_bytes: None,
            response_tx,
            parent_id: None,
            supervisor_tx: None,
            subscription_tx: Some(subscription_tx),
        })
        .await
        .expect("failed to send SpawnActor");

    let actor_id = response_rx
        .await
        .expect("response channel closed")
        .expect("SpawnActor failed");

    info!("Actor spawned: {}", actor_id);

    // ── 3. Wait for actor to be ready by retrying the TCP connection ─────
    // Instead of a fixed sleep, poll the listener until it accepts.
    let mut stream = None;
    for _ in 0..50 {
        match TcpStream::connect(listen_addr).await {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    let mut stream = stream.expect("failed to connect to echo listener after retries");

    // Drain any init events
    while subscription_rx.try_recv().is_ok() {}

    // ── 4. Exercise with real TCP traffic ────────────────────────────────
    let payload = b"hello theater";

    stream
        .write_all(payload)
        .await
        .expect("failed to send payload");

    // Shut down the write side so the actor sees a clean boundary.
    stream.shutdown().await.ok();

    let mut buf = vec![0u8; payload.len()];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut buf))
        .await
        .expect("read timed out — actor may not have echoed")
        .expect("failed to read echo response");

    assert_eq!(n, payload.len());
    assert_eq!(&buf[..n], payload, "echo mismatch");
    info!("Echo verified: {:?}", std::str::from_utf8(&buf[..n]));

    // Give the actor a moment to finish its close + logging before we pull the chain
    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── 5. Pull the chain and validate event types ───────────────────────
    let (events_tx, events_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorEvents {
            actor_id: actor_id.clone(),
            response_tx: events_tx,
        })
        .await
        .expect("failed to send GetActorEvents");

    let chain_events = events_rx
        .await
        .expect("events channel closed")
        .expect("GetActorEvents failed");

    info!("Chain has {} events", chain_events.len());
    for (i, event) in chain_events.iter().enumerate() {
        info!("  event[{}]: type={}", i, event.event_type);
    }

    // Collect event types for assertions
    let event_types: Vec<&str> = chain_events.iter().map(|e| e.event_type.as_str()).collect();

    // The chain should contain exactly these operation types (order may vary
    // slightly between runtime versions, so check presence rather than exact order).
    assert!(
        chain_events.len() >= 8,
        "expected at least 8 chain events (init + handle-connection + tcp ops + logs), got {}",
        chain_events.len()
    );

    // Must have wasm events (init call/return + handle-connection call/return)
    let wasm_count = event_types.iter().filter(|t| **t == "wasm").count();
    assert!(
        wasm_count >= 3,
        "expected at least 3 wasm events, got {}",
        wasm_count
    );

    // Must have TCP operations in the chain
    assert!(
        event_types.contains(&"theater:simple/tcp/receive"),
        "chain missing tcp/receive event. events: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"theater:simple/tcp/send"),
        "chain missing tcp/send event. events: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"theater:simple/tcp/close"),
        "chain missing tcp/close event. events: {:?}",
        event_types
    );

    // Must have log events from the actor
    let log_count = event_types
        .iter()
        .filter(|t| **t == "theater:simple/runtime/log")
        .count();
    assert!(
        log_count >= 3,
        "expected at least 3 log events (init + new connection + echoed/closed), got {}",
        log_count
    );

    // ── 6. Serialize the chain (could be saved as fixture) ───────────────
    let chain_json =
        serde_json::to_string_pretty(&chain_events).expect("failed to serialize chain");
    info!("Chain JSON length: {} bytes", chain_json.len());

    // ── 7. Tear down ────────────────────────────────────────────────────
    let (stop_tx, stop_rx) = oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id,
            response_tx: stop_tx,
        })
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(3), stop_rx).await;

    drop(theater_tx);
    let _ = tokio::time::timeout(Duration::from_secs(3), runtime_handle).await;
}
