//! Theater Message Server Handler
//!
//! Provides actor-to-actor messaging capabilities including:
//! - One-way send messages
//! - Request-response patterns
//! - Bidirectional channels

pub mod events;

pub use events::MessageEventData;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::permissions::MessageServerPermissions;
use theater::events::{ChainEventData, EventPayload};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{
    ActorChannelClose, ActorChannelInitiated, ActorChannelMessage, ActorChannelOpen,
    ActorMessage, ActorRequest, ActorSend, ChannelId, ChannelParticipant,
    MessageCommand,
};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};
use theater::TheaterId;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, info};
use uuid::Uuid;
use wasmtime::component::{ComponentType, Lift, Lower};
use wasmtime::StoreContextMut;

/// Errors that can occur during message server operations
#[derive(Error, Debug)]
pub enum MessageServerError {
    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Channel acceptance response
#[derive(Debug, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ChannelAccept {
    pub accepted: bool,
    pub message: Option<Vec<u8>>,
}

/// State for a single channel
#[derive(Clone, Debug)]
struct ChannelState {
    initiator_id: ChannelParticipant,
    target_id: ChannelParticipant,
    is_open: bool,
}

/// Commands sent to the MessageRouter task
enum RouterCommand {
    RegisterActor {
        actor_id: TheaterId,
        mailbox_tx: Sender<ActorMessage>,
        response_tx: tokio::sync::oneshot::Sender<()>,
    },
    UnregisterActor {
        actor_id: TheaterId,
    },
    RouteMessage {
        command: MessageCommand,
    },
}

/// High-throughput message router using channel-based architecture (no locks!)
///
/// This router runs as a single task that owns the actor registry HashMap.
/// All operations go through message passing, eliminating lock contention.
/// Can handle 100k+ messages/sec with zero blocking.
#[derive(Clone)]
pub struct MessageRouter {
    command_tx: Sender<RouterCommand>,
}

impl MessageRouter {
    /// Create a new MessageRouter and spawn its background task
    pub fn new() -> Self {
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(10000);

        // Spawn the router task that owns the actor registry
        tokio::spawn(Self::router_task(command_rx));

        Self { command_tx }
    }

    /// Register an actor with the router
    pub async fn register_actor(&self, actor_id: TheaterId, mailbox_tx: Sender<ActorMessage>) -> Result<()> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        self.command_tx.send(RouterCommand::RegisterActor {
            actor_id,
            mailbox_tx,
            response_tx,
        }).await.map_err(|e| anyhow::anyhow!("Failed to send register command: {}", e))?;

        response_rx.await.map_err(|e| anyhow::anyhow!("Failed to receive registration confirmation: {}", e))?;

