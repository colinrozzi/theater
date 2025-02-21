use crate::actor_handle::ActorHandle;
use crate::config::RuntimeHostConfig;
use crate::messages::TheaterCommand;
use crate::store::ActorStore;
use crate::host::host_wrapper::HostFunctionBoundary;
use anyhow::Result;
use std::future::Future;
use std::path::PathBuf;
use tracing::{error, info};
use wasmtime::StoreContextMut;

#[derive(Clone)]
pub struct RuntimeHost {
    actor_handle: ActorHandle,
}

impl RuntimeHost {
    pub fn new(_config: RuntimeHostConfig, actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for runtime");
        let mut actor = self.actor_handle.inner().lock().await;
        let name = actor.name.clone();
        let mut runtime = actor
            .linker
            .instance("ntwk:theater/runtime")
            .expect("Failed to get runtime instance");

        let boundary = HostFunctionBoundary::new("ntwk:theater/runtime", "log");
        runtime
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

        let boundary = HostFunctionBoundary::new("ntwk:theater/runtime", "spawn");
        runtime
            .func_wrap_async(
                "spawn",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                     (manifest,): (String,)|
                     -> Box<dyn Future<Output = Result<(String,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let boundary = boundary.clone();
                    info!("Spawning actor with manifest: {}", manifest);
                    
                    Box::new(async move {
                        // Record spawn request
                        let _ = boundary.wrap(&mut ctx, manifest.clone(), |_| Ok(()));
                        
                        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                        info!("sending spawn command");
                        match theater_tx
                            .send(TheaterCommand::SpawnActor {
                                manifest_path: PathBuf::from(manifest),
                                response_tx,
                                parent_id: Some(ctx.data().id.clone()),
                            })
                            .await
                        {
                            Ok(_) => {
                                // await the Ok(actor_id) response on the response channel
                                let actor_id =
                                    response_rx.await.expect("Failed to get actor id").unwrap();
                                info!("spawned actor with id: {}", actor_id);
                                
                                // Record successful spawn
                                let actor_id_str = actor_id.to_string();
                                let _ = boundary.wrap(&mut ctx, actor_id_str.clone(), |_| Ok(()));
                                Ok((actor_id_str,))
                            }
                            Err(e) => {
                                error!("Failed to send spawn command: {}", e);
                                // Record spawn failure
                                let err = format!("Failed to send spawn command: {}", e);
                                let _ = boundary.wrap(&mut ctx, err.clone(), |_| Ok(()));
                                Err(anyhow::anyhow!(err))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap spawn function");

        let boundary = HostFunctionBoundary::new("ntwk:theater/runtime", "get-state");
        runtime
            .func_wrap(
                "get-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(Vec<u8>,)> {
                    // Record the state request
                    let _ = boundary.wrap(&mut ctx, "state_request", |_| Ok(()));
                    
                    // Return current state
                    let state = ctx.data().get_last_event()
                        .map(|e| e.data.clone())
                        .unwrap_or_default();
                    
                    // Record the response
                    let _ = boundary.wrap(&mut ctx, state.clone(), |_| Ok(()));
                    
                    Ok((state,))
                },
            )
            .expect("Failed to wrap get-state function");

        info!("Runtime host functions added");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        let mut actor = self.actor_handle.inner().lock().await;
        let init_export = actor
            .find_export("ntwk:theater/actor", "init")
            .expect("Failed to find init export");
        actor.exports.insert("init".to_string(), init_export);
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        Ok(())
    }
}