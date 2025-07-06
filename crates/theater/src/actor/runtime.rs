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
use crate::host::environment::EnvironmentHost;
use crate::host::filesystem::FileSystemHost;
use crate::host::framework::HttpFramework;
use crate::host::handler::Handler;
use crate::host::http_client::HttpClientHost;
use crate::host::message_server::MessageServerHost;
use crate::host::process::ProcessHost;
use crate::host::random::RandomHost;
use crate::host::runtime::RuntimeHost;
use crate::host::store::StoreHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::timing::TimingHost;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::metrics::MetricsCollector;
use crate::shutdown::ShutdownType;
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use crate::store::ContentStore;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::HandlerConfig;
use crate::ManifestConfig;
use crate::MessageServerConfig;
use crate::Result;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;
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
pub struct ActorRuntime {
    /// Unique identifier for this actor
    pub actor_id: TheaterId,
    /// Handles to the running handler tasks
    pub handler_tasks: Vec<JoinHandle<()>>,
    /// Controller for graceful shutdown of all components
    pub shutdown_controller: ShutdownController,
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

impl ActorRuntime {
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        state_bytes: Option<Vec<u8>>,
        theater_tx: Sender<TheaterCommand>,
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        mut operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        mut info_rx: Receiver<ActorInfo>,
        info_tx: Sender<ActorInfo>,
        mut control_rx: Receiver<ActorControl>,
        control_tx: Sender<ActorControl>,
        init: bool,
        mut parent_shutdown_receiver: ShutdownReceiver,
        engine: wasmtime::Engine,
        parent_permissions: HandlerPermission,
    ) {
        info!("Actor runtime starting communication loops");
        let paused = Arc::new(RwLock::new(false));

        // Create setup task with status reporting
        let (status_tx, mut status_rx) = mpsc::channel::<String>(10);
        let mut setup_task = Some(tokio::spawn(Self::build_actor_resources(
            id.clone(),
            config.clone(),
            state_bytes,
            theater_tx.clone(),
            actor_sender,
            actor_mailbox,
            operation_tx.clone(),
            info_tx.clone(),
            control_tx.clone(),
            init,
            engine,
            parent_permissions,
            status_tx,
        )));

        // These will be set once setup completes
        let mut actor_instance: Option<Arc<RwLock<ActorInstance>>> = None;
        let mut metrics: Option<Arc<RwLock<MetricsCollector>>> = None;
        let mut handler_tasks: Vec<JoinHandle<()>> = Vec::new();

        let mut current_operation: Option<JoinHandle<()>> = None;
        let mut shutdown_requested = false;
        let mut shutdown_response_tx: Option<oneshot::Sender<Result<(), ActorError>>> = None;
        let mut shutdown_controller = ShutdownController::new();
        let mut current_status = "Starting".to_string();

        loop {
            tokio::select! {
                // Handle setup completion
                result = async {
                    match setup_task.as_mut() {
                        Some(task) => task.await,
                        None => std::future::pending().await,
                    }
                } => {
                    match result {
                        Ok(Ok((instance, handlers, metrics_collector))) => {
                            info!("Actor setup completed successfully");
                            actor_instance = Some(Arc::new(RwLock::new(instance)));
                            metrics = Some(Arc::new(RwLock::new(metrics_collector)));
                            setup_task = None; // Clear the task
                            current_status = "Running".to_string();
                            let actor_handle = ActorHandle::new(operation_tx.clone(), info_tx.clone(), control_tx.clone());

                            // Call init function if needed
                            if init {
                                let init_id = id.clone();
                                let actor_handle = actor_handle.clone();
                                info!("Calling init function for actor: {:?}", init_id);
                                tokio::spawn(async move {
                                    match actor_handle
                                        .call_function::<(String,), ()>(
                                            "theater:simple/actor.init".to_string(),
                                            (init_id.to_string(),),
                                        )
                                        .await
                                    {
                                        Ok(_) => {
                                            debug!("Successfully called init function for actor: {:?}", init_id);
                                        }
                                        Err(e) => {
                                            error!("Failed to call init function for actor {}: {}", init_id, e);
                                        }
                                    }
                                });
                            }

                            // Start the handler tasks
                            for mut handler in handlers {
                                info!("Starting handler: {:?}", handler.name());
                                let handler_actor_handle = actor_handle.clone();
                                let handler_shutdown = shutdown_controller.subscribe();
                                let theater_tx = theater_tx.clone();
                                let id = id.clone();
                                let handler_task = tokio::spawn(async move {
                                    match handler.start(
                                        handler_actor_handle,
                                        handler_shutdown,
                                    ).await {
                                        Ok(_) => {
                                            info!("Handler {} started successfully", handler.name());
                                        }
                                        Err(e) => {
                                            error!("Failed to start handler {}: {:?}", handler.name(), e);
                                            // Notify theater runtime of the error
                                            let _ = theater_tx.send(TheaterCommand::ActorError {
                                                actor_id: id.clone(),
                                                error: ActorError::HandlerError(e.to_string()),
                                            }).await;
                                        }
                                    }
                                });
                                handler_tasks.push(handler_task);
                            }
                            info!("All handlers started successfully for actor: {}", id);

                            // If shutdown was requested during startup, handle it now
                            if shutdown_requested {
                                info!("Shutdown was requested during startup, shutting down now");
                                if let Some(response_tx) = shutdown_response_tx.take() {
                                    let _ = response_tx.send(Ok(()));
                                }
                                break; // Exit the loop
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Actor setup failed: {:?}", e);

                            // Notify theater runtime of the error
                            let _ = theater_tx.send(TheaterCommand::ActorError {
                                actor_id: id.clone(),
                                error: e.clone(),
                            }).await;


                            // Handle any pending shutdown request
                            if let Some(response_tx) = shutdown_response_tx.take() {
                                let _ = response_tx.send(Err(e));
                            }
                            break; // Exit on setup failure
                        }
                        Err(e) => {
                            error!("Setup task panicked: {:?}", e);

                            // Handle any pending shutdown request
                            if let Some(response_tx) = shutdown_response_tx.take() {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError(e.to_string())));
                            }
                            break;
                        }
                    }
                }

                // Handle status updates from setup process
                Some(new_status) = status_rx.recv() => {
                    current_status = new_status;
                    debug!("Actor {} startup status: {}", id, current_status);
                }

                // Handle operations only after setup is complete
                Some(op) = operation_rx.recv(), if actor_instance.is_some() && current_operation.is_none() && !*paused.read().await => {
                    let actor_instance = actor_instance.as_ref().unwrap();
                    let metrics = metrics.as_ref().unwrap();

                    info!("Received operation: {:?}", op);
                    match op {
                        ActorOperation::CallFunction { name, params, response_tx } => {
                            info!("Processing function call: {}", name);
                            let theater_tx = theater_tx.clone();
                            let metrics = metrics.clone();
                            let actor_instance = actor_instance.clone();
                            let paused = paused.clone();

                            current_operation = Some(tokio::spawn(async move {
                                let mut actor_instance = actor_instance.write().await;
                                let metrics = metrics.write().await;
                                match Self::execute_call(
                                    &mut actor_instance,
                                    &name,
                                    params,
                                    &theater_tx,
                                    &metrics,
                                ).await {
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
                            }));
                        }
                        ActorOperation::UpdateComponent { component_address: _, response_tx } => {
                            error!("UpdateComponent operation is not implemented yet");
                            if let Err(e) = response_tx.send(Err(ActorError::UpdateComponentError("Not implemented".to_string()))) {
                                error!("Failed to send update component response: {:?}", e);
                            }
                        }
                    }
                }

                // Clean up completed operations
                _ = async {
                    match current_operation.as_mut() {
                        Some(task) => task.await,
                        None => std::future::pending().await,
                    }
                } => {
                    info!("Operation completed");
                    current_operation = None;

                    // Check if shutdown was requested and no more operations are running
                    if shutdown_requested {
                        info!("Shutdown requested and operation completed - shutting down gracefully");
                        if let Some(response_tx) = shutdown_response_tx.take() {
                            let _ = response_tx.send(Ok(()));
                        }
                        break;
                    }
                }

                // Handle info requests (works during startup too!)
                Some(info) = info_rx.recv() => {
                    info!("Received info request: {:?}", info);
                    match info {
                        ActorInfo::GetStatus { response_tx } => {
                            let status = if shutdown_requested {
                                if setup_task.is_some() {
                                    "Shutting down (during startup)".to_string()
                                } else if current_operation.is_some() {
                                    "Shutting down (waiting for operation)".to_string()
                                } else {
                                    "Shutting down".to_string()
                                }
                            } else if setup_task.is_some() {
                                current_status.clone()
                            } else if *paused.read().await {
                                "Paused".to_string()
                            } else if current_operation.is_some() {
                                "Processing".to_string()
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
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
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
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
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
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
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
                                        if let Err(send_err) = response_tx.send(Err(ActorError::UnexpectedError(e.to_string()))) {
                                            error!("Failed to send save chain error response: {:?}", send_err);
                                        }
                                    }
                                }
                            } else {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Actor still starting".to_string())));
                            }
                        }
                    }
                }

                // Handle control commands (works during startup too!)
                Some(control) = control_rx.recv() => {
                    info!("Received control command: {:?}", control);
                    match control {
                        ActorControl::Shutdown { response_tx } => {
                            info!("Shutdown requested");
                            if setup_task.is_some() {
                                // Still setting up - mark for shutdown after setup completes
                                info!("Shutdown requested during setup - will shutdown after setup completes");
                                shutdown_requested = true;
                                shutdown_response_tx = Some(response_tx);
                                current_status = "Shutting down (during startup)".to_string();
                            } else if current_operation.is_some() {
                                // Operation running - mark for shutdown after completion
                                info!("Operation running - will shutdown after completion");
                                shutdown_requested = true;
                                shutdown_response_tx = Some(response_tx);
                            } else {
                                // No operation running - shutdown immediately
                                info!("No operation running - shutting down immediately");
                                let _ = response_tx.send(Ok(()));
                                break;
                            }
                        }
                        ActorControl::Terminate { response_tx } => {
                            info!("Terminate requested");
                            // Abort setup or current operation
                            if let Some(task) = setup_task.take() {
                                task.abort();
                            }
                            if let Some(task) = current_operation.take() {
                                task.abort();
                            }
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send terminate confirmation: {:?}", e);
                            }
                            break;
                        }
                        ActorControl::Pause { response_tx } => {
                            if setup_task.is_some() {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Cannot pause during startup".to_string())));
                            } else if shutdown_requested {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            } else {
                                *paused.write().await = true;
                                let _ = response_tx.send(Ok(()));
                            }
                        }
                        ActorControl::Resume { response_tx } => {
                            if setup_task.is_some() {
                                let _ = response_tx.send(Err(ActorError::UnexpectedError("Cannot resume during startup".to_string())));
                            } else if shutdown_requested {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            } else {
                                let response = if *paused.read().await {
                                    *paused.write().await = false;
                                    Ok(())
                                } else {
                                    Err(ActorError::NotPaused)
                                };
                                let _ = response_tx.send(response);
                            }
                        }
                    }
                }

