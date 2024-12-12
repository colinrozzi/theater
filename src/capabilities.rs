use anyhow::Result;
use serde_json::Value;
use wasmtime::component::{Component, ComponentExportIndex, Linker};

use crate::store::Store;

/// Represents a set of capabilities that a WASM component can implement
pub trait ActorCapability: Send {
    /// Set up host functions in the linker
    fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()>;

    /// Get required export indices from component
    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>>;

    /// Return interface name this capability implements
    fn interface_name(&self) -> &str;
    
    #[cfg(test)]
    fn create_test_component(engine: &wasmtime::Engine) -> Result<Component> 
    where Self: Sized {
        Component::new(engine, r#"
            (component
                (import "ntwk:simple-actor/runtime" (instance $runtime
                    (export "log" (func (param "msg" string)))
                    (export "send" (func (param "address" string) (param "msg" list u8)))
                ))
                
                (core module $m
                    (import "" "log" (func $host_log (param i32 i32)))
                    (import "" "send" (func $host_send (param i32 i32 i32 i32)))
                    (func $init (export "init"))
                    (func $handle (export "handle"))
                    (func $state_contract (export "state-contract"))
                    (func $message_contract (export "message-contract"))
                )
                
                (core instance $i (instantiate $m
                    (with "" (instance
                        (export "log" (func $runtime "log"))
                        (export "send" (func $runtime "send"))
                    ))
                ))
                
                (func (export "init") (canon lift (core func $i "init")))
                (func (export "handle") (canon lift (core func $i "handle")))
                (func (export "state-contract") (canon lift (core func $i "state-contract")))
                (func (export "message-contract") (canon lift (core func $i "message-contract")))
            )
        "#.as_bytes())
    }
}

/// The base actor capability that all actors must implement
pub struct BaseActorCapability;

impl ActorCapability for BaseActorCapability {
    fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()> {
        let mut runtime = linker.instance("ntwk:simple-actor/runtime")?;

        // Add log function
        runtime.func_wrap(
            "log",
            |_: wasmtime::StoreContextMut<'_, Store>, (msg,): (String,)| {
                println!("[WASM] {}", msg);
                Ok(())
            },
        )?;

        // Add send function
        runtime.func_wrap(
            "send",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>, (address, msg): (String, Vec<u8>)| {
                // Convert message bytes to JSON Value
                let msg_value: Value = serde_json::from_slice(&msg).map_err(|e| {
                    println!("Failed to parse message as JSON: {}", e);
                    wasmtime::Error::msg("Invalid message format")
                })?;

                // Get store reference
                let store = ctx.data_mut();

                // If we have HTTP host, send the message
                if let Some(http) = &store.http {
                    // Spawn task to send message since we can't await in this context
                    let http = http.clone();
                    let address = address.clone();
                    tokio::spawn(async move {
                        if let Err(e) = http.send_message(address, msg_value).await {
                            eprintln!("Failed to send message: {}", e);
                        }
                    });
                } else {
                    println!("Warning: No HTTP host available for sending messages");
                }

                Ok(())
            },
        )?;

        Ok(())
    }

    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>> {
        let (_, instance) = component
            .export_index(None, "ntwk:simple-actor/actor")
            .expect("Failed to get actor instance");

        let mut exports = Vec::new();

        println!("Getting init export");

        // Get required function exports
        let (_, init) = component
            .export_index(Some(&instance), "init")
            .expect("Failed to get init export");
        exports.push(("init".to_string(), init));

        println!("Getting handle export");

        let (_, handle) = component
            .export_index(Some(&instance), "handle")
            .expect("Failed to get handle export");
        exports.push(("handle".to_string(), handle));

        println!("Getting verify export");

        let (_, state_contract) = component
            .export_index(Some(&instance), "state-contract")
            .expect("Failed to get state contract export");
        exports.push(("state-contract".to_string(), state_contract));

        println!("Getting message contract export");

        let (_, message_contract) = component
            .export_index(Some(&instance), "message-contract")
            .expect("Failed to get message contract export");
        exports.push(("message-contract".to_string(), message_contract));

        Ok(exports)
    }

    fn interface_name(&self) -> &str {
        "ntwk:simple-actor/actor"
    }
}

/// HTTP actor capability
pub struct HttpCapability;

impl ActorCapability for HttpCapability {
    fn setup_host_functions(&self, linker: &mut Linker<Store>) -> Result<()> {
        let mut runtime = linker.instance("ntwk:simple-http-actor/http-runtime")?;

        // Add log function
        runtime.func_wrap(
            "log",
            |_: wasmtime::StoreContextMut<'_, Store>, (msg,): (String,)| {
                println!("[WASM] {}", msg);
                Ok(())
            },
        )?;

        // Add send function - reuse same implementation as BaseActorCapability
        runtime.func_wrap(
            "send",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>, (address, msg): (String, Vec<u8>)| {
                let msg_value: Value = serde_json::from_slice(&msg).map_err(|e| {
                    println!("Failed to parse message as JSON: {}", e);
                    wasmtime::Error::msg("Invalid message format")
                })?;

                let store = ctx.data_mut();

                if let Some(http) = &store.http {
                    let http = http.clone();
                    let address = address.clone();
                    tokio::spawn(async move {
                        if let Err(e) = http.send_message(address, msg_value).await {
                            eprintln!("Failed to send message: {}", e);
                        }
                    });
                } else {
                    println!("Warning: No HTTP host available for sending messages");
                }

                Ok(())
            },
        )?;

        Ok(())
    }

    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>> {
        let (_, instance) = component
            .export_index(None, "ntwk:simple-http-actor/http-actor")
            .expect("Failed to get HTTP actor instance");

        let mut exports = Vec::new();

        println!("Getting init export");

        // Get required function exports
        let (_, init) = component
            .export_index(Some(&instance), "init")
            .expect("Failed to get init export");
        exports.push(("init".to_string(), init));

        println!("Getting handle export");

        let (_, handle) = component
            .export_index(Some(&instance), "handle")
            .expect("Failed to get handle export");
        exports.push(("handle".to_string(), handle));

        println!("Getting verify export");

        let (_, state_contract) = component
            .export_index(Some(&instance), "state-contract")
            .expect("Failed to get state contract export");
        exports.push(("state-contract".to_string(), state_contract));

        println!("Getting message contract export");

        let (_, message_contract) = component
            .export_index(Some(&instance), "message-contract")
            .expect("Failed to get message contract export");
        exports.push(("message-contract".to_string(), message_contract));

        println!("Getting HTTP contract export");

        let (_, http_contract) = component
            .export_index(Some(&instance), "http-contract")
            .expect("Failed to get HTTP contract export");
        exports.push(("http-contract".to_string(), http_contract));

        println!("Getting HTTP handler export");

        let (_, handle_http) = component
            .export_index(Some(&instance), "handle-http")
            .expect("Failed to get HTTP handler export");
        exports.push(("handle-http".to_string(), handle_http));

        Ok(exports)
    }

    fn interface_name(&self) -> &str {
        "ntwk:simple-http-actor/http-actor"
    }
}
