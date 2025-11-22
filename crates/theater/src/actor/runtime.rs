//! # Actor Runtime
//!
//! The Actor Runtime is responsible for initializing, running, and managing the lifecycle
//! of WebAssembly actors within the Theater system. It coordinates the various components
//! that an actor needs to function, including execution, handlers, and communication channels.

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::actor::types::ActorOperation;
use crate::config::permissions::HandlerPermission;
use crate::events::theater_runtime::TheaterRuntimeEventData;
use crate::events::wasm::WasmEventData;
use crate::events::{ChainEventData, EventData};
use crate::handler::Handler;
use crate::handler::HandlerRegistry;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::metrics::MetricsCollector;
use crate::shutdown::ShutdownType;
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use crate::store::ContentStore;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::ManifestConfig;

use crate::Result;
use crate::StateChain;
use serde_json::Value;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, error, info};
use wasmtime::Engine;

use super::types::ActorControl;
use super::types::ActorInfo;

/// Maximum time to wait for graceful shutdown before forceful termination
#[allow(dead_code)]
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// # ActorRuntime
///
/// Coordinates the execution and lifecycle of a single WebAssembly actor within the Theater system.
///
/// `ActorRuntime` manages the various components that make up an actor's execution environment,
/// including handlers and communication channels. It's responsible for starting the actor,
/// setting up its capabilities via handlers, executing operations, and ensuring proper shutdown.
pub struct ActorRuntime {
    /// Unique identifier for this actor
    pub id: TheaterId,
    config: ManifestConfig,
    chain: Arc<SyncRwLock<StateChain>>,
    handlers: Vec<Box<dyn Handler>>,
    actor_instance: ActorInstance,
    metrics: MetricsCollector,
    operation_rx: Receiver<ActorOperation>,
    info_rx: Receiver<ActorInfo>,
    control_rx: Receiver<ActorControl>,
    theater_tx: Sender<TheaterCommand>,
    actor_phase_manager: ActorPhaseManager,
}

/// # Result of starting an actor
///
/// Represents the outcome of attempting to start an actor.
///
/// This enum provides detailed information about whether an actor was successfully
/// started or encountered errors during initialization. It includes the actor's ID
/// in both success and failure cases, and detailed error information in the failure case.
#[derive(Debug)]
pub enum StartActorResult {
    /// Actor successfully started
    Success(TheaterId),
    /// Actor failed to start with error message
    Failure(TheaterId, String),
    /// Actor failed to start with permission or validation error
    Error(String),
}

#[derive(Debug)]
pub enum ActorRuntimeError {
    SetupError { message: String },
    ActorError(ActorError),
    UnknownError(anyhow::Error),
}

impl From<ActorError> for ActorRuntimeError {
    fn from(error: ActorError) -> Self {
        ActorRuntimeError::ActorError(error)
    }
}

