//! # Actor Runtime
//!
//! The Actor Runtime is responsible for initializing, running, and managing the lifecycle
//! of WebAssembly actors within the Theater system. It coordinates the various components
//! that an actor needs to function, including execution, handlers, and communication channels.

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::actor::types::ActorOperation;
use crate::events::theater_runtime::TheaterRuntimeEventData;
use crate::events::wasm::WasmEventData;
use crate::events::{ChainEventData, EventData};
use crate::handler::Handler;
use crate::handler::HandlerRegistry;
use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::metrics::MetricsCollector;
use crate::store::ContentStore;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::ManifestConfig;

use crate::Result;
use crate::ShutdownController;
use crate::ShutdownType;
use crate::StateChain;
use serde_json::Value;
use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};
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
    SetupError {
        message: String,
    },
    ActorInstanceNotFound {
        message: String,
    },
    ActorPhaseError {
        expected: ActorPhase,
        found: ActorPhase,
        message: String,
    },
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

impl std::fmt::Display for ActorRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorRuntimeError::SetupError { message } => write!(f, "Setup Error: {}", message),
            ActorRuntimeError::ActorInstanceNotFound { message } => {
                write!(f, "Actor Instance Not Found: {}", message)
            }
            ActorRuntimeError::ActorPhaseError {
                expected,
                found,
                message,
            } => write!(
                f,
                "Actor Phase Error: expected {:?}, found {:?}. {}",
                expected, found, message
            ),
            ActorRuntimeError::ActorError(err) => write!(f, "Actor Error: {}", err),
            ActorRuntimeError::UnknownError(err) => write!(f, "Unknown Error: {}", err),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorPhase {
    Starting,
    Running,
    Paused,
    ShuttingDown,
}

impl Default for ActorPhase {
    fn default() -> Self {
        ActorPhase::Starting
    }
}

impl std::fmt::Display for ActorPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorPhase::Starting => write!(f, "Starting"),
            ActorPhase::Running => write!(f, "Running"),
            ActorPhase::Paused => write!(f, "Paused"),
            ActorPhase::ShuttingDown => write!(f, "Shutting Down"),
        }
    }
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
        let current_phase = self.current_phase.read().await;
        *current_phase == phase
    }

    pub async fn wait_for_phase(&self, phase: ActorPhase) {
        loop {
            let notified = self.notify.notified(); // Subscribe first
            {
                let current_phase = self.current_phase.read().await;
                if *current_phase == phase {
                    break;
                }
            }

            notified.await;
        }
    }
}

