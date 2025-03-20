use crate::actor_executor::{ActorError, ActorOperation};
use crate::actor_runtime::ActorRuntime;
use crate::chain::ChainEvent;
use crate::id::TheaterId;
use crate::messages::{ActorChannelClose, ActorChannelMessage, ActorChannelOpen, ChannelId};
use crate::messages::{ActorMessage, ActorStatus, TheaterCommand};
use crate::metrics::ActorMetrics;
use crate::shutdown::{ShutdownController, DEFAULT_SHUTDOWN_TIMEOUT};
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

pub struct TheaterRuntime {
    actors: HashMap<TheaterId, ActorProcess>,
    pub theater_tx: Sender<TheaterCommand>,
    theater_rx: Receiver<TheaterCommand>,
    subscriptions: HashMap<TheaterId, Vec<Sender<ChainEvent>>>,
    channels: HashMap<ChannelId, HashSet<TheaterId>>,
}

pub struct ActorProcess {
    pub actor_id: TheaterId,
    pub process: JoinHandle<ActorRuntime>,
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    pub operation_tx: mpsc::Sender<ActorOperation>,
    pub children: HashSet<TheaterId>,
    pub status: ActorStatus,
    pub manifest_path: String,
    pub shutdown_controller: ShutdownController,
}

impl TheaterRuntime {
    pub async fn new(
        theater_tx: Sender<TheaterCommand>,
        theater_rx: Receiver<TheaterCommand>,
    ) -> Result<Self> {
        Ok(Self {
            theater_tx,
            theater_rx,
            actors: HashMap::new(),
            subscriptions: HashMap::new(),
            channels: HashMap::new(),
        })
    }

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
                } => {
                    debug!(
                        "Processing SpawnActor command for manifest: {:?}",
                        manifest_path
                    );
                    match self
                        .spawn_actor(manifest_path.clone(), init_bytes, parent_id, true)
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
                } => {
                    debug!(
                        "Processing ResumeActor command for manifest: {:?}",
                        manifest_path
                    );
                    match self
                        .spawn_actor(manifest_path.clone(), state_bytes, parent_id, false)
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
                TheaterCommand::GetActors { response_tx } => {
                    debug!("Getting list of actors");
                    let actors = self.actors.keys().cloned().collect();
                    if let Err(e) = response_tx.send(Ok(actors)) {
                        error!("Failed to send actor list: {:?}", e);
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
                    if let Some(proc) = self.actors.get_mut(&target_id) {
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
                            let _ = response_tx
                                .send(Err(anyhow::anyhow!("Failed to send channel open message")));
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
                                        let register_cmd = TheaterCommand::RegisterChannel {
                                            channel_id: channel_id_clone.clone(),
                                            participants: vec![
                                                initiator_id_clone.clone(),
                                                target_id_clone.clone(),
                                            ],
                                        };

                                        if let Err(e) = theater_tx_clone.send(register_cmd).await {
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
                                    error!("Failed to receive channel open response: {}", e);
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
                        let _ = response_tx.send(Err(anyhow::anyhow!("Target actor not found")));
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

                        for actor_id in participant_ids {
                            debug!("Delivering message to actor {:?}", actor_id);
                            if let Some(proc) = self.actors.get_mut(actor_id) {
                                if proc.actor_id == sender_id {
                                    // Skip sending the message back to the sender
                                    continue;
                                }

                                let actor_message =
                                    ActorMessage::ChannelMessage(ActorChannelMessage {
                                        channel_id: channel_id.clone(),
                                        data: message.clone(),
                                    });

                                match proc.mailbox_tx.send(actor_message).await {
                                    Ok(_) => {
                                        successful_delivery = true;
                                        debug!("Delivered channel message to actor {:?}", actor_id);
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
                    for actor_id in &participant_ids {
                        if let Some(proc) = self.actors.get_mut(&actor_id) {
                            let actor_message = ActorMessage::ChannelClose(ActorChannelClose {
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
                    let participant_set: HashSet<TheaterId> = participants.into_iter().collect();

                    // Register the channel with its participants
                    self.channels.insert(channel_id.clone(), participant_set);

                    debug!("Successfully registered channel {:?}", channel_id);
                }
            };
        }
        info!("Theater runtime shutting down");
        Ok(())
    }

    async fn spawn_actor(
        &mut self,
        manifest_path: String,
        init_bytes: Option<Vec<u8>>,
        parent_id: Option<TheaterId>,
        init: bool,
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
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        // Create a shutdown controller for this specific actor
        let (shutdown_controller, shutdown_receiver) = ShutdownController::new();
        let (mailbox_tx, mailbox_rx) = mpsc::channel(100);
        let (operation_tx, operation_rx) = mpsc::channel(100);
        let theater_tx = self.theater_tx.clone();

        let actor_operation_tx = operation_tx.clone();
        let shutdown_receiver_clone = shutdown_receiver;
        let actor_runtime_process = tokio::spawn(async move {
            let actor_id = TheaterId::generate();
            debug!("Initializing actor runtime");
            debug!("Starting actor runtime");
            response_tx.send(actor_id.clone()).unwrap();
            ActorRuntime::start(
                actor_id,
                &manifest,
                init_bytes,
                theater_tx,
                mailbox_rx,
                operation_rx,
                actor_operation_tx,
                init,
                shutdown_receiver_clone,
            )
            .await
            .unwrap()
        });

        match response_rx.await {
            Ok(actor_id) => {
                debug!(
                    "Received actor ID from runtime initialization: {:?}",
                    actor_id
                );
                let process = ActorProcess {
                    actor_id: actor_id.clone(),
                    process: actor_runtime_process,
                    mailbox_tx,
                    operation_tx,
                    children: HashSet::new(),
                    status: ActorStatus::Running,
                    manifest_path: manifest_path.clone(),
                    shutdown_controller,
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
            Err(e) => {
                error!("Failed to receive actor ID: {}", e);
                Err(anyhow::anyhow!("Failed to receive actor ID"))
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
                if let Err(e) = subscriber.send(event.clone()).await {
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
            warn!("No subscribers found for actor: {:?}", actor_id);
            false
        };

        // Remove the entry if needed
        if should_remove {
            self.subscriptions.remove(&actor_id);
            debug!("Removed empty subscription entry for actor {:?}", actor_id);
        }

        Ok(())
    }

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

            // Allow more time for graceful shutdown to ensure proper resource cleanup
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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
        for (channel_id, participants) in self.channels.iter_mut() {
            if participants.remove(&actor_id) {
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

    async fn restart_actor(&mut self, actor_id: TheaterId) -> Result<()> {
        debug!("Starting actor restart process for: {:?}", actor_id);

        // Get the actor's info before stopping it
        let (manifest_path, parent_id) = if let Some(proc) = self.actors.get(&actor_id) {
            let manifest = proc.manifest_path.clone();

            // Find the parent ID
            let parent_id = self.actors.iter().find_map(|(id, proc)| {
                if proc.children.contains(&actor_id) {
                    Some(id.clone())
                } else {
                    None
                }
            });

            (manifest, parent_id)
        } else {
            return Err(anyhow::anyhow!("Actor not found"));
        };

        // Get the actor's state
        let state_bytes = self
            .get_actor_state(actor_id.clone())
            .await
            .expect("Failed to get actor state");

        // Stop the actor
        self.stop_actor(actor_id).await?;

        // THIS IS WRONG!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        // we need to rethink how the restart works. is this even the place to have it? How should
        // we handle the state? I don't know.

        // Spawn it again
        self.spawn_actor(manifest_path, state_bytes, parent_id, false)
            .await?;

        Ok(())
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
    pub async fn get_channel_status(&self, channel_id: &ChannelId) -> Option<Vec<TheaterId>> {
        self.channels
            .get(channel_id)
            .map(|participants| participants.iter().cloned().collect())
    }

    // Get a list of all channels and their participants
    pub async fn list_channels(&self) -> Vec<(ChannelId, Vec<TheaterId>)> {
        self.channels
            .iter()
            .map(|(id, participants)| (id.clone(), participants.iter().cloned().collect()))
            .collect()
    }

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
