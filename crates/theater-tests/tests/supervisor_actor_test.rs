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
use theater::utils::ResourceCache;
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
fn full_registry(theater_tx: mpsc::Sender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(SelfHostConfig {}, theater_tx, None));
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
fn start_runtime() -> mpsc::Sender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
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
    theater_tx: &mpsc::Sender<TheaterCommand>,
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
        .await
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
    theater_tx: &mpsc::Sender<TheaterCommand>,
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
        .await
        .expect("send SpawnActor");
    tokio::time::timeout(SPAWN_TIMEOUT, rx)
        .await
        .expect("spawn timed out")
        .expect("spawn channel closed")
        .expect("spawn failed")
}

async fn is_descendant(
    theater_tx: &mpsc::Sender<TheaterCommand>,
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
        .await
        .expect("send IsDescendant");
    rx.await.expect("recv").expect("is_descendant ok")
}

async fn get_descendants(
    theater_tx: &mpsc::Sender<TheaterCommand>,
    root: theater::id::TheaterId,
) -> std::collections::HashSet<theater::id::TheaterId> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetDescendants {
            root,
            response_tx: tx,
        })
        .await
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
