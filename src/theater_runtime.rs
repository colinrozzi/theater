use crate::actor_runtime::ActorRuntime;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, ActorStatus, TheaterCommand};
use crate::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use wasmtime::chain::MetaEvent;

pub struct TheaterRuntime {
    actors: HashMap<TheaterId, ActorProcess>,
    pub theater_tx: Sender<TheaterCommand>,
    theater_rx: Receiver<TheaterCommand>,
    event_subscribers: Vec<mpsc::Sender<(TheaterId, Vec<MetaEvent>)>>,
}

pub struct ActorProcess {
    pub actor_id: TheaterId,
    pub process: JoinHandle<ActorRuntime>,
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    pub children: HashSet<TheaterId>,
    pub status: ActorStatus,
}

impl TheaterRuntime {
    pub async fn new() -> Result<Self> {
        let (theater_tx, theater_rx) = mpsc::channel(32);
        Ok(Self {
            theater_tx,
            theater_rx,
            actors: HashMap::new(),
            event_subscribers: Vec::new(),
        })
    }

    pub async fn subscribe_to_events(&mut self) -> Result<mpsc::Receiver<(TheaterId, Vec<MetaEvent>)>> {
        let (tx, rx) = mpsc::channel(32);
        self.event_subscribers.push(tx);
        Ok(rx)
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("Theater runtime starting");

        while let Some(cmd) = self.theater_rx.recv().await {
            info!("Received command: {:?}", cmd.to_log());
            match cmd {
                TheaterCommand::SpawnActor {
                    manifest_path,
                    parent_id,
                    response_tx,
                } => {
                    let actor_id = self
                        .spawn_actor(manifest_path, parent_id)
                        .await
                        .expect("Failed to spawn actor");
                    let _ = response_tx.send(Ok(actor_id.clone()));
                    info!("Actor spawned with id: {:?}", actor_id);
                }
                TheaterCommand::StopActor {
                    actor_id,
                    response_tx,
                } => {
                    self.stop_actor(actor_id)
                        .await
                        .expect("Failed to stop actor");
                    let _ = response_tx.send(Ok(()));
                }
                TheaterCommand::SendMessage {
                    actor_id,
                    actor_message,
                } => {
                    if let Some(proc) = self.actors.get_mut(&actor_id) {
                        proc.mailbox_tx
                            .send(actor_message)
                            .await
                            .expect("Failed to send message");
                    }
                }
                TheaterCommand::NewEvent { actor_id, event } => {
                    info!("Received new event from actor {:?}", actor_id);
                    // Forward event to subscribers
                    self.event_subscribers.retain_mut(|tx| {
                        tx.try_send((actor_id.clone(), event.clone())).is_ok()
                    });
                    
                    self.handle_actor_event(actor_id, event)
                        .await
                        .expect("Failed to handle event");
                }
                TheaterCommand::GetActors { response_tx } => {
                    let actors = self.actors.keys().cloned().collect();
                    let _ = response_tx.send(Ok(actors));
                }
                TheaterCommand::GetActorStatus { actor_id, response_tx } => {
                    let status = self.actors.get(&actor_id)
                        .map(|proc| proc.status.clone())
                        .unwrap_or(ActorStatus::Stopped);
                    let _ = response_tx.send(Ok(status));
                }
            };
        }
        info!("Theater runtime shutting down");
        Ok(())
    }

    async fn spawn_actor(
        &mut self,
        manifest_path: PathBuf,
        parent_id: Option<TheaterId>,
    ) -> Result<TheaterId> {
        // start the actor in a new process
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let (mailbox_tx, mailbox_rx) = mpsc::channel(100);
        let theater_tx = self.theater_tx.clone();
        let actor_runtime_process = tokio::spawn(async move {
            let components = ActorRuntime::from_file(manifest_path, theater_tx, mailbox_rx)
                .await
                .unwrap();
            let actor_id = components.id.clone();
            response_tx.send(actor_id).unwrap();
            ActorRuntime::start(components).await.unwrap()
        });
        let actor_id = response_rx.await.unwrap();
        let process = ActorProcess {
            actor_id: actor_id.clone(),
            process: actor_runtime_process,
            mailbox_tx,
            children: HashSet::new(),
            status: ActorStatus::Running,
        };

        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.actors.get_mut(&parent_id) {
                parent.children.insert(actor_id.clone());
            } else {
                warn!(
                    "Parent actor {:?} not found for new actor {:?}",
                    parent_id, actor_id
                );
            }
        }

        self.actors.insert(actor_id.clone(), process);
        Ok(actor_id)
    }

    fn stop_actor_tree(&mut self, actor_id: TheaterId) -> Result<()> {
        // Get the children before removing the actor
        let children = if let Some(proc) = self.actors.get(&actor_id) {
            proc.children.clone()
        } else {
            HashSet::new()
        };

        // Recursively stop all children first
        for child_id in children {
            self.stop_actor_tree(child_id.clone())
                .expect(format!("Failed to stop child actor {:?}", child_id).as_str());
        }

        // Finally stop this actor
        if let Some(mut proc) = self.actors.remove(&actor_id) {
            proc.process.abort();
            proc.status = ActorStatus::Stopped;
        }

        Ok(())
    }

    async fn handle_actor_event(
        &mut self,
        actor_id: TheaterId,
        event: Vec<MetaEvent>,
    ) -> Result<()> {
        // Find the parent of this actor
        let parent_id = self.actors.iter().find_map(|(id, proc)| {
            if proc.children.contains(&actor_id) {
                Some(id.clone())
            } else {
                None
            }
        });

        // If there's a parent, forward the event
        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.actors.get(&parent_id) {
                let event_message = ActorMessage {
                    data: serde_json::to_vec(&event)?,
                    response_tx: tokio::sync::oneshot::channel().0, // We don't need the response
                };
                parent.mailbox_tx.send(event_message).await?;
            }
        }

        Ok(())
    }

    // Helper method to get an actor's parent
    pub fn get_parent(&self, actor_id: &TheaterId) -> Option<TheaterId> {
        self.actors.iter().find_map(|(id, proc)| {
            if proc.children.contains(actor_id) {
                Some(id.clone())
            } else {
                None
            }
        })
    }

    async fn stop_actor(&mut self, actor_id: TheaterId) -> Result<()> {
        if let Some(mut proc) = self.actors.remove(&actor_id) {
            proc.process.abort();
            proc.status = ActorStatus::Stopped;
        }
        Ok(())
    }
}