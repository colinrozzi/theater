//! Theater Message Server Handler
//!
//! Provides actor-to-actor messaging capabilities including:
//! - One-way send messages
//! - Request-response patterns
//! - Bidirectional channels

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::{ActorError, WitActorError};
use theater::config::permissions::MessageServerPermissions;
use theater::events::message::MessageEventData;
use theater::events::{ChainEventData, EventData};
use theater::handler::Handler;
use theater::messages::{
    ActorChannelClose, ActorChannelInitiated, ActorChannelMessage, ActorChannelOpen,
    ActorLifecycleEvent, ActorMessage, ActorRequest, ActorSend, ChannelId, ChannelParticipant,
    MessageCommand,
};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};
use theater::TheaterId;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};
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
#[derive(Clone)]
struct ChannelState {
    is_open: bool,
}

/// Registry entry for each actor managed by the message-server
struct ActorRegistryEntry {
    actor_id: TheaterId,
    mailbox_tx: Sender<ActorMessage>,
    actor_handle: ActorHandle,
    channels: HashSet<ChannelId>,
}

/// The MessageServerHandler provides actor-to-actor communication with complete separation from the runtime.
///
/// Architecture:
/// - Receives lifecycle events from runtime (ActorSpawned, ActorStopped)
/// - Maintains its own actor registry
/// - Creates and consumes mailboxes for all actors
/// - Routes messages via MessageCommand
///
/// Enables actors to:
/// - Send one-way messages
/// - Make request-response calls
/// - Open bidirectional channels
/// - Manage outstanding requests
#[derive(Clone)]
pub struct MessageServerHandler {
    // Lifecycle event channel (receives notifications from runtime)
    lifecycle_rx: Arc<Mutex<Option<Receiver<ActorLifecycleEvent>>>>,

    // Message command channel (receives routing requests from host functions)
    message_command_tx: Sender<MessageCommand>,
    message_command_rx: Arc<Mutex<Option<Receiver<MessageCommand>>>>,

    // Actor registry (message-server owns this)
    actor_registry: Arc<RwLock<HashMap<TheaterId, ActorRegistryEntry>>>,

    // Channel state tracking
    active_channels: Arc<Mutex<HashMap<ChannelId, ChannelState>>>,

    // Request-response tracking
    outstanding_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<Vec<u8>>>>>,

    #[allow(dead_code)]
    permissions: Option<MessageServerPermissions>,
}

impl MessageServerHandler {
    /// Create a new MessageServerHandler
    ///
    /// Returns (handler, lifecycle_tx, message_tx) tuple:
    /// - handler: The MessageServerHandler instance
    /// - lifecycle_tx: Channel for runtime to send lifecycle events
    /// - message_tx: Channel for host functions to send message commands
    ///
    /// # Arguments
    /// * `permissions` - Optional permission restrictions
    pub fn new(
        permissions: Option<MessageServerPermissions>,
    ) -> (Self, Sender<ActorLifecycleEvent>, Sender<MessageCommand>) {
        let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(100);
        let (message_command_tx, message_command_rx) = tokio::sync::mpsc::channel(1000);

        let handler = Self {
            lifecycle_rx: Arc::new(Mutex::new(Some(lifecycle_rx))),
            message_command_tx: message_command_tx.clone(),
            message_command_rx: Arc::new(Mutex::new(Some(message_command_rx))),
            actor_registry: Arc::new(RwLock::new(HashMap::new())),
            active_channels: Arc::new(Mutex::new(HashMap::new())),
            outstanding_requests: Arc::new(Mutex::new(HashMap::new())),
            permissions,
        };

        (handler, lifecycle_tx, message_command_tx)
    }