impl From<anyhow::Error> for ActorRuntimeError {
    fn from(error: anyhow::Error) -> Self {
        ActorRuntimeError::UnknownError(error)
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorPhase {
    Starting,
    Running,
    Paused,
    ShuttingDown,
}


#[derive(Clone)]
pub struct ActorPhaseManager {
    current_phase: Arc<RwLock<ActorPhase>>,
    notify: Arc<tokio::sync::Notify>,
}

impl ActorPhaseManager {
    pub fn new() -> Self {
        Self {
            current_phase: Arc::new(RwLock::new(ActorPhase::Starting)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub async fn set_phase(&self, phase: ActorPhase) {
        let mut current_phase = self.current_phase.write().await;
        *current_phase = phase;
        self.notify.notify_waiters();
    }

    pub async fn get_phase(&self) -> ActorPhase {
        let current_phase = self.current_phase.read().await;
        current_phase.clone()
    }

    pub async fn is_phase(&self, phase: ActorPhase) -> bool {
        let current_phase= self.current_phase.read().await;
        *current_phase == phase
    }

    pub async fn wait_for_phase(&self, phase: ActorPhase) {
        loop {
        
            let notified = self.notify.notified(); // Subscribe first
            {
                let current_phase= self.current_phase.read().await;
                if *current_phase == phase {
                    break;
                }
            }
           
            notified.await;
        }
    }
}

impl ActorRuntime {
    pub async fn new(
        id: TheaterId,
        config: &ManifestConfig,
        initial_state: Option<Value>,
        engine: Engine,
        chain: Arc<SyncRwLock<StateChain>>,
        handler_registry: HandlerRegistry,
        theater_tx: Sender<TheaterCommand>,
        operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        info_rx: Receiver<ActorInfo>,
        info_tx: Sender<ActorInfo>,
        control_rx: Receiver<ActorControl>,
        control_tx: Sender<ActorControl>,
    ) -> Result<Self, ActorRuntimeError> {
        let actor_phase_manager = ActorPhaseManager::new();
        let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

        debug!("Setting up actor store");

        // Create actor store
        let actor_store =
            ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone(), chain);

        // Store manifest
        let manifest_store = ContentStore::from_id("manifest");
        debug!("Storing manifest for actor: {}", id);
        debug!("Manifest store: {:?}", manifest_store);
        let manifest_id = match manifest_store
            .store(
                config
                    .clone()
                    .into_fixed_bytes()
                    .expect("Failed to serialize manifest"),
            )
            .await
        {
            Ok(id) => id,
            Err(e) => {
                let error_message = format!("Failed to store manifest: {}", e);
                error!("{}", error_message);
                return Err(e.into());
            }
        };

        actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorLoadCall {
                manifest: config.clone(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Loading actor [{}] from manifest [{}] ", id, manifest_id).into(),
        });

        debug!("Creating handlers");

        actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::CreatingHandlers),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Creating handlers for actor [{}]", id).into(),
        });

        debug!("Creating component");

        let mut actor_component = ActorComponent::new(
            config.name.clone(),
            config.component.clone(),
            actor_store,
            engine,
        )
        .await
        .map_err(|e| {
            let error_message = format!(
                "Failed to create actor component for actor {}: {}",
                config.name, e
            );
            error!("{}", error_message);
            e.into()
        })?;

        debug!("Setting up host functions");

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::CreatingHandlers),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Setting up host functions for actor [{}]", id).into(),
        });

        let handlers = handler_registry.setup_handlers(&mut actor_component);

        debug!("Instantiating component");

        let actor_instance = actor_component.instantiate().await.map_err(|e| {
            let error_message = format!("Failed to instantiate actor {}: {}", id, e);
            error!("{}", error_message);
            ActorRuntimeError::SetupError {
                message: error_message,
            }
        })?;

        debug!("Initializing state");

        // Initialize state if needed
        let init_state = match initial_state {
            Some(state) => Some(serde_json::to_vec(&state).map_err(|e| {
                ActorError::UnexpectedError(format!("Failed to serialize initial state: {}", e))
            })?),
            None => None,
        };

        actor_instance.store.data_mut().set_state(init_state);

        debug!("Ready");

        let metrics = MetricsCollector::new();

        Ok(Self {
            id,
            config: config.clone(),
            chain,
            actor_instance,
            handlers,
            metrics,
            operation_rx,
            control_rx,
            info_rx,
            theater_tx,
            actor_phase_manager,
        })
    }

    pub async fn start(self: Self) {
        info!("Actor runtime starting communication loops");
        self.actor_phase_manager
            .set_phase(ActorPhase::Running)
            .await;

        // These will be set once setup completes
        let mut actor_instance: Option<Arc<RwLock<ActorInstance>>> = None;
        let mut metrics: Option<Arc<RwLock<MetricsCollector>>> = None;
        let mut handler_tasks: Vec<JoinHandle<()>> = Vec::new();
        let shutdown_response_tx: Option<oneshot::Sender<Result<()>>> = None;
        let mut operation_rx = self.operation_rx;
        let mut info_rx = self.info_rx;
        let mut control_rx = self.control_rx;

        let info_handle = {
            let actor_instance = actor_instance.clone();
            let metrics = metrics.clone();
            let actor_phase_manager = self.actor_phase_manager.clone();

            tokio::spawn(Self::info_loop(
                info_rx,
                actor_instance,
                metrics,
                actor_phase_manager
            ))
        };

        let operation_handle = {
            let actor_instance = actor_instance.clone();
            let metrics = metrics.clone();
            let theater_tx = self.theater_tx.clone();
            let actor_phase_manager = self.actor_phase_manager.clone();

            tokio::spawn(Self::operation_loop(
                operation_rx,
                actor_instance,
                metrics,
                theater_tx,
                actor_phase_manager,
            ))
        };

        while let Some(control) = control_rx.recv().await {
            info!("Received control command: {:?}", control);
            match control {
                ActorControl::Shutdown { response_tx } => {
                    info!("Shutdown requested");
                    self.actor_phase_manager.set_phase(ActorPhase::ShuttingDown).await;

                    // Wait for operation and info loops to finish gracefully
                    let (_ , _ ) = tokio::join!(
                        operation_handle,
                        info_handle
                    );

                    if let Err(e) = response_tx.send(Ok(())) {
                        error!("Failed to send shutdown confirmation: {:?}", e);
                    }
                    break;
                }
                ActorControl::Terminate { response_tx } => {
                    info!("Terminate requested");
                    // Abort info and operation loops
                    operation_handle.abort();
                    info_handle.abort();
                    if let Err(e) = response_tx.send(Ok(())) {
                        error!("Failed to send terminate confirmation: {:?}", e);
                    }
                    break;
                }
                ActorControl::Pause { response_tx } => {
                    if self.actor_phase_manager.is_phase(ActorPhase::ShuttingDown).await {
                        let _ = response_tx.send(Err(ActorError::ShuttingDown));
                    } else {
                        self.actor_phase_manager.set_phase(ActorPhase::Paused).await;
                        let _ = response_tx.send(Ok(()));
                    }
                }
                ActorControl::Resume { response_tx } => {
                    match self.actor_phase_manager.get_phase().await {
                        ActorPhase::Starting | ActorPhase::Running => {
                            let _ = response_tx.send(Err(ActorError::NotPaused));
                        }
                        ActorPhase::ShuttingDown => {
                            let _ = response_tx.send(Err(ActorError::ShuttingDown));
                        }
                        ActorPhase::Paused => { 
                            self.actor_phase_manager.set_phase(ActorPhase::Running).await;
                            let _ = response_tx.send(Ok(()));
                        }
                    }
                }
            }
        }
        
        // Gonna have to send the shutdown signal to all our handlers / respond to the shutdown
        // request

        info!("Actor runtime communication loop exiting, performing cleanup");
        if let Some(ref metrics) = metrics {
            let metrics = metrics.read().await;
            Self::perform_cleanup(shutdown_controller, handler_tasks, &metrics).await;
        } else {
            info!("Actor was shut down during startup, no cleanup needed");
        }
    }

    async fn operation_loop(
        mut operation_rx: Receiver<ActorOperation>,
        actor_instance: Option<Arc<RwLock<ActorInstance>>>,
        metrics: Option<Arc<RwLock<MetricsCollector>>>,
        theater_tx: Sender<TheaterCommand>,
        actor_phase_manager: ActorPhaseManager
    ) {
        // Handle operations
        while let Some(op) = operation_rx.recv().await && !*paused.read().await {
            info!("Received operation: {:?}", op);
            match op {
                ActorOperation::CallFunction {
                    name,
                    params,
                    response_tx,
                } => {
                    info!("Processing function call: {}", name);
                    if let (Some(ref actor_instance), Some(ref metrics)) =
                        (&actor_instance, &metrics)
                    {
                        let mut actor_instance = actor_instance.write().await;
                        let metrics = metrics.write().await;
                        match Self::execute_call(
                            &mut actor_instance,
                            &name,
                            params,
                            &theater_tx,
                            &metrics,
                        )
                        .await
                        {
                            Ok(result) => {
                                if let Err(e) = response_tx.send(Ok(result)) {
                                    error!("Failed to send function call response for operation '{}': {:?}", name, e);
                                }
                            }
                            Err(actor_error) => {
                                let _ = theater_tx
                                    .send(TheaterCommand::ActorError {
                                        actor_id: actor_instance.id().clone(),
                                        error: actor_error.clone(),
                                    })
                                    .await;

                                error!("Operation '{}' failed with error: {:?}", name, actor_error);
                                if let Err(send_err) = response_tx.send(Err(actor_error)) {
                                    error!("Failed to send function call error response for operation '{}': {:?}", name, send_err);
                                }

                                *paused.write().await = true;
                            }
                        }
                    } else {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Actor still starting".to_string(),
                        )));
                    }
                }
            }
        }
    }

    async fn info_loop(
        mut info_rx: Receiver<ActorInfo>,
        actor_instance: Option<Arc<RwLock<ActorInstance>>>,
        metrics: Option<Arc<RwLock<MetricsCollector>>>,
        actor_phase_manager: ActorPhaseManager,
    ) {
        // Handle info requests
        while let Some(info) = info_rx.recv().await {
            info!("Received info request: {:?}", info);
            match info {
                ActorInfo::GetStatus { response_tx } => {
                    let status = if shutdown_requested {
                        "Shutting down".to_string()
                    } else if *paused.read().await {
                        "Paused".to_string()
                    } else {
                        "Idle".to_string()
                    };

                    if let Err(e) = response_tx.send(Ok(status)) {
                        error!("Failed to send status response: {:?}", e);
                    }
                }
                ActorInfo::GetState { response_tx } => {
                    if let Some(ref actor_instance) = actor_instance {
                        let actor_instance = actor_instance.read().await;
                        let state = actor_instance.store.data().get_state();
                        if let Err(e) = response_tx.send(Ok(state)) {
                            error!("Failed to send state response: {:?}", e);
                        }
                    } else {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Actor still starting".to_string(),
                        )));
                    }
                }
                ActorInfo::GetChain { response_tx } => {
                    if let Some(ref actor_instance) = actor_instance {
                        let actor_instance = actor_instance.read().await;
                        let chain = actor_instance.store.data().get_chain();
                        if let Err(e) = response_tx.send(Ok(chain)) {
                            error!("Failed to send chain response: {:?}", e);
                        }
                    } else {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Actor still starting".to_string(),
                        )));
                    }
                }
                ActorInfo::GetMetrics { response_tx } => {
                    if let Some(ref metrics) = metrics {
                        let metrics = metrics.read().await;
                        let metrics_data = metrics.get_metrics().await;
                        if let Err(e) = response_tx.send(Ok(metrics_data)) {
                            error!("Failed to send metrics response: {:?}", e);
                        }
                    } else {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Actor still starting".to_string(),
                        )));
                    }
                }
                ActorInfo::SaveChain { response_tx } => {
                    if let Some(ref actor_instance) = actor_instance {
                        let actor_instance = actor_instance.read().await;
                        match actor_instance.save_chain() {
                            Ok(_) => {
                                if let Err(e) = response_tx.send(Ok(())) {
                                    error!("Failed to send save chain response: {:?}", e);
                                }
                            }
                            Err(e) => {
                                if let Err(send_err) = response_tx
                                    .send(Err(ActorError::UnexpectedError(e.to_string())))
                                {
                                    error!(
                                        "Failed to send save chain error response: {:?}",
                                        send_err
                                    );
                                }
                            }
                        }
                    } else {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Actor still starting".to_string(),
                        )));
                    }
                }
            }
        }
    }

    ///
    /// Calls a function in the WebAssembly actor with the given parameters,
    /// updates the actor's state based on the result, and records the
    /// operation in the actor's chain.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - The WebAssembly actor instance
    /// * `name` - Name of the function to call
    /// * `params` - Serialized parameters for the function
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `metrics` - Collector for performance metrics
    ///
    /// ## Returns
    ///
    /// * `Ok(Vec<u8>)` - Serialized result of the function call
    /// * `Err(ActorError)` - Error that occurred during execution
    async fn execute_call(
        actor_instance: &mut ActorInstance,
        name: &String,
        params: Vec<u8>,
        _theater_tx: &mpsc::Sender<TheaterCommand>,
        metrics: &MetricsCollector,
    ) -> Result<Vec<u8>, ActorError> {
        // Validate the function exists
        if !actor_instance.has_function(&name) {
            error!("Function '{}' not found in actor", name);
            return Err(ActorError::FunctionNotFound(name.to_string()));
        }

        let start = Instant::now();

        let state = actor_instance.store.data().get_state();
        debug!(
            "Executing call to function '{}' with state size: {:?}",
            name,
            state.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        actor_instance
            .store
            .data_mut()
            .record_event(ChainEventData {
                event_type: "wasm".to_string(),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: Some(format!("Wasm call to function '{}'", name)),
                data: EventData::Wasm(WasmEventData::WasmCall {
                    function_name: name.clone(),
                    params: params.clone(),
                }),
            });

        // Execute the call
        let (new_state, results) = match actor_instance.call_function(&name, state, params).await {
            Ok(result) => {
                actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Wasm call to function '{}' completed", name)),
                        data: EventData::Wasm(WasmEventData::WasmResult {
                            function_name: name.clone(),
                            result: result.clone(),
                        }),
                    });
                result
            }
            Err(e) => {
                let event = actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Wasm call to function '{}' failed", name)),
                        data: EventData::Wasm(WasmEventData::WasmError {
                            function_name: name.clone(),
                            message: e.to_string(),
                        }),
                    });

                error!("Failed to execute function '{}': {}", name, e);
                return Err(ActorError::Internal(event));
            }
        };

        debug!(
            "Call to '{}' completed, new state size: {:?}",
            name,
            new_state.as_ref().map(|s| s.len()).unwrap_or(0)
        );
        actor_instance.store.data_mut().set_state(new_state);

        // Record metrics
        let duration = start.elapsed();
        metrics.record_operation(duration, true).await;

        Ok(results)
    }

    /// # Perform final cleanup when shutting down
    ///
    /// This method is called during shutdown to release resources and log
    /// final metrics before the actor terminates.
    async fn perform_cleanup(
        shutdown_controller: ShutdownController,
        handler_tasks: Vec<JoinHandle<()>>,
        metrics: &MetricsCollector,
    ) {
        info!("Performing final cleanup");

        // Signal shutdown to all handler components
        info!("Signaling shutdown to all handler components");
        shutdown_controller
            .signal_shutdown(ShutdownType::Graceful)
            .await;

        // If any handlers are still running, abort them
        for task in handler_tasks {
            if !task.is_finished() {
                debug!("Aborting handler task that didn't shut down gracefully");
                task.abort();
            }
        }

        // Log final metrics
        let final_metrics = metrics.get_metrics().await;
        info!("Final metrics at shutdown: {:?}", final_metrics);

        info!("Actor runtime cleanup complete");
    }

    /// # Stop the actor runtime
    ///
    /// Gracefully shuts down the actor runtime and all its components.
    /// This method is retained for API compatibility but delegates to the
    /// shutdown controller.
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - The runtime was successfully shut down
    /// * `Err(anyhow::Error)` - An error occurred during shutdown
    pub async fn stop(mut self) -> Result<()> {
        info!("Initiating actor runtime shutdown");

        // Signal shutdown to all components
        info!("Signaling shutdown to all components");
        self.shutdown_controller
            .signal_shutdown(ShutdownType::Graceful)
            .await;

        // If any handlers are still running, abort them
        for task in self.handler_tasks.drain(..) {
            if !task.is_finished() {
                debug!("Aborting handler task that didn't shut down gracefully");
                task.abort();
            }
        }

        info!("Actor runtime shutdown complete");
        Ok(())
    }
}
