//! Regression lock: **a steady-state guest panic yields a `Failed` termination
//! that monitors receive.**
//!
//! `packr-guest` 0.24.1 fixed the guest panic handler: a panic in a post-init
//! handler callback now turns into a wasm **trap**. Before that fix the handler
//! was `loop {}` — a panic SPUN SILENTLY: the actor never trapped, the runtime
//! never reported `Failed`, and the terminal event was never delivered, so a
//! supervisor/monitor could not catch a steady-state crash. Now the trap → the
//! runtime reports `Failed` → the terminal event flows to monitors. This test
//! locks that end-to-end path so it can't silently regress.
//!
//! Shape (mirrors `supervisor_actor_test` + `message_server_client_test`):
//!   - spawn `panic-child` (comes up healthy in `init`),
//!   - spawn `monitor-test` watching it (via the `lifecycle` handler),
//!   - call `panic-child`'s `boom` — it panics → traps → surfaces to the caller
//!     as an `ActorError` (asserted `Err`),
//!   - poll the monitor's state until it records `"terminated"` — i.e. the
//!     `Failed` terminal event reached the monitor's `handle-actor-event`.
//!
//! ## What only CI verifies
//! This harness is checked with `cargo check -p theater-tests --test
//! panic_yields_failed_test`; the *behavior* is verified only in CI, because it
//! needs the two actors **built to wasm32** (there is no wasm32 target locally):
//!   - building `panic-child` (pinned `packr-guest` 0.24.1) and `monitor-test`,
//!   - the actual spawn → `boom` → **trap** → `Failed` → deliver-to-monitor —
//!     the 0.24.1 panic-trap only happens on wasm32, so only a CI run that reads
//!     the real `.wasm` exercises the fix this test locks.

use std::sync::Arc;
use std::time::Duration;

use theater::config::actor_manifest::{HandlerConfig, ManifestConfig};
use theater::config::inheritance::HandlerPermissionPolicy;
use theater::handler::HandlerRegistry;
use theater::id::TheaterId;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::{Value, ValueType};
use theater::utils::ResourceCache;
use theater_handler_lifecycle::LifecycleHandler;
use theater_handler_self::{SelfHandler, SelfHostConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

const OP_TIMEOUT: Duration = Duration::from_secs(10);

/// A registry with the `self` + `lifecycle` handlers — enough for `panic-child`
/// (self only) and `monitor-test` (self + lifecycle's `monitor` /
/// `handle-actor-event`). Registered as templates with `None` permissions; the
/// runtime clones each per actor.
fn start_runtime() -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    registry.register(LifecycleHandler::new(theater_tx.clone()));
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

fn wasm_path(actor_dir: &str, wasm_name: &str) -> String {
    format!(
        "{}/../../test-actors/{}/target/wasm32-unknown-unknown/release/{}",
        env!("CARGO_MANIFEST_DIR"),
        actor_dir,
        wasm_name
    )
}

/// A minimal `self`-only manifest. The `lifecycle` interface is served to every
/// actor because the handler is registered in the registry (cf.
/// `supervisor_actor_test`, where `monitor-test` spawns with a `self`-only
/// manifest yet still monitors).
fn self_only_manifest(name: &str, package: &str) -> ManifestConfig {
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: package.to_string(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![HandlerConfig::unit("self")],
    }
}

/// Set up an actor from its wasm (no auto-init) and return its id.
async fn setup_actor(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    name: &str,
    dir: &str,
    wasm_name: &str,
) -> TheaterId {
    let path = wasm_path(dir, wasm_name);
    let wasm_bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read {} wasm at {}: {}. Build it: \
             cd test-actors/{} && cargo build --release --target wasm32-unknown-unknown",
            name, path, e, dir
        )
    });
    let (setup_tx, setup_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SetupActor {
            wasm_bytes,
            name: Some(name.to_string()),
            manifest: Some(self_only_manifest(name, &path)),
            init_state: default_init_state(),
            response_tx: setup_tx,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SetupActor");
    timeout(OP_TIMEOUT, setup_rx)
        .await
        .expect("setup timed out")
        .expect("setup channel closed")
        .expect("setup failed")
}

/// Fetch an actor's handle (used to drive `init` and `boom`).
async fn get_handle(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    id: TheaterId,
) -> theater::actor::handle::ActorHandle {
    let (h_tx, h_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: id,
            response_tx: h_tx,
        })
        .expect("send GetActorHandle");
    timeout(OP_TIMEOUT, h_rx)
        .await
        .expect("handle timed out")
        .expect("handle channel closed")
        .expect("handle present")
}

