use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::shutdown::ShutdownReceiver;
use crate::actor_store::ActorStore;
use crate::config::SupervisorHostConfig;
use crate::events::supervisor::SupervisorEventData;
use crate::events::{ChainEventData, EventData};
use crate::messages::TheaterCommand;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::ChainEvent;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::{error, info};
use wasmtime::StoreContextMut;

pub struct SupervisorHost {}

#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct SupervisorEvent {
    event_type: String,
    actor_id: String,
    data: Option<Vec<u8>>,
}

impl SupervisorHost {
    pub fn new(_config: SupervisorHostConfig) -> Self {
        Self {}
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up host functions for supervisor");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/supervisor")
            .expect("Could not instantiate ntwk:theater/supervisor");

        // spawn-child implementation
        let _ = interface
            .func_wrap_async(
                "spawn",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (manifest, init_bytes): (String, Option<Vec<u8>>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    // Record spawn child call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/spawn".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::SpawnChildCall {
                            manifest_path: manifest.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Spawning child from manifest: {}", manifest)),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::SpawnActor {
                                manifest_path: manifest,
                                init_bytes,
                                response_tx,
                                parent_id: Some(parent_id),
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(actor_id)) => {
                                        let actor_id_str = actor_id.to_string();
                                        
                                        // Record spawn child result event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/spawn".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::SpawnChildResult {
                                                child_id: actor_id_str.clone(),
                                                success: true,
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Successfully spawned child with ID: {}", actor_id_str)),
                                        });
                                        
