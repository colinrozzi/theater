use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use thiserror::Error;
use wasmtime::component::ComponentExportIndex;
use wasmtime::component::{Component, Instance, Linker};
use wasmtime::{Engine, Store};

use crate::{Actor, ActorInput, ActorOutput};

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
    linker: Linker<()>,
    init_index: ComponentExportIndex,
    handle_index: ComponentExportIndex,
    state_contract_index: ComponentExportIndex,
    message_contract_index: ComponentExportIndex,
    // Optional indices for HTTP support
    http_contract_index: Option<ComponentExportIndex>,
    handle_http_index: Option<ComponentExportIndex>,
}

impl WasmActor {
    pub fn from_file<P: AsRef<Path>>(manifest_path: P) -> Result<Self> {
        let engine = Engine::default();

        // Read and parse manifest
        let manifest = std::fs::read_to_string(&manifest_path)
            .map_err(|e| WasmError::ManifestError(e.to_string()))?;
        let manifest: toml::Value =
            toml::from_str(&manifest).map_err(|e| WasmError::ManifestError(e.to_string()))?;

        // Get WASM file path
        let wasm_path = manifest["component_path"]
            .as_str()
            .ok_or_else(|| WasmError::ManifestError("Missing component_path".into()))?;

        // Read interfaces
        let implements = manifest["interfaces"]["implements"]
            .as_array()
            .ok_or_else(|| WasmError::ManifestError("Missing interfaces.implements".into()))?;

        // Check for required interfaces
        let has_actor = implements.iter().any(|i| {
            i.as_str()
                .map(|s| s == "ntwk:simple-actor/actor")
                .unwrap_or(false)
        });
        if !has_actor {
            return Err(WasmError::ManifestError(
                "Component must implement ntwk:simple-actor/actor".into(),
            )
            .into());
        }

        // Check for HTTP interface
        let has_http = implements.iter().any(|i| {
            i.as_str()
                .map(|s| s == "ntwk:simple-http-actor/http-actor")
                .unwrap_or(false)
        });

        // Load and instantiate component
        let wasm_bytes = std::fs::read(wasm_path)
            .map_err(|e| WasmError::ManifestError(format!("Failed to read WASM file: {}", e)))?;
        let component = Component::new(&engine, &wasm_bytes).map_err(|e| WasmError::WasmError {
            context: "component creation",
            message: e.to_string(),
        })?;

        // Set up linker with runtime functions
        let mut linker = Linker::new(&engine);
        let mut runtime =
            linker
                .instance("ntwk:simple-actor/runtime")
                .map_err(|e| WasmError::WasmError {
                    context: "runtime setup",
                    message: e.to_string(),
                })?;

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

        // Get export indices for required functions
        let (_, actor_instance) = component
            .export_index(None, "ntwk:simple-actor/actor")
            .expect("Failed to get actor instance");

        let (_, init_index) = component
            .export_index(Some(&actor_instance), "init")
            .expect("Failed to get init index");

        let (_, handle_index) = component
            .export_index(Some(&actor_instance), "handle")
            .expect("Failed to get handle index");

        let (_, state_contract_index) = component
            .export_index(Some(&actor_instance), "state-contract")
            .expect("Failed to get state-contract index");

        let (_, message_contract_index) = component
            .export_index(Some(&actor_instance), "message-contract")
            .expect("Failed to get message-contract index");

        // Get HTTP-specific exports if available
        let (http_contract_index, handle_http_index) = if has_http {
            let (_, http_instance) = component
                .export_index(None, "ntwk:simple-http-actor/http-actor")
                .expect("Failed to get http-actor instance");

            let (_, http_contract) = component
                .export_index(Some(&http_instance), "http-contract")
                .expect("Failed to get http-contract index");

            let (_, handle_http) = component
                .export_index(Some(&http_instance), "handle-http")
                .expect("Failed to get handle-http index");

            (Some(http_contract), Some(handle_http))
        } else {
            (None, None)
        };

        Ok(WasmActor {
            engine,
            component,
            linker,
            init_index,
            handle_index,
            state_contract_index,
            message_contract_index,
            http_contract_index,
            handle_http_index,
        })
    }

