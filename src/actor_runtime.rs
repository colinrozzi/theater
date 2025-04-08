//! # Actor Runtime
//!
//! The Actor Runtime is responsible for initializing, running, and managing the lifecycle
//! of WebAssembly actors within the Theater system. It coordinates the various components
//! that an actor needs to function, including handlers, the executor, and communication channels.
//!
//! ## Purpose
//!
//! The Actor Runtime serves as the glue between the Theater system and individual actors by:
//!
//! - Managing the full lifecycle of an actor from instantiation to shutdown
//! - Setting up host function handlers that provide capabilities to the actor
//! - Initializing the actor with its configuration and state
//! - Propagating shutdown signals from parent to child components
//! - Coordinating the execution of all components that make up an actor's environment
//!
//! Each actor in the system has its own dedicated runtime, providing isolation and independent
//! management of resources.

use crate::actor_executor::ActorExecutor;
use crate::actor_executor::ActorOperation;
use crate::actor_handle::ActorHandle;
use crate::actor_store::ActorStore;
use crate::config::{HandlerConfig, ManifestConfig};
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
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use crate::store::ContentStore;
use crate::wasm::ActorComponent;
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
/// ## Purpose
///
/// `ActorRuntime` manages the various components that make up an actor's execution environment,
/// including the executor, handlers, and communication channels. It's responsible for starting
/// the actor, setting up its capabilities via handlers, and ensuring proper shutdown.
///
/// ## Example
///
/// ```rust
/// use theater::actor_runtime::ActorRuntime;
/// use theater::config::ManifestConfig;
/// use theater::id::TheaterId;
/// use tokio::sync::mpsc;
///
/// async fn start_actor(
///     id: TheaterId,
///     config: &ManifestConfig,
///     theater_tx: mpsc::Sender<TheaterCommand>,
///     shutdown_receiver: ShutdownReceiver,
/// ) -> Result<(), Box<dyn std::error::Error>> {
///     // Create the necessary channels
///     let (actor_sender, _) = mpsc::channel(32);
///     let (_, actor_mailbox) = mpsc::channel(32);
///     let (operation_tx, operation_rx) = mpsc::channel(32);
///     let (response_tx, mut response_rx) = mpsc::channel(1);
///     
///     // Start the actor runtime
///     let mut runtime = ActorRuntime::start(
///         id.clone(),
///         config,
///         None, // No initial state
///         theater_tx,
///         actor_sender,
///         actor_mailbox,
///         operation_rx,
///         operation_tx,
///         true, // Initialize the actor
///         shutdown_receiver,
///         response_tx,
///     ).await?;
///     
///     // Wait for the start result
///     match response_rx.recv().await {
///         Some(StartActorResult::Success(actor_id)) => {
///             println!("Actor started successfully: {}", actor_id);
///         }
///         Some(StartActorResult::Failure(actor_id, error)) => {
///             println!("Actor {} failed to start: {}", actor_id, error);
///             return Err(error.into());
///         }
///         None => {
///             println!("No response received from actor start");
///             return Err("No response".into());
///         }
///     }
///     
///     // Later, stop the actor
///     runtime.stop().await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Safety
///
/// While `ActorRuntime` itself is safe to use, it manages WebAssembly execution which is
/// inherently unsafe. The runtime ensures isolation between actors and proper handling of
/// errors from WebAssembly code.
///
/// ## Security
///
/// The runtime enforces security boundaries by:
/// - Setting up appropriate handler capabilities based on the actor's configuration
/// - Ensuring proper isolation between actors
/// - Managing resource allocation and cleanup
/// - Recording events for auditing
///
/// ## Implementation Notes
///
/// The runtime uses Tokio tasks for concurrent execution of the actor's components:
/// - One task for the actor executor
/// - Separate tasks for each handler
/// - A monitoring task for shutdown propagation
pub struct ActorRuntime {
    /// Unique identifier for this actor
    pub actor_id: TheaterId,
    /// Handles to the running handler tasks
    handler_tasks: Vec<JoinHandle<()>>,
    /// Handle to the actor executor task
    actor_executor_task: JoinHandle<()>,
    /// Controller for graceful shutdown of all components
    shutdown_controller: ShutdownController,
}

