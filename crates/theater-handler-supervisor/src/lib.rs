//! Theater Supervisor Handler
//!
//! Provides supervisor capabilities for spawning and managing child actors.

pub mod events;


use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::{ActorError, WitActorError};
use theater::config::actor_manifest::SupervisorHostConfig;
use theater::config::permissions::SupervisorPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{ActorResult, TheaterCommand};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};
use theater::ChainEvent;

// Composite integration
use theater::composite_bridge::{
    AsyncCtx, CompositeInstance, HostLinkerBuilder, LinkerError, Value,
};

use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::{error, info};
use wasmtime::StoreContextMut;

/// Errors that can occur during supervisor operations
#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}


/// The SupervisorHandler provides child actor management capabilities.
///
/// This handler enables actors to:
/// - Spawn new child actors
/// - Resume child actors from saved state
/// - List, restart, and stop children
/// - Get child state and event chains
/// - Receive notifications when children error, exit, or are stopped
#[derive(Clone)]
pub struct SupervisorHandler {
    channel_tx: tokio::sync::mpsc::Sender<ActorResult>,
    channel_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<ActorResult>>>>,
    #[allow(dead_code)]
    permissions: Option<SupervisorPermissions>,
}

impl SupervisorHandler {
    /// Create a new SupervisorHandler
    ///
    /// # Arguments
    /// * `config` - Configuration for the supervisor handler
    /// * `permissions` - Optional permission restrictions
    ///
    /// # Returns
    /// The SupervisorHandler (receiver is stored internally)
    pub fn new(
        _config: SupervisorHostConfig,
        permissions: Option<SupervisorPermissions>,
    ) -> Self {
        let (channel_tx, channel_rx) = tokio::sync::mpsc::channel(100);
        Self {
            channel_tx,
            channel_rx: Arc::new(Mutex::new(Some(channel_rx))),
            permissions,
        }
    }

    /// Get a clone of the supervisor channel sender
    ///
    /// This is used by parent actors when spawning children so the supervisor
    /// can receive notifications about child lifecycle events.
    pub fn get_sender(&self) -> tokio::sync::mpsc::Sender<ActorResult> {
        self.channel_tx.clone()
    }

    /// Process child actor results received via the channel
    ///
    /// This should be called in a loop to handle child lifecycle events.
    async fn process_child_result(
        actor_handle: &ActorHandle,
        actor_result: ActorResult,
    ) -> Result<()> {
        info!("Processing child result");

        match actor_result {
            ActorResult::Error(child_error) => {
                actor_handle
                    .call_function::<(String, WitActorError), ()>(
                        "theater:simple/supervisor-handlers.handle-child-error".to_string(),
                        (child_error.actor_id.to_string(), child_error.error.into()),
                    )
                    .await?;
            }
            ActorResult::Success(child_result) => {
                info!("Child result: {:?}", child_result);
                actor_handle
                    .call_function::<(String, Option<Vec<u8>>), ()>(
                        "theater:simple/supervisor-handlers.handle-child-exit".to_string(),
                        (
                            child_result.actor_id.to_string(),
                            child_result.result.into(),
                        ),
                    )
                    .await?;
            }
            ActorResult::ExternalStop(stop_data) => {
                info!("External stop received for actor: {}", stop_data.actor_id);
                actor_handle
                    .call_function::<(String,), ()>(
                        "theater:simple/supervisor-handlers.handle-child-external-stop"
                            .to_string(),
                        (stop_data.actor_id.to_string(),),
                    )
                    .await?;
            }
        }

        Ok(())
    }
}

