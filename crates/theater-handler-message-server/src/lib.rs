//! Theater Message Server Handler
//!
//! Provides actor-to-actor messaging capabilities including:
//! - One-way send messages
//! - Request-response patterns
//! - Bidirectional channels
//!
//! # TODO: Messaging Address Refactor
//!
//! Currently this handler uses `TheaterId` (the runtime's internal actor identifier)
//! for message addressing. This is problematic because:
//!
//! 1. TheaterId is a runtime concept - it's generated when spawning actors and is
//!    non-deterministic (uses UUIDs)
//! 2. Using TheaterId for messaging couples the actor's identity to runtime internals
//! 3. This breaks chain reproducibility - the same actor run twice gets different IDs,
//!    so message addresses in the chain differ
//!
//! The messaging system should have its own address concept that is:
//! - Separate from the runtime's internal actor tracking
//! - Configurable/deterministic (e.g., from manifest config)
//! - Part of the actor's "world" rather than runtime internals
//!
//! This would allow:
//! - Actors to have stable, reproducible addresses
//! - Chain events to be deterministic
//! - Clear separation between runtime bookkeeping and actor behavior

pub mod events;


use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::permissions::MessageServerPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{
    ActorChannelClose, ActorChannelInitiated, ActorChannelMessage, ActorChannelOpen,
    ActorMessage, ActorRequest, ActorSend, ChannelId, ChannelParticipant,
    MessageCommand,
};
use theater::shutdown::ShutdownReceiver;
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

