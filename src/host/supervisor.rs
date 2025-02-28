use crate::actor_executor::ActorError;
use crate::wasm::ActorComponent;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::messages::TheaterCommand;
use crate::actor_store::ActorStore;
use crate::ChainEvent;
use tokio::sync::oneshot;
use std::future::Future;
use wasmtime::StoreContextMut;
use crate::actor_handle::ActorHandle;
use crate::config::SupervisorHostConfig;
use crate::wasm::ActorInstance;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{error, info};

pub struct SupervisorHost {
}

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
    pub fn new(
        _config: SupervisorHostConfig,
    ) -> Self {
        Self {
        }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up host functions for supervisor");

        let mut interface = actor_component.linker.instance("ntwk:theater/supervisor").expect("Could not instantiate ntwk:theater/supervisor");

                // spawn-child implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "spawn");
        let _ = interface
            .func_wrap_async(
                "spawn",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (manifest,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();
                    let parent_id = store.id.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, manifest.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::SpawnActor {
                                manifest_path: manifest,
                                response_tx,
                                parent_id: Some(parent_id),
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(actor_id)) => {
                                        let actor_id_str = actor_id.to_string();
                                        let _ = boundary.wrap(&mut ctx, actor_id_str.clone(), |_| Ok(()));
                                        Ok((Ok(actor_id_str),))
                                    }
                                    Ok(Err(e)) => Ok((Err(e.to_string()),)),
                                    Err(e) => Ok((Err(format!("Failed to receive response: {}", e)),))
                                }
                            }
                            Err(e) => Ok((Err(format!("Failed to send spawn command: {}", e)),))
                        }
                    })
                },
            )
            .expect("Failed to wrap spawn function");

        // list-children implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "list-children");
        interface 
            .func_wrap_async(
                "list-children",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      ()| -> Box<dyn Future<Output = Result<(Vec<String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();
                    let boundary = boundary.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, "list_children".to_string(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::ListChildren {
                                parent_id,
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(children) => {
                                        let children_str: Vec<String> = children
                                            .into_iter()
                                            .map(|id| id.to_string())
                                            .collect();
                                        Ok((children_str,))
                                    }
                                    Err(e) => Err(anyhow::anyhow!("Failed to receive children list: {}", e))
                                }
                            }
                            Err(e) => Err(anyhow::anyhow!("Failed to send list children command: {}", e))
                        }
                    })
                },
            )
            .expect("Failed to wrap list-children function");

        // stop-child implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "stop-child");
        interface
            .func_wrap_async(
                "stop-child",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, child_id.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::StopActor {
                                actor_id: child_id.parse()?,
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(())) => Ok((Ok(()),)),
                                    Ok(Err(e)) => Ok((Err(e.to_string()),)),
                                    Err(e) => Ok((Err(format!("Failed to receive stop response: {}", e)),))
                                }
                            }
                            Err(e) => Ok((Err(format!("Failed to send stop command: {}", e)),))
                        }
                    })
                },
            )
            .expect("Failed to wrap stop-child function");

        // restart-child implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "restart-child");
       interface 
            .func_wrap_async(
                "restart-child",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, child_id.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::RestartActor {
                                actor_id: child_id.parse()?,
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(())) => Ok((Ok(()),)),
                                    Ok(Err(e)) => Ok((Err(e.to_string()),)),
                                    Err(e) => Ok((Err(format!("Failed to receive restart response: {}", e)),))
                                }
                            }
                            Err(e) => Ok((Err(format!("Failed to send restart command: {}", e)),))
                        }
                    })
                },
            )
            .expect("Failed to wrap restart-child function");

        // get-child-state implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "get-child-state");
       interface 
            .func_wrap_async(
                "get-child-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<Option<Vec<u8>>, String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, child_id.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::GetActorState {
                                actor_id: child_id.parse()?,
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(state)) => Ok((Ok(state),)),
                                    Ok(Err(e)) => Ok((Err(e.to_string()),)),
                                    Err(e) => Ok((Err(format!("Failed to receive state: {}", e)),))
                                }
                            }
                            Err(e) => Ok((Err(format!("Failed to send state request: {}", e)),))
                        }
                    })
                },
            )
            .expect("Failed to wrap get-child-state function");

        // get-child-events implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "get-child-events");
       interface 
            .func_wrap_async(
                "get-child-events",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<ChainEvent>, String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, child_id.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::GetActorEvents {
                                actor_id: child_id.parse()?,
                                response_tx,
                            })
                            .await
                        {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(Ok(events)) => {
                                        let _ = boundary.wrap(&mut ctx, events.clone(), |_| Ok(()));
                                        Ok((Ok(events),))
                                    }
                                    Ok(Err(e)) => Ok((Err(e.to_string()),)),
                                    Err(e) => Ok((Err(format!("Failed to receive events: {}", e)),))
                                }
                            }
                            Err(e) => Ok((Err(format!("Failed to send events request: {}", e)),))
                        }
                    })
                },
            )
            .expect("Failed to wrap get-child-events function");

        info!("Supervisor host functions added");

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding functions for supervisor");
        Ok(())
    }

    pub async fn start(&self, _actor_handle: ActorHandle) -> Result<()> {
        info!("Starting supervisor host");
        Ok(())
    }
}
