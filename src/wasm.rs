use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::marker::Copy;
use std::path::PathBuf;
use thiserror::Error;
use wasmtime::component::{Component, ComponentExportIndex, Instance, Linker};
use wasmtime::{Engine, StoreContextMut};

use crate::config::ManifestConfig;
use crate::messages::TheaterCommand;
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
#[derive(Clone)]
pub struct WasmActor {
    engine: Engine,
    component: Component,
    linker: Linker<Store>,
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
            exports: HashMap::new(),
            store,
        };

        actor.add_runtime_host_func().await?;
        actor.add_runtime_exports()?;

        Ok(actor)
    }

    async fn add_runtime_host_func(&mut self) -> Result<()> {
        let mut runtime = self.linker.instance("ntwk:theater/runtime")?;

        runtime.func_wrap(
            "log",
            |ctx: wasmtime::StoreContextMut<'_, Store>, (msg,): (String,)| {
                let id = ctx.data().id.clone();
                info!("[ACTOR] [{}] {}", id, msg);
                Ok(())
            },
        )?;

        // Add send function
        runtime.func_wrap(
            "send",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>, (address, msg): (String, Vec<u8>)| {
                let store = ctx.data_mut();
                //send(store, address, msg);

                let msg_value: Value =
                    serde_json::from_slice(&msg).expect("Failed to parse message as JSON");

                let _result = tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    let _response = client
                        .post(&address)
                        .json(&msg_value)
                        .send()
                        .await
                        .expect("Failed to send message");
                });
                Ok(())
            },
        )?;

        runtime.func_wrap_async(
            "spawn",
            |mut ctx: wasmtime::StoreContextMut<'_, Store>,
             (manifest,): (String,)|
             -> Box<dyn Future<Output = Result<()>> + Send> {
                let store = ctx.data_mut();
                let theater_tx = store.theater_tx.clone();
                info!("Spawning actor with manifest: {}", manifest);
                Box::new(async move {
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    info!("sending spawn command");
                    match theater_tx
                        .send(TheaterCommand::SpawnActor {
                            manifest_path: PathBuf::from(manifest),
                            response_tx,
                        })
                        .await
                    {
                        Ok(_) => info!("spawn command sent"),
                        Err(e) => error!("error sending spawn command: {:?}", e),
                    }
                    Ok(())
                })
            },
        );

        runtime.func_wrap(
            "get-chain",
            |mut ctx: StoreContextMut<'_, Store>, ()| -> Result<(Vec<u8>,)> {
                let chain = ctx.get_chain();
                Ok((serde_json::to_vec(&json!(chain))?,))
            },
        )?;

        Ok(())
    }

    fn add_runtime_exports(&mut self) -> Result<()> {
        let init_export = self.find_export("ntwk:theater/runtime", "init")?;
        let handle_export = self.find_export("ntwk:theater/runtime", "handle")?;
        self.exports.insert("init".to_string(), init_export);
        self.exports.insert("handle".to_string(), handle_export);
        Ok(())
    }

    fn find_export(
        &mut self,
        interface_name: &str,
        export_name: &str,
    ) -> Result<ComponentExportIndex> {
        let (_, instance) = self
            .component
            .export_index(None, interface_name)
            .expect("Failed to find interface export");
        let (_, export) = self
            .component
            .export_index(None, export_name)
            .expect("Failed to find export");
        Ok(export)
    }

    fn get_export(&self, name: &str) -> Option<&ComponentExportIndex> {
        self.exports.get(name)
    }

    pub async fn call_func<T, U>(&self, export_name: &str, args: T) -> Result<U>
    where
        T: wasmtime::component::Lower
            + wasmtime::component::ComponentNamedList
            + Send
            + Sync
            + Serialize
            + Debug,
        U: wasmtime::component::Lift
            + wasmtime::component::ComponentNamedList
            + Send
            + Sync
            + Serialize
            + Debug
            + Clone,
    {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await?;

        info!("Calling function: {}", export_name);
        let index = self
            .get_export(export_name)
            .ok_or_else(|| WasmError::WasmError {
                context: "function lookup",
                message: format!("Function {} not found", export_name),
            })?;

        let func = instance
            .get_func(&mut store, *index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function access",
                message: format!("Failed to get function {}", export_name),
            })?;

        info!("params type: {:?}", func.params(&mut store));
        info!("results type: {:?}", func.results(&mut store));

        let typed = func
            .typed::<T, U>(&mut store)
            .map_err(|e| WasmError::WasmError {
                context: "function type",
                message: format!("typed call failed: {}", e),
            })?;

        let result =
            typed
                .call_async(&mut store, args)
                .await
                .map_err(|e| WasmError::WasmError {
                    context: "function call",
                    message: e.to_string(),
                })?;

        Ok(result)
    }

    // I am going to add a function that will allow us to set up functions to be imports that
    // will add the call and result of that function to the chain, similar to the call_func for
    // exports
    //
    // also, I think I can move ownership of the chain into the actor, as the chain should be used
    // for call_func and set up in wrap_func
}
