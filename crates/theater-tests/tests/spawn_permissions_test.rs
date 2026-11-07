//! A spawned actor gets at most what its spawner has.
//!
//! Spawns through the real `TheaterRuntime` with `parent_id` set, the way
//! `runtime.spawn` does: a host-started parent gets the host's filesystem root,
//! hosts and programs; a child's `[[handler]]` block may narrow them but not
//! widen them, and a grandchild is held to the child's narrowed set, not the
//! host's. Refusals happen before the wasm is instantiated; the actors that do
//! start are the plain `state-test` actor.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use theater::config::actor_manifest::{
    FileSystemHandlerConfig, HandlerConfig, HttpClientHandlerConfig, ManifestConfig,
    ReplayHandlerConfig,
};
use theater::config::inheritance::{HandlerInheritance, HandlerPermissionPolicy};
use theater::config::permissions::HttpClientPermissions;
use theater::handler::HandlerRegistry;
use theater::id::TheaterId;
use theater::messages::{default_init_state, TheaterCommand};
use theater::utils::ResourceCache;
use theater::SpawnError;
use theater_handler_filesystem::FileSystemHandler;
use theater_handler_http_client::HttpClientHandler;
use theater_handler_process::{ProcessHandler, ProcessHandlerConfig};
use theater_handler_rpc::{RpcHandler, RpcHandlerConfig};
use theater_handler_self::{SelfHandler, SelfHostConfig};
use theater_handler_store::{StoreHandler, StoreHandlerConfig};
use tokio::sync::{mpsc, oneshot};

const SPAWN_TIMEOUT: Duration = Duration::from_secs(10);

fn wasm() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-actors/state-test/target/wasm32-unknown-unknown/release/state_test_actor.wasm"
    );
    std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "read {path}: {e}. Build it: \
             cd test-actors/state-test && cargo build --release --target wasm32-unknown-unknown"
        )
    })
}

/// The host's handlers: filesystem rooted at `root`, two hosts, two programs,
/// rpc callable by children.
fn start_runtime(root: &Path) -> mpsc::UnboundedSender<TheaterCommand> {
    let (theater_tx, theater_rx) = mpsc::unbounded_channel::<TheaterCommand>();
    let tx_for_runtime = theater_tx.clone();
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    registry.register(FileSystemHandler::new(
        FileSystemHandlerConfig {
            path: Some(root.to_path_buf()),
            new_dir: None,
            allowed_commands: None,
        },
        None,
    ));
    registry.register(HttpClientHandler::new(HttpClientHandlerConfig {
        allowed_hosts: vec!["a.example".into(), "b.example".into()],
    }));
    registry.register(StoreHandler::new(
        StoreHandlerConfig {
            base_path: Some(root.join("store")),
            store_id: None,
        },
        None,
    ));
    registry.register(ProcessHandler::new(ProcessHandlerConfig {
        allowed_programs: Some(vec!["git".into(), "ls".into()]),
        ..Default::default()
    }));
    registry.register(RpcHandler::with_config(
        theater_tx.clone(),
        RpcHandlerConfig {
            callers: Some(vec!["children".into()]),
        },
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
        .expect("create runtime");
        runtime.run().await
    });
    theater_tx
}

fn manifest(name: &str, handlers: Vec<HandlerConfig>) -> ManifestConfig {
    let mut all = vec![HandlerConfig::unit("self")];
    all.extend(handlers);
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: String::new(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy::default(),
        handlers: all,
    }
}

fn fs(path: PathBuf) -> HandlerConfig {
    HandlerConfig::new(
        "filesystem",
        FileSystemHandlerConfig {
            path: Some(path),
            new_dir: None,
            allowed_commands: None,
        },
    )
}

fn hosts(hosts: &[&str]) -> HandlerConfig {
    HandlerConfig::new(
        "http-client",
        HttpClientHandlerConfig {
            allowed_hosts: hosts.iter().map(|h| h.to_string()).collect(),
        },
    )
}

fn programs(programs: &[&str]) -> HandlerConfig {
    HandlerConfig::new(
        "process",
        ProcessHandlerConfig {
            allowed_programs: Some(programs.iter().map(|p| p.to_string()).collect()),
            ..Default::default()
        },
    )
}

