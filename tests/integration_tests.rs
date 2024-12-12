use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;
use theater::capabilities::{ActorCapability, BaseActorCapability};
use theater::{ActorInput, Store, WasmActor};
use wasmtime::{Engine, Store as WasmStore};
use wasmtime::component::{Component, Linker};

fn get_test_wasm_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("examples/actors/simple-actor-sample/target/wasm32-unknown-unknown/release");
    path.push("simple_actor_sample.wasm");
    path
}

fn get_test_manifest_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("reference/simple-actor.toml");
    path
}

#[test]
fn test_simple_actor_component() -> Result<()> {
    let wasm_path = get_test_wasm_path();
    assert!(wasm_path.exists(), "WASM component not found at {:?}. Did you build it?", wasm_path);

    let engine = Engine::default();
    let mut linker = Linker::<Store>::new(&engine);
    let _store = WasmStore::new(&engine, Store::new());
    
    let capability = BaseActorCapability;
    capability.setup_host_functions(&mut linker)?;
    
    // Load the actual WASM component
    let component = Component::from_file(&engine, &wasm_path)?;
    
    // Test that required exports are present
    let exports = capability.get_exports(&component)?;
    
    assert_eq!(exports.len(), 4);
    assert!(exports.iter().any(|(name, _)| name == "init"));
    assert!(exports.iter().any(|(name, _)| name == "handle"));
    assert!(exports.iter().any(|(name, _)| name == "state-contract"));
    assert!(exports.iter().any(|(name, _)| name == "message-contract"));
    
    Ok(())
}

#[tokio::test]
async fn test_wasm_actor_lifecycle() -> Result<()> {
    let manifest_path = get_test_manifest_path();
    assert!(manifest_path.exists(), "Manifest not found at {:?}", manifest_path);

    // Create actor with empty store
    let store = Store::new();
    let actor = WasmActor::new(manifest_path, store)?;

    // Test initialization
    let initial_state = actor.init()?;
    assert!(actor.verify_state(&initial_state));

    // Test message handling
    let test_message = json!({"action": "increment"});
    let (output, new_state) = actor.handle_input(
        ActorInput::Message(test_message.clone()),
        &initial_state
    )?;

    // Verify new state
    assert!(actor.verify_state(&new_state));

    Ok(())
}

#[tokio::test]
async fn test_wasm_actor_http_handling() -> Result<()> {
    let manifest_path = get_test_manifest_path();
    let store = Store::new();
    let actor = WasmActor::new(manifest_path, store)?;

    let initial_state = actor.init()?;

    // Test HTTP request handling
    let (output, new_state) = actor.handle_input(
        ActorInput::HttpRequest {
            method: "GET".to_string(),
            uri: "/".to_string(),
            headers: vec![],
            body: None,
        },
        &initial_state
    )?;

    assert!(actor.verify_state(&new_state));

    Ok(())
}
