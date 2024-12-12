use anyhow::Result;
use std::path::PathBuf;
use theater::capabilities::{ActorCapability, BaseActorCapability}; 
use wasmtime::{Engine, Store};
use wasmtime::component::{Component, Linker};

fn get_test_component_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("examples/actors/simple-actor-sample/target/wasm32-unknown-unknown/release");
    path.push("simple_actor_sample.wasm");
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
    assert!(component_path.exists(), "Test component not found at {:?}. Did you build it?", component_path);
    
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

#[test]
fn test_log_host_function() -> Result<()> {
    let engine = Engine::default();
    let mut linker = Linker::<theater::Store>::new(&engine);
    let mut store = Store::new(&engine, theater::Store::new());
    
    let capability = BaseActorCapability;
    capability.setup_host_functions(&mut linker)?;
    
    let component_path = get_test_component_path();
    assert!(component_path.exists(), "Test component not found at {:?}. Did you build it?", component_path);
    
    // Load the pre-built component that uses log
    let component = Component::from_file(&engine, &component_path)?;
    
    let instance = linker.instantiate(&mut store, &component)?;
    
    // Get and call the exported function
    let log_test = instance.get_func(&mut store, "log_test").unwrap();
    log_test.call(&mut store, &[], &mut [])?;
    
    Ok(())
}

#[test]
fn test_send_host_function() -> Result<()> {
    let engine = Engine::default();
    let mut linker = Linker::<theater::Store>::new(&engine);
    let store = theater::Store::new();
    let mut wasm_store = Store::new(&engine, store);
    
    let capability = BaseActorCapability;
    capability.setup_host_functions(&mut linker)?;
    
    let component_path = get_test_component_path();
    assert!(component_path.exists(), "Test component not found at {:?}. Did you build it?", component_path);
    
    // Load the pre-built component that uses send
    let component = Component::from_file(&engine, &component_path)?;
    
    let instance = linker.instantiate(&mut wasm_store, &component)?;
    
    // Get and call the exported function
    let send_test = instance.get_func(&mut wasm_store, "send_test").unwrap();
    send_test.call(&mut wasm_store, &[], &mut [])?;
    
    Ok(())
}
