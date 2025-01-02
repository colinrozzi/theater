use crate::chain::HashChain;
use crate::config::ComponentConfig;
use anyhow::Result;
use core::pin::{pin, Pin};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::{error, info};
use wasmtime::component::{
    Component, ComponentExportIndex, ComponentNamedList, Lift, Linker, Lower,
};
use wasmtime::Engine;
use wasmtime::StoreContextMut;
use wasmtime::{AsContext, AsContextMut};

#[derive(Clone)]
struct Store {
    pub chain: Arc<RwLock<HashChain>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    pub type_: String,
    pub data: serde_json::Value,
}

pub struct Process {
    engine: Engine,
    component: Component,
    linker: Linker<Store>,
    exports: HashMap<String, ComponentExportIndex>,
    store: Store,
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

impl Process {
    pub async fn new(config: &ComponentConfig) -> Result<Self> {
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

        let chain = Arc::new(RwLock::new(HashChain::new()));

        Ok(Self {
            engine,
            component,
            linker,
            exports: HashMap::new(),
            store: Store { chain },
        })
    }

    pub fn wrap<F, Params, Return>(
        &mut self,
        instance_name: &str,
        func_name: &str,
        func: F,
    ) -> Result<()>
    where
        F: Fn(StoreContextMut<'_, Store>, Params) -> Return + Send + Sync + 'static,
        Params: Lower + Lift + ComponentNamedList + Serialize + 'static,
        Return: Lower + Lift + ComponentNamedList + Serialize + 'static,
    {
        let mut instance = self.linker.instance(instance_name)?;
        // GROSS !!!!! I do not understand this well enough i am sure there is an elegant way to do
        // this
        let func_name = func_name.to_string(); // Clone the string to own it
        let f_name = func_name.to_string();
        let chain = self.store.chain.clone();

        let wrapped_func = move |mut ctx: wasmtime::StoreContextMut<'_, Store>,
                                 params: Params|
              -> Result<Return> {
            chain.write().unwrap().add(Event {
                type_: "function-call".to_string(),
                data: json!({
                    "function": &func_name,
                    "args": params,
                }),
            });
            let result = {
                let ctx_ref = &mut ctx;
                func(ctx_ref.as_context_mut(), params)
            };
            chain.write().unwrap().add(Event {
                type_: "function-result".to_string(),
                data: json!({
                    "function": func_name,
                    "result": result,
                }),
            });
            Ok(result)
        };
        instance.func_wrap(&f_name, wrapped_func)
    }

    pub fn wrap_async<F, Params, Return>(
        &mut self,
        instance_name: &str,
        func_name: &str,
        func: F,
    ) -> Result<()>
    where
        // Notice this signature is unchanged: it returns an unpinned Box<dyn Future<...>>
        F: for<'a> Fn(
                StoreContextMut<'a, Store>,
                Params,
            ) -> Box<dyn Future<Output = Result<Return>> + Send + 'a>
            + Send
            + Sync
            + 'static,
        Params: Lower + Lift + ComponentNamedList + Serialize + 'static,
        Return: Lower + Lift + ComponentNamedList + Serialize + 'static,
    {
        let mut instance = self.linker.instance(instance_name)?;
        let func_name = func_name.to_string();
        let f_name = func_name.clone();
        let chain = self.store.chain.clone();

        let ch = chain.clone();
        let wrapped_func = move |mut ctx: wasmtime::StoreContextMut<'_, Store>,
                                 params: Params|
              -> Box<dyn Future<Output = Result<Return>> + Send> {
            // 1) Log the function call
            ch.write().unwrap().add(Event {
                type_: "function-call".to_string(),
                data: json!({
                    "function": &func_name,
                    "args": params,
                }),
            });

            let c = ch.clone();
            // 2) Return a new `Box::new(async move { ... })` future.
            Box::new(async move {
                // a) Get the user-supplied (unpinned) future:
                let future: Box<dyn Future<Output = Result<Return>> + Send> =
                    func(ctx.as_context_mut(), params);

                // b) Pin it on the stack, so we can safely `.await`:
                pin!(future);

                // c) Now .await is valid
                let result = future.await;

                // 3) Log the result
                match &result {
                    Ok(val) => {
                        c.write().unwrap().add(Event {
                            type_: "function-result".to_string(),
                            data: json!({
                                "function": &func_name,
                                "result": val,
                            }),
                        });
                    }
                    Err(e) => {
                        c.write().unwrap().add(Event {
                            type_: "function-result".to_string(),
                            data: json!({
                                "function": &func_name,
                                "error": e.to_string(),
                            }),
                        });
                    }
                }

                // 4) Return the user's result
                result
            })
        };

        instance.func_wrap_async(&f_name, wrapped_func)?;
        Ok(())
    }

    pub async fn call_func<T, U>(&mut self, export_name: &str, args: T) -> Result<U>
    where
        T: wasmtime::component::Lower
            + wasmtime::component::ComponentNamedList
            + Send
            + Sync
            + Serialize,
        U: wasmtime::component::Lift
            + wasmtime::component::ComponentNamedList
            + Send
            + Sync
            + Serialize,
    {
        let mut store = wasmtime::Store::new(&self.engine, self.store.clone());
        let instance = self
            .linker
            .instantiate_async(&mut store, &self.component)
            .await?;
        let chain = self.store.chain.clone();
        // record the function call in the chain
        let event = Event {
            type_: "function-call".to_string(),
            data: json!({
                "function": export_name,
                "args": args,
            }),
        };

        chain.write().unwrap().add(event);

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

        let result_event = Event {
            type_: "function-result".to_string(),
            data: json!({
                "function": export_name,
                "result": result,
            }),
        };

        chain.write().unwrap().add(result_event);
        Ok(result)
    }

    fn get_export(&self, export_name: &str) -> Option<&ComponentExportIndex> {
        self.exports.get(export_name)
    }
}