    /// Process incoming actor messages
    async fn process_message(
        &mut self,
        msg: ActorMessage,
        actor_handle: ActorHandle,
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
                info!("Got request: id={}, data size={}", request_id, data.len());

                // Store the response sender
                {
                    let mut requests = self.outstanding_requests.lock().unwrap();
                    requests.insert(request_id.clone(), response_tx);
                }

                // Call the actor's request handler
                let response = actor_handle
                    .call_function::<(String, Vec<u8>), (Option<Vec<u8>>,)>(
                        "theater:simple/message-server-client.handle-request".to_string(),
                        (request_id.clone(), data),
                    )
                    .await?;

                // If the actor returned a response immediately, send it
                if let Some(response_data) = response.0 {
                    let mut requests = self.outstanding_requests.lock().unwrap();
                    if let Some(tx) = requests.remove(&request_id) {
                        let _ = tx.send(response_data);
                    }
                }
            }
            ActorMessage::ChannelOpen(ActorChannelOpen {
                initiator_id,
                channel_id,
                initial_msg,
                response_tx,
            }) => {
                info!("Received channel open request: channel_id={}", channel_id);

                // Call the actor's channel open handler
                let response = actor_handle
                    .call_function::<(String, Vec<u8>), (ChannelAccept,)>(
                        "theater:simple/message-server-client.handle-channel-open".to_string(),
                        (initiator_id.to_string(), initial_msg),
                    )
                    .await?;

                let channel_accept = response.0;

                if channel_accept.accepted {
                    // Track the channel as open
                    let mut channels = self.active_channels.lock().unwrap();
                    channels.insert(
                        channel_id.clone(),
                        ChannelState { is_open: true },
                    );
                }

                // Send the response back
                let _ = response_tx.send(Ok(channel_accept.accepted));

                // If accepted and there's an initial response message, send it
                if channel_accept.accepted && channel_accept.message.is_some() {
                    // The initial response will be handled by the channel flow
                }
            }
            ActorMessage::ChannelMessage(ActorChannelMessage { channel_id, msg }) => {
                info!("Received channel message: channel_id={}", channel_id);
                actor_handle
                    .call_function::<(String, Vec<u8>), ()>(
                        "theater:simple/message-server-client.handle-channel-message".to_string(),
                        (channel_id.as_str().to_string(), msg),
                    )
                    .await?;
            }
            ActorMessage::ChannelClose(ActorChannelClose { channel_id }) => {
                info!("Received channel close: channel_id={}", channel_id);

                // Mark channel as closed (drop lock before await)
                {
                    let mut channels = self.active_channels.lock().unwrap();
                    if let Some(state) = channels.get_mut(&channel_id) {
                        state.is_open = false;
                    }
                }

                actor_handle
                    .call_function::<(String,), ()>(
                        "theater:simple/message-server-client.handle-channel-close".to_string(),
                        (channel_id.as_str().to_string(),),
                    )
                    .await?;
            }
            ActorMessage::ChannelInitiated(ActorChannelInitiated {
                target_id: _,
                channel_id,
                initial_msg: _,
            }) => {
                // Track the channel as open (from initiator side)
                let mut channels = self.active_channels.lock().unwrap();
                channels.insert(channel_id.clone(), ChannelState { is_open: true });
            }
        }
        Ok(())
    }
}

impl Handler for MessageServerHandler {
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn name(&self) -> &str {
        "message-server"
    }

    fn imports(&self) -> Option<String> {
        Some("theater:simple/message-server-host".to_string())
    }

