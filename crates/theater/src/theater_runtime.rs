//! # Theater Runtime
//!
//! The `theater_runtime` module implements the core runtime environment for the Theater
//! actor system. It manages actor lifecycle, message passing, and event handling across
//! the entire system.

use crate::actor::runtime::ActorRuntime;
use crate::actor::types::{ActorControl, ActorError, ActorInfo, ActorOperation};
use crate::chain::ChainEvent;
use crate::config::actor_manifest::HandlerConfig;
use crate::handler::HandlerRegistry;
use crate::id::TheaterId;
use crate::replay::ReplayHandler;
use crate::messages::{ActorMessage, ActorStatus, TheaterCommand};
use crate::messages::{
    ActorResult, ChannelId, ChannelParticipant, ChildError, ChildExternalStop, ChildResult,
};
use crate::metrics::ActorMetrics;
use crate::shutdown::{ShutdownController, ShutdownType};
use crate::utils::{self, resolve_reference};
use crate::Result;
use crate::TheaterRuntimeError;
use crate::{ManifestConfig, StateChain};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// # TheaterRuntime
///
/// The central runtime for the Theater actor system, responsible for managing actors and their lifecycles.
///
/// ## Purpose
///
/// TheaterRuntime is the core component that coordinates actors within the Theater system.
/// It handles actor creation, destruction, communication, and provides the foundation for
/// the actor supervision system. The runtime also manages channels for communication between
/// actors and external systems.
///
/// ## Example
///
/// ```rust,no_run
/// use theater::theater_runtime::TheaterRuntime;
/// use theater::messages::TheaterCommand;
/// use tokio::sync::mpsc;
/// use anyhow::Result;
///
/// async fn example() -> Result<()> {
///     // Create channels for theater commands
///     let (theater_tx, theater_rx) = mpsc::channel(100);
///     
///     // Initialize the runtime
///     let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None, Default::default()).await?;
///     
///     // Start a background task to run the runtime
///     let runtime_handle = tokio::spawn(async move {
///         runtime.run().await
///     });
///     
///     // Use the theater_tx to send commands to the runtime
///     // ...
///     
///     Ok(())
/// }
/// ```
///
/// ## Safety
///
/// TheaterRuntime provides a safe interface to the WebAssembly actors. All potentially unsafe
/// operations involving WebAssembly execution are handled in the ActorRuntime and ActorExecutor
/// components with appropriate checks and validations.
///
/// ## Security
///
/// TheaterRuntime enforces sandbox boundaries for actors, preventing unauthorized
/// access to system resources. Each actor runs in an isolated WebAssembly environment with
/// controlled capabilities defined in its manifest.
///
/// ## Implementation Notes
///
/// The runtime uses a command-based architecture where all operations are sent as messages
/// through channels. This allows for asynchronous processing and helps maintain isolation
/// between components.
pub struct TheaterRuntime<E: crate::events::EventPayload + Clone> {
    /// Map of active actors indexed by their ID
    actors: HashMap<TheaterId, ActorProcess>,
    /// Map of chains index by actor ID
    chains: HashMap<TheaterId, Arc<RwLock<StateChain<E>>>>,
    /// Sender for commands to the runtime
    theater_tx: Sender<TheaterCommand>,
    /// Receiver for commands to the runtime
    theater_rx: Receiver<TheaterCommand>,
    /// Map of event subscriptions for actors
    subscriptions: HashMap<TheaterId, Vec<Sender<Result<ChainEvent, ActorError>>>>,
    /// Map of active communication channels
    channels: HashMap<ChannelId, HashSet<ChannelParticipant>>,
    /// Optional channel to send channel events back to the server
    #[allow(dead_code)]
    channel_events_tx: Option<Sender<crate::messages::ChannelEvent>>,
    /// wasm engine
    wasm_engine: wasmtime::Engine,
    /// Handler registry
    pub handler_registry: HandlerRegistry<E>,
    marker: PhantomData<E>,
}

/// # ActorProcess
///
/// A container for the running actor and its associated channels and metadata.
///
/// ## Purpose
///
/// ActorProcess encapsulates all the runtime information needed to manage a single actor,
/// including communication channels, status information, and relationships to other actors.
///
/// ## Implementation Notes
///
/// The ActorProcess is maintained by the TheaterRuntime and typically not accessed directly
/// by users of the library. It contains internal channels used to communicate with the actor's
/// execution environment.
pub struct ActorProcess {
    /// Unique identifier for the actor
    pub actor_id: TheaterId,
    /// Actor Name
    pub name: String,
    /// Task handle for the running actor
    pub process: JoinHandle<()>,
    /// Channel for sending messages to the actor
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    /// Channel for sending operations to the actor
    pub operation_tx: mpsc::Sender<ActorOperation>,
    /// Channel for sending actor information commands
    pub info_tx: mpsc::Sender<ActorInfo>,
    /// Channel for sending control commands to the actor
    pub control_tx: mpsc::Sender<ActorControl>,
    /// Set of child actor IDs
    pub children: HashSet<TheaterId>,
    /// Current status of the actor
    pub status: ActorStatus,
    /// Path to the actor's manifest
    pub manifest_path: String,
    /// Actor Manifest
    pub manifest: ManifestConfig,
    /// Controller for graceful shutdown
    pub shutdown_controller: ShutdownController,
    /// Optional supervisor channel for actor supervision
    pub supervisor_tx: Option<Sender<ActorResult>>,
}

