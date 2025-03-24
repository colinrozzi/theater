use anyhow::Result;
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

use crate::actor_store::ActorStore;
use crate::config::ManifestConfig;
use crate::id::TheaterId;
use crate::utils::resolve_reference;
use tracing::{debug, error, info};
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
        info!("Loading WASM component from: {}", config.component_path);
        let wasm_bytes = resolve_reference(&config.component_path).await?;

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
        let (_interface_component_item, interface_component_export_index) =
            match self.component.export_index(None, interface_name) {
                Some(export) => export,
                None => {
                    error!(
                        "Interface '{}' not found in component exports",
                        interface_name
                    );
                    return Err(WasmError::WasmError {
                        context: "find_function_export",
                        message: format!(
                            "Interface '{}' not found in component exports",
                            interface_name
                        ),
                    });
                }
            };
        info!("Found interface export: {}", interface_name);

        let (func_component_item, func_component_export_index) = match self
            .component
            .export_index(Some(&interface_component_export_index), export_name)
        {
            Some(export) => export,
            None => {
                error!(
                    "Function '{}' not found in interface '{}'",
                    export_name, interface_name
                );
                return Err(WasmError::WasmError {
                    context: "find_function_export",
                    message: format!(
                        "Function '{}' not found in interface '{}'",
                        export_name, interface_name
                    ),
                });
            }
        };
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

pub struct ActorInstance {
    pub actor_component: ActorComponent,
    pub instance: Instance,
    pub store: Store<ActorStore>,
    pub functions: HashMap<String, Box<dyn TypedFunction>>,
}

impl ActorInstance {
    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    pub fn id(&self) -> TheaterId {
        self.actor_component.actor_store.id.clone()
    }

    pub async fn call_function(
        &mut self,
        name: &str,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        let func = match self.functions.get(name) {
            Some(f) => f,
            None => {
                error!(
                    "Function '{}' not found in functions table. Available functions: {:?}",
                    name,
                    self.functions.keys().collect::<Vec<_>>()
                );
                return Err(anyhow::anyhow!(
                    "Function '{}' not found in functions table",
                    name
                ));
            }
        };
        func.call_func(&mut self.store, state, params).await
    }

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
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}.{}", interface, function_name);
        let func =
            TypedComponentFunction::<P, R>::new(&mut self.store, &self.instance, export_index)
                .expect("Failed to create typed function");
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn register_function_no_params<R>(
        &mut self,
        interface: &str,
        function_name: &str,
    ) -> Result<()>
    where
        R: ComponentType + Lift + ComponentNamedList + Send + Sync + 'static,
        R: Serialize,
    {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}.{}", interface, function_name);
        let func =
            TypedComponentFunctionNoParams::<R>::new(&mut self.store, &self.instance, export_index)
                .expect("Failed to create typed function");
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn register_function_no_result<P>(
        &mut self,
        interface: &str,
        function_name: &str,
    ) -> Result<()>
    where
        P: ComponentType + Lower + ComponentNamedList + Send + Sync + 'static,
        for<'de> P: Deserialize<'de>,
    {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let name = format!("{}.{}", interface, function_name);
        let func = TypedComponentFunctionNoResult::<P>::new(
            &mut self.store,
            &self.instance,
            export_index,
        )?;
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }

    pub fn register_function_no_params_no_result(
        &mut self,
        interface: &str,
        function_name: &str,
    ) -> Result<()> {
        let export_index = self
            .actor_component
            .find_function_export(interface, function_name)
            .map_err(|e| {
                error!("Failed to find function export: {}", e);
                e
            })?;
        let name = format!("{}.{}", interface, function_name);
        debug!(
            "Found function: {}.{} with export index: {:?}",
            interface, function_name, export_index
        );
        let func = TypedComponentFunctionNoParamsNoResult::new(
            &mut self.store,
            &self.instance,
            export_index,
        )?;
        self.functions.insert(name.to_string(), Box::new(func));
        Ok(())
    }
}

pub struct TypedComponentFunction<P, R>
where
    P: ComponentNamedList,
    R: ComponentNamedList,
{
    func: TypedFunc<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>, R), String>,)>,
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
            .typed::<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>, R), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunction { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: P,
    ) -> Result<(Option<Vec<u8>>, R), String> {
        match self.func.call_async(&mut *store, (state, params)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res.0,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("Failed to call WebAssembly function: {}", e);
                error!("{}", error_msg);
                return Err(error_msg);
            }
        }
    }
}

pub trait TypedFunction: Send + Sync + 'static {
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>>;
}

