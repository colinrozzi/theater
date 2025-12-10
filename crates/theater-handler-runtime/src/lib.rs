//! # Runtime Handler
//!
//! Provides runtime information and control capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to log messages, get state information, and request shutdown.

use std::future::Future;
use std::pin::Pin;
use tracing::{error, info};
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::config::permissions::RuntimePermissions;
use theater::events::{runtime::RuntimeEventData, ChainEventData, EventData};
use theater::handler::Handler;
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};
use tokio::sync::mpsc::Sender;

/// Handler for providing runtime information and control to WebAssembly actors
#[derive(Clone)]
pub struct RuntimeHandler {
    config: RuntimeHostConfig,
    theater_tx: Sender<TheaterCommand>,
    permissions: Option<RuntimePermissions>,
}

impl RuntimeHandler {
    pub fn new(
        config: RuntimeHostConfig,
        theater_tx: Sender<TheaterCommand>,
        permissions: Option<RuntimePermissions>,
    ) -> Self {
        Self {
            config,
            theater_tx,
            permissions,
        }
    }
}

impl Handler for RuntimeHandler {
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting runtime handler");

        Box::pin(async {
            // Runtime handler doesn't need a background task, but we should wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;
            info!("Runtime handler received shutdown signal");
            info!("Runtime handler shut down");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> anyhow::Result<()> {
        info!("Setting up runtime host functions");
        
        let name1 = actor_component.name.clone();
        let name2 = actor_component.name.clone();
        let theater_tx = self.theater_tx.clone();

        let mut interface = match actor_component.linker.instance("theater:simple/runtime") {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "runtime-setup".to_string(),
                    data: EventData::Runtime(RuntimeEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "runtime-setup".to_string(),
                    data: EventData::Runtime(RuntimeEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/runtime: {}",
                    e
                ));
            }
        };

        // Log function
        interface
            .func_wrap(
                "log",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (msg,): (String,)| {
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

                    info!("[ACTOR] [{}] [{}] {}", id, name1, msg);
                    Ok(())
                },
            )
            .map_err(|e| {
                // Record function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "runtime-setup".to_string(),
                    data: EventData::Runtime(RuntimeEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "log_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to wrap log function: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap log function: {}", e)
            })?;

        // Get state function
        interface
            .func_wrap(
                "get-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> anyhow::Result<(Vec<u8>,)> {
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
            .map_err(|e| {
                // Record function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "runtime-setup".to_string(),
                    data: EventData::Runtime(RuntimeEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "get_state_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to wrap get-state function: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap get-state function: {}", e)
            })?;

        // Shutdown function
        interface
            .func_wrap_async(
                "shutdown",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (data,): (Option<Vec<u8>>,)|
                      -> Box<dyn Future<Output = anyhow::Result<(Result<(), String>,)>> + Send> {
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
                        name2,
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
            .map_err(|e| {
                // Record function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "runtime-setup".to_string(),
                    data: EventData::Runtime(RuntimeEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "shutdown_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to wrap shutdown function: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap shutdown function: {}", e)
            })?;

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "runtime-setup".to_string(),
            data: EventData::Runtime(RuntimeEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Runtime host functions setup completed successfully".to_string()),
        });

        Ok(())
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> anyhow::Result<()> {
        actor_instance.register_function_no_result::<(String,)>("theater:simple/actor", "init")
    }

    fn name(&self) -> &str {
        "runtime"
    }

    fn imports(&self) -> Option<String> {
        Some("theater:simple/runtime".to_string())
    }

    fn exports(&self) -> Option<String> {
        Some("theater:simple/actor".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::RuntimeHostConfig;
    use tokio::sync::mpsc;

    #[test]
    fn test_runtime_handler_creation() {
        let config = RuntimeHostConfig {};
        let (tx, _rx) = mpsc::channel(100);
        
        let handler = RuntimeHandler::new(config, tx, None);
        assert_eq!(handler.name(), "runtime");
        assert_eq!(handler.imports(), Some("theater:simple/runtime".to_string()));
        assert_eq!(handler.exports(), Some("theater:simple/actor".to_string()));
    }
}
