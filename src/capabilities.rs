use anyhow::Result;
use wasmtime::component::{Component, ComponentExportIndex, Linker};

/// Represents a set of capabilities that a WASM component can implement
pub trait ActorCapability: Send {
    /// Set up host functions in the linker
    fn setup_host_functions(&self, linker: &mut Linker<()>) -> Result<()>;
    
    /// Get required export indices from component
    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>>;
    
    /// Return interface name this capability implements
    fn interface_name(&self) -> &str;
}

/// The base actor capability that all actors must implement
pub struct BaseActorCapability;

impl ActorCapability for BaseActorCapability {
    fn setup_host_functions(&self, linker: &mut Linker<()>) -> Result<()> {
        let mut runtime = linker.instance("ntwk:simple-actor/runtime")?;
        
        // Add log function
        runtime.func_wrap(
            "log",
            |_: wasmtime::StoreContextMut<'_, ()>, (msg,): (String,)| {
                println!("[WASM] {}", msg);
                Ok(())
            },
        )?;

        // Add send function
        runtime.func_wrap(
            "send",
            |_: wasmtime::StoreContextMut<'_, ()>, (actor_id, msg): (String, Vec<u8>)| {
                println!("Message send requested to {}", actor_id);
                // TODO: Implement actual message sending
                Ok(())
            },
        )?;

        Ok(())
    }

    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>> {
        let (_, instance) = component.export_index(None, "ntwk:simple-actor/actor")?;
        
        let mut exports = Vec::new();
        
        // Get required function exports
        let (_, init) = component.export_index(Some(&instance), "init")?;
        exports.push(("init".to_string(), init));

        let (_, handle) = component.export_index(Some(&instance), "handle")?;
        exports.push(("handle".to_string(), handle));

        let (_, state_contract) = component.export_index(Some(&instance), "state-contract")?;
        exports.push(("state-contract".to_string(), state_contract));

        let (_, message_contract) = component.export_index(Some(&instance), "message-contract")?;
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
    fn setup_host_functions(&self, linker: &mut Linker<()>) -> Result<()> {
        // Currently no host functions needed for HTTP
        Ok(())
    }

    fn get_exports(&self, component: &Component) -> Result<Vec<(String, ComponentExportIndex)>> {
        let (_, instance) = component.export_index(None, "ntwk:simple-http-actor/http-actor")?;
        
        let mut exports = Vec::new();
        
        let (_, http_contract) = component.export_index(Some(&instance), "http-contract")?;
        exports.push(("http-contract".to_string(), http_contract));

        let (_, handle_http) = component.export_index(Some(&instance), "handle-http")?;
        exports.push(("handle-http".to_string(), handle_http));

        Ok(exports)
    }

    fn interface_name(&self) -> &str {
        "ntwk:simple-http-actor/http-actor"
    }
}