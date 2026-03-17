//! Theater Supervisor Handler
//!
//! Provides supervisor capabilities for spawning and managing child actors.

pub mod events;


use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::chain::ChainEvent;
use theater::config::actor_manifest::SupervisorHostConfig;
use theater::config::permissions::SupervisorPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{ActorResult, TheaterCommand};
use theater::shutdown::ShutdownReceiver;
use theater::utils::resolve_reference;
use theater::ManifestConfig;

// Pack integration
use theater::pack_bridge::{
    AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
    parse_pact,
};

use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use theater::id::TheaterId;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded supervisor.pact file content
const SUPERVISOR_PACT: &str = include_str!("../../../pact/supervisor.pact");

/// Declare the theater:simple/supervisor interface from the pact file.
///
/// Functions for spawning and managing child actors:
/// - spawn(manifest: string, init-bytes: option<list<u8>>, wasm-bytes: option<list<u8>>) -> result<string, string>
/// - spawn-and-wait(manifest: string, init-bytes: option<list<u8>>, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, string>
/// - resume(manifest: string, state-bytes: option<list<u8>>, wasm-bytes: option<list<u8>>) -> result<string, string>
/// - list-children() -> list<string>
/// - restart-child(child-id: string) -> result<_, string>
/// - stop-child(child-id: string) -> result<_, string>
/// - get-child-state(child-id: string) -> result<option<list<u8>>, string>
/// - get-child-events(child-id: string) -> result<list<list<u8>>, string>
///
/// Note: chain-event is approximated as list<u8> for interface hashing.
fn supervisor_interface() -> InterfaceImpl {
    let pact = parse_pact(SUPERVISOR_PACT)
        .expect("embedded supervisor.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

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
/// - Subscribe to all events from children (via subscription_tx)
/// - Clean up children on shutdown
#[derive(Clone)]
pub struct SupervisorHandler {
    /// Channel for receiving lifecycle events (ActorResult) from children
    channel_tx: tokio::sync::mpsc::Sender<ActorResult>,
    channel_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<ActorResult>>>>,
    /// Channel for receiving all ChainEvents from children (via subscription_tx)
    event_tx: tokio::sync::mpsc::Sender<Result<ChainEvent, ActorError>>,
    event_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<Result<ChainEvent, ActorError>>>>>,
    /// Set of currently active child actor IDs
    children: Arc<Mutex<HashSet<TheaterId>>>,
    /// Theater command sender for stopping children on shutdown
    theater_tx: Arc<Mutex<Option<mpsc::Sender<TheaterCommand>>>>,
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
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(1024);
        Self {
            channel_tx,
            channel_rx: Arc::new(Mutex::new(Some(channel_rx))),
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            children: Arc::new(Mutex::new(HashSet::new())),
            theater_tx: Arc::new(Mutex::new(None)),
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

    /// Get a clone of the event subscription sender
    ///
    /// This is used when spawning children to subscribe to all their events.
    /// The supervisor will receive every ChainEvent from its children.
    pub fn get_event_sender(&self) -> tokio::sync::mpsc::Sender<Result<ChainEvent, ActorError>> {
        self.event_tx.clone()
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![supervisor_interface()]
    }

    /// Process child actor results received via the channel
    ///
    /// This should be called in a loop to handle child lifecycle events.
    /// Also removes the child from tracking when it exits.
    async fn process_child_result(
        actor_handle: &ActorHandle,
        actor_result: ActorResult,
        children: &Arc<Mutex<HashSet<TheaterId>>>,
    ) -> Result<()> {
        info!("Processing child result");

        // Get the child ID for tracking removal
        let child_id = match &actor_result {
            ActorResult::Error(e) => e.actor_id.clone(),
            ActorResult::Success(r) => r.actor_id.clone(),
            ActorResult::ExternalStop(s) => s.actor_id.clone(),
        };

        // Remove from tracking
        {
            let mut children_guard = children.lock().unwrap();
            if children_guard.remove(&child_id) {
                debug!("Removed child {} from tracking, remaining: {}", child_id, children_guard.len());
            }
        }

        match actor_result {
            ActorResult::Error(child_error) => {
                // handle-child-error(state, params: tuple<string, wit-actor-error>)
                let params = Value::Tuple(vec![
                    Value::String(child_error.actor_id.to_string()),
                    actor_error_to_value(child_error.error),
                ]);
                actor_handle
                    .call_function(
                        "theater:simple/supervisor-handlers.handle-child-error".to_string(),
                        params,
                    )
                    .await?;
            }
            ActorResult::Success(child_result) => {
                // handle-child-exit(state, params: tuple<string, option<list<u8>>>)
                info!("Child result: {:?}", child_result);
                let params = Value::Tuple(vec![
                    Value::String(child_result.actor_id.to_string()),
                    option_bytes_to_value(child_result.result.into()),
                ]);
                actor_handle
                    .call_function(
                        "theater:simple/supervisor-handlers.handle-child-exit".to_string(),
                        params,
                    )
                    .await?;
            }
            ActorResult::ExternalStop(stop_data) => {
                // handle-child-external-stop(state, params: tuple<string>)
                info!("External stop received for actor: {}", stop_data.actor_id);
                let params = Value::Tuple(vec![
                    Value::String(stop_data.actor_id.to_string()),
                ]);
                actor_handle
                    .call_function(
                        "theater:simple/supervisor-handlers.handle-child-external-stop"
                            .to_string(),
                        params,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Process a chain event received from a child actor.
    ///
    /// This is called for every event a child records, allowing the supervisor
    /// to monitor child activity, implement logging, or trigger reactions.
    async fn process_child_event(
        actor_handle: &ActorHandle,
        event_result: Result<ChainEvent, ActorError>,
    ) -> Result<()> {
        match event_result {
            Ok(event) => {
                debug!(
                    "Supervisor received child event: type={}, data_len={}",
                    event.event_type,
                    event.data.len()
                );

                // Call the actor's handle-child-event export if it exists
                // The actor can implement this to react to child events
                let params = Value::Tuple(vec![
                    Value::String(event.event_type.clone()),
                    Value::List {
                        elem_type: ValueType::U8,
                        items: event.data.iter().map(|b| Value::U8(*b)).collect(),
                    },
                ]);

                // Try to call handle-child-event, but don't fail if it doesn't exist
                match actor_handle
                    .call_function(
                        "theater:simple/supervisor-handlers.handle-child-event".to_string(),
                        params,
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        // Only log at debug level since this export is optional
                        debug!(
                            "Could not call handle-child-event (may not be implemented): {}",
                            e
                        );
                    }
                }
            }
            Err(e) => {
                error!("Supervisor received error from child subscription: {}", e);
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
        Some(self.interfaces().iter().map(|i| i.name().to_string()).collect())
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/supervisor-handlers".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<theater::pack_bridge::InterfaceImpl> {
        vec![supervisor_interface()]
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up supervisor host functions (Pack)");

        // Check if already satisfied
        if ctx.is_satisfied("theater:simple/supervisor") {
            info!("theater:simple/supervisor already satisfied by another handler, skipping");
            return Ok(());
        }

        let supervisor_tx = self.channel_tx.clone();
        let event_tx = self.event_tx.clone();
        let children = self.children.clone();
        let theater_tx_holder = self.theater_tx.clone();

        builder.interface("theater:simple/supervisor")?
            // spawn: func(manifest: string, wasm-bytes: option<list<u8>>) -> result<string, string>
            // Spawns a child actor. If wasm-bytes is provided, uses those bytes instead of loading from manifest.package.
            .func_async_result("spawn", {
                let supervisor_tx = supervisor_tx.clone();
                let event_tx = event_tx.clone();
                let children = children.clone();
                let theater_tx_holder = theater_tx_holder.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let supervisor_tx = supervisor_tx.clone();
                    let event_tx = event_tx.clone();
                    let children = children.clone();
                    let theater_tx_holder = theater_tx_holder.clone();
                    async move {
                        // Parse input: (string, option<list<u8>>, option<list<u8>>)
                        // init-bytes is passed to init, wasm-bytes is used if provided
                        let (manifest_path, _init_bytes, provided_wasm_bytes) = match input {
                            Value::Tuple(args) if args.len() == 3 => {
                                let manifest = match &args[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Err(Value::String("Invalid manifest argument".to_string())),
                                };
                                let init_bytes = parse_optional_bytes(&args[1]);
                                let wasm_bytes = parse_optional_bytes(&args[2]);
                                (manifest, init_bytes, wasm_bytes)
                            }
                            _ => return Err(Value::String("Invalid spawn arguments: expected (string, option<list<u8>>, option<list<u8>>)".to_string())),
                        };

                        if let Some(ref bytes) = provided_wasm_bytes {
                            debug!("spawn: manifest={}, wasm_bytes={} bytes", manifest_path, bytes.len());
                        } else {
                            debug!("spawn: manifest={}, wasm_bytes=None (will load from manifest.package)", manifest_path);
                        }

                        // Load and parse manifest
                        let manifest_str = match resolve_reference(&manifest_path).await {
                            Ok(bytes) => match String::from_utf8(bytes) {
                                Ok(s) => s,
                                Err(e) => return Err(Value::String(format!("Invalid manifest encoding: {}", e))),
                            },
                            Err(e) => return Err(Value::String(format!("Failed to load manifest: {}", e))),
                        };

                        let manifest = match ManifestConfig::from_toml_str(&manifest_str) {
                            Ok(m) => m,
                            Err(e) => return Err(Value::String(format!("Failed to parse manifest: {}", e))),
                        };

                        // Resolve wasm bytes
                        let wasm_bytes = match provided_wasm_bytes {
                            Some(bytes) => bytes,
                            None => {
                                match resolve_reference(&manifest.package).await {
                                    Ok(bytes) => bytes,
                                    Err(e) => return Err(Value::String(format!("Failed to load WASM: {}", e))),
                                }
                            }
                        };

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();
                        let name = Some(manifest.name.clone());
                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::SpawnActor {
                            wasm_bytes,
                            name,
                            manifest: Some(manifest),
                            response_tx,
                            supervisor_tx: Some(supervisor_tx),
                            subscription_tx: Some(event_tx),
                        };

                        if let Err(e) = theater_tx.send(cmd).await {
                            return Err(Value::String(format!("Failed to send spawn command: {}", e)));
                        }

                        // Store theater_tx for shutdown (first spawn stores it)
                        {
                            let mut holder = theater_tx_holder.lock().unwrap();
                            if holder.is_none() {
                                *holder = Some(theater_tx);
                            }
                        }

                        match response_rx.await {
                            Ok(Ok(actor_id)) => {
                                // Track the child
                                {
                                    let mut children_guard = children.lock().unwrap();
                                    children_guard.insert(actor_id.clone());
                                    debug!("Tracking child {}, total children: {}", actor_id, children_guard.len());
                                }
                                Ok(Value::String(actor_id.to_string()))
                            }
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive response: {}", e))),
                        }
                    }
                }
            })?
            // spawn-and-wait: func(manifest: string, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, string>
            // Spawns a child actor and waits for it to complete. Returns the child's final result.
            // If timeout-ms is provided, returns an error if the child doesn't complete within that time.
            .func_async_result("spawn-and-wait", {
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    async move {
                        // Parse input: (string, option<list<u8>>, option<u64>)
                        let (manifest_path, provided_wasm_bytes, timeout_ms) = match input {
                            Value::Tuple(args) if args.len() == 3 => {
                                let manifest = match &args[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Err(Value::String("Invalid manifest argument".to_string())),
                                };
                                let wasm_bytes = parse_optional_bytes(&args[1]);
                                let timeout_ms = parse_optional_u64(&args[2]);
                                (manifest, wasm_bytes, timeout_ms)
                            }
                            _ => return Err(Value::String("Invalid spawn-and-wait arguments: expected (string, option<list<u8>>, option<u64>)".to_string())),
                        };

                        debug!("spawn-and-wait: manifest={}, timeout={:?}ms", manifest_path, timeout_ms);

                        // Load and parse manifest
                        let manifest_str = match resolve_reference(&manifest_path).await {
                            Ok(bytes) => match String::from_utf8(bytes) {
                                Ok(s) => s,
                                Err(e) => return Err(Value::String(format!("Invalid manifest encoding: {}", e))),
                            },
                            Err(e) => return Err(Value::String(format!("Failed to load manifest: {}", e))),
                        };

                        let manifest = match ManifestConfig::from_toml_str(&manifest_str) {
                            Ok(m) => m,
                            Err(e) => return Err(Value::String(format!("Failed to parse manifest: {}", e))),
                        };

                        // Resolve wasm bytes
                        let wasm_bytes = match provided_wasm_bytes {
                            Some(bytes) => bytes,
                            None => {
                                match resolve_reference(&manifest.package).await {
                                    Ok(bytes) => bytes,
                                    Err(e) => return Err(Value::String(format!("Failed to load WASM: {}", e))),
                                }
                            }
                        };

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();

                        // Create a dedicated channel for this spawn to receive the child's result
                        let (result_tx, mut result_rx) = mpsc::channel::<ActorResult>(1);

                        let name = Some(manifest.name.clone());
                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::SpawnActor {
                            wasm_bytes,
                            name,
                            manifest: Some(manifest),
                            response_tx,
                            supervisor_tx: Some(result_tx),
                            subscription_tx: None,
                        };

                        if let Err(e) = theater_tx.send(cmd).await {
                            return Err(Value::String(format!("Failed to send spawn command: {}", e)));
                        }

                        // Wait for the actor to spawn
                        let actor_id = match response_rx.await {
                            Ok(Ok(id)) => id,
                            Ok(Err(e)) => return Err(Value::String(format!("Failed to spawn actor: {}", e))),
                            Err(e) => return Err(Value::String(format!("Failed to receive spawn response: {}", e))),
                        };

                        debug!("spawn-and-wait: child {} spawned, waiting for completion", actor_id);

                        // Wait for the child to complete
                        let wait_result = if let Some(ms) = timeout_ms {
                            tokio::time::timeout(Duration::from_millis(ms), result_rx.recv()).await
                        } else {
                            // No timeout - wait indefinitely
                            Ok(result_rx.recv().await)
                        };

                        match wait_result {
                            Ok(Some(ActorResult::Success(child_result))) => {
                                debug!("spawn-and-wait: child {} completed successfully", actor_id);
                                Ok(option_bytes_to_value(child_result.result))
                            }
                            Ok(Some(ActorResult::Error(child_error))) => {
                                Err(Value::String(format!("Child actor {} failed: {}", child_error.actor_id, child_error.error)))
                            }
                            Ok(Some(ActorResult::ExternalStop(stop))) => {
                                Err(Value::String(format!("Child actor {} was stopped externally", stop.actor_id)))
                            }
                            Ok(None) => {
                                Err(Value::String(format!("Child actor {} result channel closed unexpectedly", actor_id)))
                            }
                            Err(_) => {
                                // Timeout - stop the child actor
                                debug!("spawn-and-wait: timeout waiting for child {}, stopping it", actor_id);
                                let (stop_tx, _) = oneshot::channel();
                                let _ = theater_tx.send(TheaterCommand::StopActor {
                                    actor_id: actor_id.clone(),
                                    response_tx: stop_tx,
                                }).await;
                                Err(Value::String(format!("Timeout waiting for child actor {} to complete", actor_id)))
                            }
                        }
                    }
                }
            })?
            // resume: func(manifest: string, wasm-bytes: option<list<u8>>) -> result<string, string>
            // Resumes an actor from a manifest with replay handler configured.
            // If wasm-bytes is provided, uses those bytes instead of loading from manifest.package.
            .func_async_result("resume", {
                let supervisor_tx = supervisor_tx.clone();
                let event_tx = event_tx.clone();
                let children = children.clone();
                let theater_tx_holder = theater_tx_holder.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let supervisor_tx = supervisor_tx.clone();
                    let event_tx = event_tx.clone();
                    let children = children.clone();
                    let theater_tx_holder = theater_tx_holder.clone();
                    async move {
                        let (manifest, wasm_bytes) = match input {
                            Value::Tuple(args) if args.len() == 2 => {
                                let manifest = match &args[0] {
                                    Value::String(s) => s.clone(),
                                    _ => return Err(Value::String("Invalid manifest argument".to_string())),
                                };
                                let wasm_bytes = parse_optional_bytes(&args[1]);
                                (manifest, wasm_bytes)
                            }
                            _ => return Err(Value::String("Invalid resume arguments: expected (string, option<list<u8>>)".to_string())),
                        };

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();

                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::ResumeActor {
                            manifest_path: manifest,
                            wasm_bytes,
                            response_tx,
                            supervisor_tx: Some(supervisor_tx),
                            subscription_tx: Some(event_tx),
                        };

                        if let Err(e) = theater_tx.send(cmd).await {
                            return Err(Value::String(format!("Failed to send resume command: {}", e)));
                        }

                        // Store theater_tx for shutdown
                        {
                            let mut holder = theater_tx_holder.lock().unwrap();
                            if holder.is_none() {
                                *holder = Some(theater_tx);
                            }
                        }

                        match response_rx.await {
                            Ok(Ok(actor_id)) => {
                                // Track the child
                                {
                                    let mut children_guard = children.lock().unwrap();
                                    children_guard.insert(actor_id.clone());
                                    debug!("Tracking resumed child {}, total children: {}", actor_id, children_guard.len());
                                }
                                Ok(Value::String(actor_id.to_string()))
                            }
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive response: {}", e))),
                        }
                    }
                }
            })?
            // list-children: func() -> list<string>
            // Returns the handler's internal tracking of children (not the runtime's)
            .func_async_result("list-children", {
                let children = children.clone();
                move |_ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let children = children.clone();
                    async move {
                        let children_guard = children.lock().unwrap();
                        let children_values: Vec<Value> = children_guard
                            .iter()
                            .map(|id| Value::String(id.to_string()))
                            .collect();
                        Ok::<Value, Value>(Value::List {
                            elem_type: ValueType::String,
                            items: children_values,
                        })
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
                            // State is already a Value, just wrap in Option
                            let state_value = match state {
                                Some(v) => Value::Option {
                                    inner_type: v.infer_type(),
                                    value: Some(Box::new(v)),
                                },
                                None => Value::Option {
                                    inner_type: ValueType::Bool, // placeholder
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

    fn supports_composite(&self) -> bool {
        true
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        mut shutdown_receiver: ShutdownReceiver,
        _event_rx: tokio::sync::broadcast::Receiver<theater::chain::ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Supervisor handler setup");

        // Take the receivers out of the Arc<Mutex<Option<>>>
        let channel_rx_opt = self.channel_rx.lock().unwrap().take();
        let event_rx_opt = self.event_rx.lock().unwrap().take();

        // Clone children and theater_tx for use in the async closure
        let children = self.children.clone();
        let theater_tx = self.theater_tx.clone();

        Box::pin(async move {
            // If we don't have a receiver (e.g., this is a cloned instance), just wait for shutdown
            let Some(mut channel_rx) = channel_rx_opt else {
                info!("Supervisor handler has no receiver (cloned instance), not starting");
                // Still need to wait for shutdown to avoid blocking the shutdown controller
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            // Get event receiver if available
            let mut event_rx = event_rx_opt;

            loop {
                tokio::select! {
                    // Process lifecycle events from children (ActorResult)
                    Some(child_result) = channel_rx.recv() => {
                        if let Err(e) = Self::process_child_result(&actor_handle, child_result, &children).await {
                            error!("Error processing child result: {}", e);
                        }
                    }
                    // Process all chain events from children
                    Some(event_result) = async {
                        if let Some(ref mut rx) = event_rx {
                            rx.recv().await
                        } else {
                            std::future::pending().await
                        }
                    } => {
                        if let Err(e) = Self::process_child_event(&actor_handle, event_result).await {
                            error!("Error processing child event: {}", e);
                        }
                    }
                    _ = &mut shutdown_receiver.receiver => {
                        info!("Shutdown signal received, stopping children");
                        break;
                    }
                }
            }

            // Stop all children on shutdown
            let children_to_stop: Vec<TheaterId> = {
                let children_guard = children.lock().unwrap();
                children_guard.iter().cloned().collect()
            };

            if !children_to_stop.is_empty() {
                info!("Stopping {} children", children_to_stop.len());

                // Get theater_tx if available
                let theater_tx_opt = {
                    let guard = theater_tx.lock().unwrap();
                    guard.clone()
                };

                if let Some(theater_tx) = theater_tx_opt {
                    for child_id in &children_to_stop {
                        debug!("Stopping child {}", child_id);
                        let (response_tx, _response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::StopActor {
                            actor_id: child_id.clone(),
                            response_tx,
                        };
                        if let Err(e) = theater_tx.send(cmd).await {
                            warn!("Failed to send stop command for child {}: {}", child_id, e);
                        }
                    }

                    // Wait for children to exit (with timeout)
                    let timeout = Duration::from_secs(5);
                    let start = std::time::Instant::now();

                    while start.elapsed() < timeout {
                        // Check if all children have exited
                        let remaining = {
                            let children_guard = children.lock().unwrap();
                            children_guard.len()
                        };

                        if remaining == 0 {
                            info!("All children have exited");
                            break;
                        }

                        // Process any remaining child results while waiting
                        tokio::select! {
                            Some(child_result) = channel_rx.recv() => {
                                if let Err(e) = Self::process_child_result(&actor_handle, child_result, &children).await {
                                    error!("Error processing child result during shutdown: {}", e);
                                }
                            }
                            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                                // Just check again
                            }
                        }
                    }

                    let remaining = {
                        let children_guard = children.lock().unwrap();
                        children_guard.len()
                    };
                    if remaining > 0 {
                        warn!("{} children did not exit within timeout", remaining);
                    }
                } else {
                    warn!("No theater_tx available, cannot stop children");
                }
            }

            info!("Supervisor handler shut down complete");
            Ok(())
        })
    }
}

/// Convert an ActorError to a Pack Value matching the WIT wit-actor-error record.
///
/// WIT: record wit-actor-error { error-type: wit-error-type, data: option<list<u8>> }
/// WIT enum wit-error-type has cases: operation-timeout(0), channel-closed(1),
/// shutting-down(2), function-not-found(3), type-mismatch(4), internal(5),
/// serialization-error(6), update-component-error(7), paused(8)
fn actor_error_to_value(error: ActorError) -> Value {
    let (tag, case_name) = match &error {
        ActorError::OperationTimeout(_) => (0, "operation-timeout"),
        ActorError::ChannelClosed => (1, "channel-closed"),
        ActorError::ShuttingDown => (2, "shutting-down"),
        ActorError::FunctionNotFound(_) => (3, "function-not-found"),
        ActorError::TypeMismatch(_) => (4, "type-mismatch"),
        ActorError::Internal(_) => (5, "internal"),
        ActorError::SerializationError => (6, "serialization-error"),
        ActorError::UpdatePackageError(_) => (7, "update-component-error"),
        ActorError::Paused => (8, "paused"),
        _ => (5, "internal"), // fallback
    };

    let error_type_value = Value::Variant {
        type_name: "wit-error-type".to_string(),
        case_name: case_name.to_string(),
        tag,
        payload: vec![],
    };

    // Encode error message as optional data bytes
    let error_msg = format!("{}", error);
    let data_value = Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: Some(Box::new(Value::List {
            elem_type: ValueType::U8,
            items: error_msg.into_bytes().into_iter().map(Value::U8).collect(),
        })),
    };

    // Record encoded as Tuple: [error-type, data]
    Value::Tuple(vec![error_type_value, data_value])
}

/// Convert Option<Vec<u8>> to a Pack Value matching option<list<u8>>
fn option_bytes_to_value(data: Option<Vec<u8>>) -> Value {
    match data {
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
    }
}

/// Parse an optional byte list from a Pack Value
fn parse_optional_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Option { value: Some(inner), .. } => {
            if let Value::List { items, .. } = inner.as_ref() {
                Some(items.iter().filter_map(|v| {
                    if let Value::U8(b) = v { Some(*b) } else { None }
                }).collect())
            } else {
                None
            }
        }
        Value::Option { value: None, .. } => None,
        _ => None,
    }
}

/// Parse an optional u64 from a Pack Value
fn parse_optional_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Option { value: Some(inner), .. } => {
            match inner.as_ref() {
                Value::U64(n) => Some(*n),
                _ => None,
            }
        }
        Value::Option { value: None, .. } => None,
        _ => None,
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

    #[test]
    fn test_supervisor_interface_hash_determinism() {
        let interface1 = supervisor_interface();
        let interface2 = supervisor_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_supervisor_handler_interface_hashes() {
        let config = SupervisorHostConfig {};
        let handler = SupervisorHandler::new(config, None);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/supervisor");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }
}
