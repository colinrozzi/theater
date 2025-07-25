use anyhow::Result;
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use theater::messages::{
    ActorMessage, ActorRequest, ActorResult, ActorSend, ActorStatus, ChannelEvent,
    ChannelParticipant,
};
use theater::{ActorError, ChainEvent, ManifestConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::Framed;
use tracing::{debug, error, info};
use uuid::Uuid;

use theater::id::TheaterId;
use theater::messages::{ChannelId, TheaterCommand};
use theater::theater_runtime::TheaterRuntime;
use theater::TheaterRuntimeError;

use crate::fragmenting_codec::FragmentingCodec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagementCommand {
    StartActor {
        manifest: String,
        initial_state: Option<Vec<u8>>,
        parent: bool,
        subscribe: bool,
    },
    StopActor {
        id: TheaterId,
    },
    TerminateActor {
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
    SendActorMessage {
        id: TheaterId,
        data: Vec<u8>,
    },
    RequestActorMessage {
        id: TheaterId,
        data: Vec<u8>,
    },
    GetActorManifest {
        id: TheaterId,
    },
    GetActorStatus {
        id: TheaterId,
    },
    RestartActor {
        id: TheaterId,
    },
    GetActorState {
        id: TheaterId,
    },
    GetActorEvents {
        id: TheaterId,
    },
    GetActorMetrics {
        id: TheaterId,
    },
    UpdateActorComponent {
        id: TheaterId,
        component: String,
    },
    // Channel management commands
    OpenChannel {
        actor_id: ChannelParticipant,
        initial_message: Vec<u8>,
    },
    SendOnChannel {
        channel_id: String,
        message: Vec<u8>,
    },
    CloseChannel {
        channel_id: String,
    },

    // Store commands
    NewStore {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagementResponse {
    ActorStarted {
        id: TheaterId,
    },
    ActorStopped {
        id: TheaterId,
    },
    ActorList {
        actors: Vec<(TheaterId, String)>,
    },
    Subscribed {
        id: TheaterId,
        subscription_id: Uuid,
    },
    Unsubscribed {
        id: TheaterId,
    },
    ActorEvent {
        event: ChainEvent,
    },
    ActorResult(ActorResult),
    ActorError {
        error: ActorError,
    },
    Error {
        error: ManagementError,
    },
    RequestedMessage {
        id: TheaterId,
        message: Vec<u8>,
    },
    SentMessage {
        id: TheaterId,
    },
    ActorStatus {
        id: TheaterId,
        status: ActorStatus,
    },
    Restarted {
        id: TheaterId,
    },
    ActorManifest {
        id: TheaterId,
        manifest: ManifestConfig,
    },
    ActorState {
        id: TheaterId,
        state: Option<Vec<u8>>,
    },
    ActorEvents {
        id: TheaterId,
        events: Vec<ChainEvent>,
    },
    ActorMetrics {
        id: TheaterId,
        metrics: serde_json::Value,
    },
    ActorComponentUpdated {
        id: TheaterId,
    },
    // Channel management responses
    ChannelOpened {
        channel_id: String,
        actor_id: ChannelParticipant,
    },
    MessageSent {
        channel_id: String,
    },
    ChannelMessage {
        channel_id: String,
        sender_id: ChannelParticipant,
        message: Vec<u8>,
    },
    ChannelClosed {
        channel_id: String,
    },

    // Store responses
    StoreCreated {
        store_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagementError {
    // Actor-related errors
    ActorNotFound,
    ActorAlreadyExists,
    ActorNotRunning,
    ActorError(String),

    // Channel-related errors
    ChannelNotFound,
    ChannelClosed,
    ChannelRejected,

    // Store-related errors
    StoreError(String),

    // Communication errors
    CommunicationError(String),

    // Request handling errors
    InvalidRequest(String),
    Timeout,

    // System errors
    RuntimeError(String),
    InternalError(String),

    // Serialization/deserialization errors
    SerializationError(String),
}

// Allow converting from TheaterRuntimeError to ManagementError
impl From<TheaterRuntimeError> for ManagementError {
    fn from(err: TheaterRuntimeError) -> Self {
        match err {
            TheaterRuntimeError::ActorNotFound(_) => ManagementError::ActorNotFound,
            TheaterRuntimeError::ActorAlreadyExists(_) => ManagementError::ActorAlreadyExists,
            TheaterRuntimeError::ActorNotRunning(_) => ManagementError::ActorNotRunning,
            TheaterRuntimeError::ActorOperationFailed(msg) => {
                ManagementError::RuntimeError(format!("Actor operation failed: {}", msg))
            }
            TheaterRuntimeError::ActorError(e) => ManagementError::ActorError(e.to_string()),
            TheaterRuntimeError::ChannelError(msg) => ManagementError::CommunicationError(msg),
            TheaterRuntimeError::ChannelNotFound(_) => ManagementError::ChannelNotFound,
            TheaterRuntimeError::ChannelRejected => ManagementError::ChannelRejected,
            TheaterRuntimeError::SerializationError(msg) => {
                ManagementError::SerializationError(msg)
            }
            TheaterRuntimeError::InternalError(msg) => ManagementError::InternalError(msg),
        }
    }
}

// Implement Display for ManagementError to provide better error messages
impl std::fmt::Display for ManagementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManagementError::ActorNotFound => write!(f, "Actor not found"),
            ManagementError::ActorAlreadyExists => write!(f, "Actor already exists"),
            ManagementError::ActorNotRunning => write!(f, "Actor is not running"),
            ManagementError::ActorError(msg) => write!(f, "Actor error: {}", msg),
            ManagementError::ChannelNotFound => write!(f, "Channel not found"),
            ManagementError::ChannelClosed => write!(f, "Channel is closed"),
            ManagementError::ChannelRejected => write!(f, "Channel was rejected"),
            ManagementError::StoreError(msg) => write!(f, "Store error: {}", msg),
            ManagementError::CommunicationError(msg) => write!(f, "Communication error: {}", msg),
            ManagementError::InvalidRequest(msg) => write!(f, "Invalid request: {}", msg),
            ManagementError::Timeout => write!(f, "Operation timed out"),
            ManagementError::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            ManagementError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            ManagementError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

// Implement Error trait for ManagementError
impl std::error::Error for ManagementError {}

#[derive(Debug)]
#[allow(dead_code)]
struct Subscription {
    id: Uuid,
    client_tx: mpsc::Sender<ManagementResponse>,
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

// ChannelEvent is now imported from theater::ChannelEvent

// Structure to track active channel subscriptions
#[derive(Debug)]
#[allow(dead_code)]
struct ChannelSubscription {
    channel_id: String,
    initiator_id: ChannelParticipant,
    target_id: ChannelParticipant,
    client_tx: mpsc::Sender<ManagementResponse>,
}

pub struct TheaterServer {
    runtime: TheaterRuntime,
    theater_tx: mpsc::Sender<TheaterCommand>,
    management_socket: TcpListener,
    subscriptions: Arc<Mutex<HashMap<TheaterId, HashSet<Subscription>>>>,
    // Field to track channel subscriptions
    channel_subscriptions: Arc<Mutex<HashMap<String, ChannelSubscription>>>,
    // Channel for runtime to send channel events back to server
    #[allow(dead_code)]
    channel_events_tx: mpsc::Sender<ChannelEvent>,
}

impl TheaterServer {
    // Process channel events and forward them to subscribed clients
    async fn process_channel_events(
        mut channel_events_rx: mpsc::Receiver<ChannelEvent>,
        channel_subscriptions: Arc<Mutex<HashMap<String, ChannelSubscription>>>,
    ) {
        while let Some(event) = channel_events_rx.recv().await {
            match event {
                ChannelEvent::Message {
                    channel_id,
                    sender_id,
                    message,
                } => {
                    tracing::debug!("Received channel message for {}", channel_id);
                    // Forward to subscribed clients
                    let subs = channel_subscriptions.lock().await;
                    if let Some(sub) = subs.get(&channel_id.0) {
                        let response = ManagementResponse::ChannelMessage {
                            channel_id: channel_id.0.clone(),
                            sender_id,
                            message,
                        };

                        tracing::debug!("Forwarding channel message to client: {:?}", response);

                        if let Err(e) = sub.client_tx.send(response).await {
                            tracing::warn!("Failed to forward channel message: {}", e);
                        } else {
                            tracing::debug!("Forwarded channel message to client");
                        }
                    }
                }
                ChannelEvent::Close { channel_id } => {
                    tracing::debug!("Received channel close event for {}", channel_id);
                    // Forward to subscribed clients
                    let mut subs = channel_subscriptions.lock().await;
                    if let Some(sub) = subs.remove(&channel_id.0) {
                        let response = ManagementResponse::ChannelClosed {
                            channel_id: channel_id.0.clone(),
                        };

                        if let Err(e) = sub.client_tx.send(response).await {
                            tracing::warn!("Failed to forward channel close event: {}", e);
                        } else {
                            tracing::debug!("Forwarded channel close event to client");
                        }
                    }
                }
            }
        }
    }

    pub async fn new(address: std::net::SocketAddr) -> Result<Self> {
        let (theater_tx, theater_rx) = mpsc::channel(32);

        // Create channel for runtime to send channel events back to server
        let (channel_events_tx, channel_events_rx) = mpsc::channel(32);

        // Pass channel_events_tx to runtime during initialization
        let runtime = TheaterRuntime::new(
            theater_tx.clone(),
            theater_rx,
            Some(channel_events_tx.clone()),
            theater::config::permissions::HandlerPermission::root(), // Root permissions for server
        )
        .await?;
        let management_socket = TcpListener::bind(address).await?;

        let channel_subscriptions = Arc::new(Mutex::new(HashMap::new()));

        // Start task to process channel events
        let channel_subs_clone = channel_subscriptions.clone();
        tokio::spawn(async move {
            Self::process_channel_events(channel_events_rx, channel_subs_clone).await;
        });

        Ok(Self {
            runtime,
            theater_tx,
            management_socket,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            channel_subscriptions,
            channel_events_tx,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!(
            "Theater server starting on {:?}",
            self.management_socket.local_addr()?
        );

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

        // Accept and handle management connections
        while let Ok((socket, addr)) = self.management_socket.accept().await {
            info!("New management connection from {}", addr);
            let runtime_tx = self.theater_tx.clone();
            let subscriptions = self.subscriptions.clone();
            let channel_subscriptions = self.channel_subscriptions.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_management_connection(
                    socket,
                    runtime_tx,
                    subscriptions,
                    channel_subscriptions,
                )
                .await
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
        channel_subscriptions: Arc<Mutex<HashMap<String, ChannelSubscription>>>,
    ) -> Result<()> {
        // Create a channel for sending responses to this client
        let (client_tx, mut client_rx) = mpsc::channel::<ManagementResponse>(32);

        let codec = FragmentingCodec::new();
        let framed = Framed::new(socket, codec);

        // Split the framed connection into read and write parts
        let (mut framed_sink, mut framed_stream) = framed.split();

        // Clone the client_tx for use in the command loop
        let cmd_client_tx = client_tx.clone();

        // Start a task to forward responses to the client
        let _response_task = tokio::spawn(async move {
            while let Some(response) = client_rx.recv().await {
                match serde_json::to_vec(&response) {
                    Ok(data) => {
                        debug!("Serialized response: {} bytes", data.len());
                        if data.len() > 10 * 1024 * 1024 {
                            debug!("Large response detected: {} MB", data.len() / 1024 / 1024);
                        }
                        if let Err(e) = framed_sink.send(Bytes::from(data)).await {
                            debug!("Error sending response to client: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error serializing response: {}", e);
                    }
                }
            }
            debug!("Response forwarder for client closed");
        });

        // Store active subscriptions for this connection to clean up on disconnect
        let mut connection_subscriptions: Vec<(TheaterId, Uuid)> = Vec::new();

        // Store active channel subscriptions for cleanup
        let mut connection_channel_subscriptions: Vec<String> = Vec::new();

        // Loop until connection closes or an error occurs
        'connection: while let Some(msg) = framed_stream.next().await {
            debug!("Received management message");
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    error!("Error receiving message: {}", e);
                    break 'connection;
                }
            };

            let cmd = match serde_json::from_slice::<ManagementCommand>(&msg) {
                Ok(c) => c,
                Err(e) => {
                    error!(
                        "Error parsing command: {} {}",
                        e,
                        String::from_utf8_lossy(&msg)
                    );
                    continue;
                }
            };
            debug!("Parsed command: {:?}", cmd);

            // Store the command for reference (used for subscription tracking)
            let _cmd_clone = cmd.clone();

            let response = match cmd {
                ManagementCommand::StartActor {
                    manifest,
                    initial_state,
                    parent,
                    subscribe,
                } => {
                    info!("Starting actor from manifest: {:?}", manifest);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    debug!("Sending SpawnActor command to runtime");
                    let supervisor_tx = if parent {
                        let (supervisor_tx, mut supervisor_rx) = mpsc::channel(32);
                        let cmd_client_tx = cmd_client_tx.clone();
                        tokio::spawn(async move {
                            while let Some(res) = supervisor_rx.recv().await {
                                debug!("Received supervisor response: {:?}", res);
                                if let Err(e) = cmd_client_tx
                                    .send(ManagementResponse::ActorResult(res))
                                    .await
                                {
                                    error!("Failed to send supervisor response: {}", e);
                                    break;
                                }
                            }
                        });
                        Some(supervisor_tx)
                    } else {
                        None
                    };
                    let subscription_tx = if subscribe {
                        let (event_tx, mut event_rx) = mpsc::channel(32);

                        // set up a task to forward events to the client
                        let cmd_client_tx = cmd_client_tx.clone();
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                debug!("Received event for subscription");
                                let response = match event {
                                    Ok(event) => ManagementResponse::ActorEvent { event },
                                    Err(e) => ManagementResponse::ActorError { error: e },
                                };
                                if let Err(e) = cmd_client_tx.send(response).await {
                                    debug!("Failed to forward event to client: {}", e);
                                    break;
                                }
                            }
                            debug!("Event forwarder for subscription stopped");
                        });

                        Some(event_tx)
                    } else {
                        None
                    };
                    match runtime_tx
                        .send(TheaterCommand::SpawnActor {
                            manifest_path: manifest.clone(),
                            init_bytes: initial_state,
                            response_tx: cmd_tx,
                            parent_id: None,
                            supervisor_tx,
                            subscription_tx,
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
                                            error: ManagementError::RuntimeError(format!(
                                                "Failed to start actor: {}",
                                                e
                                            )),
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to receive spawn response: {}", e);
                                    ManagementResponse::Error {
                                        error: ManagementError::CommunicationError(format!(
                                            "Failed to receive spawn response: {}",
                                            e
                                        )),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to send SpawnActor command: {}", e);
                            ManagementResponse::Error {
                                error: ManagementError::CommunicationError(format!(
                                    "Failed to send spawn command: {}",
                                    e
                                )),
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
                            error: ManagementError::RuntimeError(format!(
                                "Failed to stop actor: {}",
                                e
                            )),
                        },
                    }
                }
                ManagementCommand::TerminateActor { id } => {
                    info!("Terminating actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::TerminateActor {
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
                            error: ManagementError::RuntimeError(format!(
                                "Failed to terminate actor: {}",
                                e
                            )),
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
                            error: ManagementError::RuntimeError(format!(
                                "Failed to list actors: {}",
                                e
                            )),
                        },
                    }
                }
                ManagementCommand::SubscribeToActor { id } => {
                    info!("New subscription request for actor: {:?}", id);
                    let subscription_id = Uuid::new_v4();
                    let subscription = Subscription {
                        id: subscription_id,
                        client_tx: cmd_client_tx.clone(),
                    };

                    debug!("Subscription created with ID: {}", subscription_id);

                    // Register the subscription in the global map
                    subscriptions
                        .lock()
                        .await
                        .entry(id.clone())
                        .or_default()
                        .insert(subscription);

                    // Set up the event channel for the subscription
                    let (event_tx, mut event_rx) = mpsc::channel(32);
                    runtime_tx
                        .send(TheaterCommand::SubscribeToActor {
                            actor_id: id.clone(),
                            event_tx,
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to subscribe: {}", e))?;

                    // Add to the list of subscriptions for this connection
                    connection_subscriptions.push((id.clone(), subscription_id));

                    // Create a task to forward events to this client
                    let client_tx_clone = cmd_client_tx.clone();
                    tokio::spawn(async move {
                        debug!(
                            "Starting event forwarder for subscription {}",
                            subscription_id
                        );
                        while let Some(event) = event_rx.recv().await {
                            debug!("Received event for subscription {}", subscription_id);
                            let response = match event {
                                Ok(event) => ManagementResponse::ActorEvent { event },
                                Err(e) => ManagementResponse::ActorError { error: e },
                            };
                            if let Err(e) = client_tx_clone.send(response).await {
                                debug!("Failed to forward event to client: {}", e);
                                break;
                            }
                        }
                        debug!(
                            "Event forwarder for subscription {} stopped",
                            subscription_id
                        );
                    });

                    ManagementResponse::Subscribed {
                        id,
                        subscription_id,
                    }
                }
                ManagementCommand::UnsubscribeFromActor {
                    id,
                    subscription_id,
                } => {
                    debug!(
                        "Removing subscription {} for actor {:?}",
                        subscription_id, id
                    );

                    // Remove subscription from the tracking list for this connection
                    connection_subscriptions
                        .retain(|(aid, sid)| *aid != id || *sid != subscription_id);

                    // Remove from the global subscriptions map
                    let mut subs = subscriptions.lock().await;
                    if let Some(actor_subs) = subs.get_mut(&id) {
                        actor_subs.retain(|sub| sub.id != subscription_id);

                        // Remove the entry if no subscriptions remain
                        if actor_subs.is_empty() {
                            subs.remove(&id);
                        }
                    }

                    debug!("Subscription removed");
                    ManagementResponse::Unsubscribed { id }
                }
                ManagementCommand::SendActorMessage { id, data } => {
                    info!("Sending message to actor: {:?}", id);
                    runtime_tx
                        .send(TheaterCommand::SendMessage {
                            actor_id: id.clone(),
                            actor_message: ActorMessage::Send(ActorSend { data: data.clone() }),
                        })
                        .await?;

                    ManagementResponse::SentMessage { id }
                }
                ManagementCommand::RequestActorMessage { id, data } => {
                    info!("Requesting message from actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::SendMessage {
                            actor_id: id.clone(),
                            actor_message: ActorMessage::Request(ActorRequest {
                                data: data.clone(),
                                response_tx: cmd_tx,
                            }),
                        })
                        .await?;

                    let response = cmd_rx.await?;
                    ManagementResponse::RequestedMessage {
                        id,
                        message: response,
                    }
                }
                ManagementCommand::GetActorManifest { id } => {
                    info!("Getting manifest for actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActorManifest {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    let manifest = cmd_rx.await?;
                    ManagementResponse::ActorManifest {
                        id,
                        manifest: manifest?,
                    }
                }
                ManagementCommand::GetActorStatus { id } => {
                    info!("Getting status for actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActorStatus {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    let status = cmd_rx.await?;
                    ManagementResponse::ActorStatus {
                        id,
                        status: status?,
                    }
                }
                ManagementCommand::RestartActor { id } => {
                    info!("Restarting actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::RestartActor {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(_) => ManagementResponse::Restarted { id },
                        Err(e) => ManagementResponse::Error {
                            error: ManagementError::RuntimeError(format!(
                                "Failed to restart actor: {}",
                                e
                            )),
                        },
                    }
                }
                ManagementCommand::GetActorState { id } => {
                    info!("Getting state for actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActorState {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    let state = cmd_rx.await?;
                    ManagementResponse::ActorState { id, state: state? }
                }
                ManagementCommand::GetActorEvents { id } => {
                    info!("Getting events for actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActorEvents {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await {
                        Ok(result) => match result {
                            Ok(events) => {
                                debug!(
                                    "Successfully retrieved {} events for actor {}",
                                    events.len(),
                                    id
                                );
                                ManagementResponse::ActorEvents { id, events }
                            }
                            Err(e) => {
                                debug!("Error getting events for actor {}: {}", id, e);
                                ManagementResponse::Error { error: e.into() }
                            }
                        },
                        Err(e) => {
                            error!("Failed to receive events response: {}", e);
                            ManagementResponse::Error {
                                error: ManagementError::CommunicationError(format!(
                                    "Failed to receive events response: {}",
                                    e
                                )),
                            }
                        }
                    }
                }
                ManagementCommand::GetActorMetrics { id } => {
                    info!("Getting metrics for actor: {:?}", id);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::GetActorMetrics {
                            actor_id: id.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    let metrics = cmd_rx.await?;
                    ManagementResponse::ActorMetrics {
                        id,
                        metrics: serde_json::to_value(metrics?)?,
                    }
                }
                ManagementCommand::UpdateActorComponent { id, component } => {
                    info!("Updating component for actor {:?} to {}", id, component);
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::UpdateActorComponent {
                            actor_id: id.clone(),
                            component: component.clone(),
                            response_tx: cmd_tx,
                        })
                        .await?;

                    match cmd_rx.await? {
                        Ok(_) => ManagementResponse::ActorComponentUpdated { id },
                        Err(e) => ManagementResponse::Error {
                            error: ManagementError::RuntimeError(format!(
                                "Failed to update actor component: {}",
                                e
                            )),
                        },
                    }
                }
                // Handle channel management commands
                ManagementCommand::OpenChannel {
                    actor_id,
                    initial_message,
                } => {
                    info!("Opening channel to actor: {:?}", actor_id);

                    // Create a response channel
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

                    // Generate a channel ID
                    let client_id = ChannelParticipant::External;
                    let channel_id = ChannelId::new(&client_id, &actor_id);
                    let channel_id_str = channel_id.0.clone();

                    // Send the channel open command to the runtime
                    runtime_tx
                        .send(TheaterCommand::ChannelOpen {
                            initiator_id: client_id.clone(),
                            target_id: actor_id.clone(),
                            channel_id: channel_id.clone(),
                            initial_message,
                            response_tx,
                        })
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to send channel open command: {}", e)
                        })?;

                    // Wait for the response
                    match response_rx.await {
                        Ok(result) => {
                            match result {
                                Ok(accepted) => {
                                    if accepted {
                                        // Channel opened successfully
                                        info!("Channel opened successfully: {}", channel_id_str);

                                        // Register the channel subscription to receive messages
                                        let channel_sub = ChannelSubscription {
                                            channel_id: channel_id_str.clone(),
                                            initiator_id: client_id.clone(),
                                            target_id: actor_id.clone(),
                                            client_tx: cmd_client_tx.clone(),
                                        };

                                        channel_subscriptions
                                            .lock()
                                            .await
                                            .insert(channel_id_str.clone(), channel_sub);

                                        // Track this channel for cleanup on disconnect
                                        connection_channel_subscriptions
                                            .push(channel_id_str.clone());

                                        ManagementResponse::ChannelOpened {
                                            channel_id: channel_id_str,
                                            actor_id,
                                        }
                                    } else {
                                        // Channel rejected by target
                                        ManagementResponse::Error {
                                            error: ManagementError::ChannelRejected,
                                        }
                                    }
                                }
                                Err(e) => ManagementResponse::Error {
                                    error: ManagementError::RuntimeError(format!(
                                        "Error opening channel: {}",
                                        e
                                    )),
                                },
                            }
                        }
                        Err(e) => ManagementResponse::Error {
                            error: ManagementError::CommunicationError(format!(
                                "Failed to receive channel open response: {}",
                                e
                            )),
                        },
                    }
                }
                ManagementCommand::SendOnChannel {
                    channel_id,
                    message,
                } => {
                    info!("Sending message on channel: {}", channel_id);

                    // Parse the channel ID
                    let channel_id_parsed = ChannelId(channel_id.clone());
                    let client_id = ChannelParticipant::External;

                    // Send the message on the channel
                    runtime_tx
                        .send(TheaterCommand::ChannelMessage {
                            channel_id: channel_id_parsed,
                            message,
                            sender_id: client_id,
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to send message on channel: {}", e))?;

                    ManagementResponse::MessageSent { channel_id }
                }
                ManagementCommand::CloseChannel { channel_id } => {
                    info!("Closing channel: {}", channel_id);

                    // Parse the channel ID
                    let channel_id_parsed = ChannelId(channel_id.clone());

                    // Close the channel
                    runtime_tx
                        .send(TheaterCommand::ChannelClose {
                            channel_id: channel_id_parsed,
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to close channel: {}", e))?;

                    // Remove from channel subscriptions
                    channel_subscriptions.lock().await.remove(&channel_id);
                    connection_channel_subscriptions.retain(|id| id != &channel_id);

                    ManagementResponse::ChannelClosed { channel_id }
                }
                ManagementCommand::NewStore {} => {
                    info!("Creating new store");
                    let (cmd_tx, cmd_rx) = tokio::sync::oneshot::channel();
                    runtime_tx
                        .send(TheaterCommand::NewStore {
                            response_tx: cmd_tx,
                        })
                        .await?;

                    let store_id = cmd_rx.await?;
                    ManagementResponse::StoreCreated {
                        store_id: store_id?.id,
                    }
                }
            };

            debug!("Sending response: {:?}", response);
            if let Err(e) = client_tx.send(response).await {
                error!("Failed to send response: {}", e);
                break;
            }
            debug!("Response sent");
        }

        // Clean up all subscriptions for this connection
        debug!(
            "Connection closed, cleaning up {} subscriptions",
            connection_subscriptions.len()
        );
        let mut subs = subscriptions.lock().await;

        for (actor_id, sub_id) in connection_subscriptions {
            if let Some(actor_subs) = subs.get_mut(&actor_id) {
                actor_subs.retain(|sub| sub.id != sub_id);

                // Remove the entry if no subscriptions remain
                if actor_subs.is_empty() {
                    subs.remove(&actor_id);
                }
            }
        }

        // Clean up channel subscriptions
        debug!(
            "Connection closed, cleaning up {} channel subscriptions",
            connection_channel_subscriptions.len()
        );
        let mut channel_subs = channel_subscriptions.lock().await;

        for channel_id in connection_channel_subscriptions {
            channel_subs.remove(&channel_id);
        }

        debug!("Cleaned up all subscriptions for the connection");
        Ok(())
    }
}
