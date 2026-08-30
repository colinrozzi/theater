//! Verifies the migrated supervisor test-actors against the reshaped
//! `theater:simple/supervisor` interface.
//!
//! Both actors declare `actor-info` + `supervisor-error` in their `pack_types!`
//! metadata and import the new view-scoped ops (`list-actors` / `spawn` /
//! `stop-actor`). The host enforces an interface *subset hash* at setup, so if
//! those type declarations don't mirror the pact exactly, the actor compiles but
//! fails to instantiate ("Interface hash mismatch"). Spawning each actor under a
//! real `TheaterRuntime` with the supervisor handler registered therefore proves
//! the migration is correct end-to-end: a successful spawn means setup +
//! interface-hash verification + `init` all passed.

use std::sync::Arc;
use std::time::Duration;

use theater::config::actor_manifest::{
    HandlerConfig, ManifestConfig, MessageServerConfig, SelfHostConfig, StoreHandlerConfig,
    SupervisorHostConfig,
};
use theater::config::inheritance::{HandlerInheritance, HandlerPermissionPolicy};
use theater::handler::HandlerRegistry;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::{Value, ValueType};
use theater::utils::ResourceCache;
use theater_handler_lifecycle::LifecycleHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_self::SelfHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use tokio::sync::{mpsc, oneshot};

const SPAWN_TIMEOUT: Duration = Duration::from_secs(10);

/// A handler registry with the full Theater handler stack. Handlers are
/// registered as templates with `None` permissions; the runtime clones each per
/// actor and threads the manifest's effective permissions in via
/// `set_permissions` at spawn time.
fn full_registry(theater_tx: mpsc::UnboundedSender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    registry.register(LifecycleHandler::new(theater_tx));
    registry.register(StoreHandler::new(StoreHandlerConfig::default(), None));
    registry.register(SupervisorHandler::new(SupervisorHostConfig {}, None));
    registry.register(MessageServerHandler::new(None, MessageRouter::new()));
    registry
}

/// A manifest that grants the supervisor capability. `supervisor: Inherit` takes
/// the parent's grant, and a top-level spawn's parent is `HandlerPermission::root()`
/// (scope: all, inspect + mutate) — so the actor gets full supervisor access.
fn granted_manifest(name: &str, wasm_path: &str, handlers: Vec<HandlerConfig>) -> ManifestConfig {
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: wasm_path.to_string(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy {
            supervisor: HandlerInheritance::Inherit,
            ..Default::default()
        },
        handlers,
    }
}

/// Start a runtime on a background task and return its command sender.
fn start_runtime() -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let registry = full_registry(theater_tx.clone());
    tokio::spawn(async move {
        let mut runtime = theater::theater_runtime::TheaterRuntime::new(
            tx_for_runtime,
            theater_rx,
            registry,
            Arc::new(ResourceCache::new()),
        )
        .await
        .expect("Failed to create runtime");
        runtime.run().await
    });
    theater_tx
}

/// Spawn an actor (setup + init) and return the runtime's spawn result.
async fn spawn_actor(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    name: &str,
    wasm_path: &str,
    manifest: ManifestConfig,
) -> std::result::Result<theater::id::TheaterId, theater::SpawnError> {
    let wasm_bytes = std::fs::read(wasm_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read {} wasm at {}: {}. Build it: \
             cd test-actors/{} && cargo build --release --target wasm32-unknown-unknown",
            name, wasm_path, e, name
        )
    });

    let (spawn_tx, spawn_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(name.to_string()),
            manifest: Some(manifest),
            init_state: default_init_state(),
            response_tx: spawn_tx,
            supervisor_tx: None,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SpawnActor");

    tokio::time::timeout(SPAWN_TIMEOUT, spawn_rx)
        .await
        .expect("spawn did not complete within timeout")
        .expect("spawn response channel closed")
}

