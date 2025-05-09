//! # Theater Runtime
//!
//! The `theater_runtime` module implements the core runtime environment for the Theater
//! actor system. It manages actor lifecycle, message passing, and event handling across
//! the entire system.

use crate::actor::runtime::{ActorRuntime, StartActorResult};
use crate::actor::types::{ActorError, ActorOperation};
use crate::chain::ChainEvent;
use crate::id::TheaterId;
use crate::messages::{
    ActorChannelClose, ActorChannelMessage, ActorChannelOpen, ChannelId, ChannelParticipant,
    ChildError,
};
use crate::messages::{ActorMessage, ActorStatus, TheaterCommand};
use crate::metrics::ActorMetrics;
use crate::shutdown::ShutdownController;
use crate::utils::resolve_reference;
use crate::ManifestConfig;
use crate::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
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
///     let mut runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx, None).await?;
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
    /// Sender for commands to the runtime
    pub theater_tx: Sender<TheaterCommand>,
    /// Receiver for commands to the runtime
    theater_rx: Receiver<TheaterCommand>,
    /// Map of event subscriptions for actors
    subscriptions: HashMap<TheaterId, Vec<Sender<Result<ChainEvent, ActorError>>>>,
    /// Map of active communication channels
    channels: HashMap<ChannelId, HashSet<ChannelParticipant>>,
    /// Optional channel to send channel events back to the server
    channel_events_tx: Option<Sender<crate::theater_server::ChannelEvent>>,
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
    pub process: JoinHandle<ActorRuntime>,
    /// Channel for sending messages to the actor
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    /// Channel for sending operations to the actor
    pub operation_tx: mpsc::Sender<ActorOperation>,
    /// Set of child actor IDs
    pub children: HashSet<TheaterId>,
    /// Parent actor ID (if any)
    pub parent_id: Option<TheaterId>,
    /// Current status of the actor
    pub status: ActorStatus,
    /// Path to the actor's manifest
    pub manifest_path: String,
    /// Actor Manifest
    pub manifest: ManifestConfig,
    /// Controller for graceful shutdown
    pub shutdown_controller: ShutdownController,
    /// Optional supervisor channel for actor supervision
    pub supervisor_tx: Option<Sender<ChildError>>,
}