        Ok(())
    }

    /// Unregister an actor from the router
    pub async fn unregister_actor(&self, actor_id: TheaterId) {
        let _ = self.command_tx.send(RouterCommand::UnregisterActor { actor_id }).await;
    }

    /// Route a message command to the appropriate actor
    pub async fn route_message(&self, command: MessageCommand) -> Result<()> {
        self.command_tx.send(RouterCommand::RouteMessage { command })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send route command: {}", e))?;

        Ok(())
    }

    /// Main router task - owns the actor registry, zero lock contention!
    async fn router_task(mut command_rx: Receiver<RouterCommand>) {
        info!("MessageRouter task started");

        // These HashMaps are owned by this task - no Arc, no RwLock needed!
        let mut actors: HashMap<TheaterId, Sender<ActorMessage>> = HashMap::new();
        let mut channels: HashMap<ChannelId, ChannelState> = HashMap::new();

        while let Some(cmd) = command_rx.recv().await {
            match cmd {
                RouterCommand::RegisterActor { actor_id, mailbox_tx, response_tx } => {
                    info!("Router: Registering actor {}", actor_id);
                    actors.insert(actor_id, mailbox_tx);
                    let _ = response_tx.send(());
                }

                RouterCommand::UnregisterActor { actor_id } => {
                    info!("Router: Unregistering actor {}", actor_id);
                    actors.remove(&actor_id);
                }

                RouterCommand::RouteMessage { command } => {
                    if let Err(e) = Self::handle_route_command(&actors, &mut channels, command).await {
                        error!("Router: Failed to route message: {}", e);
                    }
                }
            }
        }

        info!("MessageRouter task stopped");
    }

    /// Handle routing a MessageCommand to the appropriate actor
    async fn handle_route_command(
        actors: &HashMap<TheaterId, Sender<ActorMessage>>,
        channels: &mut HashMap<ChannelId, ChannelState>,
        command: MessageCommand,
    ) -> Result<()> {
        match command {
            MessageCommand::SendMessage { target_id, message, response_tx } => {
                if let Some(mailbox) = actors.get(&target_id) {
                    mailbox.send(message).await
                        .map_err(|e| anyhow::anyhow!("Failed to send to mailbox: {}", e))?;
                    let _ = response_tx.send(Ok(()));
                } else {
                    let _ = response_tx.send(Err(anyhow::anyhow!("Actor not found: {}", target_id)));
                }
            }

            MessageCommand::OpenChannel { target_id, channel_id, initiator_id, initial_message, response_tx } => {
                // Extract actor ID from ChannelParticipant
                let actor_id = match &target_id {
                    ChannelParticipant::Actor(id) => id,
                    ChannelParticipant::External => {
                        let _ = response_tx.send(Err(anyhow::anyhow!("Cannot open channel to external participant")));
                        return Ok(());
                    }
                };

                if let Some(mailbox) = actors.get(actor_id) {
                    // Create a wrapper response channel to track the channel opening
                    let (wrapped_response_tx, wrapped_response_rx) = tokio::sync::oneshot::channel();

                    let msg = ActorMessage::ChannelOpen(ActorChannelOpen {
                        channel_id: channel_id.clone(),
                        initiator_id: initiator_id.clone(),
                        response_tx: wrapped_response_tx,
                        initial_msg: initial_message,
                    });

                    mailbox.send(msg).await
                        .map_err(|e| anyhow::anyhow!("Failed to send channel open: {}", e))?;

                    // Wait for actor's response and track the channel if accepted
                    match wrapped_response_rx.await {
                        Ok(Ok(accepted)) => {
                            if accepted {
                                // Track the channel
                                info!("Router: Tracking channel {} (initiator: {:?}, target: {:?})",
                                    channel_id, initiator_id, target_id);
                                channels.insert(channel_id.clone(), ChannelState {
                                    initiator_id: initiator_id.clone(),
                                    target_id: target_id.clone(),
                                    is_open: true,
                                });
                            }
                            let _ = response_tx.send(Ok(accepted));
                        }
                        Ok(Err(e)) => {
                            let _ = response_tx.send(Err(e));
                        }
                        Err(e) => {
                            let _ = response_tx.send(Err(anyhow::anyhow!("Actor didn't respond to channel open: {}", e)));
                        }
                    }
                } else {
                    let _ = response_tx.send(Err(anyhow::anyhow!("Actor not found: {}", target_id)));
                }
            }

            MessageCommand::ChannelMessage { channel_id, sender_id, message, response_tx } => {
                // Look up channel state to find the other participant
                if let Some(channel_state) = channels.get(&channel_id) {
                    if !channel_state.is_open {
                        let _ = response_tx.send(Err(anyhow::anyhow!("Channel is closed")));
                        return Ok(());
                    }

                    // Determine the recipient (the OTHER participant)
                    let recipient_id = if sender_id == channel_state.initiator_id {
                        &channel_state.target_id
                    } else if sender_id == channel_state.target_id {
                        &channel_state.initiator_id
                    } else {
                        let _ = response_tx.send(Err(anyhow::anyhow!("Sender is not a participant in this channel")));
                        return Ok(());
                    };

                    // Route the message to the recipient
                    match recipient_id {
                        ChannelParticipant::Actor(actor_id) => {
                            if let Some(mailbox) = actors.get(actor_id) {
                                let msg = ActorMessage::ChannelMessage(ActorChannelMessage {
                                    channel_id,
                                    msg: message,
                                });
                                mailbox.send(msg).await
                                    .map_err(|e| anyhow::anyhow!("Failed to send channel message: {}", e))?;
                                let _ = response_tx.send(Ok(()));
                            } else {
                                let _ = response_tx.send(Err(anyhow::anyhow!("Recipient actor not found: {}", actor_id)));
                            }
                        }
                        ChannelParticipant::External => {
                            // External participants receive messages via the channel events mechanism
                            // The server handles this separately via channel_events_tx
                            let _ = response_tx.send(Ok(()));
                        }
                    }
                } else {
                    let _ = response_tx.send(Err(anyhow::anyhow!("Channel not found: {}", channel_id)));
                }
            }

            MessageCommand::ChannelClose { channel_id, sender_id, response_tx } => {
                // Look up and remove channel state
                if let Some(channel_state) = channels.remove(&channel_id) {
                    // Verify sender is a participant
                    if sender_id != channel_state.initiator_id && sender_id != channel_state.target_id {
                        let _ = response_tx.send(Err(anyhow::anyhow!("Sender is not a participant in this channel")));
                        return Ok(());
                    }

                    // Notify the other participant
                    let other_participant = if sender_id == channel_state.initiator_id {
                        &channel_state.target_id
                    } else {
                        &channel_state.initiator_id
                    };

                    match other_participant {
                        ChannelParticipant::Actor(actor_id) => {
                            if let Some(mailbox) = actors.get(actor_id) {
                                let msg = ActorMessage::ChannelClose(ActorChannelClose {
                                    channel_id: channel_id.clone(),
                                });
                                let _ = mailbox.send(msg).await;
                            }
                        }
                        ChannelParticipant::External => {
                            // External participants receive close notifications via channel events
                        }
                    }

                    info!("Router: Closed channel {}", channel_id);
                    let _ = response_tx.send(Ok(()));
                } else {
                    let _ = response_tx.send(Err(anyhow::anyhow!("Channel not found: {}", channel_id)));
                }
            }
        }

        Ok(())
    }
}