                                        Ok((Ok(actor_id_str),))
                                    }
                                    Ok(Err(e)) => {
                                        // Record spawn child error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/spawn".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "spawn".to_string(),
                                                child_id: None,
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to spawn child: {}", e)),
                                        });
                                        
                                        Ok((Err(e.to_string()),))
                                    }
                                    Err(e) => {
                                        // Record spawn child error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/spawn".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "spawn".to_string(),
                                                child_id: None,
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to receive spawn response: {}", e)),
                                        });
                                        
                                        Ok((Err(format!("Failed to receive response: {}", e)),))
                                    }
                                }
                            }
                            Err(e) => {
                                // Record spawn child error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/spawn".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "spawn".to_string(),
                                        child_id: None,
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send spawn command: {}", e)),
                                });
                                
                                Ok((Err(format!("Failed to send spawn command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap spawn function");

        // spawn-child implementation
        let _ = interface
            .func_wrap_async(
                "resume",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (manifest, state_bytes): (String, Option<Vec<u8>>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    // Record spawn child call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/spawn".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::ResumeChildCall {
                            manifest_path: manifest.clone(),
                            initial_state: state_bytes.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Resuming child from manifest: {}", manifest)),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::ResumeActor {
                                manifest_path: manifest,
                                state_bytes,
                                response_tx,
                                parent_id: Some(parent_id),
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(actor_id)) => {
                                        let actor_id_str = actor_id.to_string();
                                        
                                        // Record spawn child result event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/resume".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::ResumeChildResult {
                                                child_id: actor_id_str.clone(),
                                                success: true,
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Successfully resumed child with ID: {}", actor_id_str)),
                                        });
                                        
                                        Ok((Ok(actor_id_str),))
                                    }
                                    Ok(Err(e)) => {
                                        // Record spawn child error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/spawn".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "resume".to_string(),
                                                child_id: None,
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to spawn child: {}", e)),
                                        });
                                        
                                        Ok((Err(e.to_string()),))
                                    }
                                    Err(e) => {
                                        // Record spawn child error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/resume".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "resume".to_string(),
                                                child_id: None,
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to receive spawn response: {}", e)),
                                        });
                                        
                                        Ok((Err(format!("Failed to receive response: {}", e)),))
                                    }
                                }
                            }
                            Err(e) => {
                                // Record spawn child error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/resume".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "resume".to_string(),
                                        child_id: None,
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send resume command: {}", e)),
                                });
                                
                                Ok((Err(format!("Failed to send resume command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap resume function");

        // list-children implementation
        let _ = interface
            .func_wrap_async(
                "list-children",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      ()| -> Box<dyn Future<Output = Result<(Vec<String>,)>> + Send> {
                    // Record list children call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/list-children".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::ListChildrenCall {}),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some("Listing children".to_string()),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::ListChildren {
                                parent_id,
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(children) => {
                                    let children_str: Vec<String> = children
                                        .into_iter()
                                        .map(|id| id.to_string())
                                        .collect();
                                    
                                    // Record list children result event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/list-children".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::ListChildrenResult {
                                            children_count: children_str.len(),
                                            success: true,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Found {} children", children_str.len())),
                                    });
                                    
                                    Ok((children_str,))
                                }
                                Err(e) => {
                                    // Record list children error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/list-children".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "list-children".to_string(),
                                            child_id: None,
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to receive children list: {}", e)),
                                    });
                                    
                                    Err(anyhow::anyhow!("Failed to receive children list: {}", e))
                                }
                            },
                            Err(e) => {
                                // Record list children error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/list-children".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "list-children".to_string(),
                                        child_id: None,
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send list children command: {}", e)),
                                });
                                
                                Err(anyhow::anyhow!(
                                    "Failed to send list children command: {}",
                                    e
                                ))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap list-children function");

        // restart-child implementation
        let _ = interface
            .func_wrap_async(
                "restart-child",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record restart child call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/restart-child".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::RestartChildCall {
                            child_id: child_id.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Restarting child: {}", child_id)),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let child_id_clone = child_id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::RestartActor {
                                actor_id: match child_id.parse() {
                                    Ok(id) => id,
                                    Err(e) => {
                                        // Record error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/restart-child".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "restart-child".to_string(),
                                                child_id: Some(child_id_clone.clone()),
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to parse child ID: {}", e)),
                                        });
                                        
                                        return Ok((Err(format!("Invalid child ID: {}", e)),));
                                    }
                                },
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(Ok(())) => {
                                    // Record restart child result event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/restart-child".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::RestartChildResult {
                                            child_id: child_id_clone.clone(),
                                            success: true,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Successfully restarted child: {}", child_id_clone)),
                                    });
                                    
                                    Ok((Ok(()),))
                                }
                                Ok(Err(e)) => {
                                    // Record restart child error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/restart-child".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "restart-child".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to restart child: {}", e)),
                                    });
                                    
                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record restart child error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/restart-child".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "restart-child".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to receive restart response: {}", e)),
                                    });
                                    
                                    Ok((Err(format!("Failed to receive restart response: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record restart child error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/restart-child".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "restart-child".to_string(),
                                        child_id: Some(child_id_clone.clone()),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send restart command: {}", e)),
                                });
                                
                                Ok((Err(format!("Failed to send restart command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap restart-child function");
            
        // stop-child implementation
        let _ = interface
            .func_wrap_async(
                "stop-child",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record stop child call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/stop-child".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::StopChildCall {
                            child_id: child_id.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Stopping child: {}", child_id)),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let child_id_clone = child_id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::StopActor {
                                actor_id: match child_id.parse() {
                                    Ok(id) => id,
                                    Err(e) => {
                                        // Record error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/stop-child".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "stop-child".to_string(),
                                                child_id: Some(child_id_clone.clone()),
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to parse child ID: {}", e)),
                                        });
                                        
                                        return Ok((Err(format!("Invalid child ID: {}", e)),));
                                    }
                                },
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(Ok(())) => {
                                    // Record stop child result event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/stop-child".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::StopChildResult {
                                            child_id: child_id_clone.clone(),
                                            success: true,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Successfully stopped child: {}", child_id_clone)),
                                    });
                                    
                                    Ok((Ok(()),))
                                }
                                Ok(Err(e)) => {
                                    // Record stop child error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/stop-child".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "stop-child".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to stop child: {}", e)),
                                    });
                                    
                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record stop child error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/stop-child".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "stop-child".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to receive stop response: {}", e)),
                                    });
                                    
                                    Ok((Err(format!("Failed to receive stop response: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record stop child error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/stop-child".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "stop-child".to_string(),
                                        child_id: Some(child_id_clone.clone()),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send stop command: {}", e)),
                                });
                                
                                Ok((Err(format!("Failed to send stop command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap stop-child function");

        // get-child-state implementation
        let _ = interface
            .func_wrap_async(
                "get-child-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<Option<Vec<u8>>, String>,)>> + Send> {
                    // Record get child state call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/get-child-state".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::GetChildStateCall {
                            child_id: child_id.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Getting state for child: {}", child_id)),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let child_id_clone = child_id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::GetActorState {
                                actor_id: match child_id.parse() {
                                    Ok(id) => id,
                                    Err(e) => {
                                        // Record error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/get-child-state".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "get-child-state".to_string(),
                                                child_id: Some(child_id_clone.clone()),
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to parse child ID: {}", e)),
                                        });
                                        
                                        return Ok((Err(format!("Invalid child ID: {}", e)),));
                                    }
                                },
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(Ok(state)) => {
                                    // Record get child state result event
                                    let state_size = state.as_ref().map_or(0, |s| s.len());
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/get-child-state".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::GetChildStateResult {
                                            child_id: child_id_clone.clone(),
                                            state_size,
                                            success: true,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Successfully retrieved state for child {}: {} bytes", 
                                            child_id_clone, 
                                            state_size
                                        )),
                                    });
                                    
                                    Ok((Ok(state),))
                                }
                                Ok(Err(e)) => {
                                    // Record get child state error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/get-child-state".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "get-child-state".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to get child state: {}", e)),
                                    });
                                    
                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record get child state error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/get-child-state".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "get-child-state".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to receive state: {}", e)),
                                    });
                                    
                                    Ok((Err(format!("Failed to receive state: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record get child state error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/get-child-state".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "get-child-state".to_string(),
                                        child_id: Some(child_id_clone.clone()),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send state request: {}", e)),
                                });
                                
                                Ok((Err(format!("Failed to send state request: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap get-child-state function");

        // get-child-events implementation
        let _ = interface
            .func_wrap_async(
                "get-child-events",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<ChainEvent>, String>,)>> + Send> {
                    // Record get child events call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/supervisor/get-child-events".to_string(),
                        data: EventData::Supervisor(SupervisorEventData::GetChildEventsCall {
                            child_id: child_id.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Getting events for child: {}", child_id)),
                    });
                    
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let child_id_clone = child_id.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::GetActorEvents {
                                actor_id: match child_id.parse() {
                                    Ok(id) => id,
                                    Err(e) => {
                                        // Record error event
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "ntwk:theater/supervisor/get-child-events".to_string(),
                                            data: EventData::Supervisor(SupervisorEventData::Error {
                                                operation: "get-child-events".to_string(),
                                                child_id: Some(child_id_clone.clone()),
                                                message: e.to_string(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to parse child ID: {}", e)),
                                        });
                                        
                                        return Ok((Err(format!("Invalid child ID: {}", e)),));
                                    }
                                },
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(Ok(events)) => {
                                    // Record get child events result event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/get-child-events".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::GetChildEventsResult {
                                            child_id: child_id_clone.clone(),
                                            events_count: events.len(),
                                            success: true,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Successfully retrieved {} events for child {}", 
                                            events.len(), 
                                            child_id_clone
                                        )),
                                    });
                                    
                                    Ok((Ok(events),))
                                }
                                Ok(Err(e)) => {
                                    // Record get child events error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/get-child-events".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "get-child-events".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to get child events: {}", e)),
                                    });
                                    
                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record get child events error event
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "ntwk:theater/supervisor/get-child-events".to_string(),
                                        data: EventData::Supervisor(SupervisorEventData::Error {
                                            operation: "get-child-events".to_string(),
                                            child_id: Some(child_id_clone.clone()),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Failed to receive events: {}", e)),
                                    });
                                    
                                    Ok((Err(format!("Failed to receive events: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record get child events error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/supervisor/get-child-events".to_string(),
                                    data: EventData::Supervisor(SupervisorEventData::Error {
                                        operation: "get-child-events".to_string(),
                                        child_id: Some(child_id_clone.clone()),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send events request: {}", e)),
                                });
                                
                                Ok((Err(format!("Failed to send events request: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap get-child-events function");

        info!("Supervisor host functions added");

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No export functions needed for supervisor");
        Ok(())
    }

    pub async fn start(&self, _actor_handle: ActorHandle, _shutdown_receiver: ShutdownReceiver) -> Result<()> {
        info!("Starting supervisor host");
        Ok(())
    }
}