impl TheaterRuntime {
    /// Creates a new TheaterRuntime with the given communication channels.
    ///
    /// ## Parameters
    ///
    /// * `theater_tx` - Sender for commands to the runtime
    /// * `theater_rx` - Receiver for commands to the runtime
    /// * `channel_events_tx` - Optional channel for sending events to external systems
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
    /// let runtime = TheaterRuntime::new(theater_tx, theater_rx, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        theater_tx: Sender<TheaterCommand>,
        theater_rx: Receiver<TheaterCommand>,
        channel_events_tx: Option<Sender<crate::theater_server::ChannelEvent>>,
    ) -> Result<Self> {
        info!("Theater runtime initializing");

        Ok(Self {
            theater_tx,
            theater_rx,
            actors: HashMap::new(),
            subscriptions: HashMap::new(),
            channels: HashMap::new(),
            channel_events_tx,
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
                    match self.stop_actor(actor_id).await {
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
                TheaterCommand::UpdateActorComponent {
                    actor_id,
                    component,
                    response_tx,
                } => {
                    debug!("Updating actor component: {:?}", actor_id);
                    match self.update_actor_component(actor_id, component).await {
                        Ok(_) => {
                            info!("Actor component updated successfully");
                            let _ = response_tx.send(Ok(()));
                        }
                        Err(e) => {
                            error!("Failed to update actor component: {}", e);
                            let _ = response_tx.send(Err(e));
                        }
                    }
                }
                TheaterCommand::SendMessage {
                    actor_id,
                    actor_message,
                } => {
                    debug!("Sending message to actor: {:?}", actor_id);
                    if let Some(proc) = self.actors.get_mut(&actor_id) {
                        if let Err(e) = proc.mailbox_tx.send(actor_message).await {
                            error!("Failed to send message to actor: {}", e);
                        }
                    } else {
                        warn!(
                            "Attempted to send message to non-existent actor: {:?}",
                            actor_id
                        );
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

                    // Use entry API to handle the subscription map more elegantly
                    match self.subscriptions.entry(actor_id.clone()) {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            entry.get_mut().push(event_tx);
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(vec![event_tx]);
                        }
                    }
                }
                // Channel-related commands
                TheaterCommand::ChannelOpen {
                    initiator_id,
                    target_id,
                    channel_id,
                    initial_message,
                    response_tx,
                } => {
                    debug!("Opening channel from {:?} to {:?}", initiator_id, target_id);
                    if initiator_id == target_id {
                        warn!("Attempted to open channel to self: {:?}", initiator_id);
                        let _ =
                            response_tx.send(Err(anyhow::anyhow!("Cannot open channel to self")));
                        continue;
                    }
                    match target_id {
                        ChannelParticipant::Actor(ref actor_id) => {
                            if let Some(proc) = self.actors.get_mut(&actor_id) {
                                // Create a oneshot channel to intercept the response
                                let (inner_tx, inner_rx) = tokio::sync::oneshot::channel();

                                // Create channel open message
                                let actor_message = ActorMessage::ChannelOpen(ActorChannelOpen {
                                    channel_id: channel_id.clone(),
                                    response_tx: inner_tx,
                                    data: initial_message,
                                });

                                // Send the message to the target actor
                                if let Err(e) = proc.mailbox_tx.send(actor_message).await {
                                    error!("Failed to send channel open message to actor: {}", e);
                                    let _ = response_tx.send(Err(anyhow::anyhow!(
                                        "Failed to send channel open message"
                                    )));
                                    continue;
                                }

                                // Process the response asynchronously
                                let channel_id_clone = channel_id.clone();
                                let initiator_id_clone = initiator_id.clone();
                                let target_id_clone = target_id.clone();
                                let theater_tx_clone = self.theater_tx.clone();

                                tokio::spawn(async move {
                                    match inner_rx.await {
                                        Ok(result) => {
                                            if let Ok(true) = &result {
                                                // Channel was accepted, register both participants via a new command
                                                // This avoids holding a mutable reference across an await point
                                                let register_cmd =
                                                    TheaterCommand::RegisterChannel {
                                                        channel_id: channel_id_clone.clone(),
                                                        participants: vec![
                                                            initiator_id_clone.clone(),
                                                            target_id_clone.clone(),
                                                        ],
                                                    };

                                                if let Err(e) =
                                                    theater_tx_clone.send(register_cmd).await
                                                {
                                                    error!(
                                                "Failed to register channel participants: {}",
                                                e
                                            );
                                                } else {
                                                    debug!("Requested registration of channel {:?} with participants {:?} and {:?}", 
                                                channel_id_clone, initiator_id_clone, target_id_clone);
                                                }
                                            }

                                            // Forward the result to the original requester
                                            let _ = response_tx.send(result);
                                        }
                                        Err(e) => {
                                            error!(
                                                "Failed to receive channel open response: {}",
                                                e
                                            );
                                            let _ = response_tx.send(Err(anyhow::anyhow!(
                                                "Failed to receive channel open response"
                                            )));
                                        }
                                    }
                                });
                            } else {
                                warn!(
                                    "Attempted to open channel to non-existent actor: {:?}",
                                    target_id
                                );
                                let _ = response_tx
                                    .send(Err(anyhow::anyhow!("Target actor not found")));
                            }
                        }
                        ChannelParticipant::External => {
                            warn!(
                                "External channel participants cannot be targeted for channel open"
                            );
                            let _ = response_tx.send(Err(anyhow::anyhow!(
                                "External participants cannot be targeted for channel open"
                            )));
                        }
                    }
                }
                TheaterCommand::ChannelMessage {
                    channel_id,
                    message,
                    sender_id,
                } => {
                    debug!("Sending message on channel: {:?}", channel_id);

                    // Look up the participants for this channel
                    if let Some(participant_ids) = self.channels.get(&channel_id) {
                        let mut successful_delivery = false;

                        for participant in participant_ids {
                            debug!("Delivering message to participant {:?}", participant);
                            if *participant == sender_id {
                                debug!("Skipping message delivery to sender");
                                continue;
                            }
                            match participant {
                                ChannelParticipant::Actor(actor_id) => {
                                    if let Some(proc) = self.actors.get_mut(actor_id) {
                                        let actor_message =
                                            ActorMessage::ChannelMessage(ActorChannelMessage {
                                                channel_id: channel_id.clone(),
                                                data: message.clone(),
                                            });

                                        match proc.mailbox_tx.send(actor_message).await {
                                            Ok(_) => {
                                                successful_delivery = true;
                                                debug!(
                                                    "Delivered channel message to actor {:?}",
                                                    actor_id
                                                );
                                            }
                                            Err(e) => {
                                                error!(
                                                    "Failed to send channel message to actor {:?}: {}",
                                                    actor_id, e
                                                );
                                            }
                                        }
                                    } else {
                                        warn!(
                                            "Actor {:?} registered for channel {:?} no longer exists",
                                            actor_id, channel_id
                                        );
                                    }
                                }
                                ChannelParticipant::External => {
                                    debug!("Sending message to server");
                                    // Send the message to the server
                                    if let Some(tx) = &self.channel_events_tx {
                                        let channel_event =
                                            crate::theater_server::ChannelEvent::Message {
                                                channel_id: channel_id.clone(),
                                                sender_id: sender_id.clone(),
                                                message: message.clone(),
                                            };

                                        if let Err(e) = tx.send(channel_event).await {
                                            error!("Failed to send message to server: {}", e);
                                        } else {
                                            debug!(
                                                "Delivered message to server for channel {:?}",
                                                channel_id
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        if !successful_delivery {
                            warn!(
                                "Failed to deliver message to any actor for channel {:?}",
                                channel_id
                            );
                        }
                    } else {
                        warn!("No actors registered for channel: {:?}", channel_id);
                    }
                }
                TheaterCommand::ChannelClose { channel_id } => {
                    debug!("Closing channel: {:?}", channel_id);

                    // Get participant IDs before removing the channel
                    let participant_ids = if let Some(ids) = self.channels.get(&channel_id) {
                        ids.clone()
                    } else {
                        HashSet::new()
                    };

                    // Remove the channel from the registry
                    self.channels.remove(&channel_id);
                    debug!("Removed channel {:?} from registry", channel_id);

                    // Notify participants about channel closure
                    let mut successful_notification = false;
                    for participant in &participant_ids {
                        match participant {
                            ChannelParticipant::Actor(actor_id) => {
                                if let Some(proc) = self.actors.get_mut(actor_id) {
                                    let actor_message =
                                        ActorMessage::ChannelClose(ActorChannelClose {
                                            channel_id: channel_id.clone(),
                                        });

                                    match proc.mailbox_tx.send(actor_message).await {
                                        Ok(_) => {
                                            successful_notification = true;
                                            debug!(
                                                "Notified actor {:?} about channel {:?} closure",
                                                actor_id, channel_id
                                            );
                                        }
                                        Err(e) => {
                                            error!(
                                        "Failed to send channel close message to actor {:?}: {}",
                                        actor_id, e
                                    );
                                        }
                                    }
                                } else {
                                    warn!("Actor {:?} registered for channel {:?} no longer exists during closure", 
                                actor_id, channel_id);
                                }
                            }
                            ChannelParticipant::External => {
                                // Send the message to the server
                                if let Some(tx) = &self.channel_events_tx {
                                    let channel_event =
                                        crate::theater_server::ChannelEvent::Close {
                                            channel_id: channel_id.clone(),
                                        };

                                    if let Err(e) = tx.send(channel_event).await {
                                        error!("Failed to send close event to server: {}", e);
                                    } else {
                                        debug!(
                                            "Notified server about closure of channel {:?}",
                                            channel_id
                                        );
                                    }
                                }
                            }
                        }
                    }

                    if !successful_notification && !participant_ids.is_empty() {
                        warn!(
                            "Failed to notify any actors about channel {:?} closure",
                            channel_id
                        );
                    }
                }
                TheaterCommand::ListChannels { response_tx } => {
                    debug!("Getting list of channels");
                    let channels = self.list_channels().await;

                    if let Err(e) = response_tx.send(Ok(channels)) {
                        error!("Failed to send channel list: {:?}", e);
                    }
                }
                TheaterCommand::GetChannelStatus {
                    channel_id,
                    response_tx,
                } => {
                    debug!("Getting status for channel: {:?}", channel_id);
                    let status = self.get_channel_status(&channel_id).await;

                    if let Err(e) = response_tx.send(Ok(status)) {
                        error!("Failed to send channel status: {:?}", e);
                    }
                }
                TheaterCommand::RegisterChannel {
                    channel_id,
                    participants,
                } => {
                    debug!(
                        "Registering channel {:?} with {} participants",
                        channel_id,
                        participants.len()
                    );

                    // Convert the Vec to a HashSet
                    let participant_set: HashSet<ChannelParticipant> =
                        participants.into_iter().collect();

                    // Register the channel with its participants
                    self.channels.insert(channel_id.clone(), participant_set);

                    debug!("Successfully registered channel {:?}", channel_id);
                }
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
        init: bool,
        supervisor_tx: Option<Sender<ChildError>>,
    ) -> Result<TheaterId> {
        debug!(
            "Starting actor spawn process from manifest: {:?}",
            manifest_path
        );

        // check if the manifest is a valid path OR starts with store:
        let manifest: ManifestConfig;
        if manifest_path.starts_with("store:") || PathBuf::from(manifest_path.clone()).exists() {
            let manifest_bytes = resolve_reference(manifest_path.as_str()).await?;
            manifest = ManifestConfig::from_vec(manifest_bytes)?;
        } else {
            manifest = ManifestConfig::from_str(manifest_path.as_str())?;
        };

        // start the actor in a new process
        let (response_tx, mut response_rx) = mpsc::channel(1);
        // Create a shutdown controller for this specific actor
        let (shutdown_controller, shutdown_receiver) = ShutdownController::new();
        let (mailbox_tx, mailbox_rx) = mpsc::channel(100);
        let (operation_tx, operation_rx) = mpsc::channel(100);
        let theater_tx = self.theater_tx.clone();

        let actor_operation_tx = operation_tx.clone();
        let shutdown_receiver_clone = shutdown_receiver;
        let actor_sender = mailbox_tx.clone();

        let actor_id = TheaterId::generate();
        debug!("Initializing actor runtime");
        debug!("Starting actor runtime");

        // Start the actor in a detached task
        let actor_id_for_task = actor_id.clone();
        let actor_name = manifest.name.clone();
        let manifest_clone = manifest.clone();
        let actor_runtime_process = tokio::spawn(async move {
            ActorRuntime::start(
                actor_id_for_task.clone(),
                &manifest_clone,
                init_bytes,
                theater_tx,
                actor_sender,
                mailbox_rx,
                operation_rx,
                actor_operation_tx,
                init,
                shutdown_receiver_clone,
                response_tx,
            )
            .await;

            // Return a dummy struct to maintain API compatibility
            ActorRuntime {
                actor_id: actor_id_for_task,
                handler_tasks: Vec::new(),
                shutdown_controller: ShutdownController::new().0,
            }
        });

        match response_rx.recv().await {
            Some(StartActorResult::Success(actor_id)) => {
                debug!(
                    "Received actor ID from runtime initialization: {:?}",
                    actor_id
                );
                let process = ActorProcess {
                    actor_id: actor_id.clone(),
                    name: actor_name,
                    process: actor_runtime_process,
                    mailbox_tx,
                    operation_tx,
                    children: HashSet::new(),
                    parent_id: parent_id.clone(),
                    status: ActorStatus::Running,
                    manifest_path: manifest_path.clone(),
                    manifest,
                    shutdown_controller,
                    supervisor_tx,
                };

                if let Some(parent_id) = parent_id {
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
            Some(StartActorResult::Failure(actor_id, e)) => {
                error!("Failed to start actor [{}]: {}", actor_id, e);
                // Abort the runtime process since it failed
                actor_runtime_process.abort();
                // Return the specific error message to the spawner
                Err(anyhow::anyhow!(
                    "Actor startup failed: [{}] {}",
                    actor_id,
                    e
                ))
            }
            None => {
                error!("Failed to receive actor ID from runtime");
                // Abort the runtime process since we couldn't get a response
                actor_runtime_process.abort();
                Err(anyhow::anyhow!(
                    "Failed to receive response from actor runtime",
                ))
            }
        }
    }

    async fn handle_actor_event(&mut self, actor_id: TheaterId, event: ChainEvent) -> Result<()> {
        debug!("Handling event for actor: {:?}", actor_id);

        // Use entry API to handle the subscription map more elegantly
        let should_remove = if let std::collections::hash_map::Entry::Occupied(mut entry) =
            self.subscriptions.entry(actor_id.clone())
        {
            let subscribers = entry.get_mut();
            let mut to_remove = Vec::new();

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
                let error_message = ChildError {
                    actor_id: actor_id.clone(),
                    error: error.clone(),
                };
                if let Err(e) = supervisor_tx.send(error_message).await {
                    error!("Failed to send error message to supervisor: {}", e);
                }
            }
        }

        // pause the actor's children
        if let Some(proc) = self.actors.get(&actor_id) {
            for child_id in &proc.children.clone() {
                if let Some(child_proc) = self.actors.get_mut(&child_id.clone()) {
                    let child_id = child_proc.actor_id.clone();
                    let operation_tx = child_proc.operation_tx.clone();
                    tokio::spawn(async move {
                        debug!("Pausing child actor: {:?}", child_id);
                        let (response_tx, response_rx) = oneshot::channel();
                        if let Err(e) = operation_tx
                            .send(ActorOperation::Pause { response_tx })
                            .await
                        {
                            error!("Failed to send error message to child actor: {}", e);
                        }
                        if let Ok(result) = response_rx.await {
                            match result {
                                Ok(_) => debug!("Child actor {:?} paused successfully", child_id),
                                Err(e) => error!("Failed to pause child actor: {}", e),
                            }
                        } else {
                            error!("Failed to receive response for pause operation");
                        }
                    });
                }
            }
        }

        // notify any actor subscribers
        if let Some(subscribers) = self.subscriptions.get(&actor_id) {
            for subscriber in subscribers {
                if let Err(e) = subscriber.send(Err(error.clone())).await {
                    error!("Failed to send error event to subscriber: {}", e);
                }
            }
        }

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
    /// 2. Signal the actor to shut down gracefully
    /// 3. Wait for a grace period to allow cleanup
    /// 4. Force abort the actor's task if still running
    /// 5. Remove the actor from the runtime's registries
    /// 6. Clean up any channel registrations
    async fn stop_actor(&mut self, actor_id: TheaterId) -> Result<()> {
        debug!("Stopping actor: {:?}", actor_id);

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
            Box::pin(self.stop_actor(child_id.clone())).await?;
            debug!("Successfully stopped child {:?}", child_id);
        }

        // Signal this specific actor to shutdown - we need to get the actor again since
        // we may have changed the actors map when stopping children
        if let Some(proc) = self.actors.get(&actor_id) {
            debug!("Sending shutdown signal to actor {:?}", actor_id);
            proc.shutdown_controller.signal_shutdown();
            debug!(
                "Shutdown signal sent to actor {:?}, waiting for grace period",
                actor_id
            );

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            debug!("Grace period for actor {:?} complete", actor_id);
        } else {
            debug!(
                "Actor {:?} no longer exists after stopping children",
                actor_id
            );
            return Ok(());
        }

        // Force abort if still running
        if let Some(proc) = self.actors.get(&actor_id) {
            debug!(
                "Force aborting actor {:?} task after grace period",
                actor_id
            );
            proc.process.abort();
            debug!("Actor {:?} task aborted", actor_id);
        }

        // Remove from actors map
        if let Some(mut removed_proc) = self.actors.remove(&actor_id) {
            removed_proc.status = ActorStatus::Stopped;
            debug!("Actor {:?} stopped and removed from runtime", actor_id);
        }

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

    async fn update_actor_component(
        &mut self,
        actor_id: TheaterId,
        component: String,
    ) -> Result<()> {
        debug!("Updating actor component for: {:?}", actor_id);

        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to update the actor's component
            let (tx, rx): (
                oneshot::Sender<Result<(), ActorError>>,
                oneshot::Receiver<Result<(), ActorError>>,
            ) = oneshot::channel();
            proc.operation_tx
                .send(ActorOperation::UpdateComponent {
                    component_address: component,
                    response_tx: tx,
                })
                .await?;

            match rx.await {
                Ok(result) => result?,
                Err(e) => return Err(anyhow::anyhow!("Failed to receive update result: {}", e)),
            }
        } else {
            return Err(anyhow::anyhow!("Actor not found"));
        }

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
            proc.operation_tx
                .send(ActorOperation::GetState { response_tx: tx })
                .await?;

            match rx.await {
                Ok(state) => Ok(state?),
                Err(e) => Err(anyhow::anyhow!("Failed to receive state: {}", e)),
            }
        } else {
            Err(anyhow::anyhow!("Actor not found"))
        }
    }

    async fn get_actor_events(&self, actor_id: TheaterId) -> Result<Vec<ChainEvent>> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's events
            let (tx, rx): (
                oneshot::Sender<Result<Vec<ChainEvent>, ActorError>>,
                oneshot::Receiver<Result<Vec<ChainEvent>, ActorError>>,
            ) = oneshot::channel();
            proc.operation_tx
                .send(ActorOperation::GetChain { response_tx: tx })
                .await?;

            match rx.await {
                Ok(events) => Ok(events?),
                Err(e) => Err(anyhow::anyhow!("Failed to receive events: {}", e)),
            }
        } else {
            Err(anyhow::anyhow!("Actor not found"))
        }
    }

    async fn get_actor_metrics(&self, actor_id: TheaterId) -> Result<ActorMetrics> {
        if let Some(proc) = self.actors.get(&actor_id) {
            // Send a message to get the actor's metrics
            let (tx, rx): (
                oneshot::Sender<Result<ActorMetrics, ActorError>>,
                oneshot::Receiver<Result<ActorMetrics, ActorError>>,
            ) = oneshot::channel();
            proc.operation_tx
                .send(ActorOperation::GetMetrics { response_tx: tx })
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
            if let Err(e) = self.stop_actor(actor_id).await {
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
