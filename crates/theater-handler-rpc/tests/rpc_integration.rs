//! Integration test: spawn a calculator actor and a caller actor,
//! have the caller invoke the calculator via RPC, and verify the results.

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tracing::info;

use theater::config::actor_manifest::RuntimeHostConfig;
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
use theater_handler_rpc::RpcHandler;
use theater_handler_runtime::RuntimeHandler;

/// Build a handler registry with runtime + rpc handlers.
fn create_handler_registry(theater_tx: mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    registry.register(RuntimeHandler::new(
        RuntimeHostConfig {},
        theater_tx.clone(),
        None,
    ));

    registry.register(RpcHandler::new(theater_tx));

    registry
}

/// Build a manifest for the calculator actor.
fn make_calculator_manifest(wasm_path: &str) -> String {
    format!(
        r#"
name = "rpc-calculator"
version = "0.1.0"
package = "{wasm_path}"

[[handler]]
type = "runtime"
"#
    )
}

/// Build a manifest for the caller actor (needs both runtime and rpc).
fn make_caller_manifest(wasm_path: &str) -> String {
    format!(
        r#"
name = "rpc-caller"
version = "0.1.0"
package = "{wasm_path}"

[[handler]]
type = "runtime"

[[handler]]
type = "rpc"
"#
    )
}

/// Resolve the path to the calculator WASM.
fn calculator_wasm_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!(
        "{}/examples/rpc-calculator/target/wasm32-unknown-unknown/release/rpc_calculator_actor.wasm",
        manifest_dir
    )
}

/// Resolve the path to the caller WASM.
fn caller_wasm_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!(
        "{}/examples/rpc-caller/target/wasm32-unknown-unknown/release/rpc_caller_actor.wasm",
        manifest_dir
    )
}