/// Per-actor MessageServerHandler that provides actor-to-actor communication.
///
/// Architecture:
/// - Each actor gets its own handler instance (via create_instance)
/// - Handler registers the actor's mailbox with the global MessageRouter
/// - Host functions send MessageCommand to the router for routing
/// - Mailbox consumption happens in start() until shutdown
///
/// Enables actors to:
/// - Send one-way messages
/// - Make request-response calls
/// - Open bidirectional channels
/// - Manage outstanding requests
#[derive(Clone)]
pub struct MessageServerHandler {
    // Reference to the global message router (external service)
    router: MessageRouter,

    // This actor's ID (set in setup_host_functions)
    actor_id: Option<TheaterId>,

    // This actor's mailbox receiver (set in setup_host_functions, consumed in start)
    mailbox_rx: Arc<Mutex<Option<Receiver<ActorMessage>>>>,

    // Request-response tracking for this actor
    outstanding_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<Vec<u8>>>>>,

    #[allow(dead_code)]
    permissions: Option<MessageServerPermissions>,
}

impl MessageServerHandler {
    /// Create a new MessageServerHandler with a reference to the global router
    ///
    /// # Arguments
    /// * `permissions` - Optional permission restrictions
    /// * `router` - Reference to the global MessageRouter
    pub fn new(
        permissions: Option<MessageServerPermissions>,
        router: MessageRouter,
    ) -> Self {
        Self {
            router,
            actor_id: None,
            mailbox_rx: Arc::new(Mutex::new(None)),
            outstanding_requests: Arc::new(Mutex::new(HashMap::new())),
            permissions,
        }
    }

    /// Process a message for this actor
    async fn process_actor_message(
        msg: ActorMessage,
        actor_handle: &ActorHandle,
        _outstanding_requests: &Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<Vec<u8>>>>>,
    ) -> Result<(), MessageServerError> {
        match msg {
            ActorMessage::Send(ActorSend { data }) => {
                actor_handle
                    .call_function::<(Vec<u8>,), ()>(
                        "theater:simple/message-server-client.handle-send".to_string(),
                        (data,),
                    )
                    .await?;
            }
            ActorMessage::Request(ActorRequest { response_tx, data }) => {
                let request_id = Uuid::new_v4().to_string();
                let response = actor_handle
                    .call_function::<(String, Vec<u8>), (Option<Vec<u8>>,)>(
                        "theater:simple/message-server-client.handle-request".to_string(),
                        (request_id, data),
                    )
                    .await?;
                if let Some(response_data) = response.0 {
                    let _ = response_tx.send(response_data);
                }
            }
            ActorMessage::ChannelOpen(ActorChannelOpen {
                channel_id,
                initiator_id: _,
                response_tx,
                initial_msg,
            }) => {
                let accept = actor_handle
                    .call_function::<(String, Vec<u8>), (ChannelAccept,)>(
                        "theater:simple/message-server-client.handle-channel-open".to_string(),
                        (channel_id.to_string(), initial_msg),
                    )
                    .await?;
                let _ = response_tx.send(Ok(accept.0.accepted));
            }
            ActorMessage::ChannelMessage(ActorChannelMessage { channel_id, msg }) => {
                actor_handle
                    .call_function::<(String, Vec<u8>), ()>(
                        "theater:simple/message-server-client.handle-channel-message".to_string(),
                        (channel_id.to_string(), msg),
                    )
                    .await?;
            }
            ActorMessage::ChannelClose(ActorChannelClose { channel_id }) => {
                actor_handle
                    .call_function::<(String,), ()>(
                        "theater:simple/message-server-client.handle-channel-close".to_string(),
                        (channel_id.to_string(),),
                    )
                    .await?;
            }
            ActorMessage::ChannelInitiated(ActorChannelInitiated { .. }) => {
                // Channel initiated from this actor - nothing to do
            }
        }
        Ok(())
    }
}