fn wasm_path(actor_dir: &str, wasm_name: &str) -> String {
    format!(
        "{}/../../test-actors/{}/target/wasm32-unknown-unknown/release/{}",
        env!("CARGO_MANIFEST_DIR"),
        actor_dir,
        wasm_name
    )
}

/// multi-handler-test imports `theater:simple/supervisor.list-actors` and calls
/// it from `init`. A successful spawn proves the `actor-info`/`supervisor-error`
/// declarations hash-match the host's supervisor interface.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_handler_actor_instantiates_against_reshaped_supervisor() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,theater=debug")
        .try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let path = wasm_path("multi-handler-test", "multi_handler_test_actor.wasm");
    let manifest = granted_manifest(
        "multi-handler-test",
        &path,
        vec![
            HandlerConfig::SelfHandler {
                config: SelfHostConfig {},
            },
            HandlerConfig::Store {
                config: StoreHandlerConfig::default(),
            },
            HandlerConfig::Supervisor {
                config: SupervisorHostConfig {},
            },
        ],
    );

    let result = spawn_actor(&theater_tx, "multi-handler-test", &path, manifest).await;
    assert!(
        result.is_ok(),
        "multi-handler-test failed to spawn (interface hash mismatch or init failure): {:?}",
        result.err()
    );
}

/// supervisor-replay-test imports `spawn` / `list-actors` / `stop-actor` and
/// exports `handle-actor-external-stop`. A successful spawn proves all three
/// reshaped import signatures hash-match the host.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_replay_actor_instantiates_against_reshaped_supervisor() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,theater=debug")
        .try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let path = wasm_path(
        "supervisor-replay-test",
        "supervisor_replay_test_actor.wasm",
    );
    let manifest = granted_manifest(
        "supervisor-replay-test",
        &path,
        vec![
            HandlerConfig::SelfHandler {
                config: SelfHostConfig {},
            },
            HandlerConfig::Supervisor {
                config: SupervisorHostConfig {},
            },
            HandlerConfig::MessageServer {
                config: MessageServerConfig {},
            },
        ],
    );

    let result = spawn_actor(&theater_tx, "supervisor-replay-test", &path, manifest).await;
    assert!(
        result.is_ok(),
        "supervisor-replay-test failed to spawn (interface hash mismatch or init failure): {:?}",
        result.err()
    );
}

/// Spawn a plain state-test actor with an explicit parent, to build a tree.
async fn spawn_with_parent(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    wasm: Vec<u8>,
    name: &str,
    parent: Option<theater::id::TheaterId>,
) -> theater::id::TheaterId {
    let manifest = ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: String::new(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![HandlerConfig::SelfHandler {
            config: SelfHostConfig {},
        }],
    };
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: wasm,
            name: Some(name.to_string()),
            manifest: Some(manifest),
            init_state: default_init_state(),
            response_tx: tx,
            supervisor_tx: None,
            subscription_tx: None,
            parent_id: parent,
        })
        .expect("send SpawnActor");
    tokio::time::timeout(SPAWN_TIMEOUT, rx)
        .await
        .expect("spawn timed out")
        .expect("spawn channel closed")
        .expect("spawn failed")
}

async fn is_descendant(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    ancestor: theater::id::TheaterId,
    target: theater::id::TheaterId,
) -> bool {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::IsDescendant {
            ancestor,
            target,
            response_tx: tx,
        })
        .expect("send IsDescendant");
    rx.await.expect("recv").expect("is_descendant ok")
}

async fn get_descendants(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    root: theater::id::TheaterId,
) -> std::collections::HashSet<theater::id::TheaterId> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetDescendants {
            root,
            response_tx: tx,
        })
        .expect("send GetDescendants");
    rx.await
        .expect("recv")
        .expect("get_descendants ok")
        .into_iter()
        .map(|(id, _, _)| id)
        .collect()
}

