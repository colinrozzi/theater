use crate::actor_executor::{ActorError, ActorOperation};
use crate::actor_runtime::ActorRuntime;
use crate::chain::ChainEvent;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, ActorSend, ActorStatus, TheaterCommand};
use crate::metrics::ActorMetrics;
use crate::utils::resolve_reference;
use crate::wasm::Event;
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
    event_subscribers: Vec<mpsc::Sender<(TheaterId, ChainEvent)>>,
}

pub struct ActorProcess {
    pub actor_id: TheaterId,
    pub process: JoinHandle<ActorRuntime>,
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    pub operation_tx: mpsc::Sender<ActorOperation>,
    pub children: HashSet<TheaterId>,
    pub status: ActorStatus,
    pub manifest_path: String,
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
            event_subscribers: Vec::new(),
        })
    }

    pub async fn subscribe_to_events(&mut self) -> Result<mpsc::Receiver<(TheaterId, ChainEvent)>> {
        let (tx, rx) = mpsc::channel(32);
        self.event_subscribers.push(tx);
        Ok(rx)
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
                    // Forward event to subscribers
                    self.event_subscribers.retain_mut(|tx| {
                        match tx.try_send((actor_id.clone(), event.clone())) {
                            Ok(_) => true,
                            Err(e) => {
                                warn!("Failed to forward event to subscriber: {}", e);
                                false
                            }
                        }
                    });

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
                        error!("Failed tk send actor status: {:?}", e);
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
        let (mailbox_tx, mailbox_rx) = mpsc::channel(100);
        let (operation_tx, operation_rx) = mpsc::channel(100);
        let theater_tx = self.theater_tx.clone();

        let actor_operation_tx = operation_tx.clone();
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
        debug!("Handling event from actor {:?}", actor_id);
        // Find the parent of this actor
        let parent_id = self.actors.iter().find_map(|(id, proc)| {
            if proc.children.contains(&actor_id) {
                Some(id.clone())
            } else {
                None
            }
        });

        let wasm_event = Event {
            event_type: event.event_type.clone(),
            parent: None,
            data: event.data.clone(),
        };

        // If there's a parent, forward the event
        if let Some(parent_id) = parent_id {
            debug!("Forwarding event to parent actor {:?}", parent_id);
            if let Some(parent) = self.actors.get(&parent_id) {
                let event_message = ActorMessage::Send(ActorSend {
                    data: serde_json::to_vec(&wasm_event)?,
                });
                if let Err(e) = parent.mailbox_tx.send(event_message).await {
                    error!("Failed to forward event to parent: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn stop_actor(&mut self, actor_id: TheaterId) -> Result<()> {
        debug!("Stopping actor: {:?}", actor_id);
        if let Some(mut proc) = self.actors.remove(&actor_id) {
            proc.process.abort();
            proc.status = ActorStatus::Stopped;
            debug!("Actor stopped and removed from runtime");
            let children = proc.children.clone();
            for child_id in children {
                Box::pin(self.stop_actor(child_id)).await?;
            }
        } else {
            warn!("Attempted to stop non-existent actor: {:?}", actor_id);
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

    pub async fn stop(&mut self) -> Result<()> {
        info!("Initiating theater runtime shutdown");

        // Stop all actors
        for actor_id in self.actors.keys().cloned().collect::<Vec<_>>() {
            self.stop_actor(actor_id).await?;
        }

        info!("Theater runtime shutdown complete");
        Ok(())
    }
}
