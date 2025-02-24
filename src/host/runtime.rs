use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::RuntimeHostConfig;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::wasm::ActorComponent;
use crate::wasm::ActorInstance;
use crate::ActorStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{error, info};
use wasmtime::StoreContextMut;

#[derive(Clone)]
pub struct RuntimeHost {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeCommand {
    Log {
        level: LogLevel,
        message: String,
        timestamp: u64,
    },
    GetState,
    GetMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeResponse {
    Log(Result<(), String>),
    State(Result<Option<Vec<u8>>, String>),
    Metrics(Result<RuntimeMetrics, String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub memory_usage: usize,
    pub total_operations: u64,
    pub uptime_seconds: u64,
}

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl RuntimeHost {
    pub fn new(_config: RuntimeHostConfig) -> Self {
        Self {}
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up runtime host functions");
        let name = actor_component.name.clone();
        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/runtime")
            .expect("Could not instantiate ntwk:theater/runtime");

        let boundary = HostFunctionBoundary::new("ntwk:theater/runtime", "log");
        interface
            .func_wrap(
                "log",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>, (msg,): (String,)| {
                    let id = ctx.data().id.clone();
                    info!("[ACTOR] [{}] [{}] {}", id, name, msg);

                    // Record the log message in the chain
                    let _ = boundary.wrap(&mut ctx, msg.clone(), |_| Ok(()));
                    Ok(())
                },
            )
            .expect("Failed to wrap log function");

        let boundary = HostFunctionBoundary::new("ntwk:theater/runtime", "get-state");
        interface
            .func_wrap(
                "get-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(Vec<u8>,)> {
                    // Record the state request
                    let _ = boundary.wrap(&mut ctx, "state_request", |_| Ok(()));

                    // Return current state
                    let state = ctx
                        .data()
                        .get_last_event()
                        .map(|e| e.data.clone())
                        .unwrap_or_default();

                    // Record the response
                    let _ = boundary.wrap(&mut ctx, state.clone(), |_| Ok(()));

                    Ok((state,))
                },
            )
            .expect("Failed to wrap get-state function");

        Ok(())
    }

    pub async fn add_exports(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Adding exports for runtime host");
        actor_component.add_export("ntwk:theater/actor", "init");
        Ok(())
    }

    pub async fn add_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance.register_function::<(), ()>("ntwk:theater/actor.init")
    }

    pub async fn start(&self, _actor_handle: ActorHandle) -> Result<()> {
        info!("Runtime host starting");
        Ok(())
    }
}