impl Handler for SupervisorHandler
{
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn name(&self) -> &str {
        "supervisor"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/supervisor".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/supervisor-handlers".to_string()])
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent, _ctx: &mut HandlerContext) -> Result<()> {
        // Record setup start

        info!("Setting up host functions for supervisor");

        let mut interface = match actor_component.linker.instance("theater:simple/supervisor") {
            Ok(interface) => {
                // Record successful linker instance creation
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/supervisor: {}",
                    e
                ));
            }
        };

        let supervisor_tx = self.channel_tx.clone();

        // spawn implementation
        info!("Registering spawn function");
        let _ = interface
            .func_wrap_async(
                "spawn",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (manifest, init_bytes): (String, Option<Vec<u8>>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    // Record spawn child call event

                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();
                    let supervisor_tx = supervisor_tx.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::SpawnActor {
                                manifest_path: manifest,
                                init_bytes,
                                response_tx,
                                parent_id: Some(parent_id),
                                supervisor_tx: Some(supervisor_tx),
                                subscription_tx: None,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(Ok(actor_id)) => {
                                    let actor_id_str = actor_id.to_string();

                                    // Record spawn child result event

                                    Ok((Ok(actor_id_str),))
                                }
                                Ok(Err(e)) => {
                                    // Record spawn child error event

                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record spawn child error event

                                    Ok((Err(format!("Failed to receive response: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record spawn child error event

                                Ok((Err(format!("Failed to send spawn command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap spawn function");

        let supervisor_tx = self.channel_tx.clone();

        // resume implementation
        info!("Registering resume function");
        let _ = interface
            .func_wrap_async(
                "resume",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (manifest, state_bytes): (String, Option<Vec<u8>>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    // Record resume child call event

                    let store = ctx.data_mut();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();
                    let supervisor_tx = supervisor_tx.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        match theater_tx
                            .send(TheaterCommand::ResumeActor {
                                manifest_path: manifest,
                                state_bytes,
                                response_tx,
                                parent_id: Some(parent_id),
                                supervisor_tx: Some(supervisor_tx),
                                subscription_tx: None,
                            })
                            .await
                        {
                            Ok(_) => match response_rx.await {
                                Ok(Ok(actor_id)) => {
                                    let actor_id_str = actor_id.to_string();

                                    // Record resume child result event

                                    Ok((Ok(actor_id_str),))
                                }
                                Ok(Err(e)) => {
                                    // Record resume child error event

                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record resume child error event

                                    Ok((Err(format!("Failed to receive response: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record resume child error event

                                Ok((Err(format!("Failed to send resume command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap resume function");

        // list-children implementation
        info!("Registering list-children function");
        let _ = interface
            .func_wrap_async(
                "list-children",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      ()|
                      -> Box<dyn Future<Output = Result<(Vec<String>,)>> + Send> {
                    // Record list children call event

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
                                    let children_str: Vec<String> =
                                        children.into_iter().map(|id| id.to_string()).collect();

                                    // Record list children result event

                                    Ok((children_str,))
                                }
                                Err(e) => {
                                    // Record list children error event

                                    Err(anyhow::anyhow!("Failed to receive children list: {}", e))
                                }
                            },
                            Err(e) => {
                                // Record list children error event

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
        info!("Registering restart-child function");
        let _ = interface
            .func_wrap_async(
                "restart-child",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record restart child call event

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

                                    Ok((Ok(()),))
                                }
                                Ok(Err(e)) => {
                                    // Record restart child error event

                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record restart child error event

                                    Ok((Err(format!("Failed to receive restart response: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record restart child error event

                                Ok((Err(format!("Failed to send restart command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap restart-child function");

        // stop-child implementation
        info!("Registering stop-child function");
        let _ = interface
            .func_wrap_async(
                "stop-child",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record stop child call event

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

                                    Ok((Ok(()),))
                                }
                                Ok(Err(e)) => {
                                    // Record stop child error event

                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record stop child error event

                                    Ok((Err(format!("Failed to receive stop response: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record stop child error event

                                Ok((Err(format!("Failed to send stop command: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap stop-child function");

        // get-child-state implementation
        info!("Registering get-child-state function");
        let _ = interface
            .func_wrap_async(
                "get-child-state",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<
                    dyn Future<Output = Result<(Result<Option<Vec<u8>>, String>,)>> + Send,
                > {
                    // Record get child state call event

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

                                    Ok((Ok(state),))
                                }
                                Ok(Err(e)) => {
                                    // Record get child state error event

                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record get child state error event

                                    Ok((Err(format!("Failed to receive state: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record get child state error event

                                Ok((Err(format!("Failed to send state request: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap get-child-state function");

        // get-child-events implementation
        info!("Registering get-child-events function");
        let _ = interface
            .func_wrap_async(
                "get-child-events",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (child_id,): (String,)|
                      -> Box<
                    dyn Future<Output = Result<(Result<Vec<ChainEvent>, String>,)>> + Send,
                > {
                    // Record get child events call event

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

                                    Ok((Ok(events),))
                                }
                                Ok(Err(e)) => {
                                    // Record get child events error event

                                    Ok((Err(e.to_string()),))
                                }
                                Err(e) => {
                                    // Record get child events error event

                                    Ok((Err(format!("Failed to receive events: {}", e)),))
                                }
                            },
                            Err(e) => {
                                // Record get child events error event

                                Ok((Err(format!("Failed to send events request: {}", e)),))
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap get-child-events function");

        // Record overall setup completion

        info!("Supervisor host functions added");

        Ok(())
    }

    fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for supervisor");

        // Register handle-child-error callback
        match actor_instance.register_function_no_result::<(String, WitActorError)>(
            "theater:simple/supervisor-handlers",
            "handle-child-error",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-child-error function");
            }
            Err(e) => {
                error!("Failed to register handle-child-error function: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to register handle-child-error function: {}",
                    e
                ));
            }
        }

        // Register handle-child-exit callback
        match actor_instance.register_function_no_result::<(String, Option<Vec<u8>>)>(
            "theater:simple/supervisor-handlers",
            "handle-child-exit",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-child-exit function");
            }
            Err(e) => {
                error!("Failed to register handle-child-exit function: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to register handle-child-exit function: {}",
                    e
                ));
            }
        }

        // Register handle-child-external-stop callback
        match actor_instance.register_function_no_result::<(String,)>(
            "theater:simple/supervisor-handlers",
            "handle-child-external-stop",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-child-external-stop function");
            }
            Err(e) => {
                error!(
                    "Failed to register handle-child-external-stop function: {}",
                    e
                );
                return Err(anyhow::anyhow!(
                    "Failed to register handle-child-external-stop function: {}",
                    e
                ));
            }
        }

        info!("Added all export functions for supervisor");
        Ok(())
    }

    // =========================================================================
    // Composite Integration
    // =========================================================================

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up supervisor host functions (Composite)");

        // Check if already satisfied
        if ctx.is_satisfied("theater:simple/supervisor") {
            info!("theater:simple/supervisor already satisfied by another handler, skipping");
            return Ok(());
        }

        let supervisor_tx = self.channel_tx.clone();

        builder.interface("theater:simple/supervisor")?
            // spawn: func(manifest: string, init-bytes: option<list<u8>>) -> result<string, string>
            .func_async_result("spawn", {
                let supervisor_tx = supervisor_tx.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let supervisor_tx = supervisor_tx.clone();
                    async move {
                        // Parse input: (string, option<list<u8>>)
                        let (manifest, init_bytes) = match input {
                            Value::Tuple(args) if args.len() == 2 => {
                                let manifest = match &args[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Err(Value::String("Invalid manifest argument".to_string())),
                                };
                                let init_bytes = match &args[1] {
                                    Value::Option { value: Some(inner), .. } => {
                                        if let Value::List { items, .. } = inner.as_ref() {
                                            Some(items.iter().filter_map(|v| {
                                                if let Value::U8(b) = v { Some(*b) } else { None }
                                            }).collect::<Vec<u8>>())
                                        } else {
                                            None
                                        }
                                    }
                                    Value::Option { value: None, .. } => None,
                                    _ => None,
                                };
                                (manifest, init_bytes)
                            }
                            _ => return Err(Value::String("Invalid spawn arguments".to_string())),
                        };

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();
                        let parent_id = store.id.clone();

                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::SpawnActor {
                            manifest_path: manifest,
                            init_bytes,
                            response_tx,
                            parent_id: Some(parent_id),
                            supervisor_tx: Some(supervisor_tx),
                            subscription_tx: None,
                        };

                        if let Err(e) = theater_tx.send(cmd).await {
                            return Err(Value::String(format!("Failed to send spawn command: {}", e)));
                        }

                        match response_rx.await {
                            Ok(Ok(actor_id)) => Ok(Value::String(actor_id.to_string())),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive response: {}", e))),
                        }
                    }
                }
            })?
            // resume: func(manifest: string, state-bytes: option<list<u8>>) -> result<string, string>
            .func_async_result("resume", {
                let supervisor_tx = supervisor_tx.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let supervisor_tx = supervisor_tx.clone();
                    async move {
                        let (manifest, state_bytes) = match input {
                            Value::Tuple(args) if args.len() == 2 => {
                                let manifest = match &args[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Err(Value::String("Invalid manifest argument".to_string())),
                                };
                                let state_bytes = match &args[1] {
                                    Value::Option { value: Some(inner), .. } => {
                                        if let Value::List { items, .. } = inner.as_ref() {
                                            Some(items.iter().filter_map(|v| {
                                                if let Value::U8(b) = v { Some(*b) } else { None }
                                            }).collect::<Vec<u8>>())
                                        } else {
                                            None
                                        }
                                    }
                                    Value::Option { value: None, .. } => None,
                                    _ => None,
                                };
                                (manifest, state_bytes)
                            }
                            _ => return Err(Value::String("Invalid resume arguments".to_string())),
                        };

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();
                        let parent_id = store.id.clone();

                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::ResumeActor {
                            manifest_path: manifest,
                            state_bytes,
                            response_tx,
                            parent_id: Some(parent_id),
                            supervisor_tx: Some(supervisor_tx),
                            subscription_tx: None,
                        };

                        if let Err(e) = theater_tx.send(cmd).await {
                            return Err(Value::String(format!("Failed to send resume command: {}", e)));
                        }

                        match response_rx.await {
                            Ok(Ok(actor_id)) => Ok(Value::String(actor_id.to_string())),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive response: {}", e))),
                        }
                    }
                }
            })?
            // list-children: func() -> list<string>
            .func_async_result("list-children", move |ctx: AsyncCtx<ActorStore>, _input: Value| {
                async move {
                    let store = ctx.data();
                    let theater_tx = store.theater_tx.clone();
                    let parent_id = store.id.clone();

                    let (response_tx, response_rx) = oneshot::channel();
                    let cmd = TheaterCommand::ListChildren {
                        parent_id,
                        response_tx,
                    };

                    if let Err(e) = theater_tx.send(cmd).await {
                        return Err(Value::String(format!("Failed to send list-children command: {}", e)));
                    }

                    match response_rx.await {
                        Ok(children) => {
                            use theater::ValueType;
                            let children_values: Vec<Value> = children
                                .into_iter()
                                .map(|id| Value::String(id.to_string()))
                                .collect();
                            Ok(Value::List {
                                elem_type: ValueType::String,
                                items: children_values,
                            })
                        }
                        Err(e) => Err(Value::String(format!("Failed to receive children list: {}", e))),
                    }
                }
            })?
            // restart-child: func(child-id: string) -> result<(), string>
            .func_async_result("restart-child", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let child_id_str = match input {
                        Value::String(s) => s,
                        Value::Tuple(args) if args.len() == 1 => {
                            match &args[0] {
                                Value::String(s) => s.clone(),
                                _ => return Err(Value::String("Invalid child-id argument".to_string())),
                            }
                        }
                        _ => return Err(Value::String("Invalid restart-child argument".to_string())),
                    };

                    let child_id = match child_id_str.parse() {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Invalid child ID: {}", e))),
                    };

                    let store = ctx.data();
                    let theater_tx = store.theater_tx.clone();

                    let (response_tx, response_rx) = oneshot::channel();
                    let cmd = TheaterCommand::RestartActor {
                        actor_id: child_id,
                        response_tx,
                    };

                    if let Err(e) = theater_tx.send(cmd).await {
                        return Err(Value::String(format!("Failed to send restart command: {}", e)));
                    }

                    match response_rx.await {
                        Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(format!("Failed to receive restart response: {}", e))),
                    }
                }
            })?
            // stop-child: func(child-id: string) -> result<(), string>
            .func_async_result("stop-child", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let child_id_str = match input {
                        Value::String(s) => s,
                        Value::Tuple(args) if args.len() == 1 => {
                            match &args[0] {
                                Value::String(s) => s.clone(),
                                _ => return Err(Value::String("Invalid child-id argument".to_string())),
                            }
                        }
                        _ => return Err(Value::String("Invalid stop-child argument".to_string())),
                    };

                    let child_id = match child_id_str.parse() {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Invalid child ID: {}", e))),
                    };

                    let store = ctx.data();
                    let theater_tx = store.theater_tx.clone();

                    let (response_tx, response_rx) = oneshot::channel();
                    let cmd = TheaterCommand::StopActor {
                        actor_id: child_id,
                        response_tx,
                    };

                    if let Err(e) = theater_tx.send(cmd).await {
                        return Err(Value::String(format!("Failed to send stop command: {}", e)));
                    }

                    match response_rx.await {
                        Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(format!("Failed to receive stop response: {}", e))),
                    }
                }
            })?
            // get-child-state: func(child-id: string) -> result<option<list<u8>>, string>
            .func_async_result("get-child-state", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let child_id_str = match input {
                        Value::String(s) => s,
                        Value::Tuple(args) if args.len() == 1 => {
                            match &args[0] {
                                Value::String(s) => s.clone(),
                                _ => return Err(Value::String("Invalid child-id argument".to_string())),
                            }
                        }
                        _ => return Err(Value::String("Invalid get-child-state argument".to_string())),
                    };

                    let child_id = match child_id_str.parse() {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Invalid child ID: {}", e))),
                    };

                    let store = ctx.data();
                    let theater_tx = store.theater_tx.clone();

                    let (response_tx, response_rx) = oneshot::channel();
                    let cmd = TheaterCommand::GetActorState {
                        actor_id: child_id,
                        response_tx,
                    };

                    if let Err(e) = theater_tx.send(cmd).await {
                        return Err(Value::String(format!("Failed to send get-state command: {}", e)));
                    }

                    match response_rx.await {
                        Ok(Ok(state)) => {
                            use theater::ValueType;
                            let state_value = match state {
                                Some(bytes) => Value::Option {
                                    inner_type: ValueType::List(Box::new(ValueType::U8)),
                                    value: Some(Box::new(Value::List {
                                        elem_type: ValueType::U8,
                                        items: bytes.into_iter().map(Value::U8).collect(),
                                    })),
                                },
                                None => Value::Option {
                                    inner_type: ValueType::List(Box::new(ValueType::U8)),
                                    value: None,
                                },
                            };
                            Ok(state_value)
                        }
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(format!("Failed to receive state: {}", e))),
                    }
                }
            })?
            // get-child-events: func(child-id: string) -> result<list<chain-event>, string>
            .func_async_result("get-child-events", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let child_id_str = match input {
                        Value::String(s) => s,
                        Value::Tuple(args) if args.len() == 1 => {
                            match &args[0] {
                                Value::String(s) => s.clone(),
                                _ => return Err(Value::String("Invalid child-id argument".to_string())),
                            }
                        }
                        _ => return Err(Value::String("Invalid get-child-events argument".to_string())),
                    };

                    let child_id = match child_id_str.parse() {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Invalid child ID: {}", e))),
                    };

                    let store = ctx.data();
                    let theater_tx = store.theater_tx.clone();

                    let (response_tx, response_rx) = oneshot::channel();
                    let cmd = TheaterCommand::GetActorEvents {
                        actor_id: child_id,
                        response_tx,
                    };

                    if let Err(e) = theater_tx.send(cmd).await {
                        return Err(Value::String(format!("Failed to send get-events command: {}", e)));
                    }

                    match response_rx.await {
                        Ok(Ok(events)) => {
                            use theater::ValueType;
                            // Convert ChainEvents to Value list
                            let events_values: Vec<Value> = events
                                .iter()
                                .map(|e| {
                                    // ChainEvent as a record: { event-type: string, data: list<u8> }
                                    Value::Tuple(vec![
                                        Value::String(e.event_type.clone()),
                                        Value::List {
                                            elem_type: ValueType::U8,
                                            items: e.data.iter().map(|b| Value::U8(*b)).collect(),
                                        },
                                    ])
                                })
                                .collect();
                            Ok(Value::List {
                                elem_type: ValueType::Tuple(vec![ValueType::String, ValueType::List(Box::new(ValueType::U8))]),
                                items: events_values,
                            })
                        }
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(format!("Failed to receive events: {}", e))),
                    }
                }
            })?;

        ctx.mark_satisfied("theater:simple/supervisor");
        Ok(())
    }

    fn register_exports_composite(&self, instance: &mut CompositeInstance) -> Result<()> {
        // Register supervisor callback export functions
        instance.register_export("theater:simple/supervisor-handlers", "handle-child-error");
        instance.register_export("theater:simple/supervisor-handlers", "handle-child-exit");
        instance.register_export("theater:simple/supervisor-handlers", "handle-child-external-stop");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Starting supervisor handler");

        // Take the receiver out of the Arc<Mutex<Option<>>>
        let channel_rx_opt = self.channel_rx.lock().unwrap().take();

        Box::pin(async move {
            // If we don't have a receiver (e.g., this is a cloned instance), just return Ok
            let Some(mut channel_rx) = channel_rx_opt else {
                info!("Supervisor handler has no receiver (cloned instance), not starting");
                return Ok(());
            };

            loop {
                tokio::select! {
                    Some(child_result) = channel_rx.recv() => {
                        if let Err(e) = Self::process_child_result(&actor_handle, child_result).await {
                            error!("Error processing child result: {}", e);
                        }
                    }
                    _ = &mut shutdown_receiver.receiver => {
                        info!("Shutdown signal received");
                        break;
                    }
                }
            }
            info!("Supervisor handler shut down complete");
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::SupervisorHostConfig;

    #[test]
    fn test_supervisor_handler_creation() {
        let config = SupervisorHostConfig {};
        let handler = SupervisorHandler::new(config, None);
        assert_eq!(handler.name(), "supervisor");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/supervisor".to_string()])
        );
        assert_eq!(
            handler.exports(),
            Some(vec!["theater:simple/supervisor-handlers".to_string()])
        );
    }

    #[test]
    fn test_supervisor_handler_clone() {
        let config = SupervisorHostConfig {};
        let handler = SupervisorHandler::new(config, None);
        let cloned = handler.create_instance(None);
        assert_eq!(cloned.name(), "supervisor");
    }
}
