use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::actor_runtime::WrappedActor;
use crate::config::RuntimeHostConfig;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::wasm::ActorComponent;
use crate::wasm::Event;
use crate::ActorStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{error, info, warn};
use wasmtime::StoreContextMut;

#[derive(Clone)]
pub struct RuntimeHost {
    actor_handle: ActorHandle,
}

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
    State(Result<Vec<u8>, String>),
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
    pub fn new(_config: RuntimeHostConfig, actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self, mut actor_component: ActorComponent) -> Result<()> {
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

    pub async fn add_exports(&self, mut actor_component: ActorComponent) -> Result<()> {
        info!("Adding exports for runtime host");
        actor_component.add_export("ntwk:theater/actor", "init");
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        info!("Runtime host starting");
        Ok(())
    }

    async fn handle_command(
        &self,
        command: RuntimeCommand,
    ) -> Result<RuntimeResponse, RuntimeError> {
        match command {
            RuntimeCommand::Log {
                level,
                message,
                timestamp,
            } => {
                let log_event = format!(
                    "[{}] [{}] {}",
                    timestamp,
                    match level {
                        LogLevel::Debug => "DEBUG",
                        LogLevel::Info => "INFO",
                        LogLevel::Warning => "WARN",
                        LogLevel::Error => "ERROR",
                    },
                    message
                );

                match level {
                    LogLevel::Debug => info!("{}", log_event),
                    LogLevel::Info => info!("{}", log_event),
                    LogLevel::Warning => warn!("{}", log_event),
                    LogLevel::Error => error!("{}", log_event),
                }

                Ok(RuntimeResponse::Log(Ok(())))
            }

            RuntimeCommand::GetState => {
                let state = self
                    .actor_handle
                    .get_state()
                    .await
                    .map_err(|e| RuntimeError::RuntimeError(e.to_string()))?;
                Ok(RuntimeResponse::State(Ok(state)))
            }

            RuntimeCommand::GetMetrics => {
                let metrics = self
                    .actor_handle
                    .get_metrics()
                    .await
                    .map_err(|e| RuntimeError::RuntimeError(e.to_string()))?;

                let runtime_metrics = RuntimeMetrics {
                    memory_usage: metrics.resource_metrics.memory_usage,
                    total_operations: metrics.operation_metrics.total_operations,
                    uptime_seconds: metrics.uptime_secs,
                };

                Ok(RuntimeResponse::Metrics(Ok(runtime_metrics)))
            }
        }
    }

    pub async fn process_runtime_event(&self, command: RuntimeCommand) -> Result<(), RuntimeError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Handle the command
        let response = self.handle_command(command).await?;

        // Create event with response
        let event = Event {
            event_type: "runtime-response".to_string(),
            parent: None,
            data: serde_json::to_vec(&response)?,
        };

        // Send event to actor
        self.actor_handle.handle_event(event).await?;

        Ok(())
    }
}