/// Spawn `monitor-test` with the subject id threaded in as its init config, so
/// its auto-run `init` calls `monitor(subject)` and establishes the watch.
async fn spawn_monitor(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    subject: TheaterId,
) -> TheaterId {
    let path = wasm_path("monitor-test", "monitor_test_actor.wasm");
    let wasm_bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read monitor-test wasm at {}: {}. Build it: \
             cd test-actors/monitor-test && cargo build --release --target wasm32-unknown-unknown",
            path, e
        )
    });
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some("panic-monitor".to_string()),
            manifest: Some(self_only_manifest("panic-monitor", &path)),
            // monitor-test reads the subject from its init config as an
            // `option<list<u8>>` of the id's utf-8 bytes.
            init_state: id_init_state(subject),
            response_tx: tx,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SpawnActor");
    timeout(OP_TIMEOUT, rx)
        .await
        .expect("spawn timed out")
        .expect("spawn channel closed")
        .expect("spawn failed")
}

/// An `option<list<u8>>` init config carrying an actor id as a utf-8 string —
/// exactly what `monitor-test`'s `subject_from_config` decodes.
fn id_init_state(id: TheaterId) -> Value {
    let s = id.to_string();
    Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: Some(Box::new(Value::List {
            elem_type: ValueType::U8,
            items: s.bytes().map(Value::U8).collect(),
        })),
    }
}

/// Read the monitor's `received` field (its record of the last lifecycle event
/// delivered to `handle-actor-event`).
async fn monitor_received(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    actor: TheaterId,
) -> Option<String> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorState {
            actor_id: actor,
            response_tx: tx,
        })
        .ok()?;
    let state = rx.await.ok()?.ok()?;
    match state {
        Value::Record { fields, .. } => fields.into_iter().find_map(|(n, v)| {
            if n == "received" {
                if let Value::String(s) = v {
                    return Some(s);
                }
            }
            None
        }),
        _ => None,
    }
}

/// A panic in a *post-init* handler call traps (packr-guest 0.24.1), the runtime
/// reports `Failed`, and that terminal event is delivered to a monitor. Before
/// 0.24.1 the panic spun silently — no trap, no `Failed`, nothing delivered —
/// so this whole path never fired and a supervisor could not catch the crash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_in_call_yields_failed_terminal_delivered_to_monitor() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // (a) Spawn panic-child (setup + explicit init) so it comes up healthy.
    let panic_child = setup_actor(
        &theater_tx,
        "panic-child",
        "panic-child",
        "panic_child_actor.wasm",
    )
    .await;
    let child_handle = get_handle(&theater_tx, panic_child.clone()).await;
    child_handle
        .call_function(
            "theater:simple/actor.init".to_string(),
            Value::Tuple(vec![]),
        )
        .await
        .expect("panic-child init");

    // (b) Spawn the monitor watching panic-child; its init establishes the watch.
    let monitor = spawn_monitor(&theater_tx, panic_child.clone()).await;
    // Give the monitor a moment to register its chain subscription.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // (c) Trigger the steady-state panic. The guest trap surfaces to the caller
    // as an ActorError.
    let boom = child_handle
        .call_function(
            "theater:simple/panic-child.boom".to_string(),
            Value::Tuple(vec![]),
        )
        .await;
    assert!(
        boom.is_err(),
        "boom must trap and surface to the caller as an ActorError, got: {:?}",
        boom
    );

    // (d) The Failed termination must reach the monitor. Before the 0.24.1 trap
    // fix this never arrived (the panic spun silently); its arrival is exactly
    // what this test locks. Poll the monitor's recorded last-event-type.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if monitor_received(&theater_tx, monitor.clone())
            .await
            .as_deref()
            == Some("terminated")
        {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "monitor never received the Failed terminal event after the guest panic; \
                 last recorded state = {:?}",
                monitor_received(&theater_tx, monitor.clone()).await
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