    fn call_init(&self, store: &mut Store<()>, instance: &Instance) -> Result<Vec<u8>> {
        let init_func = instance
            .get_func(&mut *store, self.init_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "init function",
                message: "Function not found".into(),
            })?;

        let typed = init_func
            .typed::<(), (Vec<u8>,)>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "init function type",
                message: e.to_string(),
            })?;

        let (result,) = typed
            .call(&mut *store, ())
            .map_err(|e| WasmError::WasmError {
                context: "init function call",
                message: e.to_string(),
            })?;

        Ok(result)
    }

    fn call_handle(
        &self,
        store: &mut Store<()>,
        instance: &Instance,
        msg: Vec<u8>,
        state: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let handle_func = instance
            .get_func(&mut *store, self.handle_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "handle function",
                message: "Function not found".into(),
            })?;

        let typed = handle_func
            .typed::<(Vec<u8>, Vec<u8>), (Vec<u8>,)>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "handle function type",
                message: e.to_string(),
            })?;

        let (result,) =
            typed
                .call(&mut *store, (msg, state))
                .map_err(|e| WasmError::WasmError {
                    context: "handle function call",
                    message: e.to_string(),
                })?;

        Ok(result)
    }

    fn verify_state_contract(
        &self,
        store: &mut Store<()>,
        instance: &Instance,
        state: Vec<u8>,
    ) -> Result<bool> {
        let func = instance
            .get_func(&mut *store, self.state_contract_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "state-contract function",
                message: "Function not found".into(),
            })?;

        let typed = func
            .typed::<(Vec<u8>,), (bool,)>(&mut *store)
            .map_err(|e| WasmError::WasmError {
                context: "state-contract function type",
                message: e.to_string(),
            })?;

        let (result,) = typed
            .call(&mut *store, (state,))
            .map_err(|e| WasmError::WasmError {
                context: "state-contract function call",
                message: e.to_string(),
            })?;

        Ok(result)
    }
}

impl Actor for WasmActor {
    fn init(&self) -> Result<Value> {
        let mut store = Store::new(&self.engine, ());
        let instance = self.linker.instantiate(&mut store, &self.component)?;

        let result = self.call_init(&mut store, &instance)?;
        let state: Value = serde_json::from_slice(&result)?;

        Ok(state)
    }

    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)> {
        let mut store = Store::new(&self.engine, ());
        let instance = self.linker.instantiate(&mut store, &self.component)?;

        let state_bytes = serde_json::to_vec(state)?;

        match input {
            ActorInput::Message(msg) => {
                let msg_bytes = serde_json::to_vec(&msg)?;
                let result = self.call_handle(&mut store, &instance, msg_bytes, state_bytes)?;
                let new_state: Value = serde_json::from_slice(&result)?;
                Ok((ActorOutput::Message(msg), new_state))
            }
            ActorInput::HttpRequest {
                method,
                uri,
                headers,
                body,
            } => {
                if self.handle_http_index.is_none() {
                    return Err(anyhow::anyhow!("Actor does not support HTTP"));
                }

                let request = serde_json::json!({
                    "method": method,
                    "uri": uri,
                    "headers": { "fields": headers },
                    "body": body,
                });

                let request_bytes = serde_json::to_vec(&request)?;
                let result = self.call_handle(&mut store, &instance, request_bytes, state_bytes)?;

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
        let mut store = Store::new(&self.engine, ());
        let instance = match self.linker.instantiate(&mut store, &self.component) {
            Ok(instance) => instance,
            Err(_) => return false,
        };

        let state_bytes = match serde_json::to_vec(state) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        self.verify_state_contract(&mut store, &instance, state_bytes)
            .unwrap_or(false)
    }
}
