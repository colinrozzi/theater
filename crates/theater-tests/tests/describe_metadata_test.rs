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
use theater::pack_bridge::{Arena, MetadataWithHashes, TypeDef};
use theater::utils::ResourceCache;
use theater_handler_lifecycle::LifecycleHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
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
) -> Option<MetadataWithHashes> {
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
