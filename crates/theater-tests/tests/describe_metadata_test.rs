//! End-to-end gate for `rpc.describe`'s data source: spawn a REAL compiled
//! actor and pull its decoded Pact metadata through the new
//! `TheaterCommand::GetActorMetadata` plumbing, asserting the schema matches
//! what actually survives CGRF embedding + decode (not the source `.pact`).
//!
//! The arena→`Value` serialization is unit-tested in
//! `theater-handler-rpc/src/describe.rs`; this test covers the other half —
//! the ActorInfo::GetMetadata → instance.get_metadata_with_hashes path and the
//! fidelity of the decoded arena for a non-trivial module (records, named-type
//! refs, `list<ref>`, `result<named, string>`, `result<_, string>`).

use std::sync::Arc;
use std::time::Duration;

use theater::config::actor_manifest::{HandlerConfig, ManifestConfig};
use theater::config::inheritance::{HandlerInheritance, HandlerPermissionPolicy};
use theater::handler::HandlerRegistry;
use theater::messages::{default_init_state, TheaterCommand};
use theater::pack_bridge::{Arena, MetadataWithHashes, TypeDef, Value, ValueType};
use theater::utils::ResourceCache;
use theater_handler_lifecycle::LifecycleHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_rpc::RpcHandler;
use theater_handler_runtime::{RuntimeHandler, RuntimeHostConfig};
use theater_handler_self::{SelfHandler, SelfHostConfig};
use theater_handler_store::{StoreHandler, StoreHandlerConfig};
use tokio::sync::{mpsc, oneshot};

const SPAWN_TIMEOUT: Duration = Duration::from_secs(10);

fn full_registry(theater_tx: mpsc::UnboundedSender<TheaterCommand>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.register(SelfHandler::new(
        SelfHostConfig {},
        theater_tx.clone(),
        None,
    ));
    registry.register(LifecycleHandler::new(theater_tx.clone()));
    registry.register(StoreHandler::new(StoreHandlerConfig::default(), None));
    registry.register(RuntimeHandler::new(RuntimeHostConfig {}, None));
    registry.register(MessageServerHandler::new(None, MessageRouter::new()));
    registry.register(RpcHandler::new(theater_tx));
    registry
}

fn manifest(name: &str, wasm_path: &str) -> ManifestConfig {
    ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: wasm_path.to_string(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy {
            runtime: HandlerInheritance::Inherit,
            ..Default::default()
        },
        handlers: vec![HandlerConfig::unit("self")],
    }
}

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
            theater_native::TokioSpawn,
        )
        .await
        .expect("create runtime");
        runtime.run().await
    });
    theater_tx
}

fn wasm_path() -> String {
    format!(
        "{}/../../test-actors/pact-contract-test/target/wasm32-unknown-unknown/release/pact_contract_test_actor.wasm",
        env!("CARGO_MANIFEST_DIR"),
    )
}

/// Fully-qualified function names under a given grouping-arena name
/// (`"exports"` / `"imports"`). The interface identity is the LEAF arena's name
/// (the embedded form leaves `Function.interface` empty), so FQ =
/// `<interface-arena>.<fn>`.
fn fn_names_under(arena: &Arena, group: &str, out: &mut Vec<String>) {
    fn collect(arena: &Arena, out: &mut Vec<String>) {
        for f in &arena.functions {
            out.push(if arena.name.is_empty() {
                f.name.clone()
            } else {
                format!("{}.{}", arena.name, f.name)
            });
        }
        for c in &arena.children {
            collect(c, out);
        }
    }
    if arena.name == group {
        collect(arena, out);
        return;
    }
    for c in &arena.children {
        fn_names_under(c, group, out);
    }
}

/// Every named record/variant/enum/flags/alias defn across the arena tree
/// (incl. function-local defs).
fn type_names(arena: &Arena, out: &mut Vec<String>) {
    let push = |td: &TypeDef, out: &mut Vec<String>| {
        let n = match td {
            TypeDef::Alias { name, .. }
            | TypeDef::Record { name, .. }
            | TypeDef::Variant { name, .. }
            | TypeDef::Enum { name, .. }
            | TypeDef::Flags { name, .. } => name.clone(),
        };
        out.push(n);
    };
    for td in &arena.types {
        push(td, out);
    }
    for f in &arena.functions {
        for td in &f.types {
            push(td, out);
        }
    }
    for c in &arena.children {
        type_names(c, out);
    }
}

