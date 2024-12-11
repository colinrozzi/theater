use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use wasmtime::component::{Component, ComponentExportIndex, Instance, Linker};
use wasmtime::Engine;

use crate::capabilities::{ActorCapability, BaseActorCapability, HttpCapability};
use crate::config::ManifestConfig;
use crate::{Actor, ActorInput, ActorOutput, Store};

#[derive(Error, Debug)]
pub enum WasmError {
    #[error("Failed to load manifest: {0}")]
    ManifestError(String),

    #[error("WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },
}

/// Implementation of the Actor trait for WebAssembly components
pub struct WasmActor {
    engine: Engine,
    component: Component,
    linker: Linker<Store>,
    capabilities: Vec<Box<dyn ActorCapability>>,
    exports: HashMap<String, ComponentExportIndex>,
    store: Store,
}

impl WasmActor {
    pub fn new<P: AsRef<Path>>(manifest_path: P, store: Store) -> Result<Self> {
        // Load and parse manifest
        let config = ManifestConfig::from_file(manifest_path)?;

        // Load WASM component
        let engine = Engine::default();
        let wasm_bytes = std::fs::read(&config.component_path)?;
        let component = Component::new(&engine, &wasm_bytes)?;
        let linker = Linker::new(&engine);

        let mut actor = WasmActor {
            engine,
            component,
            linker,
            capabilities: Vec::new(),
            exports: HashMap::new(),
            store,
        };

        if config.interface() == "ntwk:simple-actor/actor" {
            actor.add_capability(Box::new(BaseActorCapability))?;
        }

        if config.implements_interface("ntwk:simple-http-actor/http-actor") {
            actor.add_capability(Box::new(HttpCapability))?;
        }

        Ok(actor)
    }

    fn add_capability(&mut self, capability: Box<dyn ActorCapability>) -> Result<()> {
        // Setup host functions
        capability.setup_host_functions(&mut self.linker)?;

        // Get and store exports
        let exports = capability.get_exports(&self.component)?;
        for (name, index) in exports {
            self.exports.insert(name, index);
        }

        self.capabilities.push(capability);
        Ok(())
    }

    fn get_export(&self, name: &str) -> Option<&ComponentExportIndex> {
        self.exports.get(name)
    }

    fn call_func<T, U>(
        &self,
        store: &mut wasmtime::Store<Store>,
        instance: &Instance,
        export_name: &str,
        args: T,
    ) -> Result<U>
    where
        T: wasmtime::component::Lower + wasmtime::component::ComponentNamedList,
        U: wasmtime::component::Lift + wasmtime::component::ComponentNamedList,
    {
        let index = self
            .get_export(export_name)
            .ok_or_else(|| WasmError::WasmError {
                context: "function lookup",
                message: format!("Function {} not found", export_name),
            })?;

        let func = instance
            .get_func(&mut *store, *index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function access",
                message: format!("Failed to get function {}", export_name),
            })?;

        let typed = func
            .typed::<T, U>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "function type",
                message: e.to_string(),
            })?;

        Ok(typed
            .call(&mut *store, args)
            .map_err(|e| WasmError::WasmError {
                context: "function call",
                message: e.to_string(),
            })?)
    }
}

impl Actor for WasmActor {
    fn init(&self) -> Result<Value> {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = self.linker.instantiate(&mut store, &self.component)?;

        let (result,) = self.call_func::<(), (Vec<u8>,)>(&mut store, &instance, "init", ())?;
        let state: Value = serde_json::from_slice(&result)?;

        Ok(state)
    }

    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)> {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = self.linker.instantiate(&mut store, &self.component)?;

        let state_bytes = serde_json::to_vec(state)?;

        match input {
            ActorInput::Message(msg) => {
                let msg_bytes = serde_json::to_vec(&msg)?;
                let (result,) = self.call_func::<(Vec<u8>, Vec<u8>), (Vec<u8>,)>(
                    &mut store,
                    &instance,
                    "handle",
                    (msg_bytes, state_bytes),
                )?;
                let new_state: Value = serde_json::from_slice(&result)?;
                Ok((ActorOutput::Message(msg), new_state))
            }
            ActorInput::HttpRequest {
                method,
                uri,
                headers,
                body,
            } => {
                if !self.exports.contains_key("handle-http") {
                    return Err(anyhow::anyhow!("Actor does not support HTTP"));
                }

                let request = serde_json::json!({
                    "method": method,
                    "uri": uri,
                    "headers": { "fields": headers },
                    "body": body,
                });

                let request_bytes = serde_json::to_vec(&request)?;
                let (result,) = self.call_func::<(Vec<u8>, Vec<u8>), (Vec<u8>,)>(
                    &mut store,
                    &instance,
                    "handle-http",
                    (request_bytes, state_bytes),
                )?;

                let response: Value = serde_json::from_slice(&result)?;
                let new_state = response["state"].clone();
                let http_response = response["response"].clone();

                let status = http_response["status"].as_u64().unwrap_or(500) as u16;
                let headers = http_response["headers"]["fields"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| {
                                let pair = v.as_array()?;
                                Some((pair[0].as_str()?.to_string(), pair[1].as_str()?.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let body = http_response["body"]
                    .as_array()
                    .map(|arr| arr.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect());

                Ok((
                    ActorOutput::HttpResponse {
                        status,
                        headers,
                        body,
                    },
                    new_state,
                ))
            }
        }
    }

    fn verify_state(&self, state: &Value) -> bool {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = match self.linker.instantiate(&mut store, &self.component) {
            Ok(instance) => instance,
            Err(_) => return false,
        };

        let state_bytes = match serde_json::to_vec(state) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        self.call_func::<(Vec<u8>,), (bool,)>(
            &mut store,
            &instance,
            "state-contract",
            (state_bytes,),
        )
        .map(|(result,)| result)
        .unwrap_or(false)
    }
}