/// The runtime owns + serves the supervision tree: IsDescendant / GetDescendants
/// answered from its own actor map over a real 3-level tree (parent → child →
/// grandchild) built via SpawnActor's parent_id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_serves_the_supervision_tree() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let path = wasm_path("state-test", "state_test_actor.wasm");
    let wasm = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path, e));

    let parent = spawn_with_parent(&theater_tx, wasm.clone(), "tree-parent", None).await;
    let child = spawn_with_parent(&theater_tx, wasm.clone(), "tree-child", Some(parent)).await;
    let grandchild =
        spawn_with_parent(&theater_tx, wasm.clone(), "tree-grandchild", Some(child)).await;

    // is-descendant: transitive down, never up.
    assert!(is_descendant(&theater_tx, parent, child).await);
    assert!(is_descendant(&theater_tx, parent, grandchild).await);
    assert!(is_descendant(&theater_tx, child, grandchild).await);
    assert!(!is_descendant(&theater_tx, child, parent).await);
    assert!(!is_descendant(&theater_tx, grandchild, parent).await);
    assert!(!is_descendant(&theater_tx, parent, parent).await); // strict

    // get-descendants: strict subtree.
    let from_parent = get_descendants(&theater_tx, parent).await;
    assert_eq!(
        from_parent.len(),
        2,
        "parent's subtree = child + grandchild"
    );
    assert!(from_parent.contains(&child) && from_parent.contains(&grandchild));
    assert!(
        !from_parent.contains(&parent),
        "descendants exclude the root"
    );

    assert_eq!(get_descendants(&theater_tx, child).await.len(), 1);
    assert!(get_descendants(&theater_tx, grandchild).await.is_empty());
}

async fn stop_actor(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    id: theater::id::TheaterId,
) {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::StopActor {
            actor_id: id,
            response_tx: tx,
        })
        .expect("send StopActor");
    // StopActor deregisters the actor before it responds.
    let _ = tokio::time::timeout(SPAWN_TIMEOUT, rx)
        .await
        .expect("stop timed out");
}

/// Death cascades along fate-links: stopping a mid-tree node tears down its
/// whole subtree, because every child is auto fate-linked (`StopSelf`) to its
/// parent at spawn. When `child` dies the runtime stops `grandchild`, which
/// dies and (had it any) would stop its own dependents — the emergent ripple.
/// Teardown is asynchronous (fire-and-forget signal + actor self-report), so we
/// poll the tree until it drains rather than expecting an instant update.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_cascade_tears_down_subtree_on_death() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let path = wasm_path("state-test", "state_test_actor.wasm");
    let wasm = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path, e));

    let parent = spawn_with_parent(&theater_tx, wasm.clone(), "d-parent", None).await;
    let child = spawn_with_parent(&theater_tx, wasm.clone(), "d-child", Some(parent)).await;
    let grandchild =
        spawn_with_parent(&theater_tx, wasm.clone(), "d-grandchild", Some(child)).await;

    assert_eq!(get_descendants(&theater_tx, parent).await.len(), 2);

    // Stop the middle node. It fate-links `grandchild`, so the runtime cascades:
    // child dies -> grandchild is stopped -> both leave the tree. The cascade
    // drains over a few command-loop turns, so poll until parent's subtree empties.
    stop_actor(&theater_tx, child).await;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if get_descendants(&theater_tx, parent).await.is_empty() {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "cascade did not drain parent's subtree: {:?}",
                get_descendants(&theater_tx, parent).await
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Both the stopped node and its cascaded descendant are gone.
    assert!(!is_descendant(&theater_tx, parent, child).await);
    assert!(!is_descendant(&theater_tx, parent, grandchild).await);
}

/// An `option<list<u8>>` init state carrying an actor id as a utf-8 string.
fn id_init_state(id: theater::id::TheaterId) -> Value {
    let s = id.to_string();
    Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: Some(Box::new(Value::List {
            elem_type: ValueType::U8,
            items: s.bytes().map(Value::U8).collect(),
        })),
    }
}

