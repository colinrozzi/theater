use crate::ChainEvent;
use anyhow::Result;
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::theater_runtime::TheaterRuntime;

#[derive(Debug, Serialize, Deserialize)]
pub enum ManagementCommand {
    StartActor {
        manifest: PathBuf,
    },
    StopActor {
        id: TheaterId,
    },
    ListActors,
    SubscribeToActor {
        id: TheaterId,
    },
    UnsubscribeFromActor {
        id: TheaterId,
        subscription_id: Uuid,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ManagementResponse {
    ActorStarted {
        id: TheaterId,
    },
    ActorStopped {
        id: TheaterId,
    },
    ActorList {
        actors: Vec<TheaterId>,
    },
    Subscribed {
        id: TheaterId,
        subscription_id: Uuid,
    },
    Unsubscribed {
        id: TheaterId,
    },
    ActorEvent {
        id: TheaterId,
        event: ChainEvent,
    },
    Error {
        message: String,
    },
}

#[derive(Debug)]
struct Subscription {
    id: Uuid,
    sender: mpsc::Sender<ManagementResponse>,
}

impl Eq for Subscription {}
impl PartialEq for Subscription {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl std::hash::Hash for Subscription {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

pub struct TheaterServer {
    runtime: TheaterRuntime,
    theater_tx: mpsc::Sender<TheaterCommand>,
    management_socket: TcpListener,
    subscriptions: Arc<Mutex<HashMap<TheaterId, HashSet<Subscription>>>>,
}

impl TheaterServer {
    pub async fn new(address: std::net::SocketAddr) -> Result<Self> {
        let (theater_tx, theater_rx) = mpsc::channel(32);
        let runtime = TheaterRuntime::new(theater_tx.clone(), theater_rx).await?;
        let management_socket = TcpListener::bind(address).await?;

        Ok(Self {
            runtime,
            theater_tx,
            management_socket,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!(
            "Theater server starting on {:?}",
            self.management_socket.local_addr()?
        );

        let mut event_rx = self.runtime.subscribe_to_events().await?;

        // Start the theater runtime in its own task
        let runtime_handle = tokio::spawn(async move {
            match self.runtime.run().await {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!("Theater runtime failed: {}", e);
                    Err(e)
                }
            }
        });

        // Start the subscription event forwarder
        let subscriptions = self.subscriptions.clone();
        tokio::spawn(async move {
            while let Some((actor_id, event)) = event_rx.recv().await {
                let subs = subscriptions.lock().await;
                if let Some(subscribers) = subs.get(&actor_id) {
                    for sub in subscribers {
                        let response = ManagementResponse::ActorEvent {
                            id: actor_id.clone(),
                            event: event.clone(),
                        };
                        if let Err(e) = sub.sender.send(response).await {
                            error!("Failed to forward event to subscriber: {}", e);
                        }
                    }
                }
            }
        });

        // Accept and handle management connections
        while let Ok((socket, addr)) = self.management_socket.accept().await {
            info!("New management connection from {}", addr);
            let runtime_tx = self.theater_tx.clone();
            let subscriptions = self.subscriptions.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    Self::handle_management_connection(socket, runtime_tx, subscriptions).await
                {
                    error!("Error handling management connection: {}", e);
                }
            });
        }

        runtime_handle.await??;
        Ok(())
    }

    async fn handle_management_connection(
        socket: TcpStream,
        runtime_tx: mpsc::Sender<TheaterCommand>,
        subscriptions: Arc<Mutex<HashMap<TheaterId, HashSet<Subscription>>>>,
    ) -> Result<()> {
        let mut framed = Framed::new(socket, LengthDelimitedCodec::new());

        while let Some(msg) = framed.next().await {
            debug!("Received management message");
            let msg = msg?;
            let cmd: ManagementCommand = serde_json::from_slice(&msg)?;
            debug!("Parsed command: {:?}", cmd);

            let response = match cmd {
                ManagementCommand::StartActor { manifest } => {
                    info!("Starting actor from manifest: {:?}", manifest);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    debug!("Sending SpawnActor command to runtime");
                    match runtime_tx
                        .send(TheaterCommand::SpawnActor {
                            manifest_path: manifest.clone(),
                            response_tx: cmd_tx,
                            parent_id: None,
                        })
                        .await
                    {
                        Ok(_) => {
                            debug!("SpawnActor command sent to runtime, awaiting response");
                            match cmd_rx.await {
                                Ok(result) => match result {
                                    Ok(actor_id) => {
                                        info!("Actor started with ID: {:?}", actor_id);
                                        ManagementResponse::ActorStarted { id: actor_id }
                                    }
                                    Err(e) => {
                                        error!("Runtime failed to start actor: {}", e);
                                        ManagementResponse::Error {
                                            message: format!("Failed to start actor: {}", e),
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to receive spawn response: {}", e);
                                    ManagementResponse::Error {
                                        message: format!("Failed to receive spawn response: {}", e),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to send SpawnActor command: {}", e);
                            ManagementResponse::Error {
                                message: format!("Failed to send spawn command: {}", e),
                            }
                        }
                    }
                }
                ManagementCommand::StopActor { id } => {
                    info!("Stopping actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::StopActor {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(_) => {
                            subscriptions.lock().await.remove(&id);
                            ManagementResponse::ActorStopped { id }
                        }
                        Err(e) => ManagementResponse::Error {
                            message: format!("Failed to stop actor: {}", e),
                        },
                    }
                }
                ManagementCommand::ListActors => {
                    debug!("Listing actors");
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActors {
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(actors) => {
                            info!("Found {} actors", actors.len());
                            ManagementResponse::ActorList { actors }
                        }
                        Err(e) => ManagementResponse::Error {
                            message: format!("Failed to list actors: {}", e),
                        },
                    }
                }
                ManagementCommand::SubscribeToActor { id } => {
                    info!("New subscription request for actor: {:?}", id);
                    let subscription_id = Uuid::new_v4();
                    let subscription = Subscription {
                        id: subscription_id,
                        sender: mpsc::channel(32).0,
                    };

                    subscriptions
                        .lock()
                        .await
                        .entry(id.clone())
                        .or_insert_with(HashSet::new)
                        .insert(subscription);

                    ManagementResponse::Subscribed {
                        id,
                        subscription_id,
                    }
                }
                ManagementCommand::UnsubscribeFromActor {
                    id,
                    subscription_id,
                } => {
                    subscriptions
                        .lock()
                        .await
                        .entry(id.clone())
                        .and_modify(|subs| {
                            subs.retain(|sub| sub.id != subscription_id);
                        });

                    ManagementResponse::Unsubscribed { id }
                }
            };

            debug!("Sending response: {:?}", response);
            framed
                .send(Bytes::from(serde_json::to_vec(&response)?))
                .await?;
            debug!("Response sent");
        }

        Ok(())
    }
}
