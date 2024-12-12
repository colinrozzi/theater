use anyhow::Result;
use theater::capabilities::{ActorCapability, BaseActorCapability}; 
use wasmtime::{Engine, Store};
use wasmtime::component::{Component, Linker};

#[test]
fn test_base_actor_capability() -> Result<()> {
    let engine = Engine::default();
    let mut linker = Linker::<theater::Store>::new(&engine);
    let _store = Store::new(&engine, theater::Store::new());
    
    let capability = BaseActorCapability;
    capability.setup_host_functions(&mut linker)?;
    
    // Test that required exports are present
    let exports = capability.get_exports(&Component::new(&engine, r#"
        (component
            (core module $m
                (func $init (export "init"))
                (func $handle (export "handle"))
                (func $state_contract (export "state-contract"))
                (func $message_contract (export "message-contract"))
            )
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "handle") (canon lift (core func $i "handle")))
            (func (export "state-contract") (canon lift (core func $i "state-contract")))
            (func (export "message-contract") (canon lift (core func $i "message-contract")))
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
    let component = Component::new(&engine, r#"
        (component
            (import "ntwk:simple-actor/runtime" (instance $runtime
                (export "log" (func (param "msg" string)))
            ))
            (core module $m
                (import "" "log" (func $log (param i32 i32)))
                (func $log_test (export "log_test")
                    (call $log (i32.const 0) (i32.const 0))
                )
            )
            (core instance $i (instantiate $m
                (with "" (instance 
                    (export "log" (func $runtime "log"))
                ))
            ))
            (func (export "log_test") (canon lift (core func $i "log_test")))
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
    let component = Component::new(&engine, r#"
        (component
            (import "ntwk:simple-actor/runtime" (instance $runtime
                (export "send" (func (param "address" string) (param "msg" list u8)))
            ))
            (core module $m
                (import "" "send" (func $send (param i32 i32) (param i32 i32)))
                (func $send_test (export "send_test")
                    (call $send
                        (i32.const 0) (i32.const 0)  ;; actor-id
                        (i32.const 0) (i32.const 0)  ;; msg
                    )
                )
            )
            (core instance $i (instantiate $m
                (with "" (instance
                    (export "send" (func $runtime "send"))
                ))
            ))
            (func (export "send_test") (canon lift (core func $i "send_test")))
        )
    "#.as_bytes())?;
    
    let instance = linker.instantiate(&mut wasm_store, &component)?;
    
    // Get and call the exported function
    let send_test = instance.get_func(&mut wasm_store, "send_test").unwrap();
    send_test.call(&mut wasm_store, &[], &mut [])?;
    
    Ok(())
}
