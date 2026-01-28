//! # Actor Runtime
//!
//! The Actor Runtime is responsible for initializing, running, and managing the lifecycle
//! of WebAssembly actors within the Theater system. It coordinates the various components
//! that an actor needs to function, including execution, handlers, and communication channels.

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::actor::types::ActorOperation;
use crate::pack_bridge::{AsyncRuntime, HostLinkerBuilder, LinkerError, PackInstance};
use crate::events::wasm::WasmEventData;
use crate::events::ChainEventData;
use crate::handler::Handler;
use crate::handler::HandlerContext;
use crate::handler::HandlerRegistry;
use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::metrics::MetricsCollector;
use crate::store::ContentStore;
use crate::utils::resolve_reference;
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
///
/// Note: The struct fields are currently unused as the runtime is driven by the
/// `start()` and `build_actor_resources()` functions which manage the instance
/// through shared wrappers.
#[allow(dead_code)]
pub struct ActorRuntime {
    /// Unique identifier for this actor
    pub id: TheaterId,
    config: ManifestConfig,
    handlers: Vec<Box<dyn Handler>>,
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
    phase_tx: Arc<tokio::sync::watch::Sender<ActorPhase>>,
    phase_rx: tokio::sync::watch::Receiver<ActorPhase>,
}

impl ActorPhaseManager {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::watch::channel(ActorPhase::Starting);
        Self {
            phase_tx: Arc::new(tx),
            phase_rx: rx,
        }
    }

    pub fn set_phase(&self, phase: ActorPhase) {
        let _ = self.phase_tx.send(phase);
    }

    pub fn get_phase(&self) -> ActorPhase {
        self.phase_rx.borrow().clone()
    }

    pub fn is_phase(&self, phase: ActorPhase) -> bool {
        *self.phase_rx.borrow() == phase
    }

    pub async fn wait_for_phase(&self, phase: ActorPhase) {
        let mut rx = self.phase_rx.clone();
        let _ = rx.wait_for(|current_phase| *current_phase == phase).await;
    }
}

