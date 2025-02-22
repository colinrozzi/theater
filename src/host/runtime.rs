use crate::actor_handle::ActorHandle;
use crate::config::RuntimeHostConfig;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::store::ActorStore;
use anyhow::Result;
use tracing::info;
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

        let boundary = HostFunctionBoundary::new("ntwk:theater/runtime", "get-state");
        runtime
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