/// Spawn an actor with an explicit init state (no parent).
async fn spawn_with_init(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    wasm: Vec<u8>,
    name: &str,
    init_state: Value,
) -> theater::id::TheaterId {
    let manifest = ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: String::new(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: vec![HandlerConfig::SelfHandler {
            config: SelfHostConfig {},
        }],
    };
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: wasm,
            name: Some(name.to_string()),
            manifest: Some(manifest),
            init_state,
            response_tx: tx,
            supervisor_tx: None,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SpawnActor");
    tokio::time::timeout(SPAWN_TIMEOUT, rx)
        .await
        .expect("spawn timed out")
        .expect("spawn channel closed")
        .expect("spawn failed")
}

/// Read the monitor-test actor's `received` state field (its record of the last
/// lifecycle event delivered to `handle-lifecycle-event`).
async fn monitor_received(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    actor: theater::id::TheaterId,
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

/// Is `id` still a live actor?
async fn is_alive(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    id: theater::id::TheaterId,
) -> bool {
    let (tx, rx) = oneshot::channel();
    // If the runtime is gone (it exits once no actors remain), nothing is alive.
    if theater_tx
        .send(TheaterCommand::GetActors { response_tx: tx })
        .is_err()
    {
        return false;
    }
    match rx.await {
        Ok(Ok(actors)) => actors.iter().any(|(aid, _, _)| *aid == id),
        _ => false,
    }
}

/// End-to-end proof of `link` (StopSelf / fate): a linking actor is stopped
/// (cause `PeerKilled`) when the actor it linked terminates — matched and
/// triggered entirely in the lifecycle handler.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn link_peer_killed_stops_the_linking_actor() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let subject_wasm =
        std::fs::read(wasm_path("state-test", "state_test_actor.wasm")).expect("read subject wasm");
    let subject = spawn_with_parent(&theater_tx, subject_wasm, "l-subject", None).await;

    let linker_wasm =
        std::fs::read(wasm_path("link-test", "link_test_actor.wasm")).expect("read link wasm");
    let linker =
        spawn_with_init(&theater_tx, linker_wasm, "l-linker", id_init_state(subject)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        is_alive(&theater_tx, linker).await,
        "linker alive after spawn"
    );

    // Stop the subject → its terminal event → the linker's handler matches its
    // link → PeerTerminated(linker) → the runtime stops the linker.
    stop_actor(&theater_tx, subject).await;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if !is_alive(&theater_tx, linker).await {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("linker was not peer-killed when its linked subject died");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// End-to-end proof of `monitor` (DeliverToWasm): a monitor actor watches a
/// subject, the subject terminates, and the terminal event is delivered to the
/// monitor's `handle-lifecycle-event` export — event goes chain → lifecycle
/// handler (host-side filter) → wasm, with the runtime not in the path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitor_delivers_lifecycle_events_to_wasm() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let temp = tempfile::tempdir().expect("temp dir");
    std::env::set_var("THEATER_HOME", temp.path());

    let theater_tx = start_runtime();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Subject: a plain state-test actor.
    let subject_wasm =
        std::fs::read(wasm_path("state-test", "state_test_actor.wasm")).expect("read subject wasm");
    let subject = spawn_with_parent(&theater_tx, subject_wasm, "m-subject", None).await;

    // Monitor: handed the subject id as init state; it calls monitor(subject).
    let monitor_wasm = std::fs::read(wasm_path("monitor-test", "monitor_test_actor.wasm"))
        .expect("read monitor wasm");
    let monitor = spawn_with_init(
        &theater_tx,
        monitor_wasm,
        "m-monitor",
        id_init_state(subject),
    )
    .await;
    // Give the monitor a moment to register its subscription.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Stop the subject -> it terminates -> the terminal event is delivered to
    // the monitor's handle-lifecycle-event.
    stop_actor(&theater_tx, subject).await;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if monitor_received(&theater_tx, monitor).await.as_deref() == Some("terminated") {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "monitor never received the terminated event; state = {:?}",
                monitor_received(&theater_tx, monitor).await
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
