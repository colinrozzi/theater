//! # Runtime Handler
//!
//! Provides runtime information and control capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to log messages, get state information, and request shutdown.

use std::future::Future;
use std::pin::Pin;
use tracing::info;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::config::permissions::RuntimePermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use tokio::sync::mpsc::Sender;

// Pack integration
use theater::pack_bridge::{
    AsyncCtx, PackInstance, Ctx, HostLinkerBuilder, LinkerError, Value,
};

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

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up runtime host functions (Pack)");

        // Check if the interface is already satisfied by another handler
        if ctx.is_satisfied("theater:simple/runtime") {
            info!("theater:simple/runtime already satisfied by another handler, skipping");
            return Ok(());
        }

        let theater_tx = self.theater_tx.clone();

        builder
            .interface("theater:simple/runtime")?
            // Log function: log(msg: string)
            .func_typed("log", |ctx: &mut Ctx<'_, ActorStore>, input: Value| {
                // Extract string from Value
                let msg = match input {
                    Value::String(s) => s,
                    _ => format!("{:?}", input),
                };

                let store = ctx.data();
                let id = store.id.clone();

                info!("[ACTOR] [{}] {}", id, msg);

                // Return unit (empty tuple)
                Value::Tuple(vec![])
            })?
            // Get chain function: get-chain() -> chain
            .func_typed(
                "get-chain",
                |ctx: &mut Ctx<'_, ActorStore>, _input: Value| {
                    let store = ctx.data();
                    let events = store.get_all_events();

                    // Convert to WIT format: list<meta-event>
                    use theater::ValueType;
                    // meta-event = { hash: u64, event: event }
                    // event = { event-type: string, parent: option<u64>, data: list<u8> }
                    let chain_events: Vec<Value> = events
                        .iter()
                        .enumerate()
                        .map(|(i, e)| {
                            let hash = Value::U64(i as u64);
                            let parent = if i > 0 {
                                Value::Option {
                                    inner_type: ValueType::U64,
                                    value: Some(Box::new(Value::U64((i - 1) as u64))),
                                }
                            } else {
                                Value::Option {
                                    inner_type: ValueType::U64,
                                    value: None,
                                }
                            };
                            let event_type = Value::String(e.event_type.clone());
                            let data = Value::List {
                                elem_type: ValueType::U8,
                                items: e.data.iter().map(|b| Value::U8(*b)).collect(),
                            };

                            // meta-event record: (hash, (event-type, parent, data))
                            Value::Tuple(vec![
                                hash,
                                Value::Tuple(vec![event_type, parent, data]),
                            ])
                        })
                        .collect();

                    // Return as chain record: (events,)
                    Value::Tuple(vec![Value::List {
                        elem_type: ValueType::Tuple(vec![ValueType::U64, ValueType::Tuple(vec![ValueType::String, ValueType::Option(Box::new(ValueType::U64)), ValueType::List(Box::new(ValueType::U8))])]),
                        items: chain_events,
                    }])
                },
            )?
            // Shutdown function: shutdown(data: option<list<u8>>) -> result<(), string>
            .func_async_result(
                "shutdown",
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = theater_tx.clone();

                    async move {
                        // Parse input: option<list<u8>>
                        let data: Option<Vec<u8>> = match input {
                            Value::Option { value: Some(inner), .. } => match *inner {
                                Value::List { items, .. } => {
                                    let result: Result<Vec<u8>, _> = items
                                        .into_iter()
                                        .map(|v| match v {
                                            Value::U8(b) => Ok(b),
                                            _ => Err("expected u8"),
                                        })
                                        .collect();
                                    result.ok()
                                }
                                _ => None,
                            },
                            Value::Option { value: None, .. } => None,
                            _ => None,
                        };

                        let store = ctx.data();
                        let actor_id = store.id.clone();

                        info!("[ACTOR] [{}] Shutdown requested: {:?}", actor_id, data);

                        // Send shutdown command
                        match theater_tx
                            .send(TheaterCommand::ShuttingDown {
                                actor_id,
                                data,
                            })
                            .await
                        {
                            Ok(_) => Ok(Value::Tuple(vec![])),
                            Err(e) => Err(Value::String(e.to_string())),
                        }
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/runtime");
        Ok(())
    }

    fn register_exports_composite(&self, instance: &mut PackInstance) -> anyhow::Result<()> {
        // Register the init export function
        instance.register_export("theater:simple/actor", "init");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
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
        assert_eq!(handler.imports(), Some(vec![
            "theater:simple/runtime".to_string(),
            "theater:simple/types".to_string(),
        ]));
        assert_eq!(handler.exports(), Some(vec!["theater:simple/actor".to_string()]));
    }
}