    fn exports(&self) -> Option<String> {
        Some("theater:simple/message-server-client".to_string())
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up message server host functions");

        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::HandlerSetupStart),
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
                    data: EventData::Message(MessageEventData::LinkerInstanceSuccess),
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "send".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'send' function wrapper".to_string()),
        });

        let theater_tx = self.theater_tx.clone();

        interface
            .func_wrap_async(
                "send",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/send".to_string(),
                        data: EventData::Message(MessageEventData::SendMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(),
                            data: msg.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Sending message to {}", address)),
                    });

                    info!("Sending message to actor: {}", address);
                    let actor_message = TheaterCommand::SendMessage {
                        actor_id: match TheaterId::parse(&address) {
                            Ok(id) => id,
                            Err(e) => {
                                let err_msg = format!("Failed to parse actor ID: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "send".to_string(),
                                        recipient: Some(address.clone()),
                                        message: err_msg.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Error sending message to {}: {}",
                                        address, err_msg
                                    )),
                                });
                                return Box::new(async move { Ok((Err(err_msg),)) });
                            }
                        },
                        actor_message: ActorMessage::Send(ActorSend { data: msg.clone() }),
                    };
                    let theater_tx = theater_tx.clone();
                    let address_clone = address.clone();

                    Box::new(async move {
                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::SendMessageResult {
                                        recipient: address_clone.clone(),
                                        success: true,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Successfully sent message to {}",
                                        address_clone
                                    )),
                                });
                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "send".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send message to {}: {}",
                                        address_clone, err
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "send_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "send".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'send' function wrapper".to_string()),
        });

        // 2. request operation
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'request' function wrapper".to_string()),
        });

        let theater_tx = self.theater_tx.clone();

        interface
            .func_wrap_async(
                "request",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/request".to_string(),
                        data: EventData::Message(MessageEventData::RequestMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(),
                            data: msg.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Requesting message from {}", address)),
                    });

                    let theater_tx = theater_tx.clone();
                    let address_clone = address.clone();

                    Box::new(async move {
                        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                        let actor_message = TheaterCommand::SendMessage {
                            actor_id: match TheaterId::parse(&address) {
                                Ok(id) => id,
                                Err(e) => {
                                    let err_msg = format!("Failed to parse actor ID: {}", e);
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "theater:simple/message-server-host/request"
                                            .to_string(),
                                        data: EventData::Message(MessageEventData::Error {
                                            operation: "request".to_string(),
                                            recipient: Some(address_clone.clone()),
                                            message: err_msg.clone(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Error requesting message from {}: {}",
                                            address_clone, err_msg
                                        )),
                                    });
                                    return Ok((Err(err_msg),));
                                }
                            },
                            actor_message: ActorMessage::Request(ActorRequest {
                                data: msg,
                                response_tx,
                            }),
                        };

                        match theater_tx.send(actor_message).await {
                            Ok(_) => match response_rx.await {
                                Ok(response) => {
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "theater:simple/message-server-host/request"
                                            .to_string(),
                                        data: EventData::Message(
                                            MessageEventData::RequestMessageResult {
                                                recipient: address_clone.clone(),
                                                data: response.clone(),
                                                success: true,
                                            },
                                        ),
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
                                        data: EventData::Message(MessageEventData::Error {
                                            operation: "request".to_string(),
                                            recipient: Some(address_clone.clone()),
                                            message: err.clone(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Failed to receive response from {}: {}",
                                            address_clone, err
                                        )),
                                    });
                                    Ok((Err(err),))
                                }
                            },
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/request"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "request".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send request to {}: {}",
                                        address_clone, err
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "request_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'request' function wrapper".to_string()),
        });

        // 3. list-outstanding-requests operation
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "list-outstanding-requests".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Setting up 'list-outstanding-requests' function wrapper".to_string(),
            ),
        });

        let outstanding_requests = self.outstanding_requests.clone();

        interface
            .func_wrap_async(
                "list-outstanding-requests",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      _: ()|
                      -> Box<dyn Future<Output = Result<(Vec<String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/list-outstanding-requests"
                            .to_string(),
                        data: EventData::Message(MessageEventData::ListOutstandingRequestsCall {}),
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
                            data: EventData::Message(
                                MessageEventData::ListOutstandingRequestsResult {
                                    request_count: ids.len(),
                                    request_ids: ids.clone(),
                                },
                            ),
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "list_outstanding_requests_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "list-outstanding-requests".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Successfully set up 'list-outstanding-requests' function wrapper".to_string(),
            ),
        });

        // 4. respond-to-request operation
        let outstanding_requests = self.outstanding_requests.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "respond-to-request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'respond-to-request' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "respond-to-request",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (request_id, response_data): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let request_id_clone = request_id.clone();

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/respond-to-request"
                            .to_string(),
                        data: EventData::Message(MessageEventData::RespondToRequestCall {
                            request_id: request_id.clone(),
                            response_size: response_data.len(),
                        }),
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
                                        data: EventData::Message(
                                            MessageEventData::RespondToRequestResult {
                                                request_id: request_id_clone.clone(),
                                                success: true,
                                            },
                                        ),
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
                                        data: EventData::Message(MessageEventData::Error {
                                            operation: "respond-to-request".to_string(),
                                            recipient: None,
                                            message: err_msg.clone(),
                                        }),
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
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "respond-to-request".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }),
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "respond_to_request_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "respond-to-request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Successfully set up 'respond-to-request' function wrapper".to_string(),
            ),
        });

        // 5. cancel-request operation
        let outstanding_requests = self.outstanding_requests.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "cancel-request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'cancel-request' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "cancel-request",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (request_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let request_id_clone = request_id.clone();

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/cancel-request"
                            .to_string(),
                        data: EventData::Message(MessageEventData::CancelRequestCall {
                            request_id: request_id.clone(),
                        }),
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
                                data: EventData::Message(MessageEventData::CancelRequestResult {
                                    request_id: request_id_clone.clone(),
                                    success: true,
                                }),
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
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "cancel-request".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }),
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "cancel_request_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "cancel-request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'cancel-request' function wrapper".to_string()),
        });

        // 6. open-channel operation
        let theater_tx = self.theater_tx.clone();
        let mailbox_tx = self.mailbox_tx.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "open-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'open-channel' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "open-channel",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (address, initial_msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    let current_actor_id = ctx.data().id.clone();
                    let address_clone = address.clone();

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/open-channel".to_string(),
                        data: EventData::Message(MessageEventData::OpenChannelCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(),
                            size: initial_msg.len(),
                        }),
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
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "open-channel".to_string(),
                                    recipient: Some(address_clone.clone()),
                                    message: err_msg.clone(),
                                }),
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

                    let command = TheaterCommand::ChannelOpen {
                        initiator_id: ChannelParticipant::Actor(current_actor_id.clone()),
                        target_id: target_id.clone(),
                        channel_id: channel_id.clone(),
                        initial_message: initial_msg.clone(),
                        response_tx,
                    };

                    let theater_tx = theater_tx.clone();
                    let channel_id_clone = channel_id_str.clone();
                    let mailbox_tx = mailbox_tx.clone();

                    Box::new(async move {
                        match theater_tx.send(command).await {
                            Ok(_) => match response_rx.await {
                                Ok(result) => match result {
                                    Ok(accepted) => {
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type:
                                                "theater:simple/message-server-host/open-channel"
                                                    .to_string(),
                                            data: EventData::Message(
                                                MessageEventData::OpenChannelResult {
                                                    recipient: address_clone.clone(),
                                                    channel_id: channel_id_clone.clone(),
                                                    accepted,
                                                },
                                            ),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!(
                                                "Channel {} to {} {}",
                                                channel_id_clone,
                                                address_clone,
                                                if accepted { "accepted" } else { "rejected" }
                                            )),
                                        });

                                        if accepted {
                                            tokio::spawn(async move {
                                                if let Err(e) = mailbox_tx
                                                    .send(ActorMessage::ChannelInitiated(
                                                        ActorChannelInitiated {
                                                            target_id: target_id.clone(),
                                                            channel_id: channel_id.clone(),
                                                            initial_msg: initial_msg.clone(),
                                                        },
                                                    ))
                                                    .await
                                                {
                                                    error!(
                                                        "Failed to send channel initiated message: {}",
                                                        e
                                                    );
                                                }
                                            });
                                            Ok((Ok(channel_id_clone),))
                                        } else {
                                            Ok((Err(
                                                "Channel request rejected by target actor"
                                                    .to_string(),
                                            ),))
                                        }
                                    }
                                    Err(e) => {
                                        let err_msg = format!("Error opening channel: {}", e);
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type:
                                                "theater:simple/message-server-host/open-channel"
                                                    .to_string(),
                                            data: EventData::Message(MessageEventData::Error {
                                                operation: "open-channel".to_string(),
                                                recipient: Some(address_clone.clone()),
                                                message: err_msg.clone(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!(
                                                "Error opening channel to {}: {}",
                                                address_clone, err_msg
                                            )),
                                        });
                                        Ok((Err(err_msg),))
                                    }
                                },
                                Err(e) => {
                                    let err_msg = format!("Failed to receive channel open response: {}", e);
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "theater:simple/message-server-host/open-channel"
                                            .to_string(),
                                        data: EventData::Message(MessageEventData::Error {
                                            operation: "open-channel".to_string(),
                                            recipient: Some(address_clone.clone()),
                                            message: err_msg.clone(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!(
                                            "Error opening channel to {}: {}",
                                            address_clone, err_msg
                                        )),
                                    });
                                    Ok((Err(err_msg),))
                                }
                            },
                            Err(e) => {
                                let err_msg = format!("Failed to send channel open command: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/open-channel"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "open-channel".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err_msg.clone(),
                                    }),
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "open_channel_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "open-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'open-channel' function wrapper".to_string()),
        });

        // 7. send-on-channel operation
        let theater_tx = self.theater_tx.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "send-on-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'send-on-channel' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "send-on-channel",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (channel_id_str, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/send-on-channel"
                            .to_string(),
                        data: EventData::Message(MessageEventData::ChannelMessageCall {
                            channel_id: channel_id_str.clone(),
                            msg: msg.clone(),
                        }),
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
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "send-on-channel".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error sending on channel {}: {}",
                                    channel_id_str, err_msg
                                )),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };

                    let command = TheaterCommand::ChannelMessage {
                        channel_id: channel_id.clone(),
                        message: msg.clone(),
                    };

                    let theater_tx = theater_tx.clone();
                    let channel_id_clone = channel_id_str.clone();

                    Box::new(async move {
                        match theater_tx.send(command).await {
                            Ok(_) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel"
                                        .to_string(),
                                    data: EventData::Message(
                                        MessageEventData::ChannelMessageResult {
                                            channel_id: channel_id_clone.clone(),
                                            success: true,
                                        },
                                    ),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Successfully sent message on channel {}",
                                        channel_id_clone
                                    )),
                                });
                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "send-on-channel".to_string(),
                                        recipient: None,
                                        message: err.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to send message on channel {}: {}",
                                        channel_id_clone, err
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "send_on_channel_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "send-on-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Successfully set up 'send-on-channel' function wrapper".to_string(),
            ),
        });

        // 8. close-channel operation
        let theater_tx = self.theater_tx.clone();

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "close-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'close-channel' function wrapper".to_string()),
        });

        interface
            .func_wrap_async(
                "close-channel",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (channel_id_str,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/close-channel".to_string(),
                        data: EventData::Message(MessageEventData::CloseChannelCall {
                            channel_id: channel_id_str.clone(),
                        }),
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
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "close-channel".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error closing channel {}: {}",
                                    channel_id_str, err_msg
                                )),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };

                    let command = TheaterCommand::ChannelClose {
                        channel_id: channel_id.clone(),
                    };

                    let theater_tx = theater_tx.clone();
                    let channel_id_clone = channel_id_str.clone();

                    Box::new(async move {
                        match theater_tx.send(command).await {
                            Ok(_) => {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel"
                                        .to_string(),
                                    data: EventData::Message(
                                        MessageEventData::CloseChannelResult {
                                            channel_id: channel_id_clone.clone(),
                                            success: true,
                                        },
                                    ),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Successfully closed channel {}",
                                        channel_id_clone
                                    )),
                                });
                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel"
                                        .to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "close-channel".to_string(),
                                        recipient: None,
                                        message: err.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to close channel {}: {}",
                                        channel_id_clone, err
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
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "close_channel_function_wrap".to_string(),
                    }),
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
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "close-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'close-channel' function wrapper".to_string()),
        });

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some(
                "Message server host functions setup completed successfully".to_string(),
            ),
        });

        info!("Message server host functions added");

        Ok(())
    }

    fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
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
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Starting message server");

        // Take the receiver out of the Option
        let mailbox_rx_opt = self.mailbox_rx.lock().unwrap().take();

        // Clone state for the async block
        let mut handler_clone = self.clone();

        Box::pin(async move {
            // If we don't have a receiver (cloned instance), just return
            let Some(mut mailbox_rx) = mailbox_rx_opt else {
                info!("Message server has no receiver (cloned instance), not starting");
                return Ok(());
            };

            loop {
                tokio::select! {
                    _ = &mut shutdown_receiver.receiver => {
                        info!("Message server received shutdown signal");
                        debug!("Message server shutting down");
                        break;
                    }
                    msg = mailbox_rx.recv() => {
                        match msg {
                            Some(message) => {
                                if let Err(e) = handler_clone.process_message(message, actor_handle.clone()).await {
                                    error!("Error processing message: {}", e);
                                }
                            }
                            None => {
                                info!("Message channel closed, shutting down");
                                break;
                            }
                        }
                    }
                }
            }
            info!("Message server shutdown complete");
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_server_handler_creation() {
        let (mailbox_tx, mailbox_rx) = tokio::sync::mpsc::channel(100);
        let (theater_tx, _theater_rx) = tokio::sync::mpsc::channel(100);
        let handler = MessageServerHandler::new(mailbox_tx, mailbox_rx, theater_tx, None);

        assert_eq!(handler.name(), "message-server");
        assert_eq!(
            handler.imports(),
            Some("theater:simple/message-server-host".to_string())
        );
        assert_eq!(
            handler.exports(),
            Some("theater:simple/message-server-client".to_string())
        );
    }

    #[test]
    fn test_message_server_handler_clone() {
        let (mailbox_tx, mailbox_rx) = tokio::sync::mpsc::channel(100);
        let (theater_tx, _theater_rx) = tokio::sync::mpsc::channel(100);
        let handler = MessageServerHandler::new(mailbox_tx, mailbox_rx, theater_tx, None);

        let cloned = handler.create_instance();
        assert_eq!(cloned.name(), "message-server");
    }
}