// Pack integration
use theater::pack_bridge::{
    AsyncCtx, PackInstance, Ctx, HostLinkerBuilder, LinkerError, Value, ValueType,
};

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
#[derive(Debug, Deserialize, Serialize)]
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

    // This actor's ID (set in setup_host_functions_composite via HandlerContext)
    actor_id: Option<TheaterId>,

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
                // handle-send(state, params: tuple<list<u8>>)
                let params = Value::Tuple(vec![bytes_to_value(data)]);
                actor_handle
                    .call_function(
                        "theater:simple/message-server-client.handle-send".to_string(),
                        params,
                    )
                    .await?;
            }
            ActorMessage::Request(ActorRequest { response_tx, data }) => {
                // handle-request(state, params: tuple<string, list<u8>>)
                let request_id = Uuid::new_v4().to_string();
                let params = Value::Tuple(vec![
                    Value::String(request_id),
                    bytes_to_value(data),
                ]);
                let result = actor_handle
                    .call_function(
                        "theater:simple/message-server-client.handle-request".to_string(),
                        params,
                    )
                    .await?;
                // Result is result<tuple<option<list<u8>>, tuple<option<list<u8>>>>, string>
                // Extract the optional response
                if let Some(response_data) = parse_option_bytes_from_tuple(&result) {
                    let _ = response_tx.send(response_data);
                }
            }
            ActorMessage::ChannelOpen(ActorChannelOpen {
                channel_id,
                initiator_id: _,
                response_tx,
                initial_msg,
            }) => {
                // handle-channel-open(state, params: tuple<string, list<u8>>)
                let params = Value::Tuple(vec![
                    Value::String(channel_id.to_string()),
                    bytes_to_value(initial_msg),
                ]);
                let result = actor_handle
                    .call_function(
                        "theater:simple/message-server-client.handle-channel-open".to_string(),
                        params,
                    )
                    .await?;
                // Result is tuple<channel-accept> where channel-accept is record {accepted: bool, message: option<list<u8>>}
                let accepted = parse_channel_accept(&result);
                let _ = response_tx.send(Ok(accepted));
            }
            ActorMessage::ChannelMessage(ActorChannelMessage { channel_id, msg }) => {
                // handle-channel-message(state, params: tuple<channel-id, list<u8>>)
                let params = Value::Tuple(vec![
                    Value::String(channel_id.to_string()),
                    bytes_to_value(msg),
                ]);
                actor_handle
                    .call_function(
                        "theater:simple/message-server-client.handle-channel-message".to_string(),
                        params,
                    )
                    .await?;
            }
            ActorMessage::ChannelClose(ActorChannelClose { channel_id }) => {
                // handle-channel-close(state, params: tuple<channel-id>)
                let params = Value::Tuple(vec![Value::String(channel_id.to_string())]);
                actor_handle
                    .call_function(
                        "theater:simple/message-server-client.handle-channel-close".to_string(),
                        params,
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

impl Handler for MessageServerHandler
{
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
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

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Starting message server handler for actor");

        // Clone what we need for the async block
        let actor_id = self.actor_id.clone();
        let router = self.router.clone();
        let outstanding_requests = self.outstanding_requests.clone();

        Box::pin(async move {
            let Some(actor_id) = actor_id else {
                error!("Message server handler has no actor_id - setup_host_functions not called?");
                return Ok(());
            };

            // Create mailbox channel and register with the router
            let (mailbox_tx, mut mailbox_rx) = tokio::sync::mpsc::channel(100);
            router.register_actor(actor_id.clone(), mailbox_tx).await?;

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

    // =========================================================================
    // Composite Integration
    // =========================================================================

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up message server host functions (Pack)");

        // Store the actor_id from context for use in start()
        if let Some(ref actor_id) = ctx.actor_id {
            self.actor_id = Some(actor_id.clone());
        }

        // Check if already satisfied
        if ctx.is_satisfied("theater:simple/message-server-host") {
            info!("theater:simple/message-server-host already satisfied, skipping");
            return Ok(());
        }

        let router = self.router.clone();
        let router2 = self.router.clone();
        let router3 = self.router.clone();
        let router4 = self.router.clone();
        let router5 = self.router.clone();
        let outstanding_requests = self.outstanding_requests.clone();
        let outstanding_requests2 = self.outstanding_requests.clone();
        let outstanding_requests3 = self.outstanding_requests.clone();

        builder
            .interface("theater:simple/message-server-host")?
            // send(address: string, msg: list<u8>) -> result<_, string>
            .func_async_result("send", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let router = router.clone();
                async move {
                    let (address, msg) = parse_address_and_message(&input)?;

                    let target_id = match TheaterId::parse(&address) {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Failed to parse actor ID: {}", e))),
                    };

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::SendMessage {
                        target_id,
                        message: ActorMessage::Send(ActorSend { data: msg }),
                        response_tx,
                    };

                    if let Err(e) = router.route_message(command).await {
                        return Err(Value::String(e.to_string()));
                    }

                    match response_rx.await {
                        Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(e.to_string())),
                    }
                }
            })?
            // request(address: string, msg: list<u8>) -> result<list<u8>, string>
            .func_async_result("request", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let router = router2.clone();
                async move {
                    let (address, msg) = parse_address_and_message(&input)?;

                    let target_id = match TheaterId::parse(&address) {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Failed to parse actor ID: {}", e))),
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
                        return Err(Value::String(e.to_string()));
                    }

                    match cmd_response_rx.await {
                        Ok(Ok(())) => {

                            match response_rx.await {
                                Ok(response) => Ok(Value::List {
                                    elem_type: ValueType::U8,
                                    items: response.into_iter().map(Value::U8).collect(),
                                }),
                                Err(e) => Err(Value::String(e.to_string())),
                            }
                        }
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(e.to_string())),
                    }
                }
            })?
            // list-outstanding-requests() -> list<string>
            .func_typed("list-outstanding-requests", move |_ctx: &mut Ctx<'_, ActorStore>, _input: Value| {
                let requests = outstanding_requests.lock().unwrap();
                let ids: Vec<Value> = requests.keys().map(|k| Value::String(k.clone())).collect();
                Value::List {
                    elem_type: ValueType::String,
                    items: ids,
                }
            })?
            // respond-to-request(request-id: string, response: list<u8>) -> result<_, string>
            .func_async_result("respond-to-request", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let outstanding = outstanding_requests2.clone();
                async move {
                    let (request_id, response_data) = parse_request_id_and_data(&input)?;

                    let mut requests = outstanding.lock().unwrap();
                    if let Some(sender) = requests.remove(&request_id) {
                        match sender.send(response_data) {
                            Ok(_) => Ok(Value::Tuple(vec![])),
                            Err(e) => Err(Value::String(format!("Failed to send response: {:?}", e))),
                        }
                    } else {
                        Err(Value::String(format!("Request ID not found: {}", request_id)))
                    }
                }
            })?
            // cancel-request(request-id: string) -> result<_, string>
            .func_async_result("cancel-request", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let outstanding = outstanding_requests3.clone();
                async move {
                    let request_id = parse_string(&input)?;

                    let mut requests = outstanding.lock().unwrap();
                    if requests.remove(&request_id).is_some() {
                        Ok(Value::Tuple(vec![]))
                    } else {
                        Err(Value::String(format!("Request ID not found: {}", request_id)))
                    }
                }
            })?
            // open-channel(address: string, initial-msg: list<u8>) -> result<string, string>
            .func_async_result("open-channel", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let router = router3.clone();
                async move {
                    let (address, initial_msg) = parse_address_and_message(&input)?;
                    let current_actor_id = ctx.data().id.clone();

                    let target_id = match TheaterId::parse(&address) {
                        Ok(id) => ChannelParticipant::Actor(id),
                        Err(e) => return Err(Value::String(format!("Failed to parse actor ID: {}", e))),
                    };

                    let channel_id = ChannelId::new(
                        &ChannelParticipant::Actor(current_actor_id.clone()),
                        &target_id,
                    );
                    let channel_id_str = channel_id.as_str().to_string();

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::OpenChannel {
                        initiator_id: ChannelParticipant::Actor(current_actor_id),
                        target_id,
                        channel_id,
                        initial_message: initial_msg,
                        response_tx,
                    };

                    if let Err(e) = router.route_message(command).await {
                        return Err(Value::String(format!("Failed to send command: {}", e)));
                    }

                    match response_rx.await {
                        Ok(Ok(accepted)) => {
                            if accepted {
                                Ok(Value::String(channel_id_str))
                            } else {
                                Err(Value::String("Channel request rejected".to_string()))
                            }
                        }
                        Ok(Err(e)) => Err(Value::String(format!("Error opening channel: {}", e))),
                        Err(e) => Err(Value::String(format!("Failed to receive response: {}", e))),
                    }
                }
            })?
            // send-on-channel(channel-id: string, msg: list<u8>) -> result<_, string>
            .func_async_result("send-on-channel", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let router = router4.clone();
                async move {
                    let (channel_id_str, msg) = parse_address_and_message(&input)?;
                    let sender_actor_id = ctx.data().id.clone();

                    let channel_id = match ChannelId::parse(&channel_id_str) {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Failed to parse channel ID: {}", e))),
                    };

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::ChannelMessage {
                        channel_id,
                        sender_id: ChannelParticipant::Actor(sender_actor_id),
                        message: msg,
                        response_tx,
                    };

                    if let Err(e) = router.route_message(command).await {
                        return Err(Value::String(e.to_string()));
                    }

                    match response_rx.await {
                        Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(e.to_string())),
                    }
                }
            })?
            // close-channel(channel-id: string) -> result<_, string>
            .func_async_result("close-channel", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let router = router5.clone();
                async move {
                    let channel_id_str = parse_string(&input)?;
                    let sender_actor_id = ctx.data().id.clone();

                    let channel_id = match ChannelId::parse(&channel_id_str) {
                        Ok(id) => id,
                        Err(e) => return Err(Value::String(format!("Failed to parse channel ID: {}", e))),
                    };

                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let command = MessageCommand::ChannelClose {
                        channel_id,
                        sender_id: ChannelParticipant::Actor(sender_actor_id),
                        response_tx,
                    };

                    if let Err(e) = router.route_message(command).await {
                        return Err(Value::String(e.to_string()));
                    }

                    match response_rx.await {
                        Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                        Ok(Err(e)) => Err(Value::String(e.to_string())),
                        Err(e) => Err(Value::String(e.to_string())),
                    }
                }
            })?;

        ctx.mark_satisfied("theater:simple/message-server-host");
        info!("Message server host functions (Pack) set up successfully");
        Ok(())
    }

    fn register_exports_composite(&self, instance: &mut PackInstance) -> anyhow::Result<()> {
        info!("Registering message server exports (Pack)");

        // Register all export functions
        instance.register_export("theater:simple/message-server-client", "handle-send");
        instance.register_export("theater:simple/message-server-client", "handle-request");
        instance.register_export("theater:simple/message-server-client", "handle-channel-open");
        instance.register_export("theater:simple/message-server-client", "handle-channel-message");
        instance.register_export("theater:simple/message-server-client", "handle-channel-close");

        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

// Helper functions for building Value params for export calls

/// Convert a Vec<u8> to a Value::List of U8
fn bytes_to_value(data: Vec<u8>) -> Value {
    Value::List {
        elem_type: ValueType::U8,
        items: data.into_iter().map(Value::U8).collect(),
    }
}

/// Parse an option<list<u8>> from a handle-request result.
/// The actual return type is:
///   result<tuple<option<list<u8>>, tuple<option<list<u8>>>>, string>
/// We need to:
/// 1. Unwrap the Result (if Ok)
/// 2. Get element 1 of the outer tuple (the response tuple)
/// 3. Get element 0 of that tuple (the option<list<u8>>)
fn parse_option_bytes_from_tuple(value: &Value) -> Option<Vec<u8>> {
    // First, unwrap the Result if present
    let inner_tuple = match value {
        Value::Result { value: Ok(inner), .. } => inner.as_ref(),
        Value::Result { value: Err(_), .. } => return None,
        // Fallback for simple tuple (backward compat)
        Value::Tuple(_) => value,
        _ => return None,
    };

    // Now we have tuple<option<list<u8>>, tuple<option<list<u8>>>>
    // Element 0 is the new state, element 1 is the response tuple
    let response_tuple = match inner_tuple {
        Value::Tuple(items) if items.len() >= 2 => &items[1],
        // Fallback for old format: tuple<option<list<u8>>>
        Value::Tuple(items) if !items.is_empty() => {
            return parse_option_bytes(&items[0]);
        }
        _ => return None,
    };

    // response_tuple is tuple<option<list<u8>>>
    // Get element 0 which is the option<list<u8>>
    let response_option = match response_tuple {
        Value::Tuple(items) if !items.is_empty() => &items[0],
        _ => return None,
    };

    parse_option_bytes(response_option)
}

/// Parse an option<list<u8>> Value into Option<Vec<u8>>
fn parse_option_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Option { value: Some(inner), .. } => {
            match inner.as_ref() {
                Value::List { items, .. } => {
                    Some(items.iter().filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    }).collect())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse a channel-accept record from the result Value.
/// The result is tuple<channel-accept> where channel-accept is
/// record { accepted: bool, message: option<list<u8>> }.
/// Records are encoded as Tuples in Pack's Graph ABI.
fn parse_channel_accept(value: &Value) -> bool {
    // Result is tuple<channel-accept>
    let accept_value = match value {
        Value::Tuple(items) if !items.is_empty() => &items[0],
        _ => return false,
    };
    // channel-accept as Tuple: [bool, option<list<u8>>]
    match accept_value {
        Value::Tuple(fields) if !fields.is_empty() => {
            matches!(&fields[0], Value::Bool(true))
        }
        // channel-accept as Record
        Value::Record { fields, .. } => {
            for (name, val) in fields {
                if name == "accepted" {
                    return matches!(val, Value::Bool(true));
                }
            }
            false
        }
        _ => false,
    }
}

// Helper functions for parsing Composite Value inputs (host function params)

fn parse_string(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        Value::Tuple(fields) if fields.len() == 1 => {
            match &fields[0] {
                Value::String(s) => Ok(s.clone()),
                _ => Err(Value::String("Expected string".to_string())),
            }
        }
        _ => Err(Value::String("Expected string".to_string())),
    }
}

fn parse_address_and_message(input: &Value) -> Result<(String, Vec<u8>), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let address = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for address".to_string())),
            };
            let msg = match &fields[1] {
                Value::List { items, .. } => {
                    items.iter().filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    }).collect::<Vec<u8>>()
                }
                _ => return Err(Value::String("Expected list<u8> for message".to_string())),
            };
            Ok((address, msg))
        }
        _ => Err(Value::String("Expected tuple (address, message)".to_string())),
    }
}

fn parse_request_id_and_data(input: &Value) -> Result<(String, Vec<u8>), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let request_id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for request_id".to_string())),
            };
            let data = match &fields[1] {
                Value::List { items, .. } => {
                    items.iter().filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    }).collect::<Vec<u8>>()
                }
                _ => return Err(Value::String("Expected list<u8> for data".to_string())),
            };
            Ok((request_id, data))
        }
        _ => Err(Value::String("Expected tuple (request_id, data)".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_server_handler_creation() {
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

    #[tokio::test]
    async fn test_message_server_handler_clone() {
        let router = MessageRouter::new();
        let handler = MessageServerHandler::new(None, router);

        let cloned = handler.create_instance(None);
        assert_eq!(cloned.name(), "message-server");
    }
}