impl ActorRuntime {
    pub async fn build_actor_resources(
        id: TheaterId,
        config: &ManifestConfig,
        initial_state: Option<Value>,
        pack_runtime: AsyncRuntime,
        chain: Arc<SyncRwLock<StateChain>>,
        mut handler_registry: HandlerRegistry,
        theater_tx: Sender<TheaterCommand>,
        operation_tx: Sender<ActorOperation>,
        info_tx: Sender<ActorInfo>,
        control_tx: Sender<ActorControl>,
        actor_phase_manager: ActorPhaseManager,
        actor_instance_wrapper: Arc<RwLock<Option<PackInstance>>>,
    ) -> Result<(ShutdownController, Vec<JoinHandle<()>>), ActorRuntimeError> {
        // ---------------- Checkpoint Setup Initial ----------------

        debug!("Setting up actor store");

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Setup Initial".into(),
            });
        }

        let handle_operation_tx = operation_tx.clone();
        let actor_handle = ActorHandle::new(handle_operation_tx, info_tx, control_tx);
        let actor_store =
            ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone(), chain);

        // ----------------- Checkpoint Store Manifest ----------------

        debug!("Storing manifest for actor: {}", id);

        // Checkpoint 1: After manifest storage
        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Store Manifest".into(),
            });
        }

        // Store manifest (but don't record an event - the manifest hash varies between runs)
        let manifest_store = ContentStore::from_id("manifest");
        debug!("Storing manifest for actor: {}", id);
        debug!("Manifest store: {:?}", manifest_store);
        let _manifest_id = manifest_store
            .store(
                config
                    .clone()
                    .into_fixed_bytes()
                    .expect("Failed to serialize manifest"),
            )
            .await;

        // ----------------- Checkpoint Load Component -----------------

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Load Package".into(),
            });
        }

        debug!("Loading package: {}", config.package);

        // Load the WASM package bytes
        let wasm_bytes = resolve_reference(&config.package).await.map_err(|e| {
            let error_message = format!(
                "Failed to load package for actor {}: {}",
                config.name, e
            );
            error!("{}", error_message);
            ActorRuntimeError::SetupError {
                message: error_message,
            }
        })?;

        // ----------------- Checkpoint Get Handlers -----------------

        debug!("Getting handlers from registry");

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Get Handlers".into(),
            });
        }

        // Get all handlers from the registry
        let mut handlers = handler_registry.get_handlers();
        debug!("Got {} handlers from registry", handlers.len());

        // ----------------- Checkpoint Instantiate with Host Functions -----------------

        debug!("Creating PackInstance with host functions");

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Instantiate".into(),
            });
        }

        // Create a closure that sets up all handler host functions
        let mut handler_ctx = HandlerContext::new();
        let handlers_for_setup = &mut handlers;

        let mut actor_instance = PackInstance::new(
            config.name.clone(),
            &wasm_bytes,
            &pack_runtime,
            actor_store,
            |builder: &mut HostLinkerBuilder<'_, ActorStore>| {
                // Set up host functions for each handler
                for handler in handlers_for_setup.iter_mut() {
                    debug!("Setting up Composite host functions for handler '{}'", handler.name());
                    match handler.setup_host_functions_composite(builder, &mut handler_ctx) {
                        Ok(()) => {
                            debug!(
                                "Handler '{}' Composite host functions set up successfully",
                                handler.name()
                            );
                        }
                        Err(e) => {
                            error!(
                                "Handler '{}' Composite host functions FAILED: {:?}",
                                handler.name(),
                                e
                            );
                            return Err(e);
                        }
                    }
                }
                Ok(())
            },
        )
        .await
        .map_err(|e| {
            let error_message = format!("Failed to instantiate actor {}: {}", id, e);
            error!("{}", error_message);
            ActorRuntimeError::SetupError {
                message: error_message,
            }
        })?;

        debug!("PackInstance created successfully");
        debug!(
            "Handler context satisfied imports: {:?}",
            handler_ctx.satisfied_imports
        );

        // ----------------- Checkpoint Register Exports -----------------

        debug!("Registering export functions");

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Register Exports".into(),
            });
        }

        // Register standard Theater actor exports
        actor_instance.register_export("theater:simple/actor", "init");

        // Let handlers register their exports
        for handler in handlers.iter() {
            if let Err(e) = handler.register_exports_composite(&mut actor_instance) {
                warn!("Handler '{}' failed to register exports: {:?}", handler.name(), e);
            }
        }

        // ----------------- Checkpoint Initialize State -----------------

        debug!("Initializing state");

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Initialize State".into(),
            });
        }

        // Initialize state if needed
        let init_state = match initial_state {
            Some(state) => Some(serde_json::to_vec(&state).map_err(|e| {
                ActorError::UnexpectedError(format!("Failed to serialize initial state: {}", e))
            })?),
            None => None,
        };

        actor_instance.actor_store.set_state(init_state);

        // ----------------- Checkpoint Finalize Setup -----------------

        debug!("Ready");

        if !actor_phase_manager.is_phase(ActorPhase::Starting) {
            let curr_phase = actor_phase_manager.get_phase();
            return Err(ActorRuntimeError::ActorPhaseError {
                expected: ActorPhase::Starting,
                found: curr_phase,
                message: "phase error found at setup task Checkpoint Finalize Setup".into(),
            });
        }

        // Put actor_instance in the shared wrapper BEFORE spawning init
        {
            let mut instance_guard = actor_instance_wrapper.write().await;
            *instance_guard = Some(actor_instance);
        }

        // In replay mode, the replay handler will call init after setting up subscriptions
        // to avoid race conditions. In normal mode, we spawn init here.
        if !handler_registry.is_replay_mode() {
            let init_actor_handle = actor_handle.clone();
            let init_id = id.clone();
            tokio::spawn(async move {
                // Call init - it's a state-only function that takes state from the store
                // and returns updated state.
                // init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>
                init_actor_handle
                    .call_function_void("theater:simple/actor.init".to_string(), vec![])
                    .await
                    .map_err(|e| {
                        error!("Failed to call actor.init for actor {}: {}", init_id, e);
                        e
                    })
            });
        } else {
            info!("Replay mode: skipping automatic init call (replay handler will drive execution)");
        }

        // Start the handlers
        let mut handler_tasks: Vec<JoinHandle<()>> = vec![];
        let mut shutdown_controller = ShutdownController::new();
        let handler_actor_handle = actor_handle.clone();
        debug!("Starting {} handlers", handlers.len());
        for mut handler in handlers {
            let handler_name = handler.name().to_string();
            debug!("Spawning task for handler: {}", handler_name);
            let actor_handle = handler_actor_handle.clone();
            let actor_instance = actor_instance_wrapper.clone();
            let shutdown_receiver = shutdown_controller.subscribe();
            let handler_task = tokio::spawn(async move {
                debug!("Handler task running: {}", handler_name);
                if let Err(e) = handler
                    .start(actor_handle, actor_instance, shutdown_receiver)
                    .await
                {
                    error!("Handler '{}' start() failed: {:?}", handler_name, e);
                } else {
                    debug!("Handler '{}' start() completed", handler_name);
                }
            });
            handler_tasks.push(handler_task);
        }

        Ok((shutdown_controller, handler_tasks))
    }

    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        initial_state: Option<Value>,
        pack_runtime: AsyncRuntime,
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
        let actor_instance_wrapper: Arc<RwLock<Option<PackInstance>>> =
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
                    pack_runtime,
                    chain,
                    handler_registry,
                    theater_tx,
                    operation_tx,
                    info_tx,
                    control_tx,
                    actor_phase_manager.clone(),
                    actor_instance_wrapper,
                )
                .await
                {
                    Ok((shutdown_controller, handlers)) => {
                        {
                            let mut handler_tasks_guard = handler_tasks.write().await;
                            *handler_tasks_guard = handlers;
                        }
                        {
                            let mut shutdown_controller_guard =
                                handlers_shutdown_controller.write().await;
                            *shutdown_controller_guard = Some(shutdown_controller);
                        }

                        actor_phase_manager.set_phase(ActorPhase::Running);
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
                    actor_phase_manager.set_phase(ActorPhase::ShuttingDown);

                    debug!("Signaled shutdown to operation and info loops");

                    // Wait for operation and info loops to finish gracefully
                    let (_, _, _) = tokio::join!(operation_handle, info_handle, setup_handle);

                    debug!("Operation and info loops have exited");

                    match handlers_shutdown_controller.write().await.take() {
                        Some(controller) => {
                            controller.signal_shutdown(ShutdownType::Graceful).await;
                        }
                        None => {
                            warn!("No handlers shutdown controller found");
                        }
                    }

                    debug!("Signaled shutdown to handlers");

                    if let Err(e) = response_tx.send(Ok(())) {
                        error!("Failed to send shutdown confirmation: {:?}", e);
                    }

                    debug!("Shutdown confirmation sent, exiting control loop");
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
                    if actor_phase_manager.is_phase(ActorPhase::ShuttingDown) {
                        let _ = response_tx.send(Err(ActorError::ShuttingDown));
                    } else {
                        actor_phase_manager.set_phase(ActorPhase::Paused);
                        let _ = response_tx.send(Ok(()));
                    }
                }
                ActorControl::Resume { response_tx } => match actor_phase_manager.get_phase() {
                    ActorPhase::Starting | ActorPhase::Running => {
                        let _ = response_tx.send(Err(ActorError::NotPaused));
                    }
                    ActorPhase::ShuttingDown => {
                        let _ = response_tx.send(Err(ActorError::ShuttingDown));
                    }
                    ActorPhase::Paused => {
                        actor_phase_manager.set_phase(ActorPhase::Running);
                        let _ = response_tx.send(Ok(()));
                    }
                },
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
        actor_instance_wrapper: Arc<RwLock<Option<PackInstance>>>,
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
        actor_instance_wrapper: &Arc<RwLock<Option<PackInstance>>>,
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
                        actor_phase_manager.set_phase(ActorPhase::Paused);
                    }
                }
            }
            ActorOperation::HandleWasiHttpRequest { response_tx, .. } => {
                // WASI HTTP incoming requests are handled directly by the http handler
                // via the SharedActorInstance, not through this operation channel.
                // If this operation is received, it indicates a configuration error.
                let err = ActorError::UnexpectedError(
                    "HandleWasiHttpRequest should be handled by the HTTP handler directly"
                        .to_string(),
                );
                let _ = response_tx.send(Err(err));
            }
        }
    }

    async fn info_loop(
        mut info_rx: Receiver<ActorInfo>,
        actor_instance_wrapper: Arc<RwLock<Option<PackInstance>>>,
        metrics: Arc<RwLock<MetricsCollector>>,
        actor_phase_manager: ActorPhaseManager,
    ) {
        // Handle info requests
        loop {
            tokio::select! {
                    biased;

                    _ = actor_phase_manager.wait_for_phase(ActorPhase::ShuttingDown) => {
                        break;
                    }

                    Some(info) = info_rx.recv() => {
                        info!("Received info request: {:?}", info);
                        match info {
                    ActorInfo::GetStatus { response_tx } => {
                        let status = actor_phase_manager.get_phase().to_string();

                        if let Err(e) = response_tx.send(Ok(status)) {
                            error!("Failed to send status response: {:?}", e);
                        }
                    }
                    ActorInfo::GetState { response_tx } => {
                        match &*actor_instance_wrapper.read().await {
                            Some(instance) => {
                                let state = instance.actor_store.get_state();
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
                                let chain = instance.actor_store.get_chain();
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
                        match &*actor_instance_wrapper.read().await {
                            None => {
                                let err =
                                    ActorError::UnexpectedError("Actor instance not found".to_string());
                                if let Err(e) = response_tx.send(Err(err)) {
                                    error!("Failed to send save chain error response: {:?}", e);
                                }
                                return;
                            }
                            Some(instance) => match instance.actor_store.save_chain() {
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
                }  // close match info
            }  // close Some(info) branch

                    else => break,
                } // close select!
        }
    }

    ///
    /// Calls a function in the WebAssembly actor with the given parameters,
    /// updates the actor's state based on the result, and records the
    /// operation in the actor's chain.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - The Composite actor instance
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
        actor_instance: &mut PackInstance,
        name: &String,
        params: Vec<u8>,
        _theater_tx: &mpsc::Sender<TheaterCommand>,
        metrics: &MetricsCollector,
    ) -> Result<Vec<u8>, ActorError> {
        // Validate the function exists
        if !actor_instance.has_function(name) {
            error!("Function '{}' not found in actor", name);
            return Err(ActorError::FunctionNotFound(name.to_string()));
        }

        let start = Instant::now();

        let state = actor_instance.actor_store.get_state();
        debug!(
            "Executing call to function '{}' with state size: {:?}",
            name,
            state.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        actor_instance.actor_store.record_event(ChainEventData {
            event_type: "wasm".to_string(),
            data: WasmEventData::WasmCall {
                function_name: name.clone(),
                params: params.clone(),
            }
            .into(),
        });

        // Execute the call
        let (new_state, results) = match actor_instance.call_function(name, state, params).await {
            Ok(result) => {
                actor_instance.actor_store.record_event(ChainEventData {
                    event_type: "wasm".to_string(),
                    data: WasmEventData::WasmResult {
                        function_name: name.clone(),
                        result: result.clone(),
                    }
                    .into(),
                });
                result
            }
            Err(e) => {
                let event = actor_instance.actor_store.record_event(ChainEventData {
                    event_type: "wasm".to_string(),
                    data: WasmEventData::WasmError {
                        function_name: name.clone(),
                        message: e.to_string(),
                    }
                    .into(),
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
        actor_instance.actor_store.set_state(new_state);

        // Record metrics
        let duration = start.elapsed();
        metrics.record_operation(duration, true).await;

        Ok(results)
    }
}
