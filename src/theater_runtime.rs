use crate::actor_runtime::ActorRuntime;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tracing::info;

pub struct TheaterRuntime {
    actors: HashMap<String, ActorProcess>,
    pub theater_tx: Sender<TheaterCommand>,
    theater_rx: Receiver<TheaterCommand>,
}

pub struct ActorProcess {
    pub actor_id: String,
    pub process: JoinHandle<ActorRuntime>,
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
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
                    response_tx,
                } => {
                    let actor_id = self.spawn_actor(manifest_path).await?;
                    let _ = response_tx.send(Ok(actor_id.clone()));
                    info!("Actor spawned with id: {:?}", actor_id);
                }
                TheaterCommand::StopActor {
                    actor_id,
                    response_tx,
                } => {
                    self.stop_actor(actor_id).await.unwrap();
                    let _ = response_tx.send(Ok(()));
                }
                TheaterCommand::SendMessage {
                    actor_id,
                    actor_message,
                } => {
                    if let Some(proc) = self.actors.get_mut(&actor_id) {
                        proc.mailbox_tx.send(actor_message).await.unwrap();
                    }
                }
            };
        }
        info!("Theater runtime shutting down");
        Ok(())
    }

    async fn spawn_actor(&mut self, manifest_path: PathBuf) -> Result<String> {
        // start the actor in a new process
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let (mailbox_tx, mailbox_rx) = mpsc::channel(100);
        let theater_tx = self.theater_tx.clone();
        let actor_runtime_process = tokio::spawn(async move {
            let components = ActorRuntime::from_file(manifest_path, theater_tx, mailbox_rx)
                .await
                .unwrap();
            let actor_id = components.name.clone();
            response_tx.send(actor_id.clone()).unwrap();
            ActorRuntime::start(components).await.unwrap()
        });
        let actor_id = response_rx.await.unwrap();
        self.actors.insert(
            actor_id.clone(),
            ActorProcess {
                actor_id: actor_id.clone(),
                process: actor_runtime_process,
                mailbox_tx,
            },
        );
        Ok(actor_id)
    }

    async fn stop_actor(&mut self, actor_id: String) -> Result<()> {
        if let Some(proc) = self.actors.remove(&actor_id) {
            proc.process.abort();
        }
        Ok(())
    }
}
