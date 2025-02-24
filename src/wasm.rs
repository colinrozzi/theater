use anyhow::Result;
// use serde::de::DeserializeOwned;
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
        let (_interface_component_item, interface_component_export_index) = self
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

pub struct TypedComponentFunction<P, R>
where
    P: ComponentNamedList,
    R: ComponentNamedList,
{
    func: TypedFunc<P, R>,
}

impl<P, R> TypedComponentFunction<P, R>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static,
    R: ComponentNamedList + Lift + Sync + Send + 'static,
{
    pub fn new(
        store: &mut Store<ActorStore>,
        instance: &Instance,
        export_index: ComponentExportIndex,
    ) -> Result<Self> {
        let func = instance
            .get_func(&mut *store, export_index)
            .ok_or_else(|| WasmError::WasmError {
                context: "function retrieval",
                message: "Function not found".to_string(),
            })?;

        // Convert the Func to a TypedFunc with the correct signature
        let typed_func = func
            .typed::<P, R>(store)
            .map_err(|e| WasmError::WasmError {
                context: "function typing",
                message: format!("Failed to type function: {}", e),
            })?;

        Ok(TypedComponentFunction { func: typed_func })
    }

    pub async fn call_func(&self, store: &mut Store<ActorStore>, params: P) -> Result<R> {
        let result =
            self.func
                .call_async(store, params)
                .await
                .map_err(|e| WasmError::WasmError {
                    context: "function call",
                    message: e.to_string(),
                })?;
        Ok(result)
    }
}

impl<P, R> TypedFunction<P, R> for TypedComponentFunction<P, R>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        params: P,
    ) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'a>> {
        Box::pin(async move {
            // This is a simplified conversion - you'll need to implement actual conversion logic
            // from Vec<u8> to P and from R to Vec<u8> based on your serialization format

            let results = self.call_func(store, params).await?;
            Ok(results)
        })
    }
}

pub trait TypedFunction<P, R>: Send + Sync + 'static
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        params: P,
    ) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'a>>;
}

pub struct ActorInstance<P, R>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    pub actor_component: ActorComponent,
    pub instance: Instance,
    pub store: Store<ActorStore>,
    pub functions: HashMap<String, Box<dyn TypedFunction<P, R>>>,
}

impl<P, R> ActorInstance
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    pub fn register_function<P, R>(&mut self, interface: &str, function_name: &str) -> Result<()>
    where
        P: ComponentType + Lower + ComponentNamedList + Send + Sync + 'static,
        R: ComponentType + Lift + ComponentNamedList + Send + Sync + 'static,
        for<'de> P: Deserialize<'de>,
        R: Serialize,
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

    pub async fn call_function<P, R>(&mut self, name: &str, params: P) -> Result<R>
    where
        P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
        R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
    {
        let func = self
            .functions
            .get(name)
            .expect("Function not found in functions table");
        func.call_func(&mut self.store, params).await
    }
}