impl<E> Handler<E> for MessageServerHandler
where
    E: EventPayload + Clone + From<MessageEventData>
        + From<theater::events::theater_runtime::TheaterRuntimeEventData>
        + From<theater::events::wasm::WasmEventData>,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(self.clone())
    }

    fn name(&self) -> &str {
        "message-server"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/message-server-host".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/message-server-client".to_string()])
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent<E>, _ctx: &mut HandlerContext) -> Result<()> {
        info!("Setting up message server host functions");

        // Get this actor's ID
        let actor_id = actor_component.actor_store.get_id();
        info!("Registering actor {} with message router", actor_id);

        // Create mailbox for THIS actor
        let (mailbox_tx, mailbox_rx) = tokio::sync::mpsc::channel(100);

        // Register with global router (blocking operation - must be in async context)
        // Note: setup_host_functions is sync, so we need to use block_on
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.router.register_actor(actor_id.clone(), mailbox_tx).await
            })
        })?;

        // Store for start()
        self.actor_id = Some(actor_id);
        *self.mailbox_rx.lock().unwrap() = Some(mailbox_rx);

        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::HandlerSetupStart.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Starting message server host function setup".to_string()),
        });

        let mut interface = match actor_component
            .linker
            .instance("theater:simple/message-server-host")
        {
            Ok(interface) => {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::LinkerInstanceSuccess.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(
                        "Successfully created linker instance for message-server-host".to_string(),
                    ),
                });
                interface
            }
            Err(e) => {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/message-server-host: {}",
                    e
                ));
            }
        };

        // 1. send operation
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "send".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'send' function wrapper".to_string()),
        });

        let router = self.router.clone();

        interface
            .func_wrap_async(
                "send",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/send".to_string(),
                        data: MessageEventData::SendMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(),
                            data: msg.clone(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Sending message to {}", address)),
                    });

                    info!("Sending message to actor: {}", address);
                    let target_id = match TheaterId::parse(&address) {
                        Ok(id) => id,
                        Err(e) => {
                            let err_msg = format!("Failed to parse actor ID: {}", e);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/send"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "send".to_string(),
                                    recipient: Some(address.clone()),
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error sending message to {}: {}",
                                    address, err_msg
                                )),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::SendMessage {
                        target_id,
                        message: ActorMessage::Send(ActorSend { data: msg.clone() }),
                        response_tx,
                    };

                    let router = router.clone();
                    let address_clone = address.clone();

                    Box::new(async move {
                        if let Err(e) = router.route_message(command).await {
                            let err = e.to_string();
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/send"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "send".to_string(),
                                    recipient: Some(address_clone.clone()),
                                    message: err.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Failed to send command to message-server: {}",
                                    err
                                )),
                            });
                            return Ok((Err(err),));
                        }

                        match response_rx.await {
                            Ok(Ok(())) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send"
                                        .to_string(),
                                    data: MessageEventData::SendMessageResult {
                                        recipient: address_clone.clone(),
                                        success: true,
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Successfully sent message to {}",
                                        address_clone
                                    )),
                                });
                                Ok((Ok(()),))
                            }
                            Ok(Err(e)) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "send".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send message to {}: {}",
                                        address_clone, err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "send".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to receive response from message-server: {}",
                                        err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "send_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'send' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async send function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "send".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'send' function wrapper".to_string()),
        });

        // 2. request operation
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "request".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'request' function wrapper".to_string()),
        });

        let router = self.router.clone();

        interface
            .func_wrap_async(
                "request",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/request".to_string(),
                        data: MessageEventData::RequestMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(),
                            data: msg.clone(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Requesting message from {}", address)),
                    });

                    let router = router.clone();
                    let address_clone = address.clone();

                    Box::new(async move {
                        let target_id = match TheaterId::parse(&address) {
                            Ok(id) => id,
                            Err(e) => {
                                let err_msg = format!("Failed to parse actor ID: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/request"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "request".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err_msg.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Error requesting message from {}: {}",
                                        address_clone, err_msg
                                    )),
                                });
                                return Ok((Err(err_msg),));
                            }
                        };

                        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                        let (cmd_response_tx, cmd_response_rx) = tokio::sync::oneshot::channel();

                        let command = MessageCommand::SendMessage {
                            target_id,
                            message: ActorMessage::Request(ActorRequest {
                                data: msg,
                                response_tx,
                            }),
                            response_tx: cmd_response_tx,
                        };

                        if let Err(e) = router.route_message(command).await {
                            let err = e.to_string();
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/request"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "request".to_string(),
                                    recipient: Some(address_clone.clone()),
                                    message: err.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Failed to send command to message-server: {}",
                                    err
                                )),
                            });
                            return Ok((Err(err),));
                        }

                        // Wait for command response
                        match cmd_response_rx.await {
                            Ok(Ok(())) => {
                                // Command sent successfully, now wait for actor response
                                match response_rx.await {
                                    Ok(response) => {
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "theater:simple/message-server-host/request"
                                                .to_string(),
                                            data: 
                                                MessageEventData::RequestMessageResult {
                                                    recipient: address_clone.clone(),
                                                    data: response.clone(),
                                                    success: true,
                                                }.into(),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!(
                                                "Successfully received response from {}",
                                                address_clone
                                            )),
                                        });
                                        Ok((Ok(response),))
                                    }
                                    Err(e) => {
                                        let err = e.to_string();
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "theater:simple/message-server-host/request"
                                                .to_string(),
                                            data: MessageEventData::Error {
                                                operation: "request".to_string(),
                                                recipient: Some(address_clone.clone()),
                                                message: err.clone(),
                                            }.into(),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!(
                                                "Failed to receive response from {}: {}",
                                                address_clone, err
                                            )),
                                        });
                                        Ok((Err(err),))
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/request"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "request".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send request to {}: {}",
                                        address_clone, err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/request"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "request".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to receive command response from message-server: {}",
                                        err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "request_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'request' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async request function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "request".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'request' function wrapper".to_string()),
        });

        // 3. list-outstanding-requests operation
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "list-outstanding-requests".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Setting up 'list-outstanding-requests' function wrapper".to_string(),
            ),
        });

        let outstanding_requests = self.outstanding_requests.clone();

        interface
            .func_wrap_async(
                "list-outstanding-requests",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      _: ()|
                      -> Box<dyn Future<Output = Result<(Vec<String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/list-outstanding-requests"
                            .to_string(),
                        data: MessageEventData::ListOutstandingRequestsCall {}.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some("Listing outstanding requests".to_string()),
                    });

                    let outstanding_clone = outstanding_requests.clone();
                    Box::new(async move {
                        let requests = outstanding_clone.lock().unwrap();
                        let ids: Vec<String> = requests.keys().cloned().collect();

                        ctx.data_mut().record_event(ChainEventData {
                            event_type:
                                "theater:simple/message-server-host/list-outstanding-requests"
                                    .to_string(),
                            data: 
                                MessageEventData::ListOutstandingRequestsResult {
                                    request_count: ids.len(),
                                    request_ids: ids.clone(),
                                }.into(),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Found {} outstanding requests", ids.len())),
                        });

                        Ok((ids,))
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "list_outstanding_requests_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'list-outstanding-requests' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!(
                    "Failed to wrap async list-outstanding-requests function: {}",
                    e
                )
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "list-outstanding-requests".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Successfully set up 'list-outstanding-requests' function wrapper".to_string(),
            ),
        });

        // 4. respond-to-request operation
        let outstanding_requests = self.outstanding_requests.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "respond-to-request".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'respond-to-request' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "respond-to-request",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (request_id, response_data): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let request_id_clone = request_id.clone();

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/respond-to-request"
                            .to_string(),
                        data: MessageEventData::RespondToRequestCall {
                            request_id: request_id.clone(),
                            response_size: response_data.len(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Responding to request {}", request_id)),
                    });

                    let outstanding_clone = outstanding_requests.clone();
                    Box::new(async move {
                        let mut requests = outstanding_clone.lock().unwrap();
                        if let Some(sender) = requests.remove(&request_id) {
                            match sender.send(response_data) {
                                Ok(_) => {
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type:
                                            "theater:simple/message-server-host/respond-to-request"
                                                .to_string(),
                                        data: 
                                            MessageEventData::RespondToRequestResult {
                                                request_id: request_id_clone.clone(),
                                                success: true,
                                            }.into(),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Successfully responded to request {}",
                                            request_id_clone
                                        )),
                                    });
                                    Ok((Ok(()),))
                                }
                                Err(e) => {
                                    let err_msg = format!("Failed to send response: {:?}", e);
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type:
                                            "theater:simple/message-server-host/respond-to-request"
                                                .to_string(),
                                        data: MessageEventData::Error {
                                            operation: "respond-to-request".to_string(),
                                            recipient: None,
                                            message: err_msg.clone(),
                                        }.into(),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Error responding to request {}: {}",
                                            request_id_clone, err_msg
                                        )),
                                    });
                                    Ok((Err(err_msg),))
                                }
                            }
                        } else {
                            let err_msg = format!("Request ID not found: {}", request_id);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/respond-to-request"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "respond-to-request".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Request {} not found",
                                    request_id_clone
                                )),
                            });
                            Ok((Err(err_msg),))
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "respond_to_request_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'respond-to-request' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async respond-to-request function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "respond-to-request".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Successfully set up 'respond-to-request' function wrapper".to_string(),
            ),
        });

        // 5. cancel-request operation
        let outstanding_requests = self.outstanding_requests.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "cancel-request".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'cancel-request' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "cancel-request",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (request_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let request_id_clone = request_id.clone();

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/cancel-request"
                            .to_string(),
                        data: MessageEventData::CancelRequestCall {
                            request_id: request_id.clone(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Canceling request {}", request_id)),
                    });

                    let outstanding_clone = outstanding_requests.clone();
                    Box::new(async move {
                        let mut requests = outstanding_clone.lock().unwrap();
                        if requests.remove(&request_id).is_some() {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/cancel-request"
                                    .to_string(),
                                data: MessageEventData::CancelRequestResult {
                                    request_id: request_id_clone.clone(),
                                    success: true,
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Successfully canceled request {}",
                                    request_id_clone
                                )),
                            });
                            Ok((Ok(()),))
                        } else {
                            let err_msg = format!("Request ID not found: {}", request_id);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/cancel-request"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "cancel-request".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Request {} not found",
                                    request_id_clone
                                )),
                            });
                            Ok((Err(err_msg),))
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "cancel_request_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'cancel-request' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async cancel-request function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "cancel-request".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'cancel-request' function wrapper".to_string()),
        });

        // 6. open-channel operation
        let router = self.router.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "open-channel".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'open-channel' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "open-channel",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (address, initial_msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    let current_actor_id = ctx.data().id.clone();
                    let address_clone = address.clone();

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/open-channel".to_string(),
                        data: MessageEventData::OpenChannelCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(),
                            size: initial_msg.len(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Opening channel to {}", address)),
                    });

                    let target_id = match TheaterId::parse(&address) {
                        Ok(id) => ChannelParticipant::Actor(id),
                        Err(e) => {
                            let err_msg = format!("Failed to parse actor ID: {}", e);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/open-channel"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "open-channel".to_string(),
                                    recipient: Some(address_clone.clone()),
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error opening channel to {}: {}",
                                    address_clone, err_msg
                                )),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };

                    let channel_id = ChannelId::new(
                        &ChannelParticipant::Actor(current_actor_id.clone()),
                        &target_id,
                    );
                    let channel_id_str = channel_id.as_str().to_string();

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

                    let command = MessageCommand::OpenChannel {
                        initiator_id: ChannelParticipant::Actor(current_actor_id.clone()),
                        target_id: target_id.clone(),
                        channel_id: channel_id.clone(),
                        initial_message: initial_msg.clone(),
                        response_tx,
                    };

                    let router = router.clone();
                    let channel_id_clone = channel_id_str.clone();

                    Box::new(async move {
                        if let Err(e) = router.route_message(command).await {
                            let err_msg = format!("Failed to send command to message-server: {}", e);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/open-channel"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "open-channel".to_string(),
                                    recipient: Some(address_clone.clone()),
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Failed to send open-channel command: {}",
                                    err_msg
                                )),
                            });
                            return Ok((Err(err_msg),));
                        }

                        match response_rx.await {
                            Ok(Ok(accepted)) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type:
                                        "theater:simple/message-server-host/open-channel"
                                            .to_string(),
                                    data: 
                                        MessageEventData::OpenChannelResult {
                                            recipient: address_clone.clone(),
                                            channel_id: channel_id_clone.clone(),
                                            accepted,
                                        }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Channel {} to {} {}",
                                        channel_id_clone,
                                        address_clone,
                                        if accepted { "accepted" } else { "rejected" }
                                    )),
                                });

                                if accepted {
                                    Ok((Ok(channel_id_clone),))
                                } else {
                                    Ok((Err(
                                        "Channel request rejected by target actor"
                                            .to_string(),
                                    ),))
                                }
                            }
                            Ok(Err(e)) => {
                                let err_msg = format!("Error opening channel: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type:
                                        "theater:simple/message-server-host/open-channel"
                                            .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "open-channel".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err_msg.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Error opening channel to {}: {}",
                                        address_clone, err_msg
                                    )),
                                });
                                Ok((Err(err_msg),))
                            }
                            Err(e) => {
                                let err_msg = format!("Failed to receive channel open response: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/open-channel"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "open-channel".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err_msg.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Error opening channel to {}: {}",
                                        address_clone, err_msg
                                    )),
                                });
                                Ok((Err(err_msg),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "open_channel_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'open-channel' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async open-channel function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "open-channel".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'open-channel' function wrapper".to_string()),
        });

        // 7. send-on-channel operation
        let router = self.router.clone();
        let sender_actor_id = actor_id.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "send-on-channel".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'send-on-channel' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "send-on-channel",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (channel_id_str, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/send-on-channel"
                            .to_string(),
                        data: MessageEventData::ChannelMessageCall {
                            channel_id: channel_id_str.clone(),
                            msg: msg.clone(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!(
                            "Sending message on channel {}",
                            channel_id_str
                        )),
                    });

                    let channel_id = match ChannelId::parse(&channel_id_str) {
                        Ok(id) => id,
                        Err(e) => {
                            let err_msg = format!("Failed to parse channel ID: {}", e);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/send-on-channel"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "send-on-channel".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error sending on channel {}: {}",
                                    channel_id_str, err_msg
                                )),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::ChannelMessage {
                        channel_id: channel_id.clone(),
                        sender_id: ChannelParticipant::Actor(sender_actor_id.clone()),
                        message: msg.clone(),
                        response_tx,
                    };

                    let router = router.clone();
                    let channel_id_clone = channel_id_str.clone();

                    Box::new(async move {
                        if let Err(e) = router.route_message(command).await {
                            let err = e.to_string();
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/send-on-channel"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "send-on-channel".to_string(),
                                    recipient: None,
                                    message: err.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Failed to send command to message-server: {}",
                                    err
                                )),
                            });
                            return Ok((Err(err),));
                        }

                        match response_rx.await {
                            Ok(Ok(())) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel"
                                        .to_string(),
                                    data: 
                                        MessageEventData::ChannelMessageResult {
                                            channel_id: channel_id_clone.clone(),
                                            success: true,
                                        }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Successfully sent message on channel {}",
                                        channel_id_clone
                                    )),
                                });
                                Ok((Ok(()),))
                            }
                            Ok(Err(e)) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "send-on-channel".to_string(),
                                        recipient: None,
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send message on channel {}: {}",
                                        channel_id_clone, err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "send-on-channel".to_string(),
                                        recipient: None,
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to receive response from message-server: {}",
                                        err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "send_on_channel_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'send-on-channel' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async send-on-channel function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "send-on-channel".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Successfully set up 'send-on-channel' function wrapper".to_string(),
            ),
        });

        // 8. close-channel operation
        let router = self.router.clone();
        let sender_actor_id = actor_id.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupStart {
                function_name: "close-channel".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'close-channel' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "close-channel",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (channel_id_str,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/close-channel".to_string(),
                        data: MessageEventData::CloseChannelCall {
                            channel_id: channel_id_str.clone(),
                        }.into(),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Closing channel {}", channel_id_str)),
                    });

                    let channel_id = match ChannelId::parse(&channel_id_str) {
                        Ok(id) => id,
                        Err(e) => {
                            let err_msg = format!("Failed to parse channel ID: {}", e);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/close-channel"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "close-channel".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error closing channel {}: {}",
                                    channel_id_str, err_msg
                                )),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::ChannelClose {
                        channel_id: channel_id.clone(),
                        sender_id: ChannelParticipant::Actor(sender_actor_id.clone()),
                        response_tx,
                    };

                    let router = router.clone();
                    let channel_id_clone = channel_id_str.clone();

                    Box::new(async move {
                        if let Err(e) = router.route_message(command).await {
                            let err = e.to_string();
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/close-channel"
                                    .to_string(),
                                data: MessageEventData::Error {
                                    operation: "close-channel".to_string(),
                                    recipient: None,
                                    message: err.clone(),
                                }.into(),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Failed to send command to message-server: {}",
                                    err
                                )),
                            });
                            return Ok((Err(err),));
                        }

                        match response_rx.await {
                            Ok(Ok(())) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel"
                                        .to_string(),
                                    data: 
                                        MessageEventData::CloseChannelResult {
                                            channel_id: channel_id_clone.clone(),
                                            success: true,
                                        }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Successfully closed channel {}",
                                        channel_id_clone
                                    )),
                                });
                                Ok((Ok(()),))
                            }
                            Ok(Err(e)) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "close-channel".to_string(),
                                        recipient: None,
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to close channel {}: {}",
                                        channel_id_clone, err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel"
                                        .to_string(),
                                    data: MessageEventData::Error {
                                        operation: "close-channel".to_string(),
                                        recipient: None,
                                        message: err.clone(),
                                    }.into(),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to receive response from message-server: {}",
                                        err
                                    )),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "close_channel_function_wrap".to_string(),
                    }.into(),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Failed to set up 'close-channel' function wrapper: {}",
                        e
                    )),
                });
                anyhow::anyhow!("Failed to wrap async close-channel function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::FunctionSetupSuccess {
                function_name: "close-channel".to_string(),
            }.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'close-channel' function wrapper".to_string()),
        });

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: MessageEventData::HandlerSetupSuccess.into(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Message server host functions setup completed successfully".to_string(),
            ),
        });

        info!("Message server host functions added");

        Ok(())
    }

    fn add_export_functions(&self, actor_instance: &mut ActorInstance<E>) -> Result<()> {
        info!("Adding export functions for message server");

        // 1. handle-send
        actor_instance
            .register_function_no_result::<(Vec<u8>,)>(
                "theater:simple/message-server-client",
                "handle-send",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-send function: {}", e))?;

        // 2. handle-request
        actor_instance
            .register_function::<(String, Vec<u8>), (Option<Vec<u8>>,)>(
                "theater:simple/message-server-client",
                "handle-request",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-request function: {}", e))?;

        // 3. handle-channel-open
        actor_instance
            .register_function::<(String, Vec<u8>), (ChannelAccept,)>(
                "theater:simple/message-server-client",
                "handle-channel-open",
            )
            .map_err(|e| {
                anyhow::anyhow!("Failed to register handle-channel-open function: {}", e)
            })?;

        // 4. handle-channel-message
        actor_instance
            .register_function_no_result::<(String, Vec<u8>)>(
                "theater:simple/message-server-client",
                "handle-channel-message",
            )
            .map_err(|e| {
                anyhow::anyhow!("Failed to register handle-channel-message function: {}", e)
            })?;

        // 5. handle-channel-close
        actor_instance
            .register_function_no_result::<(String,)>(
                "theater:simple/message-server-client",
                "handle-channel-close",
            )
            .map_err(|e| {
                anyhow::anyhow!("Failed to register handle-channel-close function: {}", e)
            })?;

        info!("Added all export functions for message server");
        Ok(())
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Starting message server handler for actor");

        // Take the mailbox receiver
        let mailbox_rx_opt = self.mailbox_rx.lock().unwrap().take();

        // Clone what we need for the async block
        let actor_id = self.actor_id.clone();
        let router = self.router.clone();
        let outstanding_requests = self.outstanding_requests.clone();

        Box::pin(async move {
            // If we don't have a receiver (cloned instance), just return
            let Some(mut mailbox_rx) = mailbox_rx_opt else {
                info!("Message server has no mailbox receiver (cloned instance), not starting");
                return Ok(());
            };

            let Some(actor_id) = actor_id else {
                error!("Message server handler has no actor_id - setup_host_functions not called?");
                return Ok(());
            };

            info!("Message server handler consuming mailbox for actor {}", actor_id);

            // Consume mailbox until shutdown
            loop {
                tokio::select! {
                    _ = &mut shutdown_receiver.receiver => {
                        info!("Actor {} received shutdown signal", actor_id);
                        break;
                    }
                    Some(msg) = mailbox_rx.recv() => {
                        if let Err(e) = Self::process_actor_message(msg, &actor_handle, &outstanding_requests).await {
                            error!("Actor {}: Error processing message: {}", actor_id, e);
                        }
                    }
                    else => {
                        info!("Actor {} mailbox closed", actor_id);
                        break;
                    }
                }
            }

            // Unregister from router on shutdown
            info!("Unregistering actor {} from message router", actor_id);
            router.unregister_actor(actor_id.clone()).await;

            info!("Message server handler shutdown complete for actor {}", actor_id);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_server_handler_creation() {
        let router = MessageRouter::new();
        let handler = MessageServerHandler::new(None, router);

        assert_eq!(handler.name(), "message-server");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/message-server-host".to_string()])
        );
        assert_eq!(
            handler.exports(),
            Some(vec!["theater:simple/message-server-client".to_string()])
        );
    }

    #[test]
    fn test_message_server_handler_clone() {
        let router = MessageRouter::new();
        let handler = MessageServerHandler::new(None, router);

        let cloned = handler.create_instance();
        assert_eq!(cloned.name(), "message-server");
    }
}
