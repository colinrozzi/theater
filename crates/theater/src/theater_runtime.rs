//! # Theater Runtime
//!
//! The `theater_runtime` module implements the core runtime environment for the Theater
//! actor system. It manages actor lifecycle, message passing, and event handling across
//! the entire system.

use crate::actor::handle::ActorHandle;
use crate::actor::runtime::ActorRuntime;
use crate::actor::types::{ActorControl, ActorError, ActorInfo, ActorOperation};
use crate::chain::ChainEvent;
use crate::config::actor_manifest::HandlerConfig;
use crate::handler::HandlerRegistry;
use crate::id::TheaterId;
use crate::messages::{
    default_init_state, ActorResult, ChannelId, ChannelParticipant, ChildError, ChildExternalStop,
    ChildResult,
};
use crate::messages::{ActorMessage, ActorStatus, TheaterCommand};
use crate::metrics::ActorMetrics;
use crate::pack_bridge::{AsyncRuntime, Value};
use crate::replay::ReplayHandler;
use crate::shutdown::{ShutdownController, ShutdownType};
use crate::utils::resolve_reference;
use crate::Result;
use crate::TheaterRuntimeError;
use crate::{ManifestConfig, StateChain};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;
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
/// use theater::handler::HandlerRegistry;
/// use theater::messages::TheaterCommand;
/// use tokio::sync::mpsc;
/// use anyhow::Result;
///
/// async fn example() -> Result<()> {
///     // Create channels for theater commands
///     let (theater_tx, theater_rx) = mpsc::channel(100);
///
///     // Initialize the runtime
///     let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None, HandlerRegistry::new()).await?;
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
pub struct TheaterRuntime {
    /// Map of active actors indexed by their ID
    actors: HashMap<TheaterId, ActorProcess>,
    /// Map of chains index by actor ID
    chains: HashMap<TheaterId, Arc<RwLock<StateChain>>>,
    /// Sender for commands to the runtime
    theater_tx: Sender<TheaterCommand>,
    /// Receiver for commands to the runtime
    theater_rx: Receiver<TheaterCommand>,
    /// Map of active communication channels
    channels: HashMap<ChannelId, HashSet<ChannelParticipant>>,
    /// Optional channel to send channel events back to the server
    #[allow(dead_code)]
    channel_events_tx: Option<Sender<crate::messages::ChannelEvent>>,
    /// Shared async runtime for WASM execution.
    ///
    /// Wraps one `wasmtime::Engine`. Shared across every actor so a future
    /// compiled-module cache can hit across spawns (`wasmtime::Component`
    /// is engine-scoped — a per-spawn Engine would defeat any cache).
    /// `AsyncRuntime::new()` configures `async_support + multi_memory`
    /// only; no per-actor fuel, epoch, or interrupt setup that would
    /// justify isolation, so a singleton is safe.
    pack_runtime: Arc<AsyncRuntime>,
    /// Handler registry
    pub handler_registry: HandlerRegistry,
    /// Global subscribers — receive tagged events from every actor's chain.
    /// Each registered Sender is added (as a tagged subscriber) to every
    /// newly created `StateChain` at spawn time. Useful for top-level
    /// observers (CLI log streaming, embedded debug consoles) that
    /// multiplex events from many actors and need to attribute them.
    global_subscribers: Vec<Sender<(TheaterId, ChainEvent)>>,
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
    /// Current status of the actor
    pub status: ActorStatus,
    /// Optional actor manifest (for handler configs, replay, etc.)
    pub manifest: Option<ManifestConfig>,
    /// Controller for graceful shutdown
    pub shutdown_controller: ShutdownController,
    /// Optional supervisor channel for actor supervision
    pub supervisor_tx: Option<Sender<ActorResult>>,
}