impl<E> TheaterRuntime<E>
where
    E: crate::events::EventPayload
        + Clone
        + From<crate::events::theater_runtime::TheaterRuntimeEventData>
        + From<crate::events::wasm::WasmEventData>
        + From<crate::events::runtime::RuntimeEventData>
        + From<crate::replay::HostFunctionCall>,
{
    /// Creates a new TheaterRuntime with the given communication channels.
    ///
    /// ## Parameters
    ///
    /// * `theater_tx` - Sender for commands to the runtime
    /// * `theater_rx` - Receiver for commands to the runtime
    /// * `channel_events_tx` - Optional channel for sending events to external systems
    /// * `message_lifecycle_tx` - Optional channel for sending actor lifecycle events to message-server
    ///
    /// ## Returns
    ///
    /// A new TheaterRuntime instance ready to be started.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// # use theater::theater_runtime::TheaterRuntime;
    /// # use theater::messages::TheaterCommand;
    /// # use tokio::sync::mpsc;
    /// # use anyhow::Result;
    /// #
    /// # async fn example() -> Result<()> {
    /// let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
    /// let runtime = TheaterRuntime::new(theater_tx, theater_rx, None, None, Default::default()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        theater_tx: Sender<TheaterCommand>,
        theater_rx: Receiver<TheaterCommand>,
        channel_events_tx: Option<Sender<crate::messages::ChannelEvent>>,
        handler_registry: HandlerRegistry<E>,
    ) -> Result<Self> {
        info!("Theater runtime initializing");
        let engine = wasmtime::Engine::new(wasmtime::Config::new().async_support(true))?;

        Ok(Self {
            theater_tx,
            theater_rx,
            actors: HashMap::new(),
            chains: HashMap::new(),
            subscriptions: HashMap::new(),
            channels: HashMap::new(),
            channel_events_tx,
            wasm_engine: engine,
            handler_registry,
            marker: PhantomData,
        })
    }

    /// Starts the runtime's main event loop, processing commands until shutdown.
    ///
    /// ## Purpose
    ///
    /// This method runs the main event loop of the runtime, processing commands from
    /// the `theater_rx` channel and dispatching them to the appropriate handlers.
    /// It will continue running until the channel is closed or an error occurs.
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - The runtime has shut down gracefully
    /// * `Err(anyhow::Error)` - An error occurred during runtime execution
    ///
    /// ## Implementation Notes
    ///
    /// This method is typically run in a separate task and should be considered the
    /// main execution context for the runtime. It handles all commands asynchronously
    /// and manages actor lifecycles.
    pub async fn run(&mut self) -> Result<()> {
        info!("Theater runtime starting");

        while let Some(cmd) = self.theater_rx.recv().await {
            debug!("Runtime received command: {:?}", cmd.to_log());
            match cmd {
                TheaterCommand::ListChildren {
                    parent_id,
                    response_tx,
                } => {
                    debug!("Getting children for actor: {:?}", parent_id);
                    if let Some(proc) = self.actors.get(&parent_id) {
                        let children = proc.children.iter().cloned().collect();
                        let _ = response_tx.send(children);
                    } else {
                        let _ = response_tx.send(Vec::new());
                    }
                }
                TheaterCommand::RestartActor {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Restarting actor: {:?}", actor_id);
                    match self.restart_actor(actor_id).await {
                        Ok(_) => {
                            let _ = response_tx.send(Ok(()));
                        }
                        Err(e) => {
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                TheaterCommand::GetActorState {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting state for actor: {:?}", actor_id);
                    match self.get_actor_state(actor_id).await {
                        Ok(state) => {
                            let _ = response_tx.send(Ok(state));
                        }
                        Err(e) => {
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                TheaterCommand::GetActorEvents {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting events for actor: {:?}", actor_id);
                    match self.get_actor_events(actor_id).await {
                        Ok(events) => {
                            let _ = response_tx.send(Ok(events));
                        }
                        Err(e) => {
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                TheaterCommand::SpawnActor {
                    manifest_path,
                    init_bytes,
                    parent_id,
                    response_tx,
                    supervisor_tx,
                    subscription_tx,
                } => {
                    debug!(
                        "Processing SpawnActor command for manifest: {:?}",
                        manifest_path
                    );
                    match self
                        .spawn_actor(
                            manifest_path.clone(),
                            init_bytes,
                            parent_id,
                            true,
                            supervisor_tx,
                            subscription_tx,
                        )
                        .await
                    {
                        Ok(actor_id) => {
                            info!("Successfully spawned actor: {:?}", actor_id);
                            if let Err(e) = response_tx.send(Ok(actor_id.clone())) {
                                error!(
                                    "Failed to send success response for actor {:?}: {:?}",
                                    actor_id, e
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to spawn actor from {:?}: {}", manifest_path, e);
                            if let Err(send_err) = response_tx.send(Err(e)) {
                                error!("Failed to send error response: {:?}", send_err);
                            }
                        }
                    }
                }
                TheaterCommand::ResumeActor {
                    manifest_path,
                    state_bytes,
                    response_tx,
                    parent_id,
                    supervisor_tx,
                    subscription_tx,
                } => {
                    debug!(
                        "Processing ResumeActor command for manifest: {:?}",
                        manifest_path
                    );
                    match self
                        .spawn_actor(
                            manifest_path.clone(),
                            state_bytes,
                            parent_id,
                            false,
                            supervisor_tx,
                            subscription_tx,
                        )
                        .await
                    {
                        Ok(actor_id) => {
                            info!("Successfully resumed actor: {:?}", actor_id);
                            if let Err(e) = response_tx.send(Ok(actor_id.clone())) {
                                error!(
                                    "Failed to send success response for actor {:?}: {:?}",
                                    actor_id, e
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to resume actor from {:?}: {}", manifest_path, e);
                            if let Err(send_err) = response_tx.send(Err(e)) {
                                error!("Failed to send error response: {:?}", send_err);
                            }
                        }
                    }
                }
                TheaterCommand::StopActor {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Stopping actor: {:?}", actor_id);
                    match self
                        .stop_actor_external(actor_id, ShutdownType::Graceful)
                        .await
                    {
                        Ok(_) => {
                            info!("Actor stopped successfully");
                            let _ = response_tx.send(Ok(()));
                        }
                        Err(e) => {
                            error!("Failed to stop actor: {}", e);
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                TheaterCommand::TerminateActor {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Terminating actor: {:?}", actor_id);
                    match self.stop_actor(actor_id, ShutdownType::Force).await {
                        Ok(_) => {
                            info!("Actor terminated successfully");
                            let _ = response_tx.send(Ok(()));
                        }
                        Err(e) => {
                            error!("Failed to terminate actor: {}", e);
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                TheaterCommand::ShuttingDown { actor_id, data } => {
                    debug!("Shutting down actor: {:?}", actor_id);
                    match self.shutdown_actor(actor_id, data).await {
                        Ok(_) => {
                            info!("Actor shut down successfully");
                        }
                        Err(e) => {
                            error!("Failed to shut down actor: {}", e);
                        }
                    }
                }
                TheaterCommand::NewEvent { actor_id, event } => {
                    debug!("Received new event from actor {:?}", actor_id);

                    if let Err(e) = self.handle_actor_event(actor_id, event).await {
                        error!("Failed to handle actor event: {}", e);
                    }
                }
                TheaterCommand::ActorError { actor_id, error } => {
                    debug!("Received error event from actor {:?}", actor_id);

                    if let Err(e) = self.handle_actor_error(actor_id, error).await {
                        error!("Failed to handle actor error event: {}", e);
                    }
                }
                TheaterCommand::ActorRuntimeError { error } => {
                    error!("Theater runtime error: {}", error);
                }
                TheaterCommand::GetActors { response_tx } => {
                    debug!("Getting list of actors");
                    let actor_info: Vec<_> = self
                        .actors
                        .iter()
                        .map(|(id, proc)| (id.clone(), proc.name.clone()))
                        .collect();
                    if let Err(e) = response_tx.send(Ok(actor_info)) {
                        error!("Failed to send actor info list: {:?}", e);
                    }
                }
                TheaterCommand::GetActorManifest {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting manifest for actor: {:?}", actor_id);
                    if let Some(proc) = self.actors.get(&actor_id) {
                        let manifest = proc.manifest.clone();
                        if let Err(e) = response_tx.send(Ok(manifest)) {
                            error!("Failed to send actor manifest: {:?}", e);
                        }
                    } else {
                        warn!("Actor {:?} not found", actor_id);
                        let _ = response_tx.send(Err(anyhow::anyhow!("Actor not found")));
                    }
                }
                TheaterCommand::GetActorStatus {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting status for actor: {:?}", actor_id);
                    let status = self
                        .actors
                        .get(&actor_id)
                        .map(|proc| proc.status.clone())
                        .unwrap_or(ActorStatus::Stopped);
                    if let Err(e) = response_tx.send(Ok(status)) {
                        error!("Failed to send actor status: {:?}", e);
                    }
                }
                TheaterCommand::GetActorMetrics {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting metrics for actor: {:?}", actor_id);
                    match self.get_actor_metrics(actor_id).await {
                        Ok(metrics) => {
                            let _ = response_tx.send(Ok(metrics));
                        }
                        Err(e) => {
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                #[allow(unused_variables)]
                TheaterCommand::SubscribeToActor { actor_id, event_tx } => {
                    debug!("Subscribing to events for actor: {:?}", actor_id);
                    self.subscribe_to_actor(actor_id, event_tx)
                        .expect("Failed to subscribe");
                }
                // Channel-related commands
                TheaterCommand::NewStore { response_tx } => {
                    debug!("Creating new content store");
                    let store_id = crate::store::ContentStore::new();
                    let _ = response_tx.send(Ok(store_id));
                }
            };
        }
        info!("Theater runtime shutting down");
        Ok(())
    }

    /// Spawns a new actor from a manifest with optional initialization data.
    ///
    /// ## Parameters
    ///
    /// * `manifest_path` - Path to the actor's manifest file or manifest content
    /// * `init_bytes` - Optional initialization data for the actor
    /// * `parent_id` - Optional ID of the parent actor
    /// * `init` - Whether to initialize the actor (true) or resume it (false)
    ///
    /// ## Returns
    ///
    /// * `Ok(TheaterId)` - The ID of the newly spawned actor
    /// * `Err(anyhow::Error)` - An error occurred during actor spawn
    ///
    /// ## Implementation Notes
    ///
    /// This method handles the entire process of spawning a new actor, including:
    /// - Loading and parsing the manifest
    /// - Creating communication channels
    /// - Spawning the actor runtime in a new task
    /// - Registering the actor with the runtime
    /// - Setting up parent-child relationships
    async fn spawn_actor(
        &mut self,
        manifest_path: String,
        init_bytes: Option<Vec<u8>>,
        parent_id: Option<TheaterId>,
        _init: bool,
        supervisor_tx: Option<Sender<ActorResult>>,
        subscription_tx: Option<Sender<Result<ChainEvent, ActorError>>>,
    ) -> Result<TheaterId> {
        debug!(
            "Starting actor spawn process from manifest: {:?}",
            manifest_path
        );

        // check if the manifest is a valid path OR starts with store:
        let manifest_str: String;

        if manifest_path.starts_with("store:")
            || manifest_path.starts_with("https:")
            || PathBuf::from(&manifest_path).exists()
        {
            debug!("Manifest path is a valid store reference or URL");
            // Resolve the store reference
            let manifest_bytes = resolve_reference(&manifest_path).await?;
            // Save as a string
            manifest_str = String::from_utf8(manifest_bytes.clone())
                .map_err(|e| TheaterRuntimeError::ActorInitializationError(e.to_string()))?;
        } else {
            debug!("Manifest is a string");
            manifest_str = manifest_path.clone();
        }

        let init_value = if let Some(bytes) = init_bytes {
            Some(
                serde_json::from_slice::<Value>(&bytes)
                    .map_err(|e| TheaterRuntimeError::ActorInitializationError(e.to_string()))?,
            )
        } else {
            None
        };

        let (manifest, init_value) =
            ManifestConfig::resolve_starting_info(&manifest_str, init_value)
                .await
                .map_err(|e| {
                    TheaterRuntimeError::ActorInitializationError(format!(
                        "Failed to resolve manifest: {}",
                        e
                    ))
                })?;

        // Create a shutdown controller for this specific actor
        let mut shutdown_controller = ShutdownController::new();
        let (mailbox_tx, _mailbox_rx) = mpsc::channel(100);
        let (operation_tx, operation_rx) = mpsc::channel(100);
        let (info_tx, info_rx) = mpsc::channel(100);
        let (control_tx, control_rx) = mpsc::channel(100);
        let theater_tx = self.theater_tx.clone();

        let shutdown_receiver = shutdown_controller.subscribe();
        let actor_operation_tx = operation_tx.clone();
        let actor_info_tx = info_tx.clone();
        let actor_control_tx = control_tx.clone();
        let _shutdown_receiver_clone = shutdown_receiver;
        let _actor_sender = mailbox_tx.clone();

        let actor_id = TheaterId::generate();
        debug!("Initializing actor runtime");
        debug!("Starting actor runtime");

        if let Some(tx) = subscription_tx {
            self.subscribe_to_actor(actor_id.clone(), tx)
                .expect("Failed to subscribe to actor");
        }

        let chain = Arc::new(RwLock::new(StateChain::new(
            actor_id.clone(),
            self.theater_tx.clone(),
        )));

        self.chains.insert(actor_id.clone(), chain.clone());

        // Check if manifest specifies a replay handler and create modified registry if so
        let handler_registry = self.create_handler_registry_for_manifest(&manifest).await?;

        // Start the actor in a detached task
        let actor_id_for_task = actor_id.clone();
        let actor_name = manifest.name.clone();
        let manifest_clone = manifest.clone();
        let engine = self.wasm_engine.clone();
        let actor_runtime_process = tokio::spawn(async move {
            let _actor_runtime = ActorRuntime::start(
                actor_id_for_task.clone(),
                &manifest_clone,
                init_value,
                engine,
                chain,
                handler_registry,
                theater_tx,
                operation_rx,
                actor_operation_tx,
                info_rx,
                actor_info_tx,
                control_rx,
                actor_control_tx,
            )
            .await;
        });

        // Create ActorHandle for lifecycle notification before moving channels
        let _actor_handle = crate::actor::handle::ActorHandle::new(
            operation_tx.clone(),
            info_tx.clone(),
            control_tx.clone(),
        );

        let process = ActorProcess {
            actor_id: actor_id.clone(),
            name: actor_name,
            process: actor_runtime_process,
            mailbox_tx,
            operation_tx,
            info_tx,
            control_tx,
            children: HashSet::new(),
            status: ActorStatus::Running,
            manifest_path: manifest_path.clone(),
            manifest,
            shutdown_controller,
            supervisor_tx,
        };

        if let Some(parent_id) = parent_id {
            debug!("Adding actor {:?} as child of {:?}", actor_id, parent_id);
            if let Some(parent) = self.actors.get_mut(&parent_id) {
                parent.children.insert(actor_id.clone());
                debug!("Added actor {:?} as child of {:?}", actor_id, parent_id);
            } else {
                warn!(
                    "Parent actor {:?} not found for new actor {:?}",
                    parent_id, actor_id
                );
            }
        }

        self.actors.insert(actor_id.clone(), process);
        debug!("Actor process registered with runtime");

        Ok(actor_id)
    }

    fn subscribe_to_actor(
        &mut self,
        actor_id: TheaterId,
        subscription_tx: Sender<Result<ChainEvent, ActorError>>,
    ) -> Result<()> {
        if let Some(subscribers) = self.subscriptions.get_mut(&actor_id) {
            subscribers.push(subscription_tx);
        } else {
            self.subscriptions
                .insert(actor_id.clone(), vec![subscription_tx]);
        }
        Ok(())
    }

    async fn handle_actor_event(&mut self, actor_id: TheaterId, event: ChainEvent) -> Result<()> {
        debug!("Handling event for actor: {:?}", actor_id);

        // Use entry API to handle the subscription map more elegantly
        let should_remove = if let std::collections::hash_map::Entry::Occupied(mut entry) =
            self.subscriptions.entry(actor_id.clone())
        {
            let subscribers: &mut Vec<Sender<Result<ChainEvent, ActorError>>> = entry.get_mut();
            let mut to_remove: Vec<usize> = Vec::new();

            // Send events and track failures
            for (index, subscriber) in subscribers.iter().enumerate() {
                if let Err(e) = subscriber.send(Ok(event.clone())).await {
                    error!("Failed to send event to subscriber: {}", e);
                    to_remove.push(index);
                }
            }

            // Remove failed subscribers in reverse order
            if !to_remove.is_empty() {
                to_remove.sort_unstable_by(|a, b| b.cmp(a));
                for index in to_remove {
                    subscribers.swap_remove(index);
                    debug!("Removed failed subscriber at index {}", index);
                }
            }

            // Check if we should remove the entire entry
            subscribers.is_empty()
        } else {
            false
        };

        // Remove the entry if needed
        if should_remove {
            self.subscriptions.remove(&actor_id);
            debug!("Removed empty subscription entry for actor {:?}", actor_id);
        }

        Ok(())
    }

    async fn handle_actor_error(&mut self, actor_id: TheaterId, error: ActorError) -> Result<()> {
        debug!("Handling error event for actor: {:?}", actor_id);

        // notify the actors parents
        if let Some(proc) = self.actors.get(&actor_id) {
            if let Some(supervisor_tx) = &proc.supervisor_tx {
                let error_message = ActorResult::Error(ChildError {
                    actor_id: actor_id.clone(),
                    error: error.clone(),
                });
                // Send error and immediately shutdown - don't wait for response
                let supervisor_tx_clone = supervisor_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = supervisor_tx_clone.send(error_message).await {
                        error!("Failed to send error message to supervisor: {}", e);
                    }
                });
            }
        }

        // notify any actor subscribers
        if let Some(subscribers) = self.subscriptions.get(&actor_id) {
            let subscribers_clone = subscribers.clone();
            let error_clone = error.clone();
            tokio::spawn(async move {
                for subscriber in subscribers_clone {
                    if let Err(e) = subscriber.send(Err(error_clone.clone())).await {
                        error!("Failed to send error event to subscriber: {}", e);
                    }
                }
            });
        }

        // Immediately shutdown the actor due to error
        debug!("Shutting down actor {:?} due to error", actor_id);
        self.stop_actor(actor_id, ShutdownType::Graceful)
            .await
            .map_err(|e| {
                error!("Failed to stop actor after error: {}", e);
                e
            })?;

        Ok(())
    }

    /// Stops an actor and its children gracefully.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - The ID of the actor to stop
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - The actor was successfully stopped
    /// * `Err(anyhow::Error)` - An error occurred during the stop process
    ///
    /// ## Implementation Notes
    ///
    /// This method stops an actor and all its children recursively. It follows these steps:
    /// 1. Stop all children of the actor
    /// 2. Signal the actor to shut down
    /// 5. Remove the actor from the runtime's registries
    /// 6. Clean up any channel registrations
    async fn stop_actor(&mut self, actor_id: TheaterId, shutdown_type: ShutdownType) -> Result<()> {
        debug!("Stopping actor: {:?}", actor_id);

        // Check if the actor exists in the registry
        if !self.actors.contains_key(&actor_id) {
            warn!("Actor {:?} not found in registry", actor_id);
            return Ok(());
        }

        let proc = match self.actors.get(&actor_id) {
            Some(proc) => proc,
            None => {
                error!("Actor {:?} not found in registry", actor_id);
                return Ok(());
            }
        };

        debug!("Actor {:?} found, proceeding with shutdown", actor_id);

        'chain_block: {
            // Get the actor's chain
            let chain = match self.chains.get(&actor_id) {
                Some(chain) => chain,
                None => {
                    error!("Actor {:?} has no associated chain", actor_id);
                    break 'chain_block;
                }
            };

            // Add the final event to the chain
            let mut writable_chain = match chain.write() {
                Ok(chain) => chain,
                Err(e) => {
                    error!(
                        "Failed to acquire write lock on chain for actor {:?}: {}, will not add final event or save chain, continuing with shutdown",
                        actor_id, e
                    );
                    break 'chain_block;
                }
            };

            debug!("Adding final event to chain for actor {:?}", actor_id);
            writable_chain
                .add_typed_event(crate::events::ChainEventData {
                    event_type: "shutdown".to_string(),
                    data: crate::events::runtime::RuntimeEventData::ShuttingDown {}.into(),
                })
                .expect("Failed to record event");
            debug!("Final event added to chain for actor {:?}", actor_id);

            if proc.manifest.save_chain() {
                debug!("Actor {:?} manifest requires chain saving", actor_id);
                writable_chain.save_chain().map_err(|e| {
                    error!("Failed to save chain for actor {:?}: {}", actor_id, e);
                    e
                })?;
            } else {
                debug!(
                    "Actor {:?} manifest does not require chain saving",
                    actor_id
                );
            }
        }

        self.chains.remove(&actor_id);

        // Find the actor's children to stop them first
        let children = if let Some(proc) = self.actors.get(&actor_id) {
            debug!(
                "Actor {:?} has {} children to stop first",
                actor_id,
                proc.children.len()
            );
            proc.children.clone()
        } else {
            debug!("Actor {:?} not found", actor_id);
            return Ok(());
        };

        // First, stop all children recursively
        for (index, child_id) in children.iter().enumerate() {
            debug!(
                "Stopping child {}/{} with ID {:?} of parent {:?}",
                index + 1,
                children.len(),
                child_id,
                actor_id
            );
            Box::pin(self.stop_actor(child_id.clone(), shutdown_type)).await?;
            debug!("Successfully stopped child {:?}", child_id);
        }

        // Get the actor process - but DON'T remove it from the map yet
        // We need to keep it in the map while shutdown is in progress
        let proc = match self.actors.get(&actor_id) {
            Some(proc) => proc,
            None => {
                debug!("Actor {:?} not found during shutdown", actor_id);
                return Ok(());
            }
        };

        // First, signal the actor runtime itself to shut down via its control channel
        // This stops the operation/info loops from processing new requests
        debug!("Sending shutdown signal to actor runtime for {:?}", actor_id);
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        if let Err(e) = proc.control_tx.send(ActorControl::Shutdown { response_tx }).await {
            error!("Failed to send shutdown signal to actor runtime for {:?}: {}", actor_id, e);
            // Continue with shutdown anyway - we'll try to clean up handlers
        } else {
            // Wait for the actor runtime to acknowledge shutdown with a timeout
            match tokio::time::timeout(std::time::Duration::from_secs(10), response_rx).await {
                Ok(Ok(Ok(_))) => {
                    debug!("Actor runtime for {:?} acknowledged shutdown", actor_id);
                }
                Ok(Ok(Err(e))) => {
                    error!("Actor runtime for {:?} returned error during shutdown: {:?}", actor_id, e);
                }
                Ok(Err(_)) => {
                    error!("Actor runtime for {:?} response channel closed", actor_id);
                }
                Err(_) => {
                    error!("Timeout waiting for actor runtime {:?} to shut down", actor_id);
                }
            }
        }

        // Now signal handlers to shut down
        // The handlers will wait for their cleanup to complete before responding
        debug!("Signaling handlers to shutdown for actor {:?}", actor_id);

        // Remove from map now, after actor runtime has shut down but before waiting on handlers
        // This ensures the actor is removed from the registry while handlers clean up
        let proc = self.actors.remove(&actor_id).unwrap(); // Safe - we just checked it exists

        proc.shutdown_controller
            .signal_shutdown(shutdown_type)
            .await;

        debug!("Actor {:?} shutdown complete", actor_id);

        // Remove actor from any channel registrations
        let mut channels_to_remove = Vec::new();
        let id_for_channels = ChannelParticipant::Actor(actor_id.clone());
        for (channel_id, participants) in self.channels.iter_mut() {
            if participants.remove(&id_for_channels) {
                debug!("Removed actor {:?} from channel {:?}", actor_id, channel_id);

                // If this was the last participant, mark the channel for removal
                if participants.is_empty() {
                    debug!("Channel {:?} is now empty, marking for removal", channel_id);
                    channels_to_remove.push(channel_id.clone());
                }
            }
        }

        // Remove any empty channels
        for channel_id in channels_to_remove {
            self.channels.remove(&channel_id);
            debug!("Removed empty channel {:?}", channel_id);
        }

        Ok(())
    }

    /// Actor is shutting itself down
    async fn shutdown_actor(&mut self, actor_id: TheaterId, data: Option<Vec<u8>>) -> Result<()> {
        debug!("Shutting down actor: {:?}", actor_id);

        // Notify the actor's supervisor if it has one
        if let Some(proc) = self.actors.get(&actor_id) {
            if let Some(supervisor_tx) = &proc.supervisor_tx {
                let message = ActorResult::Success(ChildResult {
                    actor_id: actor_id.clone(),
                    result: data,
                });
                if let Err(e) = supervisor_tx.send(message).await {
                    error!("Failed to send shutdown message to supervisor: {}", e);
                }
            }
        }

        self.stop_actor(actor_id, ShutdownType::Graceful).await?;

        Ok(())
    }

    /// Actor is shut down externally
    /// note: external might not be the right word here. External to the actor, not external to the
    /// system, so if a parent actor stops it, it still shows up as an external stop. What this
    /// means is that the actor did not error out or shut itself down
    async fn stop_actor_external(
        &mut self,
        actor_id: TheaterId,
        shutdown_type: ShutdownType,
    ) -> Result<()> {
        debug!("Stopping actor externally: {:?}", actor_id);

        // notify the actors parents
        if let Some(proc) = self.actors.get(&actor_id) {
            if let Some(supervisor_tx) = &proc.supervisor_tx {
                let error_message = ActorResult::ExternalStop(ChildExternalStop {
                    actor_id: actor_id.clone(),
                });
                if let Err(e) = supervisor_tx.send(error_message).await {
                    error!("Failed to send error message to supervisor: {}", e);
                }
            }
        }

        self.stop_actor(actor_id, shutdown_type).await?;
        Ok(())
    }

    async fn restart_actor(&mut self, actor_id: TheaterId) -> Result<()> {
        debug!("Starting actor restart process for: {:?}", actor_id);

        Err(anyhow::anyhow!("Actor restart not implemented"))
    }

    async fn get_actor_state(&self, actor_id: TheaterId) -> Result<Option<Vec<u8>>> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's state
            let (tx, rx): (
                oneshot::Sender<Result<Option<Vec<u8>>, ActorError>>,
                oneshot::Receiver<Result<Option<Vec<u8>>, ActorError>>,
            ) = oneshot::channel();
            proc.info_tx
                .send(ActorInfo::GetState { response_tx: tx })
                .await?;

            match rx.await {
                Ok(state) => Ok(state?),
                Err(e) => Err(anyhow::anyhow!("Failed to receive state: {}", e)),
            }
        } else {
            // The error here needs to be anyhow::Error since that's what the function returns
            Err(anyhow::Error::new(TheaterRuntimeError::ActorNotFound(
                actor_id,
            )))
        }
    }

    async fn get_actor_events(
        &self,
        actor_id: TheaterId,
    ) -> std::result::Result<Vec<ChainEvent>, TheaterRuntimeError> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's events
            let (tx, rx): (
                oneshot::Sender<Result<Vec<ChainEvent>, ActorError>>,
                oneshot::Receiver<Result<Vec<ChainEvent>, ActorError>>,
            ) = oneshot::channel();

            if let Err(e) = proc
                .info_tx
                .send(ActorInfo::GetChain { response_tx: tx })
                .await
            {
                return Err(TheaterRuntimeError::ChannelError(format!(
                    "Failed to send GetChain operation: {}",
                    e
                )));
            }

            match rx.await {
                Ok(events) => match events {
                    Ok(events) => Ok(events),
                    Err(e) => Err(TheaterRuntimeError::ActorError(e)),
                },
                Err(e) => Err(TheaterRuntimeError::ChannelError(format!(
                    "Failed to receive events: {}",
                    e
                ))),
            }
        } else {
            let events = utils::read_events_from_filesystem(&actor_id).map_err(|e| {
                TheaterRuntimeError::ChannelError(format!("Failed to read events: {}", e))
            })?;
            Ok(events)
        }
    }

    async fn get_actor_metrics(&self, actor_id: TheaterId) -> Result<ActorMetrics> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's metrics
            let (tx, rx): (
                oneshot::Sender<Result<ActorMetrics, ActorError>>,
                oneshot::Receiver<Result<ActorMetrics, ActorError>>,
            ) = oneshot::channel();
            proc.info_tx
                .send(ActorInfo::GetMetrics { response_tx: tx })
                .await?;

            match rx.await {
                Ok(metrics) => Ok(metrics?),
                Err(e) => Err(anyhow::anyhow!("Failed to receive metrics: {}", e)),
            }
        } else {
            Err(anyhow::anyhow!("Actor not found"))
        }
    }

    // Get status of a specific channel
    pub async fn get_channel_status(
        &self,
        channel_id: &ChannelId,
    ) -> Option<Vec<ChannelParticipant>> {
        self.channels
            .get(channel_id)
            .map(|participants| participants.iter().cloned().collect())
    }

    // Get a list of all channels and their participants
    pub async fn list_channels(&self) -> Vec<(ChannelId, Vec<ChannelParticipant>)> {
        self.channels
            .iter()
            .map(|(id, participants)| (id.clone(), participants.iter().cloned().collect()))
            .collect()
    }

    /// Creates a handler registry for an actor based on its manifest.
    ///
    /// If the manifest contains a replay handler configuration, this method
    /// will create a new registry with the ReplayHandler included. Otherwise,
    /// it returns a clone of the runtime's default handler registry.
    async fn create_handler_registry_for_manifest(
        &self,
        manifest: &ManifestConfig,
    ) -> Result<HandlerRegistry<E>> {
        // Check if the manifest has a replay handler config
        for handler_config in &manifest.handlers {
            if let HandlerConfig::Replay { config } = handler_config {
                info!(
                    "Found replay handler config, loading chain from: {:?}",
                    config.chain
                );

                // Load the chain from the file
                let chain_bytes = tokio::fs::read(&config.chain).await.map_err(|e| {
                    TheaterRuntimeError::ActorInitializationError(format!(
                        "Failed to read replay chain file {:?}: {}",
                        config.chain, e
                    ))
                })?;

                let chain_events: Vec<ChainEvent> =
                    serde_json::from_slice(&chain_bytes).map_err(|e| {
                        TheaterRuntimeError::ActorInitializationError(format!(
                            "Failed to parse replay chain JSON: {}",
                            e
                        ))
                    })?;

                info!(
                    "Loaded replay chain with {} events from {:?}",
                    chain_events.len(),
                    config.chain
                );

                // Clone the base registry and prepend ReplayHandler
                // ReplayHandler will intercept imports, other handlers will handle exports
                let mut registry = self.handler_registry.clone();
                registry.prepend(ReplayHandler::new(chain_events));

                return Ok(registry);
            }
        }

        // No replay config, use the default registry
        Ok(self.handler_registry.clone())
    }

    /// Stops the entire runtime and all actors gracefully.
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - The runtime was successfully stopped
    /// * `Err(anyhow::Error)` - An error occurred during the stop process
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// # use theater::theater_runtime::TheaterRuntime;
    /// # use anyhow::Result;
    /// #
    /// # async fn example(mut runtime: TheaterRuntime) -> Result<()> {
    /// // Shut down the runtime
    /// runtime.stop().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This method stops all actors managed by the runtime and cleans up any resources.
    pub async fn stop(&mut self) -> Result<()> {
        info!("Initiating theater runtime shutdown");

        // Stop all actors
        for actor_id in self.actors.keys().cloned().collect::<Vec<_>>() {
            debug!("Stopping actor {} as part of theater shutdown", actor_id);
            if let Err(e) = self.stop_actor(actor_id, ShutdownType::Graceful).await {
                error!("Error stopping actor during shutdown: {}", e);
                // Continue with other actors even if one fails
            }
        }

        // Clear any remaining channel registrations
        if !self.channels.is_empty() {
            debug!(
                "Clearing {} remaining channel registrations",
                self.channels.len()
            );
            self.channels.clear();
        }

        info!("Theater runtime shutdown complete");
        Ok(())
    }
}
