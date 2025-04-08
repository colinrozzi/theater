//! # Actor Runtime
//!
//! The Actor Runtime is responsible for initializing, running, and managing the lifecycle
//! of WebAssembly actors within the Theater system. It coordinates the various components
//! that an actor needs to function, including execution, handlers, and communication channels.

use crate::actor::handle::ActorHandle;
use crate::actor::operations::OperationsProcessor;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorOperation;
use crate::config::HandlerConfig;
use crate::config::ManifestConfig;
use crate::events::theater_runtime::TheaterRuntimeEventData;
use crate::events::{ChainEventData, EventData};
use crate::host::filesystem::FileSystemHost;
use crate::host::framework::HttpFramework;
use crate::host::handler::Handler;
use crate::host::http_client::HttpClientHost;
use crate::host::message_server::MessageServerHost;
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
use crate::Result;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
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
        let (actor_store, manifest_id) = match Self::setup_actor_store(
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
        let handlers =
            Self::create_handlers(actor_sender, actor_mailbox, theater_tx.clone(), config);

        // Create component
        let mut actor_component =
            match Self::create_actor_component(config, actor_store, id.clone(), &response_tx).await
            {
                Ok(component) => component,
                Err(_) => return, // Error already reported
            };

        // Setup host functions
        let mut handlers = match Self::setup_host_functions(
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
    ) -> Vec<Handler> {
        let mut handlers = Vec::new();

        handlers.push(Handler::MessageServer(MessageServerHost::new(
            actor_sender,
            actor_mailbox,
            theater_tx.clone(),
        )));

        for handler_config in &config.handlers {
            let handler = match handler_config {
                HandlerConfig::MessageServer(_) => {
                    panic!("MessageServer handler is already added")
                }
                HandlerConfig::FileSystem(config) => {
                    Handler::FileSystem(FileSystemHost::new(config.clone()))
                }
                HandlerConfig::HttpClient(config) => {
                    Handler::HttpClient(HttpClientHost::new(config.clone()))
                }
                HandlerConfig::HttpFramework(_) => Handler::HttpFramework(HttpFramework::new()),
                HandlerConfig::Runtime(config) => {
                    Handler::Runtime(RuntimeHost::new(config.clone()))
                }
                HandlerConfig::Supervisor(config) => {
                    Handler::Supervisor(SupervisorHost::new(config.clone()))
                }
                HandlerConfig::Store(config) => Handler::Store(StoreHost::new(config.clone())),
                HandlerConfig::Timing(config) => Handler::Timing(TimingHost::new(config.clone())),
            };
            handlers.push(handler);
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
        match ActorComponent::new(
            config.name.clone(),
            config.component_path.clone(),
            actor_store,
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
        let config_state = config.load_init_state().unwrap_or(None);

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
        mut actor_instance: ActorInstance,
        actor_handle: ActorHandle,
        handlers: Vec<Handler>,
        operation_rx: Receiver<ActorOperation>,
        parent_shutdown_receiver: ShutdownReceiver,
        theater_tx: Sender<TheaterCommand>,
        id: TheaterId,
        init: bool,
        response_tx: Sender<StartActorResult>,
    ) {
        // Create a local shutdown controller for this runtime
        let (shutdown_controller, _) = ShutdownController::new();

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

        // Spawn the main runtime task that runs and processes operations
        tokio::spawn(async move {
            OperationsProcessor::run(
                actor_instance,
                operation_rx,
                metrics,
                parent_shutdown_receiver,
                theater_tx,
                shutdown_controller,
                handler_tasks,
            )
            .await;
        });

        // Initialize the actor if needed
        if init {
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
        }
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
    pub async fn stop(&mut self) -> Result<()> {
        info!("Initiating actor runtime shutdown");

        // Signal shutdown to all components
        info!("Signaling shutdown to all components");
        self.shutdown_controller.signal_shutdown();

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

