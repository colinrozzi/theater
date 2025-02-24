use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use wasmtime::component::TypedFunc;
use wasmtime::component::{
    Component, ComponentExportIndex, ComponentNamedList, ComponentType, Instance, Lift, Linker,
    Lower,
};
use wasmtime::{Engine, Store};

use crate::config::ManifestConfig;
use crate::store::ActorStore;
use tracing::{error, info};
use wasmtime::component::types::ComponentItem;

pub type Json = Vec<u8>;

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct Event {
    #[component(name = "event-type")]
    pub event_type: String,
    pub parent: Option<u64>,
    pub data: Json,
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

    #[error("Function types were incorrect for {func_name} call \n Expected params: {expected_params} \n Expected result: {expected_result} \n Error: {err}")]
    GetFuncTypedError {
        context: &'static str,
        func_name: String,
        expected_params: String,
        expected_result: String,
        err: wasmtime::Error,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStats {
    pub state_size: usize,
    pub exports_table_size: usize,
    pub store_size: usize,
    pub num_exports: usize,
    pub num_chain_events: usize,
}

pub struct ActorComponent {
    pub name: String,
    pub component: Component,
    pub actor_store: ActorStore,
    pub linker: Linker<ActorStore>,
    pub engine: Engine,
    pub exports: HashMap<String, ComponentExportIndex>,
}

impl ActorComponent {
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

        Ok(ActorComponent {
            name: config.name.clone(),
            component,
            actor_store,
            linker,
            engine,
            exports: HashMap::new(),
        })
    }

    pub fn find_function_export(
        &mut self,
        interface_name: &str,
        export_name: &str,
    ) -> Result<ComponentExportIndex, WasmError> {
        info!(
            "Finding export: {} from interface: {}",
            export_name, interface_name
        );
        let (interface_component_item, interface_component_export_index) = self
            .component
            .export_index(None, interface_name)
            .expect(format!("Failed to find interface export: {}", interface_name).as_str());
        info!("Found interface export: {}", interface_name);
        let (func_component_item, func_component_export_index) = self
            .component
            .export_index(Some(&interface_component_export_index), export_name)
            .expect(format!("Failed to find export: {}", export_name).as_str());
        match func_component_item {
            ComponentItem::ComponentFunc(component_func) => {
                info!("Found export: {}", export_name);
                let params = component_func.params();
                for param in params {
                    info!("Param: {:?}", param);
                }
                let results = component_func.results();
                for result in results {
                    info!("Result: {:?}", result);
                }

                Ok(func_component_export_index)
            }
            _ => {
                error!(
                    "Export {} is not a function, it is a {:?}",
                    export_name, func_component_item
                );

                Err(WasmError::WasmError {
                    context: "export type",
                    message: format!(
                        "Export {} is not a function, it is a {:?}",
                        export_name, func_component_item
                    ),
                })
            }
        }
    }

    pub async fn instantiate(self) -> Result<ActorInstance> {
        let mut store = Store::new(&self.engine, self.actor_store.clone());

        let instance = self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await
            .map_err(|e| WasmError::WasmError {
                context: "instantiation",
                message: e.to_string(),
            })?;

        Ok(ActorInstance {
            actor_component: self,
            instance,
            store,
            functions: HashMap::new(),
        })
    }
}

pub trait WithState {
    type Params: ComponentType + Lower + Lift + DeserializeOwned + Send + Sync + ComponentNamedList;
}

impl<P> WithState for P
where
    P: ComponentType + Lower + Lift + DeserializeOwned + Send + Sync + ComponentNamedList,
{
    type Params = (Option<Vec<u8>>, P);
}

pub struct TypedComponentFunction<P, R>
where
    P: ComponentType + Lower + DeserializeOwned + Send + Sync,
    R: ComponentType + Lift + Serialize + Send + Sync,
{
    func: TypedFunc<(Option<Vec<u8>>, P), (Option<Vec<u8>>, R)>,
}

impl<P: WithState, R: WithState> TypedComponentFunction<P, R>
where
    P: ComponentType + Lower + DeserializeOwned + Send + Sync,
    R: ComponentType + Lift + Serialize + Send + Sync,
{
    pub fn new(
        store: &mut Store<ActorStore>,
        instance: &Instance,
        export_index: ComponentExportIndex,
    ) -> Result<Self> {
        let func = instance.get_typed_func::<P::Params, R::Params>(store, export_index)?;
        Ok(Self { func })
    }
}

pub trait ComponentFunction: Send + Sync {
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>>;
}

impl<P, R> ComponentFunction for TypedComponentFunction<P, R>
where
    P: ComponentType
        + Lower
        + DeserializeOwned
        + ComponentNamedList
        + Send
        + Sync
        + Serialize
        + Debug
        + Clone,
    R: ComponentType + Lift + Serialize + ComponentNamedList + Send + Sync + Serialize + Debug,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            let params: P = serde_json::from_slice(&params)?;
            let (new_state, result) = self.func.call_async(store, (state, params)).await?;
            let result_bytes = serde_json::to_vec(&result)?;
            Ok((new_state, result_bytes))
        })
    }
}

pub struct ActorInstance {
    pub actor_component: ActorComponent,
    pub instance: Instance,
    pub store: Store<ActorStore>,
    pub functions: HashMap<String, Box<dyn ComponentFunction>>,
}

impl ActorInstance {
    pub fn register_function<P, R>(&mut self, interface: &str, function_name: &str) -> Result<()>
    where
        P: ComponentType
            + Lower
            + DeserializeOwned
            + ComponentNamedList
            + Send
            + Sync
            + Serialize
            + Debug
            + Clone
            + 'static,
        R: ComponentType
            + Lift
            + Serialize
            + ComponentNamedList
            + Send
            + Sync
            + Serialize
            + Debug
            + 'static,
    {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        info!(
            "Found function: {}:{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}:{}", interface, function_name);
        let func =
            TypedComponentFunction::<P, R>::new(&mut self.store, &self.instance, export_index)?;
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    pub async fn call_function(
        &mut self,
        name: &str,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        let func = self
            .functions
            .get(name)
            .expect("Function not found in functions table");
        func.call_func(&mut self.store, state, params).await
    }
}