impl<P, R> TypedFunction for TypedComponentFunction<P, R>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            // This is a simplified conversion - you'll need to implement actual conversion logic
            // from Vec<u8> to P and from R to Vec<u8> based on your serialization format
            let params_deserialized: P = serde_json::from_slice(&params)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize params: {}", e))?;

            match self.call_func(store, state, params_deserialized).await {
                Ok((new_state, result)) => {
                    let result_serialized = serde_json::to_vec(&result)
                        .map_err(|e| anyhow::anyhow!("Failed to serialize result: {}", e))?;

                    Ok((new_state, result_serialized))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}

pub struct TypedComponentFunctionNoParams<R>
where
    R: ComponentNamedList,
{
    func: TypedFunc<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>, R),), String>,)>,
}

impl<R> TypedComponentFunctionNoParams<R>
where
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

        let typed_func = func
            .typed::<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>, R),), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunctionNoParams { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
    ) -> Result<((Option<Vec<u8>>, R),), String> {
        let result = match self.func.call_async(&mut *store, (state,)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("Failed to call WebAssembly function (no params): {}", e);
                error!("{}", error_msg);
                return Err(error_msg);
            }
        };
        result.0
    }
}

impl<R> TypedFunction for TypedComponentFunctionNoParams<R>
where
    R: ComponentNamedList + Lift + Sync + Send + 'static + Serialize,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        _params: Vec<u8>, // Ignore params
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            match self.call_func(store, state).await {
                Ok(((new_state, result),)) => {
                    let result_serialized = serde_json::to_vec(&result)
                        .map_err(|e| anyhow::anyhow!("Failed to serialize result: {}", e))?;

                    Ok((new_state, result_serialized))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}

pub struct TypedComponentFunctionNoResult<P>
where
    P: ComponentNamedList,
{
    func: TypedFunc<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>,), String>,)>,
}

impl<P> TypedComponentFunctionNoResult<P>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static,
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

        let typed_func = func
            .typed::<(Option<Vec<u8>>, P), (Result<(Option<Vec<u8>>,), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunctionNoResult { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: P,
    ) -> Result<(Option<Vec<u8>>,), String> {
        let result = match self.func.call_async(&mut *store, (state, params)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!("Failed to call WebAssembly function (no result): {}", e);
                error!("{}", error_msg);
                return Err(error_msg);
            }
        };
        result.0
    }
}

impl<P> TypedFunction for TypedComponentFunctionNoResult<P>
where
    P: ComponentNamedList + Lower + Sync + Send + 'static + for<'de> Deserialize<'de>,
{
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            let params_deserialized: P = serde_json::from_slice(&params)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize params: {}", e))?;

            match self.call_func(store, state, params_deserialized).await {
                Ok((new_state,)) => {
                    // Return empty Vec<u8> as result
                    Ok((new_state, serde_json::to_vec(&()).unwrap()))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}

pub struct TypedComponentFunctionNoParamsNoResult {
    func: TypedFunc<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>,),), String>,)>,
}

impl TypedComponentFunctionNoParamsNoResult {
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

        let typed_func = func
            .typed::<(Option<Vec<u8>>,), (Result<((Option<Vec<u8>>,),), String>,)>(store)
            .inspect_err(|e| {
                error!("Failed to get typed function: {}", e);
            })?;

        Ok(TypedComponentFunctionNoParamsNoResult { func: typed_func })
    }

    pub async fn call_func(
        &self,
        store: &mut Store<ActorStore>,
        state: Option<Vec<u8>>,
    ) -> Result<((Option<Vec<u8>>,),), String> {
        let result = match self.func.call_async(&mut *store, (state,)).await {
            Ok(res) => match self.func.post_return_async(store).await {
                Ok(_) => res,
                Err(e) => {
                    let error_msg = format!("Failed to post return: {}", e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            },
            Err(e) => {
                let error_msg = format!(
                    "Failed to call WebAssembly function (no params, no result): {}",
                    e
                );
                error!("{}", error_msg);
                return Err(error_msg);
            }
        };
        result.0
    }
}

impl TypedFunction for TypedComponentFunctionNoParamsNoResult {
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        _params: Vec<u8>, // Ignore params
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>> {
        Box::pin(async move {
            match self.call_func(store, state).await {
                Ok(((new_state,),)) => {
                    // Return empty Vec<u8> as result
                    Ok((new_state, Vec::new()))
                }
                Err(e) => Err(anyhow::anyhow!("Failed to call function: {}", e)),
            }
        })
    }
}