async fn get_metadata(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    actor_id: theater::id::TheaterId,
) -> Option<std::sync::Arc<MetadataWithHashes>> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorMetadata {
            actor_id,
            response_tx: tx,
        })
        .expect("send GetActorMetadata");
    tokio::time::timeout(SPAWN_TIMEOUT, rx)
        .await
        .expect("metadata query timed out")
        .expect("metadata response channel closed")
}

#[tokio::test]
async fn describe_metadata_matches_compiled_module() {
    let path = wasm_path();
    let wasm_bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "read pact-contract-test wasm at {}: {}. Build it: \
             cd test-actors/pact-contract-test && cargo build --release --target wasm32-unknown-unknown",
            path, e
        )
    });

    let theater_tx = start_runtime();

    let (spawn_tx, spawn_rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some("pact-contract-test".to_string()),
            manifest: Some(manifest("pact-contract-test", &path)),
            init_state: default_init_state(),
            response_tx: spawn_tx,
            subscription_tx: None,
            parent_id: None,
        })
        .expect("send SpawnActor");
    let actor_id = tokio::time::timeout(SPAWN_TIMEOUT, spawn_rx)
        .await
        .expect("spawn timed out")
        .expect("spawn channel closed")
        .expect("spawn failed");

    // The new plumbing returns the DECODED metadata of the live module.
    let md = get_metadata(&theater_tx, actor_id)
        .await
        .expect("GetActorMetadata returned None for a live actor");

    let mut types = Vec::new();
    type_names(&md.arena, &mut types);
    let mut exports = Vec::new();
    fn_names_under(&md.arena, "exports", &mut exports);
    let mut imports = Vec::new();
    fn_names_under(&md.arena, "imports", &mut imports);

    // `todo-item` IS referenced by exported signatures (add -> result<todo-item>,
    // list -> result<list<todo-item>>), so it survives embedding + decode — and
    // appears once per referencing function (function-local type defs).
    assert!(
        types.iter().any(|n| n == "todo-item"),
        "decoded arena must carry the `todo-item` record; got types {:?}",
        types
    );
    // `actor-state` is declared in the source .pact but is NOT reachable from any
    // EXPORTED signature (get-state returns dynamic `value`, not actor-state), so
    // it is correctly ABSENT from the embedded interface metadata. This is the
    // "test what survives embedding/decoding, not the source AST" point: describe
    // reflects the callable surface, not every type the source happens to define.
    assert!(
        !types.iter().any(|n| n == "actor-state"),
        "actor-state is unreachable from exports, so must NOT be in embedded metadata; got {:?}",
        types
    );

    // The exported callable surface, fully qualified by interface-arena name
    // (what a describe consumer keys on to build an rpc.call).
    for want in [
        "theater:simple/actor.init",
        "theater:simple/actor.get-state",
        "theater:todo/actions.add",
        "theater:todo/actions.toggle",
        "theater:todo/actions.list",
    ] {
        assert!(
            exports.iter().any(|n| n == want),
            "decoded arena must export `{}`; got exports {:?}",
            want,
            exports
        );
    }

    // Imports are classified separately: the actor imports theater:simple/self.log.
    assert!(
        imports.iter().any(|n| n == "theater:simple/self.log"),
        "theater:simple/self.log must be classified as an IMPORT; got imports {:?}",
        imports
    );
    assert!(
        !exports.iter().any(|n| n == "theater:simple/self.log"),
        "an import must not appear under exports; got exports {:?}",
        exports
    );

    // Unknown actor id -> None (the describe host fn maps this to an explicit err).
    let unknown = theater::id::TheaterId::generate();
    assert!(
        get_metadata(&theater_tx, unknown).await.is_none(),
        "unknown actor id must yield None"
    );
}

fn probe_wasm_path() -> String {
    format!(
        "{}/../../test-actors/describe-probe-test/target/wasm32-unknown-unknown/release/describe_probe_test_actor.wasm",
        env!("CARGO_MANIFEST_DIR"),
    )
}

