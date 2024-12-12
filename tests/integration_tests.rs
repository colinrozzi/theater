use anyhow::Result;
use std::path::PathBuf;
use theater::capabilities::{ActorCapability, BaseActorCapability};
use wasmtime::{Engine, Store};
use wasmtime::component::{Component, Linker};

fn get_test_wasm_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("examples/actors/simple-actor-sample/target/wasm32-unknown-unknown/release");
    path.push("simple-actor-sample.wasm");
    path
}

#[test]
fn test_simple_actor_component() -> Result<()> {
    let wasm_path = get_test_wasm_path();
    assert!(wasm_path.exists(), "WASM component not found at {:?}. Did you build it?", wasm_path);

    let engine = Engine::default();
    let mut linker = Linker::<theater::Store>::new(&engine);
    let store = Store::new(&engine, theater::Store::new());
    
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
