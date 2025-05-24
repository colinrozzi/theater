//! # Actor Runtime
//!
//! The Actor Runtime is responsible for initializing, running, and managing the lifecycle
//! of WebAssembly actors within the Theater system. It coordinates the various components
//! that an actor needs to function, including execution, handlers, and communication channels.

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::actor::types::ActorOperation;
use crate::config::HandlerConfig;
use crate::config::ManifestConfig;
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
use crate::host::runtime::RuntimeHost;
use crate::host::store::StoreHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::timing::TimingHost;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::metrics::MetricsCollector;
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use crate::store::ContentStore;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::MessageServerConfig;
use crate::Result;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

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
}

impl ActorRuntime {
    /// # Start a new actor runtime
    ///
    /// Initializes and starts an actor runtime with the specified configuration,
    /// setting up all necessary components for the actor to run.
    ///
    /// ## Parameters
    ///
    /// * `id` - Unique identifier for the actor
    /// * `config` - Configuration for the actor from its manifest
    /// * `state_bytes` - Optional initial state for the actor
    /// * `theater_tx` - Channel for sending commands back to the Theater runtime
    /// * `actor_sender` - Channel for sending messages to the actor
    /// * `actor_mailbox` - Channel for receiving messages from other actors
    /// * `operation_rx` - Channel for receiving operations to perform
    /// * `operation_tx` - Channel for sending operations to the executor
    /// * `init` - Whether to initialize the actor (call the init function)
    /// * `parent_shutdown_receiver` - Receiver for shutdown signals from the parent
    /// * `response_tx` - Channel for sending the start result back to the caller
    ///
    /// This method is "start and forget" - it spawns the actor task and does not return anything.
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        state_bytes: Option<Vec<u8>>,
        theater_tx: Sender<TheaterCommand>,
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        init: bool,
        parent_shutdown_receiver: ShutdownReceiver,
        response_tx: Sender<StartActorResult>,
    ) {
        let actor_handle = ActorHandle::new(operation_tx.clone());

        // Setup actor store and manifest
        let (actor_store, _manifest_id) = match Self::setup_actor_store(
            id.clone(),
            theater_tx.clone(),
            actor_handle.clone(),
            config,
            &response_tx,
        )
        .await
        {
            Ok(result) => result,
            Err(_) => return, // Error already reported
        };

        // Create handlers
        let handlers = Self::create_handlers(
            actor_sender,
            actor_mailbox,
            theater_tx.clone(),
            config,
            actor_handle.clone(),
        );

        // Create component
        let mut actor_component =
            match Self::create_actor_component(config, actor_store, id.clone(), &response_tx).await
            {
                Ok(component) => component,
                Err(_) => return, // Error already reported
            };

        // Setup host functions
        let handlers = match Self::setup_host_functions(
            &mut actor_component,
            handlers,
            id.clone(),
            &response_tx,
        )
        .await
        {
            Ok(handlers) => handlers,
            Err(_) => return, // Error already reported
        };

        // Instantiate component
        let mut actor_instance =
            match Self::instantiate_component(actor_component, id.clone(), &response_tx).await {
                Ok(instance) => instance,
                Err(_) => return, // Error already reported
            };

        // Setup export functions
        if let Err(_) =
            Self::setup_export_functions(&mut actor_instance, &handlers, id.clone(), &response_tx)
                .await
        {
            return; // Error already reported
        }

        // Initialize state if needed
        let init_state = if init {
            match Self::initialize_state(config, state_bytes, id.clone(), &response_tx).await {
                Ok(state) => state,
                Err(_) => return, // Error already reported
            }
        } else {
            None
        };

        actor_instance.store.data_mut().set_state(init_state);

        // Start runtime
        Self::start_runtime(
            actor_instance,
            actor_handle.clone(),
            handlers,
            operation_rx,
            parent_shutdown_receiver,
            theater_tx,
            id.clone(),
            init,
            response_tx,
            config,
        )
        .await;
    }

    /// Sets up the actor store and stores the manifest
    async fn setup_actor_store(
        id: TheaterId,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
        config: &ManifestConfig,
        response_tx: &Sender<StartActorResult>,
    ) -> Result<(ActorStore, String)> {
        // Create actor store
        let actor_store =
            match ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone()) {
                Ok(store) => store,
                Err(e) => {
                    let error_message = format!("Failed to create actor store: {}", e);
                    error!("{}", error_message);
                    if let Err(send_err) = response_tx
                        .send(StartActorResult::Failure(id.clone(), error_message))
                        .await
                    {
                        error!("Failed to send failure response: {}", send_err);
                    }
                    return Err(e.into());
                }
            };

        // Store manifest
        let manifest_store = ContentStore::from_id("manifest");
        let manifest_id = match manifest_store
            .store(config.clone().into_fixed_bytes())
            .await
        {
            Ok(id) => id,
            Err(e) => {
                let error_message = format!("Failed to store manifest: {}", e);
                error!("{}", error_message);
                if let Err(send_err) = response_tx
                    .send(StartActorResult::Failure(id.clone(), error_message))
                    .await
                {
                    error!("Failed to send failure response: {}", send_err);
                }
                return Err(e.into());
            }
        };

        actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorLoadCall {
                manifest_id: manifest_id.clone(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Loading actor [{}] from manifest [{}] ", id, manifest_id).into(),
        });

        Ok((actor_store, manifest_id.to_string()))
    }

    /// Creates all the handlers needed by the actor
    fn create_handlers(
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        theater_tx: Sender<TheaterCommand>,
        config: &ManifestConfig,
        actor_handle: ActorHandle,
    ) -> Vec<Handler> {
        let mut handlers = Vec::new();

        if config
            .handlers
            .contains(&HandlerConfig::MessageServer(MessageServerConfig {}))
        {
            handlers.push(Handler::MessageServer(MessageServerHost::new(
                actor_sender,
                actor_mailbox,
                theater_tx.clone(),
            )));
        }

        for handler_config in &config.handlers {
            let handler = match handler_config {
                HandlerConfig::MessageServer(_) => {
                    debug!("MessageServer handler already created");
                    None
                }
                HandlerConfig::Environment(config) => {
                    Some(Handler::Environment(EnvironmentHost::new(config.clone())))
                }
                HandlerConfig::FileSystem(config) => {
                    Some(Handler::FileSystem(FileSystemHost::new(config.clone())))
                }
                HandlerConfig::HttpClient(config) => {
                    Some(Handler::HttpClient(HttpClientHost::new(config.clone())))
                }
                HandlerConfig::HttpFramework(_) => {
                    Some(Handler::HttpFramework(HttpFramework::new()))
                }
                HandlerConfig::Runtime(config) => Some(Handler::Runtime(RuntimeHost::new(
                    config.clone(),
                    theater_tx.clone(),
                ))),
                HandlerConfig::Supervisor(config) => {
                    Some(Handler::Supervisor(SupervisorHost::new(config.clone())))
                }
                HandlerConfig::Process(config) => Some(Handler::Process(ProcessHost::new(
                    config.clone(),
                    actor_handle.clone(),
                ))),
                HandlerConfig::Store(config) => {
                    Some(Handler::Store(StoreHost::new(config.clone())))
                }
                HandlerConfig::Timing(config) => {
                    Some(Handler::Timing(TimingHost::new(config.clone())))
                }
            };
            if let Some(handler) = handler {
                handlers.push(handler);
            }
        }

        handlers
    }

    /// Creates and initializes the actor component
    async fn create_actor_component(
        config: &ManifestConfig,
        actor_store: ActorStore,
        id: TheaterId,
        response_tx: &Sender<StartActorResult>,
    ) -> Result<ActorComponent> {
        match ActorComponent::new(config.name.clone(), config.component.clone(), actor_store).await
        {
            Ok(component) => Ok(component),
            Err(e) => {
                let error_message = format!(
                    "Failed to create actor component for actor {}: {}",
                    config.name, e
                );
                error!("{}", error_message);
                // Send failure result back to spawner with detailed error message
                if let Err(send_err) = response_tx
                    .send(StartActorResult::Failure(id.clone(), error_message))
                    .await
                {
                    error!("Failed to send failure response: {}", send_err);
                }
                Err(e.into())
            }
        }
    }

    /// Sets up host functions for all handlers
    async fn setup_host_functions(
        actor_component: &mut ActorComponent,
        mut handlers: Vec<Handler>,
        id: TheaterId,
        response_tx: &Sender<StartActorResult>,
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
                // Send failure result back to spawner with detailed error message
                if let Err(send_err) = response_tx
                    .send(StartActorResult::Failure(id.clone(), error_message))
                    .await
                {
                    error!("Failed to send failure response: {}", send_err);
                }
                return Err(e.into());
            }
        }
        Ok(handlers)
    }

    /// Instantiates the actor component
    async fn instantiate_component(
        actor_component: ActorComponent,
        id: TheaterId,
        response_tx: &Sender<StartActorResult>,
    ) -> Result<ActorInstance> {
        match actor_component.instantiate().await {
            Ok(instance) => Ok(instance),
            Err(e) => {
                let error_message = format!("Failed to instantiate actor {}: {}", id, e);
                error!("{}", error_message);
                // Send failure result back to spawner with detailed error message
                if let Err(send_err) = response_tx
                    .send(StartActorResult::Failure(id.clone(), error_message))
                    .await
                {
                    error!("Failed to send failure response: {}", send_err);
                }
                Err(e.into())
            }
        }
    }

    /// Sets up export functions for all handlers
    async fn setup_export_functions(
        actor_instance: &mut ActorInstance,
        handlers: &[Handler],
        id: TheaterId,
        response_tx: &Sender<StartActorResult>,
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
                // Send failure result back to spawner with detailed error message
                if let Err(send_err) = response_tx
                    .send(StartActorResult::Failure(id.clone(), error_message))
                    .await
                {
                    error!("Failed to send failure response: {}", send_err);
                }
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
        response_tx: &Sender<StartActorResult>,
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
                info!("Final init state ready: {:?}", state.is_some());
                Ok(state)
            }
            Err(e) => {
                let error_message = format!("Failed to merge initial states: {}", e);
                error!("{}", error_message);
                if let Err(send_err) = response_tx
                    .send(StartActorResult::Failure(id.clone(), error_message))
                    .await
                {
                    error!("Failed to send failure response: {}", send_err);
                }
                Err(e.into())
            }
        }
    }

    /// Starts the runtime, handlers, and operations processor
    async fn start_runtime(
        actor_instance: ActorInstance,
        actor_handle: ActorHandle,
        handlers: Vec<Handler>,
        operation_rx: Receiver<ActorOperation>,
        parent_shutdown_receiver: ShutdownReceiver,
        theater_tx: Sender<TheaterCommand>,
        id: TheaterId,
        init: bool,
        response_tx: Sender<StartActorResult>,
        config: &ManifestConfig,
    ) {
        // Create a local shutdown controller for this runtime
        let mut shutdown_controller = ShutdownController::new();

        // Start the handlers
        let mut handler_tasks: Vec<JoinHandle<()>> = Vec::new();
        for mut handler in handlers {
            info!("Starting handler: {:?}", handler.name());
            let handler_actor_handle = actor_handle.clone();
            let handler_shutdown = shutdown_controller.subscribe();
            let handler_task = tokio::spawn(async move {
                if let Err(e) = handler.start(handler_actor_handle, handler_shutdown).await {
                    warn!("Handler failed: {:?}", e);
                }
            });
            handler_tasks.push(handler_task);
        }

        // Notify the caller that the actor has started
        if let Err(e) = response_tx
            .send(StartActorResult::Success(id.clone()))
            .await
        {
            error!("Failed to send success response: {}", e);
            // Even though we couldn't send the response, we'll continue since setup was successful
        }

        // Prepare metrics collector
        let metrics = MetricsCollector::new();

        let config = config.clone();

        // Spawn the main runtime task that runs and processes operations
        tokio::spawn(async move {
            Self::run_operations_loop(
                actor_instance,
                operation_rx,
                metrics,
                parent_shutdown_receiver,
                theater_tx,
                shutdown_controller,
                handler_tasks,
                config,
            )
            .await;
        });

        // Initialize the actor if needed
        if init {
            tokio::spawn(async move {
                info!("Calling init function for actor: {:?}", id);
                match actor_handle
                    .call_function::<(String,), ()>(
                        "ntwk:theater/actor.init".to_string(),
                        (id.to_string(),),
                    )
                    .await
                {
                    Ok(_) => {
                        debug!("Successfully called init function for actor: {:?}", id);
                    } // Successfully called init function
                    Err(e) => {
                        let error_message =
                            format!("Failed to call init function for actor {}: {}", id, e);
                        error!("{}", error_message);
                        // We already notified success, so we can't send a failure now
                        // The best we can do is log the error
                    }
                }
            });
        }
    }

    /// # Run the actor operations processing loop
    ///
    /// Main loop for processing actor operations and handling shutdown signals.
    /// This method runs continuously until a shutdown is signaled or the operation
    /// channel is closed.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - The WebAssembly actor instance
    /// * `operation_rx` - Channel for receiving operations to perform
    /// * `metrics` - Collector for performance metrics
    /// * `shutdown_receiver` - Receiver for shutdown signals
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `shutdown_controller` - Controller for graceful shutdown
    /// * `handler_tasks` - Tasks for handling actor operations
    async fn run_operations_loop(
        mut actor_instance: ActorInstance,
        mut operation_rx: mpsc::Receiver<ActorOperation>,
        metrics: MetricsCollector,
        mut shutdown_receiver: ShutdownReceiver,
        theater_tx: mpsc::Sender<TheaterCommand>,
        shutdown_controller: crate::shutdown::ShutdownController,
        handler_tasks: Vec<JoinHandle<()>>,
        config: ManifestConfig,
    ) {
        info!("Actor runtime starting operation processing loop");
        let mut shutdown_initiated = false;
        let mut paused = false;

        loop {
            tokio::select! {
                // Monitor shutdown channel
                _ = &mut shutdown_receiver.receiver => {
                    info!("Actor runtime received shutdown signal");
                    debug!("Actor runtime starting shutdown sequence");
                    shutdown_initiated = true;
                    debug!("Shutdown status: {}", shutdown_initiated);
                    debug!("Actor runtime marked as shutting down, will reject new operations");
                    break;
                }
                Some(op) = operation_rx.recv() => {
                    if shutdown_initiated {
                        // Reject operations during shutdown
                        debug!("Rejecting operation during shutdown");
                        match op {
                            ActorOperation::Shutdown { response_tx } => {
                                let _ = response_tx.send(Ok(()));
                            }
                            ActorOperation::CallFunction { response_tx, .. } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::GetMetrics { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::GetChain { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::GetState { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::UpdateComponent { response_tx, .. } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::Pause { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::Resume { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::SaveChain { response_tx } => {
                                let response = match actor_instance.save_chain() {
                                    Ok(_) => Ok(()),
                                    Err(e) => Err(ActorError::UnexpectedError(e.to_string())),
                                };
                                response_tx.send(response).expect("Failed to send save chain response");
                            }
                        }
                        continue;
                    }
                    debug!("Processing actor operation");

                    if paused {
                            debug!("Actor is paused, rejecting operation");
                            match op {
                                ActorOperation::CallFunction { response_tx, .. } => {
                                    let _ = response_tx.send(Err(ActorError::Paused));
                                }
                        ActorOperation::GetMetrics { response_tx } => {
                            debug!("Processing GetMetrics operation");
                            let metrics = metrics.get_metrics().await;
                            if let Err(e) = response_tx.send(Ok(metrics)) {
                                error!("Failed to send metrics response: {:?}", e);
                            }
                            debug!("GetMetrics operation completed");
                        }

                        ActorOperation::GetChain { response_tx } => {
                            debug!("Processing GetChain operation");
                            let chain = actor_instance.store.data().get_chain();
                            debug!("Retrieved chain with {} events", chain.len());
                            if let Err(e) = response_tx.send(Ok(chain)) {
                                error!("Failed to send chain response: {:?}", e);
                            }
                            debug!("GetChain operation completed");
                        }

                        ActorOperation::GetState { response_tx } => {
                            debug!("Processing GetState operation");
                            let state = actor_instance.store.data().get_state();
                            debug!("Retrieved state with size: {:?}", state.as_ref().map(|s| s.len()).unwrap_or(0));
                            if let Err(e) = response_tx.send(Ok(state)) {
                                error!("Failed to send state response: {:?}", e);
                            }
                            debug!("GetState operation completed");
                        }

                        ActorOperation::UpdateComponent { component_address, response_tx } => {
                            debug!("Processing UpdateComponent operation for component: {}", component_address);

                            match Self::update_component(&mut actor_instance, &component_address, &config).await {
                                Ok(_) => {
                                    debug!("Component update successful");
                                    if let Err(e) = response_tx.send(Ok(())) {
                                        error!("Failed to send update component response: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("UpdateComponent operation failed: {:?}", e);
                                    if let Err(send_err) = response_tx.send(Err(e)) {
                                        error!("Failed to send update component error response: {:?}", send_err);
                                    }
                                }
                            }
                        }
                        ActorOperation::Shutdown { response_tx } => {
                            info!("Shutdown operation received while paused");
                            shutdown_initiated = true;
                            let _ = response_tx.send(Ok(()));
                            continue;
                        }
                        ActorOperation::Pause { response_tx } => {
                            let _ = response_tx.send(Err(ActorError::Paused));
                        }
                        ActorOperation::Resume { response_tx } => {
                            debug!("Resuming actor");
                            paused = false;
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send resume response: {:?}", e);
                            } else {
                                info!("Actor resumed successfully");
                            }
                        }
                        ActorOperation::SaveChain { response_tx } => {
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
                }
                            }
                    } else {


                    match op {
                        ActorOperation::CallFunction { name, params, response_tx } => {
                            match Self::execute_call(&mut actor_instance, &name, params, &theater_tx, &metrics).await {
                                Ok(result) => {
                                    if let Err(e) = response_tx.send(Ok(result)) {
                                        error!("Failed to send function call response for operation '{}': {:?}", name, e);
                                    }
                                }
                                Err(actor_error) => {
                                    // Notify the theater runtime about the error
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

                                    // Pause the actor
                                    paused = true;
                                }
                            }
                        }

                        ActorOperation::GetMetrics { response_tx } => {
                            debug!("Processing GetMetrics operation");
                            let metrics = metrics.get_metrics().await;
                            if let Err(e) = response_tx.send(Ok(metrics)) {
                                error!("Failed to send metrics response: {:?}", e);
                            }
                            debug!("GetMetrics operation completed");
                        }

                        ActorOperation::GetChain { response_tx } => {
                            debug!("Processing GetChain operation");
                            let chain = actor_instance.store.data().get_chain();
                            debug!("Retrieved chain with {} events", chain.len());
                            if let Err(e) = response_tx.send(Ok(chain)) {
                                error!("Failed to send chain response: {:?}", e);
                            }
                            debug!("GetChain operation completed");
                        }

                        ActorOperation::GetState { response_tx } => {
                            debug!("Processing GetState operation");
                            let state = actor_instance.store.data().get_state();
                            debug!("Retrieved state with size: {:?}", state.as_ref().map(|s| s.len()).unwrap_or(0));
                            if let Err(e) = response_tx.send(Ok(state)) {
                                error!("Failed to send state response: {:?}", e);
                            }
                            debug!("GetState operation completed");
                        }

                        ActorOperation::Shutdown { response_tx } => {
                            info!("Processing shutdown request");
                            shutdown_initiated = true;
                            debug!("Shutdown status: {}", shutdown_initiated);
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send shutdown confirmation: {:?}", e);
                            } else {
                                info!("Shutdown confirmation sent successfully");
                            }
                            info!("Breaking from operation loop to begin shutdown process");
                            break;
                        }

                        ActorOperation::UpdateComponent { component_address, response_tx } => {
                            debug!("Processing UpdateComponent operation for component: {}", component_address);

                            match Self::update_component(&mut actor_instance, &component_address, &config).await {
                                Ok(_) => {
                                    debug!("Component update successful");
                                    if let Err(e) = response_tx.send(Ok(())) {
                                        error!("Failed to send update component response: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("UpdateComponent operation failed: {:?}", e);
                                    if let Err(send_err) = response_tx.send(Err(e)) {
                                        error!("Failed to send update component error response: {:?}", send_err);
                                    }
                                }
                            }
                        }

                        ActorOperation::Pause { response_tx } => {
                            debug!("Processing Pause operation");
                            paused = true;
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send pause response: {:?}", e);
                            }
                            debug!("Actor paused successfully");
                        }

                                ActorOperation::Resume { response_tx } => {
                            debug!("Processing Resume operation");
                            paused = false;
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send resume response: {:?}", e);
                            }
                            debug!("Actor resumed successfully");
                                }
                            ActorOperation::SaveChain { response_tx } => {
                            debug!("Processing SaveChain operation");
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
                            debug!("SaveChain operation completed");
                            }
                        }
                        }
                }

                else => {
                    info!("Operation channel closed, shutting down");
                    break;
                }
            }
        }

        info!("Actor runtime operation loop shutting down");
        Self::perform_cleanup(shutdown_controller, handler_tasks, &metrics).await;
    }

    /// # Execute a function call in the WebAssembly actor
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
                timestamp: start.elapsed().as_secs(),
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
                        timestamp: start.elapsed().as_secs(),
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
                        timestamp: start.elapsed().as_secs(),
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
    async fn update_component(
        actor_instance: &mut ActorInstance,
        component_address: &str,
        config: &ManifestConfig,
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

        // Create a temporary response channel for error handling during setup
        let (response_tx, _) = mpsc::channel::<StartActorResult>(1);

        // Set up actor store - reuse the existing one without creating a new one
        let actor_store = actor_instance.actor_component.actor_store.clone();

        let (actor_sender_spoof, actor_mailbox_spoof) = mpsc::channel::<ActorMessage>(1);
        let (theater_tx_spoof, _) = mpsc::channel::<TheaterCommand>(1);
        let handlers = Self::create_handlers(
            actor_sender_spoof,
            actor_mailbox_spoof,
            theater_tx_spoof,
            &new_config,
            actor_instance
                .actor_component
                .actor_store
                .get_actor_handle(),
        );

        // Create new component - reusing existing method
        let mut new_actor_component = match Self::create_actor_component(
            &new_config,
            actor_store,
            actor_id.clone(),
            &response_tx,
        )
        .await
        {
            Ok(component) => component,
            Err(e) => {
                let error_message = format!("Failed to create actor component: {}", e);
                Self::record_update_error(actor_instance, component_address, &error_message);
                return Err(ActorError::UpdateComponentError(error_message));
            }
        };

        // Setup host functions - reusing existing method
        let handlers = match Self::setup_host_functions(
            &mut new_actor_component,
            handlers,
            actor_id.clone(),
            &response_tx,
        )
        .await
        {
            Ok(handlers) => handlers,
            Err(e) => {
                let error_message = format!("Failed to setup host functions: {}", e);
                Self::record_update_error(actor_instance, component_address, &error_message);
                return Err(ActorError::UpdateComponentError(error_message));
            }
        };

        // Instantiate component - reusing existing method
        let mut new_instance =
            match Self::instantiate_component(new_actor_component, actor_id.clone(), &response_tx)
                .await
            {
                Ok(instance) => instance,
                Err(e) => {
                    let error_message = format!("Failed to instantiate component: {}", e);
                    Self::record_update_error(actor_instance, component_address, &error_message);
                    return Err(ActorError::UpdateComponentError(error_message));
                }
            };

        // Setup export functions - reusing existing method
        if let Err(e) = Self::setup_export_functions(
            &mut new_instance,
            &handlers,
            actor_id.clone(),
            &response_tx,
        )
        .await
        {
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
        shutdown_controller.signal_shutdown().await;

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
        self.shutdown_controller.signal_shutdown().await;

        // Wait a bit for graceful shutdown
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

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
