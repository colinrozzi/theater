use crate::actor_handle::ActorHandle;
use crate::chain::ChainEvent;
use crate::config::SupervisorHostConfig;
use crate::messages::TheaterCommand;
use crate::store::ActorStore;
use crate::host::host_wrapper::HostFunctionBoundary;
use anyhow::Result;
use std::future::Future;
use std::path::PathBuf;
use tracing::info;
use wasmtime::StoreContextMut;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct SupervisorHost {
    actor_handle: ActorHandle,
}

impl SupervisorHost {
    pub fn new(_config: SupervisorHostConfig, actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for supervisor");
        let mut actor = self.actor_handle.inner().lock().await;
        let mut supervisor = actor
            .linker
            .instance("ntwk:theater/supervisor")
            .expect("Failed to get supervisor instance");

        // spawn-child implementation
        let boundary = HostFunctionBoundary::new("ntwk:theater/supervisor", "spawn");
        let _ = supervisor
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
                                manifest_path: PathBuf::from(manifest),
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
        supervisor
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
        supervisor
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
        supervisor
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
        supervisor
            .func_wrap_async(
                "get-child-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();

                    Box::new(async move {
                        let _ = boundary.wrap(&mut ctx, child_id.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::GetChildState {
                                child_id: child_id.parse()?,
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
        supervisor
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
                            .send(TheaterCommand::GetChildEvents {
                                child_id: child_id.parse()?,
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

    pub async fn add_exports(&self) -> Result<()> {
        // No exports needed for supervisor
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        // No startup needed for supervisor
        Ok(())
    }
}