                // Handle parent shutdown signals
                shutdown_signal = &mut parent_shutdown_receiver.receiver => {
                    info!("Received shutdown signal from parent");
                    match shutdown_signal {
                        Ok(shutdown_signal) => {
                            info!("Shutdown signal received: {:?}", shutdown_signal);
                            match shutdown_signal.shutdown_type {
                                ShutdownType::Graceful => {
                                    info!("Graceful shutdown requested");
                                    if setup_task.is_some() || current_operation.is_some() {
                                        info!("Setup/operation running - will shutdown after completion");
                                        shutdown_requested = true;
                                    } else {
                                        info!("No setup/operation running - shutting down immediately");
                                        break;
                                    }
                                }
                                ShutdownType::Force => {
                                    info!("Forceful shutdown requested");
                                    if let Some(task) = setup_task.take() {
                                        task.abort();
                                    }
                                    if let Some(task) = current_operation.take() {
                                        task.abort();
                                    }
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to receive shutdown signal: {:?}", e);
                            info!("Exiting runtime communication loop due to shutdown signal error");
                            if setup_task.is_some() || current_operation.is_some() {
                                shutdown_requested = true;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }

        info!("Actor runtime communication loop exiting, performing cleanup");
        if let Some(ref metrics) = metrics {
            let metrics = metrics.read().await;
            Self::perform_cleanup(shutdown_controller, handler_tasks, &metrics).await;
        } else {
            info!("Actor was shut down during startup, no cleanup needed");
        }
    }

    /// Builds the complete actor instance using existing startup logic
    async fn build_actor_resources(
        id: TheaterId,
        config: ManifestConfig,
        state_bytes: Option<Vec<u8>>,
        theater_tx: Sender<TheaterCommand>,
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_tx: Sender<ActorOperation>,
        info_tx: Sender<ActorInfo>,
        control_tx: Sender<ActorControl>,
        init: bool,
        engine: wasmtime::Engine,
        parent_permissions: HandlerPermission,
        status_tx: Sender<String>,
    ) -> Result<(ActorInstance, Vec<Handler>, MetricsCollector), ActorError> {
        let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

        let _ = status_tx.send("Setting up actor store".to_string()).await;

        // Setup actor store and manifest
        let (actor_store, _manifest_id) = Self::setup_actor_store(
            id.clone(),
            theater_tx.clone(),
            actor_handle.clone(),
            &config,
        )
        .await
        .map_err(|e| ActorError::UnexpectedError(format!("Failed to setup actor store: {}", e)))?;

        let _ = status_tx.send("Validating permissions".to_string()).await;

        // Calculate effective permissions
        let effective_permissions = config.calculate_effective_permissions(&parent_permissions);

        // Validate that the manifest doesn't exceed effective permissions
        if let Err(e) = crate::config::enforcement::validate_manifest_permissions(
            &config,
            &effective_permissions,
        ) {
            return Err(ActorError::UnexpectedError(format!(
                "Permission validation failed: {}",
                e
            )));
        }

        let _ = status_tx.send("Creating handlers".to_string()).await;

        // Create handlers with effective permissions and validation
        let handlers = Self::create_handlers(
            actor_sender,
            actor_mailbox,
            theater_tx.clone(),
            &config,
            actor_handle.clone(),
            &effective_permissions,
        )
        .map_err(|e| ActorError::UnexpectedError(format!("Handler creation failed: {}", e)))?;

        let _ = status_tx.send("Creating component".to_string()).await;

        // Create component
        let mut actor_component =
            Self::create_actor_component(&config, actor_store, engine.clone())
                .await
                .map_err(|e| {
                    ActorError::UnexpectedError(format!("Component creation failed: {}", e))
                })?;

        let _ = status_tx
            .send("Setting up host functions".to_string())
            .await;

        // Setup host functions
        let handlers = Self::setup_host_functions(&mut actor_component, handlers)
            .await
            .map_err(|e| {
                ActorError::UnexpectedError(format!("Host function setup failed: {}", e))
            })?;

        let _ = status_tx.send("Instantiating component".to_string()).await;

        // Instantiate component
        let mut actor_instance = Self::instantiate_component(actor_component, id.clone())
            .await
            .map_err(|e| {
                ActorError::UnexpectedError(format!("Component instantiation failed: {}", e))
            })?;

        let _ = status_tx
            .send("Setting up export functions".to_string())
            .await;

        // Setup export functions
        Self::setup_export_functions(&mut actor_instance, &handlers)
            .await
            .map_err(|e| {
                ActorError::UnexpectedError(format!("Export function setup failed: {}", e))
            })?;

        let _ = status_tx.send("Initializing state".to_string()).await;

        // Initialize state if needed
        let init_state = if init {
            Self::initialize_state(&config, state_bytes, id.clone())
                .await
                .map_err(|e| {
                    ActorError::UnexpectedError(format!("State initialization failed: {}", e))
                })?
        } else {
            None
        };

        actor_instance.store.data_mut().set_state(init_state);

        let _ = status_tx.send("Ready".to_string()).await;

        let metrics = MetricsCollector::new();

        Ok((actor_instance, handlers, metrics))
    }

    /// Sets up the actor store and stores the manifest
    async fn setup_actor_store(
        id: TheaterId,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
        config: &ManifestConfig,
    ) -> Result<(ActorStore, String)> {
        // Create actor store
        let actor_store =
            match ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone()) {
                Ok(store) => store,
                Err(e) => {
                    let error_message = format!("Failed to create actor store: {}", e);
                    error!("{}", error_message);
                    return Err(e.into());
                }
            };

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

        Ok((actor_store, manifest_id.to_string()))
    }

    /// Creates all the handlers needed by the actor with permission validation
    fn create_handlers(
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        theater_tx: Sender<TheaterCommand>,
        config: &ManifestConfig,
        actor_handle: ActorHandle,
        effective_permissions: &crate::config::permissions::HandlerPermission,
    ) -> Result<Vec<Handler>, String> {
        let mut handlers = Vec::new();

        // Check if message server is permitted and requested
        if config
            .handlers
            .contains(&HandlerConfig::MessageServer(MessageServerConfig {}))
        {
            if effective_permissions.message_server.is_none() {
                return Err(
                    "MessageServer handler requested but not permitted by effective permissions"
                        .to_string(),
                );
            }
            handlers.push(Handler::MessageServer(
                MessageServerHost::new(
                    actor_sender,
                    actor_mailbox,
                    theater_tx.clone(),
                    effective_permissions.message_server.clone(),
                ),
                effective_permissions.message_server.clone(),
            ));
        }

        for handler_config in &config.handlers {
            let handler = match handler_config {
                HandlerConfig::MessageServer(_) => {
                    debug!("MessageServer handler already created");
                    None
                }
                HandlerConfig::Environment(config) => {
                    // Check permission before creating handler
                    if effective_permissions.environment.is_none() {
                        return Err("Environment handler requested but not permitted by effective permissions".to_string());
                    }
                    Some(Handler::Environment(
                        EnvironmentHost::new(
                            config.clone(),
                            effective_permissions.environment.clone(),
                        ),
                        effective_permissions.environment.clone(),
                    ))
                }
                HandlerConfig::FileSystem(config) => {
                    // Check permission before creating handler
                    if effective_permissions.file_system.is_none() {
                        return Err("FileSystem handler requested but not permitted by effective permissions".to_string());
                    }
                    Some(Handler::FileSystem(
                        FileSystemHost::new(
                            config.clone(),
                            effective_permissions.file_system.clone(),
                        ),
                        effective_permissions.file_system.clone(),
                    ))
                }
                HandlerConfig::HttpClient(config) => {
                    // Check permission before creating handler
                    if effective_permissions.http_client.is_none() {
                        return Err("HttpClient handler requested but not permitted by effective permissions".to_string());
                    }
                    Some(Handler::HttpClient(
                        HttpClientHost::new(
                            config.clone(),
                            effective_permissions.http_client.clone(),
                        ),
                        effective_permissions.http_client.clone(),
                    ))
                }
                HandlerConfig::HttpFramework(_) => {
                    // Check permission before creating handler
                    if effective_permissions.http_framework.is_none() {
                        return Err("HttpFramework handler requested but not permitted by effective permissions".to_string());
                    }
                    Some(Handler::HttpFramework(
                        HttpFramework::new(effective_permissions.http_framework.clone()),
                        effective_permissions.http_framework.clone(),
                    ))
                }
                HandlerConfig::Runtime(config) => {
                    // Check permission before creating handler
                    if effective_permissions.runtime.is_none() {
                        return Err(
                            "Runtime handler requested but not permitted by effective permissions"
                                .to_string(),
                        );
                    }
                    Some(Handler::Runtime(
                        RuntimeHost::new(
                            config.clone(),
                            theater_tx.clone(),
                            effective_permissions.runtime.clone(),
                        ),
                        effective_permissions.runtime.clone(),
                    ))
                }
                HandlerConfig::Supervisor(config) => {
                    // Check permission before creating handler
                    if effective_permissions.supervisor.is_none() {
                        return Err("Supervisor handler requested but not permitted by effective permissions".to_string());
                    }
                    Some(Handler::Supervisor(
                        SupervisorHost::new(
                            config.clone(),
                            effective_permissions.supervisor.clone(),
                        ),
                        effective_permissions.supervisor.clone(),
                    ))
                }
                HandlerConfig::Process(config) => {
                    // Check permission before creating handler
                    if effective_permissions.process.is_none() {
                        return Err(
                            "Process handler requested but not permitted by effective permissions"
                                .to_string(),
                        );
                    }
                    Some(Handler::Process(
                        ProcessHost::new(
                            config.clone(),
                            actor_handle.clone(),
                            effective_permissions.process.clone(),
                        ),
                        effective_permissions.process.clone(),
                    ))
                }
                HandlerConfig::Store(config) => {
                    // Check permission before creating handler
                    if effective_permissions.store.is_none() {
                        return Err(
                            "Store handler requested but not permitted by effective permissions"
                                .to_string(),
                        );
                    }
                    Some(Handler::Store(
                        StoreHost::new(config.clone(), effective_permissions.store.clone()),
                        effective_permissions.store.clone(),
                    ))
                }
                HandlerConfig::Timing(config) => {
                    // Check permission before creating handler
                    if effective_permissions.timing.is_none() {
                        return Err(
                            "Timing handler requested but not permitted by effective permissions"
                                .to_string(),
                        );
                    }
                    Some(Handler::Timing(
                        TimingHost::new(config.clone(), effective_permissions.timing.clone()),
                        effective_permissions.timing.clone(),
                    ))
                }
                HandlerConfig::Random(config) => {
                    // Check permission before creating handler
                    if effective_permissions.random.is_none() {
                        return Err(
                            "Random handler requested but not permitted by effective permissions"
                                .to_string(),
                        );
                    }
                    Some(Handler::Random(
                        RandomHost::new(config.clone(), effective_permissions.random.clone()),
                        effective_permissions.random.clone(),
                    ))
                }
            };
            if let Some(handler) = handler {
                handlers.push(handler);
            }
        }

        Ok(handlers)
    }

    /// Creates and initializes the actor component
    async fn create_actor_component(
        config: &ManifestConfig,
        actor_store: ActorStore,
        engine: wasmtime::Engine,
    ) -> Result<ActorComponent> {
        match ActorComponent::new(
            config.name.clone(),
            config.component.clone(),
            actor_store,
            engine,
        )
        .await
        {
            Ok(component) => Ok(component),
            Err(e) => {
                let error_message = format!(
                    "Failed to create actor component for actor {}: {}",
                    config.name, e
                );
                error!("{}", error_message);
                Err(e.into())
            }
        }
    }

    /// Sets up host functions for all handlers
    async fn setup_host_functions(
        actor_component: &mut ActorComponent,
        mut handlers: Vec<Handler>,
    ) -> Result<Vec<Handler>> {
        for handler in &mut handlers {
            info!(
                "Setting up host functions for handler: {:?}",
                handler.name()
            );
            if let Err(e) = handler.setup_host_functions(actor_component).await {
                let error_message = format!(
                    "Failed to set up host functions for handler {}: {}",
                    handler.name(),
                    e
                );
                error!("{}", error_message);
                return Err(e.into());
            }
        }
        Ok(handlers)
    }

    /// Instantiates the actor component
    async fn instantiate_component(
        actor_component: ActorComponent,
        id: TheaterId,
    ) -> Result<ActorInstance> {
        match actor_component.instantiate().await {
            Ok(instance) => Ok(instance),
            Err(e) => {
                let error_message = format!("Failed to instantiate actor {}: {}", id, e);
                error!("{}", error_message);
                Err(e.into())
            }
        }
    }

    /// Sets up export functions for all handlers
    async fn setup_export_functions(
        actor_instance: &mut ActorInstance,
        handlers: &[Handler],
    ) -> Result<()> {
        for handler in handlers {
            info!("Creating functions for handler: {:?}", handler.name());
            if let Err(e) = handler.add_export_functions(actor_instance).await {
                let error_message = format!(
                    "Failed to create export functions for handler {}: {}",
                    handler.name(),
                    e
                );
                error!("{}", error_message);
                return Err(e.into());
            }
        }
        Ok(())
    }

    /// Initializes and merges actor state
    async fn initialize_state(
        config: &ManifestConfig,
        state_bytes: Option<Vec<u8>>,
        id: TheaterId,
    ) -> Result<Option<Vec<u8>>> {
        info!("Loading init state for actor: {:?}", id);

        // Get state from config if available
        let config_state = config
            .load_init_state()
            .await
            .expect("Failed to load init state");

        // Merge with provided state
        match crate::utils::merge_initial_states(config_state, state_bytes) {
            Ok(state) => {
                info!("Final init state ready: {:?}", state);
                Ok(state)
            }
            Err(e) => {
                let error_message = format!("Failed to merge initial states: {}", e);
                error!("{}", error_message);
                Err(e.into())
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

    /// Updates the WebAssembly component of an actor instance.
    ///
    /// This method implements hot-swapping of the WebAssembly component while preserving
    /// the actor's state and reusing the existing setup logic.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - Mutable reference to the actor instance to update
    /// * `component_address` - The address of the new component to load
    /// * `handlers` - The existing handlers
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - If the component was successfully updated
    /// * `Err(ActorError)` - If the update failed
    #[allow(dead_code)]
    async fn update_component(
        actor_instance: &mut ActorInstance,
        component_address: &str,
        config: &ManifestConfig,
        engine: wasmtime::Engine,
    ) -> Result<(), ActorError> {
        let actor_id = actor_instance.id();
        info!(
            "Updating component for actor {} to: {}",
            actor_id, component_address
        );

        let mut new_config = config.clone();

        // Get current state before updating
        let current_state = actor_instance.store.data().get_state();

        // Record update started event
        actor_instance
            .store
            .data_mut()
            .record_event(ChainEventData {
                event_type: "theater-runtime".to_string(),
                data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorUpdateStart {
                    new_component_address: component_address.to_string(),
                }),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: format!("Starting update to component [{}]", component_address).into(),
            });

        // Create a temporary config for the new component
        new_config.component = component_address.to_string();

        // Set up actor store - reuse the existing one without creating a new one
        let actor_store = actor_instance.actor_component.actor_store.clone();

        let (actor_sender_spoof, actor_mailbox_spoof) = mpsc::channel::<ActorMessage>(1);
        let (theater_tx_spoof, _) = mpsc::channel::<TheaterCommand>(1);

        // Calculate effective permissions for update (use root permissions for now)
        let parent_permissions = crate::config::permissions::HandlerPermission::root();
        let effective_permissions = new_config.calculate_effective_permissions(&parent_permissions);

        let handlers = match Self::create_handlers(
            actor_sender_spoof,
            actor_mailbox_spoof,
            theater_tx_spoof,
            &new_config,
            actor_instance
                .actor_component
                .actor_store
                .get_actor_handle(),
            &effective_permissions,
        ) {
            Ok(handlers) => handlers,
            Err(e) => {
                error!("Handler creation failed during update: {}", e);
                todo!();
                //return Err(ActorError::UpdateError(format!("Handler creation failed: {}", e)));
            }
        };

        // Create new component - reusing existing method
        let mut new_actor_component =
            match Self::create_actor_component(&new_config, actor_store, engine).await {
                Ok(component) => component,
                Err(e) => {
                    let error_message = format!("Failed to create actor component: {}", e);
                    Self::record_update_error(actor_instance, component_address, &error_message);
                    return Err(ActorError::UpdateComponentError(error_message));
                }
            };

        // Setup host functions - reusing existing method
        let handlers = match Self::setup_host_functions(&mut new_actor_component, handlers).await {
            Ok(handlers) => handlers,
            Err(e) => {
                let error_message = format!("Failed to setup host functions: {}", e);
                Self::record_update_error(actor_instance, component_address, &error_message);
                return Err(ActorError::UpdateComponentError(error_message));
            }
        };

        // Instantiate component - reusing existing method
        let mut new_instance =
            match Self::instantiate_component(new_actor_component, actor_id.clone()).await {
                Ok(instance) => instance,
                Err(e) => {
                    let error_message = format!("Failed to instantiate component: {}", e);
                    Self::record_update_error(actor_instance, component_address, &error_message);
                    return Err(ActorError::UpdateComponentError(error_message));
                }
            };

        // Setup export functions - reusing existing method
        if let Err(e) = Self::setup_export_functions(&mut new_instance, &handlers).await {
            let error_message = format!("Failed to setup export functions: {}", e);
            Self::record_update_error(actor_instance, component_address, &error_message);
            return Err(ActorError::UpdateComponentError(error_message));
        }

        // Swap the instance
        std::mem::swap(actor_instance, &mut new_instance);

        // Restore state
        actor_instance.store.data_mut().set_state(current_state);

        // Record update success
        actor_instance
            .store
            .data_mut()
            .record_event(ChainEventData {
                event_type: "theater-runtime".to_string(),
                data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorUpdateComplete {
                    new_component_address: component_address.to_string(),
                }),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: format!("Successfully updated to component [{}]", component_address)
                    .into(),
            });

        info!("Component updated successfully for actor {}", actor_id);
        Ok(())
    }

    /// Helper method to record update errors in the actor's chain
    #[allow(dead_code)]
    fn record_update_error(
        actor_instance: &mut ActorInstance,
        component_address: &str,
        error_message: &str,
    ) {
        error!("{}", error_message);
        actor_instance
            .store
            .data_mut()
            .record_event(ChainEventData {
                event_type: "theater-runtime".to_string(),
                data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorUpdateError {
                    new_component_address: component_address.to_string(),
                    error: error_message.to_string(),
                }),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                description: format!(
                    "Failed to update to component [{}]: {}",
                    component_address, error_message
                )
                .into(),
            });
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