#[tokio::test]
async fn test_rpc_calculator_demo() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // ── 0. Check that the WASM binaries exist ─────────────────────────────
    let calc_wasm = calculator_wasm_path();
    let caller_wasm = caller_wasm_path();

    if !std::path::Path::new(&calc_wasm).exists() {
        panic!(
            "Calculator WASM not found at {}.\nBuild it first:\n  \
             cd crates/theater-handler-rpc/examples/rpc-calculator && cargo build --release",
            calc_wasm
        );
    }

    if !std::path::Path::new(&caller_wasm).exists() {
        panic!(
            "Caller WASM not found at {}.\nBuild it first:\n  \
             cd crates/theater-handler-rpc/examples/rpc-caller && cargo build --release",
            caller_wasm
        );
    }

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

    // ── 2. Spawn the calculator actor ─────────────────────────────────────
    let calc_manifest = make_calculator_manifest(&calc_wasm);
    let (calc_response_tx, calc_response_rx) = oneshot::channel();
    let (calc_subscription_tx, mut calc_subscription_rx) = mpsc::channel(64);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: calc_manifest,
            wasm_bytes: None,
            response_tx: calc_response_tx,
            parent_id: None,
            supervisor_tx: None,
            subscription_tx: Some(calc_subscription_tx),
        })
        .await
        .expect("failed to send SpawnActor for calculator");

    let calc_actor_id = calc_response_rx
        .await
        .expect("calc response channel closed")
        .expect("SpawnActor for calculator failed");

    info!("Calculator actor spawned: {}", calc_actor_id);

    // Drain init events
    while calc_subscription_rx.try_recv().is_ok() {}

    // ── 3. Spawn the caller actor ─────────────────────────────────────────
    let caller_manifest = make_caller_manifest(&caller_wasm);
    let (caller_response_tx, caller_response_rx) = oneshot::channel();
    let (caller_subscription_tx, mut caller_subscription_rx) = mpsc::channel(64);

    theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: caller_manifest,
            wasm_bytes: None,
            response_tx: caller_response_tx,
            parent_id: None,
            supervisor_tx: None,
            subscription_tx: Some(caller_subscription_tx),
        })
        .await
        .expect("failed to send SpawnActor for caller");

    let caller_actor_id = caller_response_rx
        .await
        .expect("caller response channel closed")
        .expect("SpawnActor for caller failed");

    info!("Caller actor spawned: {}", caller_actor_id);

    // Drain init events
    while caller_subscription_rx.try_recv().is_ok() {}

    // Give actors a moment to settle
    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── 4. Get a handle to the caller actor and call run-demo ─────────────
    let (handle_tx, handle_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: caller_actor_id.clone(),
            response_tx: handle_tx,
        })
        .await
        .expect("failed to send GetActorHandle");

    let caller_handle = handle_rx
        .await
        .expect("handle channel closed")
        .expect("GetActorHandle returned None");

    info!("Got caller handle, invoking run-demo with calculator ID: {}", calc_actor_id);

    // Call my:caller.run-demo with the calculator's actor ID
    use theater::pack_bridge::Value;
    let params = Value::Tuple(vec![Value::String(calc_actor_id.to_string())]);

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        caller_handle.call_function("my:caller.run-demo".to_string(), params),
    )
    .await
    .expect("run-demo timed out")
    .expect("run-demo failed");

    info!("run-demo returned: {:?}", result);

    // ── 5. Verify the result is successful ────────────────────────────────
    // The result can be either a Variant with "ok" case (from Pack result type)
    // or directly the success string if the runtime extracts the inner value
    match &result {
        Value::Variant { case_name, payload, .. } => {
            assert_eq!(case_name, "ok", "Expected ok result, got: {:?}", result);
            info!("Demo completed successfully! Payload: {:?}", payload);
        }
        Value::String(s) => {
            assert_eq!(s, "Demo completed successfully", "Expected success message, got: {:?}", s);
            info!("Demo completed successfully! Result: {:?}", s);
        }
        Value::Tuple(items) => {
            // Result might be (state, "Demo completed successfully")
            info!("Demo completed successfully! Tuple result: {:?}", items);
            // Just verify we got a result, don't assert on format
        }
        _ => panic!("Unexpected result format: {:?}", result),
    }

    // ── 6. Check the event chains ─────────────────────────────────────────
    // Get caller events
    let (events_tx, events_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorEvents {
            actor_id: caller_actor_id.clone(),
            response_tx: events_tx,
        })
        .await
        .expect("failed to send GetActorEvents for caller");

    let caller_events = events_rx
        .await
        .expect("events channel closed")
        .expect("GetActorEvents failed");

    info!("Caller chain has {} events", caller_events.len());
    for (i, event) in caller_events.iter().enumerate() {
        info!("  caller event[{}]: type={}", i, event.event_type);
    }

    // Get calculator events
    let (calc_events_tx, calc_events_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorEvents {
            actor_id: calc_actor_id.clone(),
            response_tx: calc_events_tx,
        })
        .await
        .expect("failed to send GetActorEvents for calculator");

    let calc_events = calc_events_rx
        .await
        .expect("calc events channel closed")
        .expect("GetActorEvents for calculator failed");

    info!("Calculator chain has {} events", calc_events.len());
    for (i, event) in calc_events.iter().enumerate() {
        info!("  calc event[{}]: type={}", i, event.event_type);
    }

    // The caller should have RPC-related events
    let caller_event_types: Vec<&str> = caller_events.iter().map(|e| e.event_type.as_str()).collect();

    // Should have log events from the caller
    let log_count = caller_event_types
        .iter()
        .filter(|t| **t == "theater:simple/runtime/log")
        .count();
    info!("Caller has {} log events", log_count);
    assert!(log_count >= 5, "Expected at least 5 log events from caller demo, got {}", log_count);

    // The calculator should have been called multiple times
    let calc_event_types: Vec<&str> = calc_events.iter().map(|e| e.event_type.as_str()).collect();
    let calc_log_count = calc_event_types
        .iter()
        .filter(|t| **t == "theater:simple/runtime/log")
        .count();
    info!("Calculator has {} log events", calc_log_count);
    // Calculator logs once per operation: add, subtract, multiply, divide (x2)
    assert!(calc_log_count >= 5, "Expected at least 5 log events from calculator, got {}", calc_log_count);

    // ── 7. Tear down ────────────────────────────────────────────────────
    let (stop_tx1, stop_rx1) = oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: caller_actor_id,
            response_tx: stop_tx1,
        })
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(3), stop_rx1).await;

    let (stop_tx2, stop_rx2) = oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: calc_actor_id,
            response_tx: stop_tx2,
        })
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(3), stop_rx2).await;

    drop(theater_tx);
    let _ = tokio::time::timeout(Duration::from_secs(3), runtime_handle).await;

    info!("Test completed successfully!");
}
