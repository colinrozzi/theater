use anyhow::Result;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{error, info};
use uuid::Uuid;

use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::theater_runtime::TheaterRuntime;
use wasmtime::chain::MetaEvent;

#[derive(Debug, Serialize, Deserialize)]
pub enum ManagementCommand {
    StartActor { manifest: PathBuf },
    StopActor { id: TheaterId },
    ListActors,
    SubscribeToActor { id: TheaterId },
    UnsubscribeFromActor { id: TheaterId },
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
    },
    Unsubscribed {
        id: TheaterId,
    },
    ActorEvent {
        id: TheaterId,
        event: Vec<MetaEvent>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Eq, PartialEq, Hash)]
struct Subscription {
    id: Uuid,
    sender: mpsc::Sender<ManagementResponse>,
}

pub struct TheaterServer {
    runtime: TheaterRuntime,
    management_socket: TcpListener,
    subscriptions: Arc<Mutex<HashMap<TheaterId, HashSet<Subscription>>>>,
}

impl TheaterServer {
    pub async fn new(address: std::net::SocketAddr) -> Result<Self> {
        let runtime = TheaterRuntime::new().await?;
        let management_socket = TcpListener::bind(address).await?;

        Ok(Self {
            runtime,
            management_socket,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        info!(
            "Theater server starting on {:?}",
            self.management_socket.local_addr()?
        );

        // Start the theater runtime in its own task
        let runtime_handle = {
            let mut runtime = std::mem::replace(&mut self.runtime, TheaterRuntime::new().await?);
            tokio::spawn(async move { runtime.run().await })
        };

        // Start the subscription event forwarder
        let subscriptions = self.subscriptions.clone();
        let mut event_rx = self.runtime.subscribe_to_events().await?;
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
            let runtime_tx = self.runtime.theater_tx.clone();
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
        let (response_tx, mut response_rx) = mpsc::channel(32);

        // Spawn task to forward responses back to client
        let mut framed_tx = framed.sink_map_err(|e| anyhow::anyhow!("Frame error: {}", e));
        tokio::spawn(async move {
            while let Some(response) = response_rx.recv().await {
                if let Err(e) = framed_tx.send(serde_json::to_vec(&response)?).await {
                    error!("Failed to send response to client: {}", e);
                    break;
                }
            }
            Ok::<_, anyhow::Error>(())
        });

        while let Some(msg) = framed.next().await {
            let msg = msg?;
            let cmd: ManagementCommand = serde_json::from_slice(&msg)?;

            match cmd {
                ManagementCommand::StartActor { manifest } => {
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::SpawnActor {
                            manifest_path: manifest,
                            response_tx: cmd_tx,
                            parent_id: None,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(actor_id) => {
                            response_tx
                                .send(ManagementResponse::ActorStarted { id: actor_id })
                                .await?
                        }
                        Err(e) => {
                            response_tx
                                .send(ManagementResponse::Error {
                                    message: format!("Failed to start actor: {}", e),
                                })
                                .await?
                        }
                    }
                }
                ManagementCommand::StopActor { id } => {
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::StopActor {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(_) => {
                            // Remove any subscriptions for this actor
                            subscriptions.lock().await.remove(&id);
                            response_tx
                                .send(ManagementResponse::ActorStopped { id })
                                .await?
                        }
                        Err(e) => {
                            response_tx
                                .send(ManagementResponse::Error {
                                    message: format!("Failed to stop actor: {}", e),
                                })
                                .await?
                        }
                    }
                }
                ManagementCommand::ListActors => {
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActors {
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(actors) => {
                            response_tx
                                .send(ManagementResponse::ActorList { actors })
                                .await?
                        }
                        Err(e) => {
                            response_tx
                                .send(ManagementResponse::Error {
                                    message: format!("Failed to list actors: {}", e),
                                })
                                .await?
                        }
                    }
                }
                ManagementCommand::SubscribeToActor { id } => {
                    let subscription = Subscription {
                        id: Uuid::new_v4(),
                        sender: response_tx.clone(),
                    };

                    subscriptions
                        .lock()
                        .await
                        .entry(id.clone())
                        .or_insert_with(HashSet::new)
                        .insert(subscription);

                    response_tx
                        .send(ManagementResponse::Subscribed { id })
                        .await?;
                }
                ManagementCommand::UnsubscribeFromActor { id } => {
                    subscriptions
                        .lock()
                        .await
                        .entry(id.clone())
                        .and_modify(|subs| {
                            subs.retain(|sub| sub.sender != response_tx);
                        });

                    response_tx
                        .send(ManagementResponse::Unsubscribed { id })
                        .await?;
                }
            }
        }

        // Clean up subscriptions for this connection
        let mut subs = subscriptions.lock().await;
        for subscribers in subs.values_mut() {
            subscribers.retain(|sub| sub.sender != response_tx);
        }

        Ok(())
    }
}

