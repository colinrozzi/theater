//! # Runtime Handler
//!
//! Provides runtime information and control capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to log messages, get state information, and request shutdown.

use std::future::Future;
use std::pin::Pin;
use tracing::info;
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::config::permissions::RuntimePermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};
use tokio::sync::mpsc::Sender;

/// Handler for providing runtime information and control to WebAssembly actors
#[derive(Clone)]
pub struct RuntimeHandler {
    #[allow(dead_code)]
    config: RuntimeHostConfig,
    theater_tx: Sender<TheaterCommand>,
    #[allow(dead_code)]
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
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
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
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up runtime host functions");

        // Check if the interface is already satisfied by another handler (e.g., ReplayHandler)
        if ctx.is_satisfied("theater:simple/runtime") {
            info!("theater:simple/runtime already satisfied by another handler, skipping import setup");
            return Ok(());
        }

        // The theater:simple/types interface contains only type definitions, no functions.
        // We need to create an empty instance for it so the linker can resolve the import.
        if let Err(e) = actor_component.linker.instance("theater:simple/types") {
            // Types interface has no functions, so if we can't create an instance,
            // it might already exist or not be needed for this component.
            // Log but don't fail - the instantiation will fail later if truly needed.
            info!("Note: Could not create theater:simple/types instance (may not be needed): {}", e);
        }

        let name1 = actor_component.name.clone();
        let name2 = actor_component.name.clone();
        let theater_tx = self.theater_tx.clone();

        let mut interface = match actor_component.linker.instance("theater:simple/runtime") {
            Ok(interface) => interface,
            Err(e) => {
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

                    // Record host function call
                    ctx.data_mut().record_host_function_call(
                        "theater:simple/runtime",
                        "log",
                        &msg,
                        &(),
                    );

                    info!("[ACTOR] [{}] [{}] {}", id, name1, msg);
                    Ok(())
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap log function: {}", e))?;

        // Get chain function - returns the actor's event chain
        // The chain record has: events: list<meta-event>
        // meta-event has: hash: u64, event: event
        // event has: event-type: string, parent: option<u64>, data: list<u8>
        //
        // WIT record -> Rust tuple:
        //   chain { events } -> (list<meta-event>,)
        //   meta-event { hash, event } -> (u64, event)
        //   event { event-type, parent, data } -> (String, Option<u64>, Vec<u8>)
        //
        // So return type is: ((Vec<(u64, (String, Option<u64>, Vec<u8>))>,),)
        // But func_wrap expects result type directly, so we return:
        //   (chain_record,) where chain_record = (events_list,)
        interface
            .func_wrap(
                "get-chain",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> anyhow::Result<((Vec<(u64, (String, Option<u64>, Vec<u8>))>,),)> {
                    // Get all events from the chain
                    let events = ctx.data().get_all_events();

                    // Convert to WIT format: list<meta-event>
                    // meta-event = { hash: u64, event: event }
                    // event = { event-type: string, parent: option<u64>, data: list<u8> }
                    let chain_events: Vec<(u64, (String, Option<u64>, Vec<u8>))> = events
                        .iter()
                        .enumerate()
                        .map(|(i, e)| {
                            // Use a simple hash based on index and event type
                            let hash = i as u64;
                            let parent = if i > 0 { Some((i - 1) as u64) } else { None };
                            let event = (e.event_type.clone(), parent, e.data.clone());
                            (hash, event)
                        })
                        .collect();

                    // Record host function call
                    ctx.data_mut().record_host_function_call(
                        "theater:simple/runtime",
                        "get-chain",
                        &(),
                        &chain_events.len(),
                    );

                    // Return as chain record: (events,)
                    Ok(((chain_events,),))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap get-chain function: {}", e))?;

        // Shutdown function
        interface
            .func_wrap_async(
                "shutdown",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (data,): (Option<Vec<u8>>,)|
                      -> Box<dyn Future<Output = anyhow::Result<(Result<(), String>,)>> + Send> {
                    info!(
                        "[ACTOR] [{}] [{}] Shutdown requested: {:?}",
                        ctx.data().id,
                        name2,
                        data
                    );
                    let theater_tx = theater_tx.clone();
                    let data_clone = data.clone();

                    Box::new(async move {
                        let result = match theater_tx
                            .send(TheaterCommand::ShuttingDown {
                                actor_id: ctx.data().id.clone(),
                                data,
                            })
                            .await
                        {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        };

                        // Record host function call with result
                        ctx.data_mut().record_host_function_call(
                            "theater:simple/runtime",
                            "shutdown",
                            &data_clone,
                            &result.is_ok(),
                        );

                        Ok((result,))
                    })
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap shutdown function: {}", e))?;

        Ok(())
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> anyhow::Result<()> {
        // init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>
        // This is a state-only function - it takes only state and returns state
        actor_instance.register_function_state_only("theater:simple/actor", "init")
    }

    fn name(&self) -> &str {
        "runtime"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec![
            "theater:simple/runtime".to_string(),
            "theater:simple/types".to_string(),
        ])
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/actor".to_string()])
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
        assert_eq!(handler.imports(), Some(vec!["theater:simple/runtime".to_string()]));
        assert_eq!(handler.exports(), Some(vec!["theater:simple/actor".to_string()]));
    }
}
