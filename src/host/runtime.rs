use crate::actor_handle::ActorHandle;
use crate::config::RuntimeHostConfig;
use crate::messages::TheaterCommand;
use crate::store::ActorStore;
use anyhow::Result;
use std::future::Future;
use std::path::PathBuf;
use tracing::{error, info};
use wasmtime::chain::Chain;
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

        runtime
            .func_wrap(
                "log",
                move |ctx: wasmtime::StoreContextMut<'_, ActorStore>, (msg,): (String,)| {
                    let id = ctx.data().id.clone();
                    info!("[ACTOR] [{}] [{}] {}", id, name, msg);
                    Ok(())
                },
            )
            .expect("Failed to wrap log function");

        runtime
            .func_wrap_async(
                "spawn",
                |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                 (manifest,): (String,)|
                 -> Box<dyn Future<Output = Result<(String,)>> + Send> {
                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    info!("Spawning actor with manifest: {}", manifest);
                    Box::new(async move {
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
                                Ok((actor_id.to_string(),))
                            }
                            Err(e) => {
                                error!("Failed to send spawn command: {}", e);
                                Err(anyhow::anyhow!("Failed to send spawn command"))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap spawn function");

        runtime
            .func_wrap(
                "get-chain",
                |ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(Chain,)> {
                    let chain = ctx.chain();
                    Ok((chain.clone(),))
                },
            )
            .expect("Failed to wrap get-chain function");

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
