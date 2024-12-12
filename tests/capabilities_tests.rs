use anyhow::Result;
use std::path::PathBuf;
use theater::capabilities::{ActorCapability, BaseActorCapability};
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

fn get_test_component_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("examples/actors/simple-actor-sample/target/wasm32-unknown-unknown/release");
    path.push("simple_actor_sample.wasm");
    println!("Test component path: {:?}", path);
    path
}

#[test]
fn test_base_actor_capability() -> Result<()> {
    let engine = Engine::default();
    let mut linker = Linker::<theater::Store>::new(&engine);
    let _store = Store::new(&engine, theater::Store::new());

    let capability = BaseActorCapability;
    capability.setup_host_functions(&mut linker)?;

    let component_path = get_test_component_path();
    assert!(
        component_path.exists(),
        "Test component not found at {:?}. Did you build it?",
        component_path
    );

    // Test that required exports are present
    let component = Component::from_file(&engine, &component_path)?;
    let exports = capability.get_exports(&component)?;

    assert_eq!(exports.len(), 4);
    assert!(exports.iter().any(|(name, _)| name == "init"));
    assert!(exports.iter().any(|(name, _)| name == "handle"));
    assert!(exports.iter().any(|(name, _)| name == "state-contract"));
    assert!(exports.iter().any(|(name, _)| name == "message-contract"));

    Ok(())
}