/// # Result of starting an actor
///
/// Represents the outcome of attempting to start an actor.
///
/// ## Purpose
///
/// This enum provides detailed information about whether an actor was successfully
/// started or encountered errors during initialization. It includes the actor's ID
/// in both success and failure cases, and detailed error information in the failure case.
///
/// ## Example
///
/// ```rust
/// use theater::actor_runtime::StartActorResult;
/// use theater::id::TheaterId;
///
/// fn handle_start_result(result: StartActorResult) {
///     match result {
///         StartActorResult::Success(id) => {
///             println!("Actor {} started successfully", id);
///             // Proceed with using the actor
///         }
///         StartActorResult::Failure(id, error) => {
///             eprintln!("Actor {} failed to start: {}", id, error);
///             // Handle the error or try recovery
///         }
///     }
/// }
/// ```
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
    /// ## Returns
    ///
    /// * `Ok(ActorRuntime)` - The started actor runtime
    /// * `Err(anyhow::Error)` - Error that occurred during startup
    ///
    /// ## Implementation Notes
    ///
    /// This method performs the following steps:
    /// 1. Creates the actor store for state management
    /// 2. Records the actor load event in the chain
    /// 3. Sets up handlers based on the configuration
    /// 4. Creates and instantiates the WebAssembly component
    /// 5. Initializes the actor if requested
    /// 6. Starts the executor and handlers
    /// 7. Sets up shutdown signal propagation
    ///
    /// The method is asynchronous and returns once the actor is fully initialized
    /// and ready to accept operations.
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
    ) -> Result<Self> {
        let actor_handle = ActorHandle::new(operation_tx.clone());
        let actor_store = ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone());

        let manifest_store = ContentStore::from_id("manifest");
        let manifest_id = manifest_store
            .store(config.clone().into_fixed_bytes())
            .await?;

        actor_store.record_event(ChainEventData {
            event_type: "theater-runtime".to_string(),
            data: EventData::TheaterRuntime(TheaterRuntimeEventData::ActorLoadCall {
                manifest_id: manifest_id.clone(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: format!("Loading actor [{}] from manifest [{}] ", id, manifest_id).into(),
        });

        // Create a local shutdown controller for this runtime
        let (shutdown_controller, _) = ShutdownController::new();
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

        let mut actor_component = match ActorComponent::new(
            config.name.clone(),
            config.component_path.clone(),
            actor_store,
        )
        .await
        {
            Ok(component) => component,
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
                return Err(e.into());
            }
        };

        // Add the host functions to the linker of the actor
        {
            for handler in &mut handlers {
                info!(
                    "Setting up host functions for handler: {:?}",
                    handler.name()
                );
                match handler.setup_host_functions(&mut actor_component).await {
                    Ok(_) => {} // Successfully set up host functions
                    Err(e) => {
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
                        return Err(e);
                    }
                }
            }
        }

        let mut actor_instance = match actor_component.instantiate().await {
            Ok(instance) => instance,
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
                return Err(e.into());
            }
        };

        {
            for handler in &handlers {
                info!("Creating functions for handler: {:?}", handler.name());
                match handler.add_export_functions(&mut actor_instance).await {
                    Ok(_) => {} // Successfully added export functions
                    Err(e) => {
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
            }
        }

        // Actor handle already created above
        let mut init_state = None;
        if init {
            info!("Loading init state for actor: {:?}", id);

            // Get state from config if available
            let config_state = config.load_init_state().unwrap_or(None);

            // Merge with provided state
            init_state = crate::utils::merge_initial_states(config_state, state_bytes)
                .expect("Failed to merge initial states");

            info!("Final init state ready: {:?}", init_state.is_some());
        }

        actor_instance.store.data_mut().set_state(init_state);

        // Create a shutdown receiver for the executor
        let executor_shutdown = shutdown_controller.subscribe();
        let mut actor_executor =
            ActorExecutor::new(actor_instance, operation_rx, executor_shutdown, theater_tx);
        let executor_task = tokio::spawn(async move { actor_executor.run().await });

        // Notify the caller that the actor has started
        if let Err(e) = response_tx
            .send(StartActorResult::Success(id.clone()))
            .await
        {
            error!("Failed to send success response: {}", e);
            // Even though we couldn't send the response, we'll return the runtime
            // since it's been initialized successfully
        }

        if init {
            match actor_handle
                .call_function::<(String,), ()>(
                    "ntwk:theater/actor.init".to_string(),
                    (id.to_string(),),
                )
                .await
            {
                Ok(_) => {} // Successfully called init function
                Err(e) => {
                    let error_message =
                        format!("Failed to call init function for actor {}: {}", id, e);
                    error!("{}", error_message);
                    // Send failure result back to spawner with detailed error message
                    if let Err(send_err) = response_tx
                        .send(StartActorResult::Failure(id.clone(), error_message.clone()))
                        .await
                    {
                        error!("Failed to send failure response: {}", send_err);
                    }
                    return Err(anyhow::anyhow!(error_message));
                }
            }
        }

        let mut handler_tasks: Vec<JoinHandle<()>> = Vec::new();

        for mut handler in handlers {
            info!("Starting handler: {:?}", handler.name());
            let actor_handle = actor_handle.clone();
            let handler_shutdown = shutdown_controller.subscribe();
            let handler_task = tokio::spawn(async move {
                if let Err(e) = handler.start(actor_handle, handler_shutdown).await {
                    warn!("Handler failed: {:?}", e);
                }
            });
            handler_tasks.push(handler_task);
        }

        // Monitor parent shutdown signal and propagate
        let shutdown_controller_clone = shutdown_controller.clone();
        let mut parent_shutdown_receiver_clone = parent_shutdown_receiver;
        tokio::spawn(async move {
            debug!("Actor waiting for parent shutdown signal");
            parent_shutdown_receiver_clone.wait_for_shutdown().await;
            info!("Actor runtime received parent shutdown signal");
            debug!("Propagating shutdown signal to all handler components");
            shutdown_controller_clone.signal_shutdown();
            debug!("Shutdown signal propagated to all components");
        });

        Ok(ActorRuntime {
            actor_id: id.clone(),
            handler_tasks,
            actor_executor_task: executor_task,
            shutdown_controller,
        })
    }

    /// # Stop the actor runtime
    ///
    /// Gracefully shuts down the actor runtime and all its components,
    /// including the executor and handlers.
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - The runtime was successfully shut down
    /// * `Err(anyhow::Error)` - An error occurred during shutdown
    ///
    /// ## Example
    ///
    /// ```rust
    /// async fn stop_actor(runtime: &mut ActorRuntime) -> Result<(), Box<dyn std::error::Error>> {
    ///     // Signal shutdown to all components and wait for completion
    ///     runtime.stop().await?;
    ///     println!("Actor has been shut down");
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This method performs the following steps:
    /// 1. Signals shutdown to all components via the shutdown controller
    /// 2. Waits briefly for tasks to shut down gracefully
    /// 3. Forcefully aborts any tasks that didn't shut down gracefully
    ///
    /// The method is designed to ensure that resources are properly cleaned up
    /// even if some components fail to shut down gracefully.
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

        // Finally abort the executor if it's still running
        if !self.actor_executor_task.is_finished() {
            debug!("Aborting executor task that didn't shut down gracefully");
            self.actor_executor_task.abort();
        }

        info!("Actor runtime shutdown complete");
        Ok(())
    }
}