async fn spawn(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    name: &str,
    path: &str,
    handlers: Vec<HandlerConfig>,
) -> theater::id::TheaterId {
    let wasm_bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {} wasm: {}", name, e));
    let m = ManifestConfig {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        package: path.to_string(),
        description: None,
        long_description: None,
        initial_state: None,
        static_package: false,
        permission_policy: HandlerPermissionPolicy {
            runtime: HandlerInheritance::Inherit,
            ..Default::default()
        },
        handlers,
    };
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::SpawnActor {
            wasm_bytes,
            name: Some(name.to_string()),
            manifest: Some(m),
            init_state: default_init_state(),
            response_tx: tx,
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

/// GUEST-LEVEL: a real actor calls `theater:simple/rpc.describe` on several
/// targets from inside an exported call (so it holds its own execution lock).
/// Exercises the host result encoding (ok vs err), the unknown-actor error
/// path, and — crucially — SELF-describe, which must NOT deadlock now that
/// describe is served from the instantiation-time cache. Everything is bounded,
/// and we assert the runtime stays responsive afterward.
#[tokio::test]
async fn guest_describe_success_unknown_and_self_no_deadlock() {
    let probe_path = probe_wasm_path();
    // Ensure the probe wasm exists (build via `nix run .#build-test-actors`).
    std::fs::read(&probe_path).unwrap_or_else(|e| {
        panic!(
            "read describe-probe wasm at {}: {}. Build it: cd test-actors/describe-probe-test \
             && cargo build --release --target wasm32-unknown-unknown",
            probe_path, e
        )
    });
    let target_path = wasm_path();

    let theater_tx = start_runtime();

    // A = the probe (imports self + rpc); B = a describe target.
    let probe_id = spawn(
        &theater_tx,
        "describe-probe",
        &probe_path,
        vec![HandlerConfig::unit("self"), HandlerConfig::unit("rpc")],
    )
    .await;
    let target_id = spawn(
        &theater_tx,
        "pact-contract-test",
        &target_path,
        vec![HandlerConfig::unit("self")],
    )
    .await;
    let unknown_id = theater::id::TheaterId::generate();

    // Get a handle to the probe and drive its `probe` export with
    // [self, target, unknown] — the self entry is the deadlock case.
    let (htx, hrx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActorHandle {
            actor_id: probe_id,
            response_tx: htx,
        })
        .expect("send GetActorHandle");
    let handle = hrx
        .await
        .expect("handle channel closed")
        .expect("probe actor handle");

    let targets = Value::List {
        elem_type: ValueType::String,
        items: vec![
            Value::String(probe_id.to_string()), // SELF — must not deadlock
            Value::String(target_id.to_string()),
            Value::String(unknown_id.to_string()),
        ],
    };

    // BOUNDED: if describe deadlocked on self (the pre-fix bug), this would hang
    // ~50 min (DEFAULT_OPERATION_TIMEOUT); the tight bound turns that into a fast
    // failure instead.
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        handle.call_function("theater:test/describe-probe.probe".to_string(), targets),
    )
    .await
    .expect("probe did not return within 10s — likely a self-describe deadlock")
    .expect("probe call failed");

    // probe returns list<bool>: [self ok, target ok, unknown err].
    let flags: Vec<bool> = match result {
        Value::List { items, .. } => items
            .into_iter()
            .map(|v| matches!(v, Value::Bool(true)))
            .collect(),
        other => panic!("probe must return a list<bool>, got {:?}", other),
    };
    assert_eq!(flags.len(), 3, "one flag per target");
    assert!(flags[0], "SELF-describe must succeed (no deadlock)");
    assert!(flags[1], "describing a live target must succeed");
    assert!(!flags[2], "describing an unknown actor must be an error");

    // The runtime stayed responsive: an unrelated command answers promptly.
    let still_responsive =
        tokio::time::timeout(Duration::from_secs(5), get_metadata(&theater_tx, target_id))
            .await
            .expect("runtime unresponsive after self-describe");
    assert!(
        still_responsive.is_some(),
        "unrelated GetActorMetadata must still resolve"
    );
}
