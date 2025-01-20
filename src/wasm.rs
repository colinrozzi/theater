use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::path::PathBuf;
use thiserror::Error;
use wasmtime::chain::Chain;
use wasmtime::component::{Component, ComponentExportIndex, ComponentType, Lift, Linker, Lower};
use wasmtime::{Engine, Store, StoreContextMut};

use crate::config::ManifestConfig;
use crate::messages::TheaterCommand;
use crate::Store as ActorStore;
use tracing::{error, info};

pub type Json = Vec<u8>;

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct Event {
    #[component(name = "event-type")]
    pub event_type: String,
    pub parent: Option<u64>,
    pub data: Json,
}

pub type ActorState = Vec<u8>;

#[derive(Error, Debug)]
pub enum WasmError {
    #[error("Failed to load manifest: {0}")]
    ManifestError(String),

    #[error("WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },

    #[error("Function types were incorrect for {func_name} call \n Expected params: {expected_params} \n Expected result: {expected_result} \n Error: {err}")]
    GetFuncTypedError {
        context: &'static str,
        func_name: String,
        expected_params: String,
        expected_result: String,
        err: wasmtime::Error,
    },
}

/// WebAssembly actor implementation
pub struct WasmActor {
    name: String,
    engine: Engine,
    component: Component,
    pub linker: Linker<ActorStore>,
    pub exports: HashMap<String, ComponentExportIndex>,
    pub store: Store<ActorStore>,
    pub actor_store: ActorStore,
    pub actor_state: ActorState,
}

impl WasmActor {
    pub async fn new(config: &ManifestConfig, actor_store: ActorStore) -> Result<Self> {
        // Load WASM component
        let engine = Engine::new(wasmtime::Config::new().async_support(true))?;
        info!(
            "Loading WASM component from: {}",
            config.component_path.display()
        );
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
        let store = Store::new(&engine, actor_store.clone());

        let mut actor = WasmActor {
            name: config.name.clone(),
            engine,
            component,
            linker,
            exports: HashMap::new(),
            store,
            actor_store,
            actor_state: vec![],
        };

        actor
            .add_runtime_host_func()
            .await
            .expect("Failed to add runtime host functions");
        actor
            .add_runtime_exports()
            .expect("Failed to add runtime exports");

        Ok(actor)
    }

    pub async fn init(&mut self) {
        info!("Initializing actor");
        let init_state_bytes = self
            .call_func::<(), (ActorState,)>("init", ())
            .await
            .unwrap();
        self.actor_state = init_state_bytes.0;
    }

    pub async fn handle_event(&mut self, event: Event) -> Result<()> {
        info!("handling event");
        info!("Event details: {:#?}", event); // Log full event structure
        info!(
            "Current actor state {}",
            serde_json::to_string(&json!(self.actor_state)).expect("Failed to serialize state")
        );

        let new_state = self
            .call_func::<(Event, ActorState), (ActorState,)>(
                "handle",
                (event, self.actor_state.clone()),
            )
            .await
            .expect("Failed to call handle function");
        self.actor_state = new_state.0;
        Ok(())
    }

    async fn add_runtime_host_func(&mut self) -> Result<()> {
        info!("Adding runtime host functions");
        let mut runtime = self
            .linker
            .instance("ntwk:theater/runtime")
            .expect("Failed to get runtime instance");

        runtime.func_wrap(
            "log",
            |ctx: wasmtime::StoreContextMut<'_, ActorStore>, (msg,): (String,)| {
                let id = ctx.data().id.clone();
                info!("[ACTOR] [{}] {}", id, msg);
                Ok(())
            },
        )?;

        let _ = runtime.func_wrap_async(
            "spawn",
            |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
             (manifest,): (String,)|
             -> Box<dyn Future<Output = Result<()>> + Send> {
                let store = ctx.data_mut();
                let theater_tx = store.theater_tx.clone();
                info!("Spawning actor with manifest: {}", manifest);
                Box::new(async move {
                    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
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
            |ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(Chain,)> {
                let chain = ctx.get_chain();
                Ok((chain.clone(),))
            },
        )?;

        info!("Runtime host functions added");

        Ok(())
    }

    fn add_runtime_exports(&mut self) -> Result<()> {
        let init_export = self
            .find_export("ntwk:theater/actor", "init")
            .expect("Failed to find init export");
        self.exports.insert("init".to_string(), init_export);
        Ok(())
    }

    pub fn find_export(
        &mut self,
        interface_name: &str,
        export_name: &str,
    ) -> Result<ComponentExportIndex> {
        info!(
            "Finding export: {} from interface: {}",
            export_name, interface_name
        );
        let (_, instance) = self
            .component
            .export_index(None, interface_name)
            .expect("Failed to find interface export");
        let (_, export) = self
            .component
            .export_index(Some(&instance), export_name)
            .expect("Failed to find export");
        Ok(export)
    }

    fn save_chain(&self) {
        let chain = self.store.get_chain();
        let chain_json = serde_json::to_string(&chain).expect("Failed to serialize chain");
        // chain path: chain/actor_name.json
        let chain_path = format!("chain/{}.json", self.name);
        std::fs::write(chain_path, chain_json).expect("Failed to write chain to file");
    }

    pub async fn call_func<T, U>(&mut self, export_name: &str, args: T) -> Result<U>
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
        let instance = self
            .linker
            .instantiate_async(&mut self.store, &self.component)
            .await
            .map_err(|e| WasmError::WasmError {
                context: "instantiation",
                message: e.to_string(),
            })?;

        info!("Calling function: {}", export_name);
        info!("Existing exports: {:?}", self.exports);
        let index = self
            .exports
            .get(export_name)
            .ok_or_else(|| WasmError::WasmError {
                context: "function lookup",
                message: format!("Function {} not found", export_name),
            })?;

        let func =
            instance
                .get_func(&mut self.store, *index)
                .ok_or_else(|| WasmError::WasmError {
                    context: "function access",
                    message: format!("Failed to get function {}", export_name),
                })?;

        info!("Function details:");
        info!("  - Name: {}", export_name);
        info!("  - Function details: {:?}", func);
        info!("  - Param types raw: {:?}", func.params(&mut self.store));
        info!("  - Result types raw: {:?}", func.results(&mut self.store));
        info!("  - Generic type T: {}", std::any::type_name::<T>());
        info!("  - Generic type U: {}", std::any::type_name::<U>());

        let typed =
            func.typed::<T, U>(&mut self.store)
                .map_err(|e| WasmError::GetFuncTypedError {
                    context: "function type",
                    func_name: export_name.to_string(),
                    expected_params: format!("{:?}", func.params(&self.store)),
                    expected_result: format!("{:?}", func.results(&self.store)),
                    err: e,
                })?;

        let result =
            typed
                .call_async(&mut self.store, args)
                .await
                .map_err(|e| WasmError::WasmError {
                    context: "function call",
                    message: e.to_string(),
                })?;

        self.save_chain();

        Ok(result)
    }
}
