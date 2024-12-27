use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use wasmtime::component::{Component, ComponentExportIndex, Instance, Linker};
use wasmtime::Engine;

use crate::capabilities::{BaseCapability, Capability, HttpCapability};
use crate::config::ManifestConfig;
use crate::Store;
use tracing::{error, info};

// Move Event and State from actor.rs
pub type State = Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    pub type_: String,
    pub data: Value,
}

impl Event {
    pub fn noop() -> Self {
        Event {
            type_: "noop".to_string(),
            data: Value::Null,
        }
    }
}

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

/// WebAssembly actor implementation
pub struct WasmActor {
    engine: Engine,
    component: Component,
    linker: Linker<Store>,
    capabilities: Vec<Capability>,
    exports: HashMap<String, ComponentExportIndex>,
    store: Store,
}

impl WasmActor {
    pub async fn new(config: &ManifestConfig, store: Store) -> Result<Self> {
        // Load WASM component
        let engine = Engine::new(wasmtime::Config::new().async_support(true))?;
        let wasm_bytes =
            std::fs::read(&config.component_path).map_err(|e| WasmError::WasmError {
                context: "component loading",
                message: format!(
                    "Failed to load WASM component from {}: {}",
                    config.component_path.display(),
                    e
                ),
            })?;
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
            actor
                .add_capability(Capability::Base(BaseCapability))
                .await?;
        }

        if config.implements_interface("ntwk:simple-http-actor/actor") {
            actor
                .add_capability(Capability::Http(HttpCapability))
                .await?;
        }

        Ok(actor)
    }

    async fn add_capability(&mut self, capability: Capability) -> Result<()> {
        // Setup host functions
        capability.setup_host_functions(&mut self.linker).await?;

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

    async fn call_func<T, U>(
        &self,
        store: &mut wasmtime::Store<Store>,
        instance: &Instance,
        export_name: &str,
        args: T,
    ) -> Result<U>
    where
        T: wasmtime::component::Lower + wasmtime::component::ComponentNamedList + Send + Sync,
        U: wasmtime::component::Lift + wasmtime::component::ComponentNamedList + Send + Sync,
    {
        info!("Calling function: {}", export_name);
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

        info!("params type: {:?}", func.params(&mut *store));
        info!("results type: {:?}", func.results(&mut *store));

        let typed = func
            .typed::<T, U>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "function type",
                message: format!("typed call failed: {}", e),
            })?;

        Ok(typed
            .call_async(&mut *store, args)
            .await
            .map_err(|e| WasmError::WasmError {
                context: "function call",
                message: e.to_string(),
            })?)
    }

    pub async fn init(&self) -> Result<Value> {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await?;

        let (result,) = self
            .call_func::<(), (Vec<u8>,)>(&mut store, &instance, "init", ())
            .await?;
        let state: Value = serde_json::from_slice(&result)?;

        Ok(state)
    }

    pub async fn handle_event(&self, state: Value, event: Event) -> Result<(State, Event)> {
        info!("Handling event: {:?}", event);
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await?;

        let event_bytes = serde_json::to_vec(&event).map_err(|e| WasmError::WasmError {
            context: "event serialization",
            message: format!("Failed to serialize event: {}", e),
        })?;
        let state_bytes = serde_json::to_vec(&state).map_err(|e| WasmError::WasmError {
            context: "state serialization",
            message: format!("Failed to serialize state: {}", e),
        })?;

        let result = self
            .call_func::<(Vec<u8>, Vec<u8>), ((Vec<u8>, Option<Vec<u8>>),)>(
                &mut store,
                &instance,
                "handle",
                (event_bytes, state_bytes),
            )
            .await?;

        let (after_state, response) = result.0;

        let new_state: Value =
            serde_json::from_slice(&after_state).map_err(|e| WasmError::WasmError {
                context: "state deserialization",
                message: format!("Failed to deserialize state: {}", e),
            })?;
        match response {
            Some(response_bytes) => {
                let response: Event =
                    serde_json::from_slice(&response_bytes).map_err(|e| WasmError::WasmError {
                        context: "response deserialization",
                        message: format!("Failed to deserialize response: {}", e),
                    })?;
                Ok((new_state, response))
            }
            None => Ok((new_state, Event::noop())),
        }
    }

    pub async fn verify_state(&self, state: &Value) -> bool {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = match self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await
        {
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
        .await
        .map(|(result,)| result)
        .unwrap_or(false)
    }
}
