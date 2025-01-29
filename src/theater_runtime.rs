use crate::actor_runtime::ActorRuntime;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
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
}

pub struct ActorProcess {
    pub actor_id: TheaterId,
    pub process: JoinHandle<ActorRuntime>,
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    pub children: HashSet<TheaterId>,
}

impl TheaterRuntime {
    pub async fn new() -> Result<Self> {
        let (theater_tx, theater_rx) = mpsc::channel(32);
        Ok(Self {
            theater_tx,
            theater_rx,
            actors: HashMap::new(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("Theater runtime starting");

        while let Some(cmd) = self.theater_rx.recv().await {
            info!("Received command: {:?}", cmd);
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
                    // Handle new events - this will be expanded later for supervisor logic
                    info!("Received new event from actor {:?}", actor_id);
                    self.handle_actor_event(actor_id, event)
                        .await
                        .expect("Failed to handle event");
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
        if let Some(proc) = self.actors.remove(&actor_id) {
            proc.process.abort();
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
        if let Some(proc) = self.actors.remove(&actor_id) {
            proc.process.abort();
        }
        Ok(())
    }
}

