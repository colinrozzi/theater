use anyhow::Result;
use theater::capabilities::{ActorCapability, BaseActorCapability};
use wasmtime::{component::Linker, Engine, Store};
use wasmtime::component::Component;

#[test]
fn test_base_actor_capability() -> Result<()> {
    let engine = Engine::default();
    let mut linker = Linker::<theater::Store>::new(&engine);
    let mut store = Store::new(&engine, theater::Store::new());
    
    let capability = BaseActorCapability;
    capability.setup_host_functions(&mut linker)?;
    
    // Test that required exports are present
    let exports = capability.get_exports(&wasmtime::Component::new(&engine, r#"
        (component
            (export "init" (func))
            (export "handle" (func))
            (export "state-contract" (func))
            (export "message-contract" (func))
        )
    "#.as_bytes())?)?;
    
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
    
    // Create a simple component that calls log
    let component = wasmtime::Component::new(&engine, r#"
        (component
            (import "ntwk:simple-actor/runtime" (func "log" (param "msg" string)))
            (core func $log_test (call-import "log" "Test message"))
            (export "log_test" (func $log_test))
        )
    "#.as_bytes())?;
    
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
    
    // Create a component that calls send
    let component = wasmtime::Component::new(&engine, r#"
        (component
            (import "ntwk:simple-actor/runtime" (func "send" (param "actor-id" string) (param "msg" (list u8))))
            (core func $send_test
                (call-import "send" "test-actor" (list u8 1 2 3)))
            (export "send_test" (func $send_test))
        )
    "#.as_bytes())?;
    
    let instance = linker.instantiate(&mut wasm_store, &component)?;
    
    // Get and call the exported function
    let send_test = instance.get_func(&mut wasm_store, "send_test").unwrap();
    send_test.call(&mut wasm_store, &[], &mut [])?;
    
    Ok(())
}