fn store(base_path: PathBuf) -> HandlerConfig {
    HandlerConfig::new(
        "store",
        StoreHandlerConfig {
            base_path: Some(base_path),
            store_id: None,
        },
    )
}

async fn spawn(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    manifest: ManifestConfig,
    parent: Option<TheaterId>,
) -> Result<TheaterId, SpawnError> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes: wasm(),
            name: Some(manifest.name.clone()),
            manifest: Some(manifest),
            init_state: default_init_state(),
            response_tx: tx,
            subscription_tx: None,
            parent_id: parent,
        })
        .expect("send SpawnActor");
    tokio::time::timeout(SPAWN_TIMEOUT, rx)
        .await
        .expect("spawn timed out")
        .expect("spawn channel closed")
}

/// The spawn is refused, and the error names `handler` and `key`.
async fn refused(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    manifest: ManifestConfig,
    parent: TheaterId,
    handler: &str,
    key: &str,
) {
    let name = manifest.name.clone();
    match spawn(theater_tx, manifest, Some(parent)).await {
        Ok(id) => panic!("{name}: spawn should be refused, got {id}"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(&format!("'{handler}'")) && msg.contains(key),
                "{name}: error should name '{handler}' and {key}: {msg}"
            );
        }
    }
}

/// A host root with `sub/` inside it and `outside/` next to it.
fn dirs() -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(base.path().join("outside")).unwrap();
    (base, root)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_cannot_widen_fs_store_hosts_or_programs() {
    let (base, root) = dirs();
    let tx = start_runtime(&root);
    let parent = spawn(&tx, manifest("parent", vec![]), None)
        .await
        .expect("host-started parent");

    refused(
        &tx,
        manifest("fs-root", vec![fs("/".into())]),
        parent,
        "filesystem",
        "path",
    )
    .await;
    refused(
        &tx,
        manifest("fs-sibling", vec![fs(base.path().join("outside"))]),
        parent,
        "filesystem",
        "path",
    )
    .await;
    refused(
        &tx,
        manifest("fs-dotdot", vec![fs(root.join("sub/../../outside"))]),
        parent,
        "filesystem",
        "..",
    )
    .await;
    std::os::unix::fs::symlink(base.path().join("outside"), root.join("link")).unwrap();
    refused(
        &tx,
        manifest("fs-symlink", vec![fs(root.join("link"))]),
        parent,
        "filesystem",
        "path",
    )
    .await;
    refused(
        &tx,
        manifest("store-root", vec![store("/".into())]),
        parent,
        "store",
        "base_path",
    )
    .await;
    refused(
        &tx,
        manifest("hosts", vec![hosts(&["a.example", "evil.example"])]),
        parent,
        "http-client",
        "allowed_hosts",
    )
    .await;
    refused(
        &tx,
        manifest("programs", vec![programs(&["git", "bash"])]),
        parent,
        "process",
        "allowed_programs",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn narrowing_works_and_grandchild_cannot_exceed_child() {
    let (_base, root) = dirs();
    let tx = start_runtime(&root);
    let parent = spawn(&tx, manifest("parent", vec![]), None)
        .await
        .expect("host-started parent");

    let child = spawn(
        &tx,
        manifest(
            "child",
            vec![
                fs(root.join("sub")),
                store(root.join("store/sub")),
                hosts(&["a.example"]),
                programs(&["git"]),
            ],
        ),
        Some(parent),
    )
    .await
    .expect("a child may narrow");

    // Back up to what the parent had: refused, the child no longer has it.
    refused(
        &tx,
        manifest("g-fs", vec![fs(root.clone())]),
        child,
        "filesystem",
        "path",
    )
    .await;
    refused(
        &tx,
        manifest("g-store", vec![store(root.join("store"))]),
        child,
        "store",
        "base_path",
    )
    .await;
    refused(
        &tx,
        manifest("g-hosts", vec![hosts(&["b.example"])]),
        child,
        "http-client",
        "allowed_hosts",
    )
    .await;
    refused(
        &tx,
        manifest("g-programs", vec![programs(&["ls"])]),
        child,
        "process",
        "allowed_programs",
    )
    .await;

    // Narrower still, or nothing asked (inherits the child's set): fine.
    spawn(
        &tx,
        manifest("g-narrower", vec![fs(root.join("sub/deeper")), hosts(&[])]),
        Some(child),
    )
    .await
    .expect("a grandchild may narrow further");
    spawn(&tx, manifest("g-inherits", vec![]), Some(child))
        .await
        .expect("a grandchild may inherit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_restriction_narrows_what_children_may_ask_for() {
    let (_base, root) = dirs();
    let tx = start_runtime(&root);
    let parent = spawn(&tx, manifest("parent", vec![]), None)
        .await
        .expect("host-started parent");

    // The child keeps the parent's handler config (both hosts) but its policy
    // narrows its grant to one host.
    let mut narrowed = manifest("child", vec![]);
    narrowed.permission_policy.http_client = HandlerInheritance::Restrict(HttpClientPermissions {
        allowed_methods: None,
        allowed_hosts: Some(vec!["a.example".into()]),
        max_redirects: None,
        timeout: None,
    });
    let child = spawn(&tx, narrowed, Some(parent))
        .await
        .expect("a policy may narrow");

    refused(
        &tx,
        manifest("g-hosts", vec![hosts(&["b.example"])]),
        child,
        "http-client",
        "allowed_hosts",
    )
    .await;
    spawn(
        &tx,
        manifest("g-ok", vec![hosts(&["a.example"])]),
        Some(child),
    )
    .await
    .expect("within the grant");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replay_chain_must_be_one_of_the_spawners_files() {
    let (base, root) = dirs();
    let tx = start_runtime(&root);
    let parent = spawn(&tx, manifest("parent", vec![]), None)
        .await
        .expect("host-started parent");
    let replay = |chain: PathBuf| HandlerConfig::new("replay", ReplayHandlerConfig { chain });

    refused(
        &tx,
        manifest("replay-etc", vec![replay("/etc/passwd".into())]),
        parent,
        "replay",
        "chain",
    )
    .await;
    std::fs::write(base.path().join("outside/x.json"), "[]").unwrap();
    std::os::unix::fs::symlink(base.path().join("outside/x.json"), root.join("x.json")).unwrap();
    refused(
        &tx,
        manifest("replay-symlink", vec![replay(root.join("x.json"))]),
        parent,
        "replay",
        "chain",
    )
    .await;

    // A path inside the root passes the check and is then read.
    let err = spawn(
        &tx,
        manifest("replay-inside", vec![replay(root.join("sub/none.json"))]),
        Some(parent),
    )
    .await
    .expect_err("the chain file does not exist");
    assert!(
        err.to_string().contains("Failed to read replay chain file"),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_cannot_be_callable_by_more_than_its_spawner_allows() {
    let (_base, root) = dirs();
    let tx = start_runtime(&root);
    let parent = spawn(&tx, manifest("parent", vec![]), None)
        .await
        .expect("host-started parent");
    let callers = |c: &[&str]| {
        HandlerConfig::new(
            "rpc",
            RpcHandlerConfig {
                callers: Some(c.iter().map(|s| s.to_string()).collect()),
            },
        )
    };

    refused(
        &tx,
        manifest("rpc-any", vec![callers(&["any"])]),
        parent,
        "rpc",
        "callers",
    )
    .await;
    let child = spawn(
        &tx,
        manifest("rpc-narrow", vec![callers(&["spawner"])]),
        Some(parent),
    )
    .await
    .expect("narrowing to its spawner is allowed");
    // The child's rule is now its spawner only; its children may not name
    // "children" (the spawner's term) because the child's list lacks it.
    refused(
        &tx,
        manifest("g-rpc", vec![callers(&["children"])]),
        child,
        "rpc",
        "callers",
    )
    .await;
    spawn(&tx, manifest("g-rpc-none", vec![callers(&[])]), Some(child))
        .await
        .expect("[] is narrower than anything");
    // A bare block, as manifests enable the handler, keeps the spawner's rule.
    spawn(
        &tx,
        manifest("rpc-bare", vec![HandlerConfig::unit("rpc")]),
        Some(parent),
    )
    .await
    .expect("a block without callers is accepted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawner_that_is_not_running_is_refused() {
    let (_base, root) = dirs();
    let tx = start_runtime(&root);
    let err = spawn(&tx, manifest("orphan", vec![]), Some(TheaterId::generate()))
        .await
        .expect_err("no spawner, no permissions to narrow from");
    assert!(err.to_string().contains("not running"), "{err}");
}