impl TheaterRuntime {
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
    /// # use theater::handler::HandlerRegistry;
    /// # use theater::messages::TheaterCommand;
    /// # use tokio::sync::mpsc;
    /// # use anyhow::Result;
    /// #
    /// # async fn example() -> Result<()> {
    /// let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(100);
    /// let runtime = TheaterRuntime::new(theater_tx, theater_rx, None, HandlerRegistry::new()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        theater_tx: Sender<TheaterCommand>,
        theater_rx: Receiver<TheaterCommand>,
        channel_events_tx: Option<Sender<crate::messages::ChannelEvent>>,
        handler_registry: HandlerRegistry,
    ) -> Result<Self> {
        info!("Theater runtime initializing with Composite runtime");
        let pack_runtime = Arc::new(AsyncRuntime::new());

        Ok(Self {
            theater_tx,
            theater_rx,
            actors: HashMap::new(),
            chains: HashMap::new(),
            channels: HashMap::new(),
            channel_events_tx,
            pack_runtime,
            handler_registry,
            global_subscribers: Vec::new(),
        })
    }

    /// Register a subscriber that will receive events from every actor.
    ///
    /// The sender is registered on every chain created from this point on.
    /// Existing chains are NOT retroactively updated — for that, use
    /// `TheaterCommand::SubscribeToActor` per actor_id.
    ///
    /// Each chain dispatches to this sender via `try_send`, so a backed-up
    /// subscriber drops events with a warning but does not stall any actor.
    /// Drain the receiver promptly (or via a drainer task with a local ring
    /// buffer) to size for your tolerance.
    pub fn add_global_subscription(&mut self, tx: Sender<(TheaterId, ChainEvent)>) {
        self.global_subscribers.push(tx);
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
                    // Note: Child tracking is now handled by the supervisor handler.
                    // This command returns empty - use supervisor handler's internal tracking instead.
                    debug!("ListChildren called for {:?} (deprecated - supervisor handles child tracking)", parent_id);
                    let _ = response_tx.send(Vec::new());
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
                TheaterCommand::SpawnActor {
                    wasm_bytes,
                    name,
                    manifest,
                    init_state,
                    response_tx,
                    supervisor_tx,
                    subscription_tx,
                } => {
                    let actor_name = name.clone().unwrap_or_else(|| "<unnamed>".to_string());
                    debug!("Processing SpawnActor command for: {}", actor_name);
                    self.spawn_actor(
                        wasm_bytes,
                        name,
                        manifest,
                        init_state,
                        /* call_init = */ true,
                        supervisor_tx,
                        subscription_tx,
                        response_tx,
                    )
                    .await;
                }
                TheaterCommand::SetupActor {
                    wasm_bytes,
                    name,
                    manifest,
                    init_state,
                    response_tx,
                    supervisor_tx,
                    subscription_tx,
                } => {
                    let actor_name = name.clone().unwrap_or_else(|| "<unnamed>".to_string());
                    debug!("Processing SetupActor command for: {}", actor_name);
                    self.spawn_actor(
                        wasm_bytes,
                        name,
                        manifest,
                        init_state,
                        /* call_init = */ false,
                        supervisor_tx,
                        subscription_tx,
                        response_tx,
                    )
                    .await;
                }
                TheaterCommand::ResumeActor {
                    manifest_path,
                    wasm_bytes,
                    response_tx,
                    supervisor_tx,
                    subscription_tx,
                } => {
                    debug!(
                        "Processing ResumeActor command for manifest: {:?}",
                        manifest_path
                    );
                    // ResumeActor loads manifest from path for replay config
                    // Load manifest
                    let manifest_result: Result<ManifestConfig, TheaterRuntimeError> = async {
                        let manifest_str = if manifest_path.starts_with("store:")
                            || manifest_path.starts_with("https:")
                            || PathBuf::from(&manifest_path).exists()
                        {
                            let manifest_bytes =
                                resolve_reference(&manifest_path).await.map_err(|e| {
                                    TheaterRuntimeError::ActorInitializationError(format!(
                                        "Failed to load manifest: {}",
                                        e
                                    ))
                                })?;
                            String::from_utf8(manifest_bytes).map_err(|e| {
                                TheaterRuntimeError::ActorInitializationError(e.to_string())
                            })?
                        } else {
                            manifest_path.clone()
                        };

                        ManifestConfig::from_toml_str(&manifest_str).map_err(|e| {
                            TheaterRuntimeError::ActorInitializationError(format!(
                                "Failed to parse manifest: {}",
                                e
                            ))
                        })
                    }
                    .await;

                    let manifest = match manifest_result {
                        Ok(m) => m,
                        Err(e) => {
                            error!("Failed to load manifest: {}", e);
                            let _ = response_tx.send(Err(e.into()));
                            continue;
                        }
                    };

                    // Resolve WASM bytes
                    let wasm_bytes = match wasm_bytes {
                        Some(bytes) => bytes,
                        None => match resolve_reference(&manifest.package).await {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Failed to load WASM: {}", e);
                                let _ = response_tx
                                    .send(Err(anyhow::anyhow!("Failed to load WASM: {}", e)));
                                continue;
                            }
                        },
                    };

                    let name = Some(manifest.name.clone());
                    // Resume goes through the replay path; the replay handler
                    // walks the recorded chain (including the original init
                    // call), so `spawn_actor` must NOT auto-init.
                    self.spawn_actor(
                        wasm_bytes,
                        name,
                        Some(manifest),
                        default_init_state(),
                        /* call_init = */ false,
                        supervisor_tx,
                        subscription_tx,
                        response_tx,
                    )
                    .await;
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
                    info!(
                        "TheaterCommand::ShuttingDown received for actor: {:?}",
                        actor_id
                    );
                    // Notify supervisor before spawning the async shutdown
                    if let Some(proc) = self.actors.get(&actor_id) {
                        if let Some(supervisor_tx) = &proc.supervisor_tx {
                            let message = ActorResult::Success(ChildResult {
                                actor_id,
                                result: data,
                            });
                            if let Err(e) = supervisor_tx.send(message).await {
                                debug!("Failed to send shutdown message to supervisor (possibly shutting down): {}", e);
                            }
                        }
                    }
                    // Spawn the stop so the event loop stays free to process
                    // child StopActor commands (prevents deadlock when parent
                    // shutdown triggers child cleanup via supervisor handler)
                    let theater_tx = self.theater_tx.clone();
                    let actor_id_clone = actor_id;
                    if let Some(proc) = self.actors.get(&actor_id) {
                        let control_tx = proc.control_tx.clone();
                        tokio::spawn(async move {
                            let start = std::time::Instant::now();
                            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                            if let Err(e) = control_tx
                                .send(ActorControl::Shutdown { response_tx })
                                .await
                            {
                                error!("Failed to send shutdown signal: {}", e);
                            } else {
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(10),
                                    response_rx,
                                )
                                .await
                                {
                                    Ok(Ok(Ok(_))) => {
                                        info!(
                                            "Actor runtime {:?} acknowledged shutdown in {:?}",
                                            actor_id_clone,
                                            start.elapsed()
                                        );
                                    }
                                    Ok(Ok(Err(e))) => {
                                        error!(
                                            "Actor runtime {:?} shutdown error: {:?}",
                                            actor_id_clone, e
                                        );
                                    }
                                    Ok(Err(_)) => {
                                        error!(
                                            "Actor runtime {:?} response channel closed",
                                            actor_id_clone
                                        );
                                    }
                                    Err(_) => {
                                        error!(
                                            "Timeout waiting for actor runtime {:?} (10s)",
                                            actor_id_clone
                                        );
                                    }
                                }
                            }
                            // Signal theater to finalize cleanup
                            let _ = theater_tx
                                .send(TheaterCommand::ActorShutdownComplete {
                                    actor_id: actor_id_clone,
                                })
                                .await;
                        });
                    }
                }
                TheaterCommand::ActorShutdownComplete { actor_id } => {
                    info!("ActorShutdownComplete for {:?}, cleaning up", actor_id);

                    // Signal handlers to shut down and remove from maps
                    if let Some(proc) = self.actors.remove(&actor_id) {
                        proc.shutdown_controller
                            .signal_shutdown(ShutdownType::Graceful)
                            .await;
                        info!("Actor {:?} handler shutdown complete", actor_id);
                    }

                    // Remove from channels
                    let id_for_channels = ChannelParticipant::Actor(actor_id);
                    let mut channels_to_remove = Vec::new();
                    for (channel_id, participants) in self.channels.iter_mut() {
                        if participants.remove(&id_for_channels) && participants.is_empty() {
                            channels_to_remove.push(channel_id.clone());
                        }
                    }
                    for channel_id in channels_to_remove {
                        self.channels.remove(&channel_id);
                    }

                    self.chains.remove(&actor_id);

                    if self.actors.is_empty() {
                        info!("All actors have shut down, exiting runtime");
                        break;
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
                        .map(|(id, proc)| (*id, proc.name.clone()))
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
                        if let Some(manifest) = proc.manifest.clone() {
                            if let Err(e) = response_tx.send(Ok(manifest)) {
                                error!("Failed to send actor manifest: {:?}", e);
                            }
                        } else {
                            let _ = response_tx.send(Err(anyhow::anyhow!("Actor has no manifest")));
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
                TheaterCommand::SubscribeToActor { actor_id, event_tx } => {
                    debug!("Subscribing to events for actor: {:?}", actor_id);
                    if let Some(chain) = self.chains.get(&actor_id) {
                        chain.write().await.add_subscriber(event_tx);
                    } else {
                        warn!(
                            "SubscribeToActor: no chain for {:?} (actor not running)",
                            actor_id
                        );
                    }
                }
                TheaterCommand::UnsubscribeFromActor { actor_id, event_tx } => {
                    debug!("Unsubscribing from events for actor: {:?}", actor_id);
                    if let Some(chain) = self.chains.get(&actor_id) {
                        chain.write().await.remove_subscriber(&event_tx);
                    } else {
                        debug!(
                            "UnsubscribeFromActor: no chain for {:?} (actor not running)",
                            actor_id
                        );
                    }
                }
                TheaterCommand::NewStore { response_tx } => {
                    debug!("Creating new content store");
                    let store_id = crate::store::ContentStore::new();
                    let _ = response_tx.send(Ok(store_id));
                }
                TheaterCommand::GetActorHandle {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting actor handle for: {:?}", actor_id);
                    let handle = self.actors.get(&actor_id).map(|proc| {
                        ActorHandle::new(
                            proc.operation_tx.clone(),
                            proc.info_tx.clone(),
                            proc.control_tx.clone(),
                        )
                    });
                    let _ = response_tx.send(handle);
                }
                TheaterCommand::GetActorExportHashes {
                    actor_id,
                    response_tx,
                } => {
                    debug!("Getting export hashes for actor: {:?}", actor_id);
                    if let Some(proc) = self.actors.get(&actor_id) {
                        let handle = ActorHandle::new(
                            proc.operation_tx.clone(),
                            proc.info_tx.clone(),
                            proc.control_tx.clone(),
                        );
                        // Query the actor for its export hashes
                        match handle.get_export_hashes().await {
                            Ok(hashes) => {
                                let _ = response_tx.send(Some(hashes));
                            }
                            Err(e) => {
                                error!("Failed to get export hashes: {:?}", e);
                                let _ = response_tx.send(None);
                            }
                        }
                    } else {
                        let _ = response_tx.send(None);
                    }
                }
                TheaterCommand::ShutdownRuntime => {
                    info!("Received shutdown runtime command");
                    break;
                }
            };
        }
        info!("Theater runtime shutting down");
        Ok(())
    }

    /// Spawns a new actor from WASM bytes.
    ///
    /// ## Parameters
    ///
    /// * `wasm_bytes` - The WASM module bytes to instantiate
    /// * `name` - Optional actor name for debugging/logging
    /// * `manifest` - Optional manifest for handler configs, replay settings, etc.
    /// * `supervisor_tx` - Optional channel for supervisor to receive lifecycle events
    /// * `subscription_tx` - Optional channel to subscribe to all actor events
    ///
    /// ## Returns
    ///
    /// * `Ok(TheaterId)` - The ID of the newly spawned actor
    /// * `Err(anyhow::Error)` - An error occurred during actor spawn
    ///
    /// ## Implementation Notes
    ///
    /// This method handles the entire process of spawning a new actor, including:
    /// - Creating communication channels
    /// - Spawning the actor runtime in a new task
    /// - Registering the actor with the runtime
    ///
    /// If no manifest is provided, the actor uses global handler defaults.
    /// Set up an actor (and optionally fire its init).
    ///
    /// `response_tx` is fired by this function or the detached init task
    /// it spawns — never by the caller. This is what unblocks the runtime
    /// command loop: when `call_init = true`, the init RPC runs in a
    /// detached tokio task so the runtime can return to processing other
    /// commands (notably another `SpawnActor` issued from inside the
    /// just-spawned actor's `init`, via `supervisor.spawn`).
    #[allow(clippy::too_many_arguments)]
    async fn spawn_actor(
        &mut self,
        wasm_bytes: Vec<u8>,
        name: Option<String>,
        manifest: Option<ManifestConfig>,
        init_state: Value,
        call_init: bool,
        supervisor_tx: Option<Sender<ActorResult>>,
        subscription_tx: Option<Sender<(TheaterId, ChainEvent)>>,
        response_tx: oneshot::Sender<Result<TheaterId>>,
    ) {
        let actor_name = name.unwrap_or_else(|| "<unnamed>".to_string());
        debug!("Starting actor spawn process for: {}", actor_name);
        debug!("WASM bytes: {} bytes", wasm_bytes.len());

        // spawn-bench: timing spans match the supervisor-side names so the
        // two streams stitch together by actor_id (post-spawn) or by name
        // (pre-spawn). spawn_actor itself runs in the runtime command loop,
        // so every elapsed_ms here represents queue-blocking time.
        let spawn_actor_start = Instant::now();

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

        let chain = Arc::new(RwLock::new(StateChain::new(actor_id)));

        // Register the optional subscription_tx + all global subscribers
        // BEFORE inserting the chain so no event escapes between actor
        // init and subscription.
        {
            let mut chain_guard = chain.write().await;
            if let Some(tx) = subscription_tx {
                chain_guard.add_subscriber(tx);
            }
            for global in self.global_subscribers.iter().cloned() {
                chain_guard.add_subscriber(global);
            }
        }

        self.chains.insert(actor_id, chain.clone());

        // The caller resolved `init_state` ahead of time (manifest precedence,
        // defaults, etc. live in the caller). spawn_actor takes it at face
        // value and stores it as the actor's initial state.
        let phase_start = Instant::now();
        let from_manifest = manifest.is_some();
        let handler_registry = if let Some(ref manifest) = manifest {
            match self.create_handler_registry_for_manifest(manifest).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to build handler registry: {}", e);
                    let _ = response_tx.send(Err(anyhow::anyhow!(
                        "Failed to build handler registry: {}",
                        e
                    )));
                    return;
                }
            }
        } else {
            self.handler_registry.clone()
        };
        info!(
            phase = "runtime.handler_registry",
            actor_id = %actor_id,
            from_manifest,
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "spawn phase complete",
        );
        let initial_state = init_state;

        // Start the actor in a detached task. The pack runtime (and the
        // wasmtime Engine inside it) is shared across every spawn —
        // `Arc::clone` is cheap and lets a future compile cache hit
        // across spawns.
        let actor_id_for_task = actor_id;
        let actor_name_for_task = actor_name.clone();
        let pack_runtime = self.pack_runtime.clone();

        // Create channel to receive setup result
        let (setup_tx, setup_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

        let phase_start = Instant::now();
        let actor_runtime_process = tokio::spawn(async move {
            let _actor_runtime = ActorRuntime::start(
                actor_id_for_task,
                actor_name_for_task,
                wasm_bytes,
                pack_runtime,
                chain,
                handler_registry,
                theater_tx,
                operation_rx,
                actor_operation_tx,
                info_rx,
                actor_info_tx,
                control_rx,
                actor_control_tx,
                initial_state,
                Some(setup_tx),
            )
            .await;
        });

        // Wait for setup to complete
        match setup_rx.await {
            Ok(Ok(())) => {
                debug!("Actor {} setup completed successfully", actor_id);
                info!(
                    phase = "runtime.setup",
                    actor_id = %actor_id,
                    elapsed_ms = phase_start.elapsed().as_millis() as u64,
                    "spawn phase complete",
                );
            }
            Ok(Err(e)) => {
                error!("Actor {} setup failed: {}", actor_id, e);
                let _ = response_tx.send(Err(anyhow::anyhow!("Actor setup failed: {}", e)));
                return;
            }
            Err(_) => {
                error!("Actor {} setup channel closed unexpectedly", actor_id);
                let _ =
                    response_tx.send(Err(anyhow::anyhow!("Actor setup failed: channel closed")));
                return;
            }
        }

        // Create ActorHandle for lifecycle notification before moving channels.
        // Also used to fire actor.init below when `call_init` is true.
        let actor_handle = crate::actor::handle::ActorHandle::new(
            operation_tx.clone(),
            info_tx.clone(),
            control_tx.clone(),
        );

        let process = ActorProcess {
            actor_id,
            name: actor_name,
            process: actor_runtime_process,
            mailbox_tx,
            operation_tx,
            info_tx,
            control_tx,
            status: ActorStatus::Running,
            manifest,
            shutdown_controller,
            supervisor_tx,
        };

        self.actors.insert(actor_id, process);
        debug!("Actor process registered with runtime");
        // elapsed_ms here is the total queue-blocking cost: every ms
        // counted is one ms the runtime command loop spent serialized
        // on this spawn instead of draining other commands.
        info!(
            phase = "runtime.register",
            actor_id = %actor_id,
            elapsed_ms = spawn_actor_start.elapsed().as_millis() as u64,
            "spawn registered (runtime command loop now free)",
        );

        // Setup is complete; the actor is reachable by RPC. From here on we
        // must not hold the runtime command loop. For `call_init = false`,
        // fire response_tx immediately and return. For `call_init = true`,
        // detach the init RPC into a tokio task and let response_tx fire
        // there — that's what lets supervisor.spawn-from-inside-init work
        // (the new SpawnActor command for the child can be picked up by the
        // command loop while the parent's init is still mid-flight).
        if call_init {
            let init_phase_start = Instant::now();
            tokio::spawn(async move {
                let init_params = Value::Tuple(vec![]);
                match actor_handle
                    .call_function("theater:simple/actor.init".to_string(), init_params)
                    .await
                {
                    Ok(_) => {
                        debug!("Actor {} init completed", actor_id);
                        info!(
                            phase = "runtime.init",
                            actor_id = %actor_id,
                            elapsed_ms = init_phase_start.elapsed().as_millis() as u64,
                            "spawn phase complete",
                        );
                        let _ = response_tx.send(Ok(actor_id));
                    }
                    Err(e) => {
                        error!("Actor {} init failed: {}", actor_id, e);
                        let _ = response_tx.send(Err(anyhow::anyhow!("actor.init failed: {}", e)));
                    }
                }
            });
        } else {
            let _ = response_tx.send(Ok(actor_id));
        }
    }

    async fn handle_actor_error(&mut self, actor_id: TheaterId, error: ActorError) -> Result<()> {
        debug!("Handling error event for actor: {:?}", actor_id);

        // notify the actors parents
        if let Some(proc) = self.actors.get(&actor_id) {
            if let Some(supervisor_tx) = &proc.supervisor_tx {
                let error_message = ActorResult::Error(ChildError {
                    actor_id,
                    error: error.clone(),
                });
                // Send error and immediately shutdown - don't wait for response
                let supervisor_tx_clone = supervisor_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = supervisor_tx_clone.send(error_message).await {
                        debug!("Failed to send error message to supervisor (possibly shutting down): {}", e);
                    }
                });
            }
        }

        // Subscribers see the actor's terminal chain event (`WasmError`
        // for crashes, `"shutdown"` for normal exit) then `recv() == None`
        // when the chain drops — no separate notification needed here.

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
        info!(
            "stop_actor called for: {:?} (shutdown_type: {:?})",
            actor_id, shutdown_type
        );

        // Check if the actor exists in the registry
        if !self.actors.contains_key(&actor_id) {
            warn!("Actor {:?} not found in registry", actor_id);
            return Ok(());
        }

        debug!("Actor {:?} found, proceeding with shutdown", actor_id);

        'chain_block: {
            let chain = match self.chains.get(&actor_id) {
                Some(chain) => chain,
                None => {
                    error!("Actor {:?} has no associated chain", actor_id);
                    break 'chain_block;
                }
            };

            let mut writable_chain = chain.write().await;
            writable_chain
                .add_typed_event(crate::events::ChainEventData {
                    event_type: "shutdown".to_string(),
                    data: crate::events::ChainEventPayload::Wasm(
                        crate::events::wasm::WasmEventData::WasmCall {
                            function_name: "shutdown".to_string(),
                            params: Value::Tuple(vec![]),
                        },
                    ),
                })
                .await
                .expect("Failed to record event");
        }

        self.chains.remove(&actor_id);

        // Get the actor process - but DON'T remove it from the map yet
        // We need to keep it in the map while shutdown is in progress
        // Note: Child stopping is now handled by the supervisor handler
        let proc = match self.actors.get(&actor_id) {
            Some(proc) => proc,
            None => {
                debug!("Actor {:?} not found during shutdown", actor_id);
                return Ok(());
            }
        };

        // First, signal the actor runtime itself to shut down via its control channel
        // This stops the operation/info loops from processing new requests
        debug!(
            "Sending shutdown signal to actor runtime for {:?}",
            actor_id
        );
        let actor_runtime_start = std::time::Instant::now();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        if let Err(e) = proc
            .control_tx
            .send(ActorControl::Shutdown { response_tx })
            .await
        {
            error!(
                "Failed to send shutdown signal to actor runtime for {:?}: {}",
                actor_id, e
            );
            // Continue with shutdown anyway - we'll try to clean up handlers
        } else {
            // Wait for the actor runtime to acknowledge shutdown with a timeout
            match tokio::time::timeout(std::time::Duration::from_secs(10), response_rx).await {
                Ok(Ok(Ok(_))) => {
                    debug!(
                        "Actor runtime for {:?} acknowledged shutdown in {:?}",
                        actor_id,
                        actor_runtime_start.elapsed()
                    );
                }
                Ok(Ok(Err(e))) => {
                    error!(
                        "Actor runtime for {:?} returned error during shutdown: {:?}",
                        actor_id, e
                    );
                }
                Ok(Err(_)) => {
                    error!("Actor runtime for {:?} response channel closed", actor_id);
                }
                Err(_) => {
                    error!(
                        "Timeout waiting for actor runtime {:?} to shut down (10s)",
                        actor_id
                    );
                }
            }
        }

        // Now signal handlers to shut down
        // The handlers will wait for their cleanup to complete before responding
        debug!("Signaling handlers to shutdown for actor {:?}", actor_id);
        let handler_start = std::time::Instant::now();

        // Remove from map now, after actor runtime has shut down but before waiting on handlers
        // This ensures the actor is removed from the registry while handlers clean up
        let proc = self.actors.remove(&actor_id).unwrap(); // Safe - we just checked it exists

        proc.shutdown_controller
            .signal_shutdown(shutdown_type)
            .await;

        debug!(
            "Actor {:?} handler shutdown complete in {:?}",
            actor_id,
            handler_start.elapsed()
        );

        // Remove actor from any channel registrations
        let mut channels_to_remove = Vec::new();
        let id_for_channels = ChannelParticipant::Actor(actor_id);
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
    #[allow(dead_code)]
    async fn shutdown_actor(&mut self, actor_id: TheaterId, data: Option<Vec<u8>>) -> Result<()> {
        debug!("Shutting down actor: {:?}", actor_id);

        // Notify the actor's supervisor if it has one
        if let Some(proc) = self.actors.get(&actor_id) {
            if let Some(supervisor_tx) = &proc.supervisor_tx {
                let message = ActorResult::Success(ChildResult {
                    actor_id,
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
                let error_message = ActorResult::ExternalStop(ChildExternalStop { actor_id });
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

    async fn get_actor_state(&self, actor_id: TheaterId) -> Result<Value> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's state
            let (tx, rx) = oneshot::channel();
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

    async fn get_actor_metrics(&self, actor_id: TheaterId) -> Result<ActorMetrics> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's metrics
            let (tx, rx) = oneshot::channel();
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

    /// Creates a handler registry for a specific actor based on its manifest.
    ///
    /// This method applies per-actor handler configurations from the manifest.
    /// Each handler in the registry will receive its matching config (if any)
    /// when `create_instance` is called.
    ///
    /// Special handling for replay: If the manifest contains a replay handler
    /// configuration, this method will load the chain and prepend a ReplayHandler.
    async fn create_handler_registry_for_manifest(
        &self,
        manifest: &ManifestConfig,
    ) -> Result<HandlerRegistry> {
        // Clone the registry with per-actor configs applied
        let mut registry = self.handler_registry.clone_with_configs(&manifest.handlers);

        // Special handling for replay handler - needs to load chain file
        for handler_config in &manifest.handlers {
            if let HandlerConfig::Replay { config } = handler_config {
                info!(
                    "Found replay handler config, loading chain from: {:?}",
                    config.chain
                );

                // Load the chain from a JSON array file. The producer (a
                // subscriber actor or external tool) is responsible for
                // writing this file — the runtime no longer emits one.
                let chain_bytes = std::fs::read(&config.chain).map_err(|e| {
                    TheaterRuntimeError::ActorInitializationError(format!(
                        "Failed to read replay chain file {:?}: {}",
                        config.chain, e
                    ))
                })?;
                let chain_events: Vec<ChainEvent> =
                    serde_json::from_slice(&chain_bytes).map_err(|e| {
                        TheaterRuntimeError::ActorInitializationError(format!(
                            "Failed to parse replay chain file {:?}: {}",
                            config.chain, e
                        ))
                    })?;

                info!(
                    "Loaded replay chain with {} events from {:?}",
                    chain_events.len(),
                    config.chain
                );

                // Store the replay chain in the registry for handlers that need it
                // (e.g., WasiHttpHandler for replaying HTTP events)
                registry.set_replay_chain(chain_events.clone());

                // Prepend ReplayHandler to intercept imports
                registry.prepend(ReplayHandler::new(chain_events));
                break;
            }
        }

        Ok(registry)
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
