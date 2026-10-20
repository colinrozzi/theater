//! End-to-end gate for the `theater:simple/process` handler.
//!
//! Spawns the `process-test` actor through the real `TheaterRuntime` with the
//! `self` + `process` handlers and drives its `probe.*` exports. This covers
//! what the handler's unit suite can't: the import-hash match for the record
//! types, Value round-trips through the Pack ABI, active-mode delivery into
//! real `process-client` exports, and the owner-stop teardown wired through
//! the runtime's shutdown path.

use std::sync::Arc;
use std::time::Duration;

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::{HandlerConfig, ManifestConfig};
use theater::config::inheritance::HandlerPermissionPolicy;
use theater::handler::HandlerRegistry;
use theater::id::TheaterId;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::Value;
use theater::utils::ResourceCache;
use theater_handler_process::{ProcessHandler, ProcessHandlerConfig};
use theater_handler_self::{SelfHandler, SelfHostConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

fn wasm_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/process-test/target/wasm32-unknown-unknown/release/process_test_actor.wasm"
    )
    .to_string()
}

fn start_runtime() -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    registry.register(ProcessHandler::new(ProcessHandlerConfig::default()));
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

fn manifest() -> ManifestConfig {
    ManifestConfig {
        name: "process-test".to_string(),
        version: "0.1.0".to_string(),
        package: wasm_path(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![HandlerConfig::unit("self"), HandlerConfig::unit("process")],
    }
}

/// Spawn the actor, run init, return its id + handle.
async fn spawn_actor(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
) -> (TheaterId, ActorHandle) {
    let wasm_bytes = std::fs::read(wasm_path()).unwrap_or_else(|e| {
        panic!(
            "read process-test wasm: {}. Build it: \
             cd test-actors/process-test && cargo build --release --target wasm32-unknown-unknown",
            e
        )
    });
    let (setup_tx, setup_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some("process-test".to_string()),
            manifest: Some(manifest()),
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
        .expect("setup ok (an import-hash mismatch fails here)");

    let (h_tx, h_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id,
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
    (actor_id, handle)
}

/// Call a probe; the runtime unwraps its `result<T, string>` (an `err` comes
/// back as the call's error).
async fn probe(handle: &ActorHandle, name: &str, args: Vec<Value>) -> Value {
    timeout(
        Duration::from_secs(20),
        handle.call_function(
            format!("theater:process-test/probe.{name}"),
            Value::Tuple(args),
        ),
    )
    .await
    .unwrap_or_else(|_| panic!("probe.{name} timed out"))
    .unwrap_or_else(|e| panic!("probe.{name} failed: {e:?}"))
}

async fn probe_string(handle: &ActorHandle, name: &str, args: Vec<Value>) -> String {
    match probe(handle, name, args).await {
        Value::String(s) => s,
        other => panic!("probe.{name}: expected string, got {other:?}"),
    }
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_handler_end_to_end() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();
    let theater_tx = start_runtime();
    let (actor_id, handle) = spawn_actor(&theater_tx).await;

    // run(): spawn + collect + wait in one host call.
    assert_eq!(
        probe_string(&handle, "run-echo", vec![]).await,
        r#"stdout="hello\n" stderr="oops\n" code=Some(3)"#
    );

    // spawn / write-stdin / close-stdin / passive read (split at 3 bytes) / wait.
    assert_eq!(
        probe_string(
            &handle,
            "cat-roundtrip",
            vec![Value::String("through the actor".into())]
        )
        .await,
        "through the actor|code=Some(0)"
    );

    // A PTY child sees a real terminal of the requested size.
    assert_eq!(probe_string(&handle, "pty-size", vec![]).await, "40 100");

    // Active mode: output pushed into on-output, then on-exit last.
    probe_string(
        &handle,
        "start-active",
        vec![Value::String(
            "echo one; echo two >&2; echo three; exit 4".into(),
        )],
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let events = loop {
        let events = probe_string(&handle, "events", vec![]).await;
        if events.contains("exit:") || tokio::time::Instant::now() > deadline {
            break events;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let lines: Vec<&str> = events.lines().collect();
    assert_eq!(
        lines.last(),
        Some(&"exit:Some(4)"),
        "on-exit must come last: {events:?}"
    );
    // Chunk boundaries are up to the OS; concatenate each stream's payloads.
    let stream = |name: &str| -> String {
        lines
            .iter()
            .filter_map(|l| l.strip_prefix(name))
            .map(|p| p.trim_matches('"'))
            .collect()
    };
    assert_eq!(
        stream("stdout:"),
        r"one\nthree\n",
        "stdout in order: {events:?}"
    );
    assert_eq!(stream("stderr:"), r"two\n", "{events:?}");

    // Stopping the owner kills what it still owns.
    let os_pid = match probe(&handle, "spawn-sleeper", vec![]).await {
        Value::U32(n) => n,
        other => panic!("spawn-sleeper: expected u32, got {other:?}"),
    };
    assert!(pid_alive(os_pid), "sleeper should be running");
    let (stop_tx, stop_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id,
            response_tx: stop_tx,
        })
        .expect("send StopActor");
    timeout(Duration::from_secs(10), stop_rx)
        .await
        .expect("stop timeout")
        .expect("stop channel")
        .expect("stop ok");
    // The handler terminates + reaps before acking shutdown, so by the time the
    // stop is acknowledged the child is gone.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while pid_alive(os_pid) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!pid_alive(os_pid), "the actor's process must die with it");
}
