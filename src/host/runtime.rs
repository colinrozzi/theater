use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::shutdown::ShutdownReceiver;
use crate::actor_store::ActorStore;
use crate::config::RuntimeHostConfig;
use crate::events::runtime::RuntimeEventData;
use crate::events::{ChainEventData, EventData};
use crate::wasm::{ActorComponent, ActorInstance};
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

        interface
            .func_wrap(
                "log",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>, (msg,): (String,)| {
                    let id = ctx.data().id.clone();

                    // Record log call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/runtime/log".to_string(),
                        data: EventData::Runtime(RuntimeEventData::Log {
                            level: "info".to_string(),
                            message: msg.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Actor log: {}", msg)),
                    });

                    info!("[ACTOR] [{}] [{}] {}", id, name, msg);
                    Ok(())
                },
            )
            .expect("Failed to wrap log function");

        interface
            .func_wrap(
                "get-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(Vec<u8>,)> {
                    // Record state request call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/runtime/get-state".to_string(),
                        data: EventData::Runtime(RuntimeEventData::StateChangeCall {
                            old_state: "unknown".to_string(),
                            new_state: "requested".to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some("Get state request".to_string()),
                    });

                    // Return current state
                    let state = ctx
                        .data()
                        .get_last_event()
                        .map(|e| e.data.clone())
                        .unwrap_or_default();

                    // Record state request result event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/runtime/get-state".to_string(),
                        data: EventData::Runtime(RuntimeEventData::StateChangeResult {
                            success: true,
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("State retrieved: {} bytes", state.len())),
                    });

                    Ok((state,))
                },
            )
            .expect("Failed to wrap get-state function");

        interface
            .func_wrap(
                "init",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (params,): (String,)|
                      -> Result<()> {
                    // Record init call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/runtime/init".to_string(),
                        data: EventData::Runtime(RuntimeEventData::InitCall {
                            params: params.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Actor initialization with params: {}", params)),
                    });

                    // Record init result event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/runtime/init".to_string(),
                        data: EventData::Runtime(RuntimeEventData::InitResult { success: true }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some("Actor initialization successful".to_string()),
                    });

                    Ok(())
                },
            )
            .expect("Failed to wrap init function");

        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance.register_function_no_result::<(String,)>("ntwk:theater/actor", "init")
    }

    pub async fn start(&self, _actor_handle: ActorHandle, _shutdown_receiver: ShutdownReceiver) -> Result<()> {
        info!("Runtime host starting");
        Ok(())
    }
}
