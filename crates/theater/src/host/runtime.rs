use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::config::actor_manifest::RuntimeHostConfig;
use crate::events::runtime::RuntimeEventData;
use crate::events::{ChainEventData, EventData};
use crate::messages::TheaterCommand;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};
use wasmtime::StoreContextMut;

#[derive(Clone)]
pub struct RuntimeHost {
    theater_tx: Sender<TheaterCommand>,
    permissions: Option<crate::config::permissions::RuntimePermissions>,
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
    pub fn new(_config: RuntimeHostConfig, theater_tx: Sender<TheaterCommand>, permissions: Option<crate::config::permissions::RuntimePermissions>) -> Self {
        Self { theater_tx, permissions }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up runtime host functions");
        let name = actor_component.name.clone();
        let mut interface = actor_component
            .linker
            .instance("theater:simple/runtime")
            .expect("Could not instantiate theater:simple/runtime");

        interface
            .func_wrap(
                "log",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>, (msg,): (String,)| {
                    let id = ctx.data().id.clone();

                    // Record log call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/runtime/log".to_string(),
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
                        event_type: "theater:simple/runtime/get-state".to_string(),
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
                        event_type: "theater:simple/runtime/get-state".to_string(),
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

        let name = actor_component.name.clone();
        let theater_tx = self.theater_tx.clone();

        interface
            .func_wrap_async(
                "shutdown",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>, (data,): (Option<Vec<u8>>,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record shutdown call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/runtime/shutdown".to_string(),
                        data: EventData::Runtime(RuntimeEventData::ShutdownCall {
                            data: data.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Actor shutdown with data: {:?}", data)),
                    });

                    info!(
                        "[ACTOR] [{}] [{}] Shutdown requested: {:?}",
                        ctx.data().id,
                        name,
                        data
                    );
                    let theater_tx = theater_tx.clone();

                    Box::new(async move {
                        match theater_tx
                            .send(TheaterCommand::ShuttingDown {
                                actor_id: ctx.data().id.clone(),
                                data,
                            })
                            .await
                        {
                            Ok(_) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/runtime/shutdown".to_string(),
                                    data: EventData::Runtime(RuntimeEventData::ShutdownRequested {
                                        success: true,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some("Shutdown command sent successfully".to_string()),
                                });
                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                // Record failed shutdown result event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/runtime/shutdown".to_string(),
                                    data: EventData::Runtime(RuntimeEventData::ShutdownRequested {
                                        success: false,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send shutdown command: {}",
                                        err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap shutdown function");

        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance.register_function_no_result::<(String,)>("theater:simple/actor", "init")
    }

    pub async fn start(
        &self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Runtime host starting");
        Ok(())
    }
}