impl ActorRuntime {
    pub async fn build_actor_resources(
        id: TheaterId,
        config: &ManifestConfig,
        initial_state: Option<Value>,
        engine: Engine,
        chain: Arc<SyncRwLock<StateChain>>,
        mut handler_registry: HandlerRegistry,
        theater_tx: Sender<TheaterCommand>,
        operation_tx: Sender<ActorOperation>,
        info_tx: Sender<ActorInfo>,
        control_tx: Sender<ActorControl>,
        actor_phase_manager: ActorPhaseManager,
    ) -> Result<(ActorInstance, ShutdownController, Vec<JoinHandle<()>>), ActorRuntimeError> {
        // ---------------- Checkpoint 1: Setup Initial ----------------

        debug!("Setting up actor store");

        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 1".into(),
            });
        }

        let handle_operation_tx = operation_tx.clone();
        let actor_handle = ActorHandle::new(handle_operation_tx, info_tx, control_tx);
        let actor_store =
            ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone(), chain);

        actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorLoadCall {
                manifest: config.clone(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Initial values set up for [{}]", id).into(),
        });

        // ----------------- Checkpoint 2: Store Manifest ----------------

        debug!("Storing manifest for actor: {}", id);

        // Checkpoint 1: After manifest storage
        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 2".into(),
            });
        }

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
            description: format!("Manifest for actor [{}] stored at [{}]", id, manifest_id).into(),
        });

        // ----------------- Checkpoint 3: Create Handlers -----------------

        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 3".into(),
            });
        }

        debug!("Creating handlers");

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
            <anyhow::Error as Into<ActorRuntimeError>>::into(e)
        })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::CreatingHandlers),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Created handlers for actor [{}]", id).into(),
        });

        // ----------------- Checkpoint 4: Setup Handlers -----------------

        debug!("Setting up handlers");

        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 4".into(),
            });
        }

        let handlers = handler_registry.setup_handlers(&mut actor_component);

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::CreatingHandlers),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Set up handlers for actor [{}]", id).into(),
        });

        // ----------------- Checkpoint 5: Instantiate Actor -----------------

        debug!("Instantiating component");

        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 5".into(),
            });
        }

        let mut actor_instance = actor_component.instantiate().await.map_err(|e| {
            let error_message = format!("Failed to instantiate actor {}: {}", id, e);
            error!("{}", error_message);
            ActorRuntimeError::SetupError {
                message: error_message,
            }
        })?;

        actor_instance
            .actor_component
            .actor_store
            .record_event(ChainEventData {
                event_type: "theater-runtime".to_string(),
                data: EventData::TheaterRuntime(TheaterRuntimeEventData::InstantiatingActor),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: format!("Instantiated actor [{}]", id).into(),
            });

        // ----------------- Checkpoint 6: Initialize State -----------------

        debug!("Initializing state");

        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 6".into(),
            });
        }

        // Initialize state if needed
        let init_state = match initial_state {
            Some(state) => Some(serde_json::to_vec(&state).map_err(|e| {
                ActorError::UnexpectedError(format!("Failed to serialize initial state: {}", e))
            })?),
            None => None,
        };

        actor_instance.store.data_mut().set_state(init_state);

        actor_instance
            .actor_component
            .actor_store
            .record_event(ChainEventData {
                event_type: "theater-runtime".to_string(),
                data: EventData::TheaterRuntime(TheaterRuntimeEventData::InitializingState),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: format!("Initialized state for actor [{}]", id).into(),
            });

        // ----------------- Checkpoint 7: Finalize Setup -----------------

        debug!("Ready");

        if actor_phase_manager.is_phase(ActorPhase::Starting).await {
            let curr_phase = actor_phase_manager.get_phase().await;
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint 7".into(),
            });
        }

        let init_actor_handle = actor_handle.clone();
        let init_id = id.clone();
        tokio::spawn(async move {
            init_actor_handle
                .call_function::<(String,), ()>(
                    "theater:simple/actor.init".to_string(),
                    (init_id.to_string(),),
                )
                .await
                .map_err(|e| {
                    error!("Failed to call actor.init for actor {}: {}", id, e);
                    e
                })
        });

        // Start the handlers
        let mut handler_tasks: Vec<JoinHandle<()>> = vec![];
        let mut shutdown_controller = ShutdownController::new();
        let handler_actor_handle = actor_handle.clone();
        for mut handler in handlers {
            let actor_handle = handler_actor_handle.clone();
            let shutdown_receiver = shutdown_controller.subscribe();
            let handler_task = tokio::spawn(async move {
                handler
                    .start(actor_handle, shutdown_receiver)
                    .await
                    .unwrap();
            });
            handler_tasks.push(handler_task);
            // Store handler task for later management
            // Note: In a real implementation, you might want to store these in a more
            // structured way
            // For simplicity, we just push them into a vector here
            // You might want to use a Mutex or RwLock if you need to modify this later
            // For now, we assume they are static after startup
            // handler_tasks.push(handler_task);
        }

        actor_instance
            .actor_component
            .actor_store
            .record_event(ChainEventData {
                event_type: "theater-runtime".to_string(),
                data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorReady),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: format!("Actor [{}] is ready", id).into(),
            });

        Ok((actor_instance, shutdown_controller, handler_tasks))
    }

    pub async fn start(
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
    ) -> () {
        info!("Actor runtime starting communication loops");
        let actor_phase_manager = ActorPhaseManager::new();

        // These will be set once setup completes
        let actor_instance_wrapper: Arc<RwLock<Option<ActorInstance>>> =
            Arc::new(RwLock::new(None));
        let metrics: Arc<RwLock<MetricsCollector>> = Arc::new(RwLock::new(MetricsCollector::new()));
        let handler_tasks: Arc<RwLock<Vec<JoinHandle<()>>>> = Arc::new(RwLock::new(vec![]));
        let handlers_shutdown_controller: Arc<RwLock<Option<ShutdownController>>> =
            Arc::new(RwLock::new(None));
        let operation_rx = operation_rx;
        let info_rx = info_rx;
        let mut control_rx = control_rx;

        let setup_handle = {
            let actor_instance_wrapper = actor_instance_wrapper.clone();
            let actor_phase_manager = actor_phase_manager.clone();
            let config = config.clone();
            let theater_tx = theater_tx.clone();
            let handler_tasks = handler_tasks.clone();
            let handlers_shutdown_controller = handlers_shutdown_controller.clone();

            tokio::spawn(async move {
                match Self::build_actor_resources(
                    id,
                    &config,
                    initial_state,
                    engine,
                    chain,
                    handler_registry,
                    theater_tx,
                    operation_tx,
                    info_tx,
                    control_tx,
                    actor_phase_manager.clone(),
                )
                .await
                {
                    Ok((actor_instance, shutdown_controller, handlers)) => {
                        {
                            let mut instance_guard = actor_instance_wrapper.write().await;
                            *instance_guard = Some(actor_instance);
                        }
                        {
                            let mut handler_tasks_guard = handler_tasks.write().await;
                            *handler_tasks_guard = handlers;
                        }
                        {
                            let mut shutdown_controller_guard =
                                handlers_shutdown_controller.write().await;
                            *shutdown_controller_guard = Some(shutdown_controller);
                        }

                        actor_phase_manager.set_phase(ActorPhase::Running).await;
                        info!("Actor setup complete, now running");
                    }
                    Err(e) => {
                        error!("Failed to set up actor runtime: {}", e);
                        // Handle setup failure (e.g., notify theater runtime)
                    }
                }
            })
        };

        let info_handle = {
            let actor_instance_wrapper = actor_instance_wrapper.clone();
            let metrics = metrics.clone();
            let actor_phase_manager = actor_phase_manager.clone();

            tokio::spawn(Self::info_loop(
                info_rx,
                actor_instance_wrapper,
                metrics,
                actor_phase_manager,
            ))
        };

        let operation_handle = {
            let actor_instance_wrapper = actor_instance_wrapper.clone();
            let metrics = metrics.clone();
            let theater_tx = theater_tx.clone();
            let actor_phase_manager = actor_phase_manager.clone();

            tokio::spawn(Self::operation_loop(
                operation_rx,
                actor_instance_wrapper,
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
                    actor_phase_manager
                        .set_phase(ActorPhase::ShuttingDown)
                        .await;

                    // Wait for operation and info loops to finish gracefully
                    let (_, _, _) = tokio::join!(operation_handle, info_handle, setup_handle);

                    match handlers_shutdown_controller.write().await.take() {
                        Some(controller) => {
                            controller.signal_shutdown(ShutdownType::Graceful).await;
                        }
                        None => {
                            warn!("No handlers shutdown controller found");
                        }
                    }

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
                    setup_handle.abort();
                    match handlers_shutdown_controller.write().await.take() {
                        Some(controller) => {
                            controller.signal_shutdown(ShutdownType::Force).await;
                        }
                        None => {
                            warn!("No handlers shutdown controller found");
                        }
                    }
                    if let Err(e) = response_tx.send(Ok(())) {
                        error!("Failed to send terminate confirmation: {:?}", e);
                    }
                    break;
                }
                ActorControl::Pause { response_tx } => {
                    if actor_phase_manager.is_phase(ActorPhase::ShuttingDown).await {
                        let _ = response_tx.send(Err(ActorError::ShuttingDown));
                    } else {
                        actor_phase_manager.set_phase(ActorPhase::Paused).await;
                        let _ = response_tx.send(Ok(()));
                    }
                }
                ActorControl::Resume { response_tx } => {
                    match actor_phase_manager.get_phase().await {
                        ActorPhase::Starting | ActorPhase::Running => {
                            let _ = response_tx.send(Err(ActorError::NotPaused));
                        }
                        ActorPhase::ShuttingDown => {
                            let _ = response_tx.send(Err(ActorError::ShuttingDown));
                        }
                        ActorPhase::Paused => {
                            actor_phase_manager.set_phase(ActorPhase::Running).await;
                            let _ = response_tx.send(Ok(()));
                        }
                    }
                }
            }
        }

        // Gonna have to send the shutdown signal to all our handlers / respond to the shutdown
        // request

        info!("Actor runtime communication loop exiting, performing cleanup");
        let metrics = metrics.read().await;

        // If any handlers are still running, abort them
        let handler_tasks = handler_tasks.read().await;
        for handle in handler_tasks.iter() {
            if !handle.is_finished() {
                info!("Aborting handler task");
                handle.abort();
            }
        }

        // Log final metrics
        let final_metrics = metrics.get_metrics().await;
        info!("Final metrics at shutdown: {:?}", final_metrics);

        info!("Actor runtime cleanup complete");
    }

    async fn operation_loop(
        mut operation_rx: Receiver<ActorOperation>,
        actor_instance_wrapper: Arc<RwLock<Option<ActorInstance>>>,
        metrics: Arc<RwLock<MetricsCollector>>,
        theater_tx: Sender<TheaterCommand>,
        actor_phase_manager: ActorPhaseManager,
    ) {
        actor_phase_manager
            .wait_for_phase(ActorPhase::Running)
            .await;

        loop {
            tokio::select! {
                biased;

                _ = actor_phase_manager.wait_for_phase(ActorPhase::ShuttingDown) => {
                    break;
                }

                _ = actor_phase_manager.wait_for_phase(ActorPhase::Paused) => {
                    actor_phase_manager.wait_for_phase(ActorPhase::Running).await;
                }

                Some(op) = operation_rx.recv() => {
                    Self::process_operation(
                        op, &actor_instance_wrapper, &metrics, &theater_tx, actor_phase_manager.clone()
                    ).await
                }

                else => break,
            }
        }
    }

    async fn process_operation(
        op: ActorOperation,
        actor_instance_wrapper: &Arc<RwLock<Option<ActorInstance>>>,
        metrics: &Arc<RwLock<MetricsCollector>>,
        theater_tx: &Sender<TheaterCommand>,
        actor_phase_manager: ActorPhaseManager,
    ) -> () {
        match op {
            ActorOperation::CallFunction {
                name,
                params,
                response_tx,
            } => {
                info!("Processing function call: {}", name);
                let mut actor_instance_guard = actor_instance_wrapper.write().await;
                let actor_instance = match &mut *actor_instance_guard {
                    Some(instance) => instance,
                    None => {
                        let err = ActorRuntimeError::ActorInstanceNotFound {
                            message: "Actor instance not found".to_string(),
                        };

                        let _ = theater_tx
                            .send(TheaterCommand::ActorRuntimeError { error: err })
                            .await;

                        let actor_err =
                            ActorError::UnexpectedError("Actor instance not found".to_string());

                        if let Err(e) = response_tx.send(Err(actor_err)) {
                            error!(
                                "Failed to send function call error response for operation '{}': {:?}",
                                name, e
                            );
                        }
                        return;
                    }
                };
                let metrics = metrics.write().await;
                match Self::execute_call(actor_instance, &name, params, &theater_tx, &metrics).await
                {
                    Ok(result) => {
                        if let Err(e) = response_tx.send(Ok(result)) {
                            error!(
                                "Failed to send function call response for operation '{}': {:?}",
                                name, e
                            );
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

                        // Pause the actor on error
                        actor_phase_manager.set_phase(ActorPhase::Paused).await;
                    }
                }
            }
        }
    }

    async fn info_loop(
        mut info_rx: Receiver<ActorInfo>,
        actor_instance_wrapper: Arc<RwLock<Option<ActorInstance>>>,
        metrics: Arc<RwLock<MetricsCollector>>,
        actor_phase_manager: ActorPhaseManager,
    ) {
        // Handle info requests
        while let Some(info) = info_rx.recv().await {
            info!("Received info request: {:?}", info);
            match info {
                ActorInfo::GetStatus { response_tx } => {
                    let status = actor_phase_manager.get_phase().await.to_string();

                    if let Err(e) = response_tx.send(Ok(status)) {
                        error!("Failed to send status response: {:?}", e);
                    }
                }
                ActorInfo::GetState { response_tx } => {
                    match &*actor_instance_wrapper.read().await {
                        Some(instance) => {
                            let state = instance.store.data().get_state();
                            if let Err(e) = response_tx.send(Ok(state)) {
                                error!("Failed to send state response: {:?}", e);
                            }
                        }
                        None => {
                            let err =
                                ActorError::UnexpectedError("Actor instance not found".to_string());
                            if let Err(e) = response_tx.send(Err(err)) {
                                error!("Failed to send state error response: {:?}", e);
                            }
                        }
                    }
                }
                ActorInfo::GetChain { response_tx } => {
                    match &*actor_instance_wrapper.read().await {
                        None => {
                            let err =
                                ActorError::UnexpectedError("Actor instance not found".to_string());
                            if let Err(e) = response_tx.send(Err(err)) {
                                error!("Failed to send chain error response: {:?}", e);
                            }
                            return;
                        }
                        Some(instance) => {
                            let chain = instance.store.data().get_chain();
                            if let Err(e) = response_tx.send(Ok(chain)) {
                                error!("Failed to send chain response: {:?}", e);
                            }
                        }
                    };
                }
                ActorInfo::GetMetrics { response_tx } => {
                    let metrics = metrics.read().await;
                    let metrics_data = metrics.get_metrics().await;
                    if let Err(e) = response_tx.send(Ok(metrics_data)) {
                        error!("Failed to send metrics response: {:?}", e);
                    }
                }
                ActorInfo::SaveChain { response_tx } => {
                    match &mut *actor_instance_wrapper.write().await {
                        None => {
                            let err =
                                ActorError::UnexpectedError("Actor instance not found".to_string());
                            if let Err(e) = response_tx.send(Err(err)) {
                                error!("Failed to send save chain error response: {:?}", e);
                            }
                            return;
                        }
                        Some(instance) => match instance.save_chain() {
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
                        },
                    };
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
}
