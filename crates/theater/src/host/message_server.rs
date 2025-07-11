use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::events::{message::MessageEventData, ChainEventData, EventData};
use crate::messages::{
    ActorChannelClose, ActorChannelInitiated, ActorChannelMessage, ActorChannelOpen, ActorMessage,
    ActorRequest, ActorSend, ChannelId, ChannelParticipant, TheaterCommand,
};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::TheaterId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use wasmtime::component::{ComponentType, Lift, Lower};

pub struct MessageServerHost {
    mailbox_tx: Sender<ActorMessage>,
    mailbox_rx: Receiver<ActorMessage>,
    theater_tx: Sender<TheaterCommand>,
    active_channels: HashMap<ChannelId, ChannelState>,
    outstanding_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<Vec<u8>>>>>,
    #[allow(dead_code)]
    permissions: Option<crate::config::permissions::MessageServerPermissions>,
}

#[derive(Clone)]
struct ChannelState {
    is_open: bool,
}

#[derive(Error, Debug)]
pub enum MessageServerError {
    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
struct ChannelAccept {
    accepted: bool,
    message: Option<Vec<u8>>,
}

impl MessageServerHost {
    pub fn new(
        mailbox_tx: Sender<ActorMessage>,
        mailbox_rx: Receiver<ActorMessage>,
        theater_tx: Sender<TheaterCommand>,
        permissions: Option<crate::config::permissions::MessageServerPermissions>,
    ) -> Self {
        Self {
            mailbox_tx,
            mailbox_rx,
            theater_tx,
            active_channels: HashMap::new(),
            outstanding_requests: Arc::new(Mutex::new(HashMap::new())),
            permissions,
        }
    }

    pub async fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Result<()> {
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
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance for message-server-host".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!("Could not instantiate theater:simple/message-server-host: {}", e));
            }
        };

        // Record attempt to setup 'send' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record the message send call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/send".to_string(),
                        data: EventData::Message(MessageEventData::SendMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(), // Could be enhanced to detect type
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
                                    event_type: "theater:simple/message-server-host/send".to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "send".to_string(),
                                        recipient: Some(address.clone()),
                                        message: err_msg.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Error sending message to {}: {}", address, err_msg)),
                                });
                                return Box::new(async move { Ok((Err(err_msg),)) });
                            }
                        },
                        actor_message: ActorMessage::Send(ActorSend {
                            data: msg.clone(),
                        }),
                    };
                    let theater_tx = theater_tx.clone();
                    let address_clone = address.clone();
                    
                    // This line causes an error, there's no outstanding_requests in this scope
                    
                    Box::new(async move {
                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                // Record successful message send result
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send".to_string(),
                                    data: EventData::Message(MessageEventData::SendMessageResult {
                                        recipient: address_clone.clone(),
                                        success: true,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Successfully sent message to {}", address_clone)),
                                });
                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                let err = e.to_string();
                                // Record failed message send result
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send".to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "send".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send message to {}: {}", address_clone, err)),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "send_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'send' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async send function: {}", e)
            })?;

        // Record successful 'send' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "send".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'send' function wrapper".to_string()),
        });

        // Record attempt to setup 'request' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    // Record the message request call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/request".to_string(),
                        data: EventData::Message(MessageEventData::RequestMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(), // Could be enhanced to detect type
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
                                        event_type: "theater:simple/message-server-host/request".to_string(),
                                        data: EventData::Message(MessageEventData::Error {
                                            operation: "request".to_string(),
                                            recipient: Some(address_clone.clone()),
                                            message: err_msg.clone(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Error requesting message from {}: {}", address_clone, err_msg)),
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
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(response) => {
                                        // Record successful message request result
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "theater:simple/message-server-host/request".to_string(),
                                            data: EventData::Message(MessageEventData::RequestMessageResult {
                                                recipient: address_clone.clone(),
                                                data: response.clone(),
                                                success: true,
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Successfully received response from {}", address_clone)),
                                        });
                                        Ok((Ok(response),))
                                    }
                                    Err(e) => {
                                        let err = e.to_string();
                                        // Record failed message request result
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "theater:simple/message-server-host/request".to_string(),
                                            data: EventData::Message(MessageEventData::Error {
                                                operation: "request".to_string(),
                                                recipient: Some(address_clone.clone()),
                                                message: err.clone(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Failed to receive response from {}: {}", address_clone, err)),
                                        });
                                        Ok((Err(err),))
                                    }
                                }
                            }
                            Err(e) => {
                                let err = e.to_string();
                                // Record failed message request result
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/request".to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "request".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to send request to {}: {}", address_clone, err)),
                                });
                                Ok((Err(err),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "request_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'request' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async request function: {}", e)
            })?;

        // Record successful 'request' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'request' function wrapper".to_string()),
        });

        // Record attempt to setup 'list-outstanding-requests' function
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "list-outstanding-requests".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'list-outstanding-requests' function wrapper".to_string()),
        });

        // Use a thread-safe reference to the outstanding_requests field
        let outstanding_requests = self.outstanding_requests.clone();

        interface
            .func_wrap_async(
                "list-outstanding-requests",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      _: ()|
                      -> Box<dyn Future<Output = Result<(Vec<String>,)>> + Send> {
                    // Record the list-outstanding-requests event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/list-outstanding-requests"
                            .to_string(),
                        data: EventData::Message(MessageEventData::ListOutstandingRequestsCall {}),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some("Listing outstanding requests".to_string()),
                    });

                    let outstanding_clone = outstanding_requests.clone();
                    Box::new(async move {
                        // Get the keys from the outstanding_requests map with proper locking
                        let requests = outstanding_clone.lock().unwrap();
                        let ids: Vec<String> = requests.keys().cloned().collect();

                        // Record the list-outstanding-requests result
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
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "list_outstanding_requests_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'list-outstanding-requests' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async list-outstanding-requests function: {}", e)
            })?;

        // Record successful 'list-outstanding-requests' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "list-outstanding-requests".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'list-outstanding-requests' function wrapper".to_string()),
        });

        // Use a thread-safe reference to the outstanding_requests field
        let outstanding_requests = self.outstanding_requests.clone();

        // Record attempt to setup 'respond-to-request' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (request_id, response_data): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    
                    let request_id_clone = request_id.clone();
                    
                    // Record the respond-to-request event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/respond-to-request".to_string(),
                        data: EventData::Message(MessageEventData::RespondToRequestCall {
                            request_id: request_id.clone(),
                            response_size: response_data.len(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Responding to request {}", request_id)),
                    });
                    
                    let outstanding_clone = outstanding_requests.clone();
                    Box::new(async move {
                        // Use mutable access to remove the request with proper locking
                        let mut requests = outstanding_clone.lock().unwrap();
                        if let Some(sender) = requests.remove(&request_id) {
                            match sender.send(response_data) {
                                Ok(_) => {
                                    // Record successful response
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "theater:simple/message-server-host/respond-to-request".to_string(),
                                        data: EventData::Message(MessageEventData::RespondToRequestResult {
                                            request_id: request_id_clone.clone(),
                                            success: true,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Successfully responded to request {}", request_id_clone)),
                                    });
                                    Ok((Ok(()),))
                                },
                                Err(e) => {
                                    let err_msg = format!("Failed to send response: {:?}", e);
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "theater:simple/message-server-host/respond-to-request".to_string(),
                                        data: EventData::Message(MessageEventData::Error {
                                            operation: "respond-to-request".to_string(),
                                            recipient: None,
                                            message: err_msg.clone(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Error responding to request {}: {}", request_id_clone, err_msg)),
                                    });
                                    Ok((Err(err_msg),))
                                }
                            }
                        } else {
                            let err_msg = format!("Request ID not found: {}", request_id);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/respond-to-request".to_string(),
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "respond-to-request".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error responding to request {}: {}", request_id_clone, err_msg)),
                            });
                            Ok((Err(err_msg),))
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "respond_to_request_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'respond-to-request' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async respond-to-request function: {}", e)
            })?;

        // Record successful 'respond-to-request' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "respond-to-request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'respond-to-request' function wrapper".to_string()),
        });

        // Use a thread-safe reference to the outstanding_requests field
        let outstanding_requests = self.outstanding_requests.clone();

        // Record attempt to setup 'cancel-request' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (request_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    
                    let request_id_clone = request_id.clone();
                    
                    // Record the cancel-request event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/cancel-request".to_string(),
                        data: EventData::Message(MessageEventData::CancelRequestCall {
                            request_id: request_id.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Canceling request {}", request_id)),
                    });
                    
                    let outstanding_clone = outstanding_requests.clone();
                    Box::new(async move {
                        // Use mutable access to remove the request with proper locking
                        let mut requests = outstanding_clone.lock().unwrap();
                        if let Some(sender) = requests.remove(&request_id) {
                            // Don't send a response, just drop the sender
                            // This will cause the requester to receive an error
                            drop(sender);
                            
                            // Record successful cancellation
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/cancel-request".to_string(),
                                data: EventData::Message(MessageEventData::CancelRequestResult {
                                    request_id: request_id_clone.clone(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully canceled request {}", request_id_clone)),
                            });
                            Ok((Ok(()),))
                        } else {
                            let err_msg = format!("Request ID not found: {}", request_id);
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/message-server-host/cancel-request".to_string(),
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "cancel-request".to_string(),
                                    recipient: None,
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error canceling request {}: {}", request_id_clone, err_msg)),
                            });
                            Ok((Err(err_msg),))
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "cancel_request_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'cancel-request' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async cancel-request function: {}", e)
            })?;

        // Record successful 'cancel-request' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "cancel-request".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'cancel-request' function wrapper".to_string()),
        });

        let theater_tx = self.theater_tx.clone();
        let mailbox_tx = self.mailbox_tx.clone();

        // Add open-channel function
        // Record attempt to setup 'open-channel' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, initial_msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    // Get the current actor ID
                    let current_actor_id = ctx.data().id.clone();
                    let address_clone = address.clone();
                    
                    // Record the channel open call event
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
                                event_type: "theater:simple/message-server-host/open-channel".to_string(),
                                data: EventData::Message(MessageEventData::Error {
                                    operation: "open-channel".to_string(),
                                    recipient: Some(address_clone.clone()),
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error opening channel to {}: {}", address_clone, err_msg)),
                            });
                            return Box::new(async move { Ok((Err(err_msg),)) });
                        }
                    };
                    
                    // Create a channel ID
                    let channel_id = ChannelId::new(&ChannelParticipant::Actor(current_actor_id.clone()), &target_id);
                    let channel_id_str = channel_id.as_str().to_string();
                    
                    // Create response channel
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    
                    // Create the command
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
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(result) => {
                                        match result {
                                            Ok(accepted) => {
                                                // Record successful channel open result
                                                ctx.data_mut().record_event(ChainEventData {
                                                    event_type: "theater:simple/message-server-host/open-channel".to_string(),
                                                    data: EventData::Message(MessageEventData::OpenChannelResult {
                                                        recipient: address_clone.clone(),
                                                        channel_id: channel_id_clone.clone(),
                                                        accepted,
                                                    }),
                                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                                    description: Some(format!("Channel {} to {} {}", 
                                                        channel_id_clone, 
                                                        address_clone,
                                                        if accepted { "accepted" } else { "rejected" }
                                                    )),
                                                });
                                                
                                                if accepted {
                                                    tokio::spawn(async move {
                                                        // Send the initial message
                                                        if let Err(e) = mailbox_tx.send(ActorMessage::ChannelInitiated(ActorChannelInitiated {
                                                            target_id: target_id.clone(),
                                                            channel_id: channel_id.clone(),
                                                            initial_msg: initial_msg.clone(),
                                                        })).await {
                                                            error!("Failed to send channel initiated message: {}", e);
                                                        }
                                                    });
                                                    Ok((Ok(channel_id_clone),))
                                                } else {
                                                    Ok((Err("Channel request rejected by target actor".to_string()),))
                                                }
                                            },
                                            Err(e) => {
                                                let err_msg = format!("Error opening channel: {}", e);
                                                ctx.data_mut().record_event(ChainEventData {
                                                    event_type: "theater:simple/message-server-host/open-channel".to_string(),
                                                    data: EventData::Message(MessageEventData::Error {
                                                        operation: "open-channel".to_string(),
                                                        recipient: Some(address_clone.clone()),
                                                        message: err_msg.clone(),
                                                    }),
                                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                                    description: Some(format!("Error opening channel to {}: {}", address_clone, err_msg)),
                                                });
                                                Ok((Err(err_msg),))
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        let err_msg = format!("Failed to receive channel open response: {}", e);
                                        ctx.data_mut().record_event(ChainEventData {
                                            event_type: "theater:simple/message-server-host/open-channel".to_string(),
                                            data: EventData::Message(MessageEventData::Error {
                                                operation: "open-channel".to_string(),
                                                recipient: Some(address_clone.clone()),
                                                message: err_msg.clone(),
                                            }),
                                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                            description: Some(format!("Error opening channel to {}: {}", address_clone, err_msg)),
                                        });
                                        Ok((Err(err_msg),))
                                    }
                                }
                            },
                            Err(e) => {
                                let err_msg = format!("Failed to send channel open command: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/open-channel".to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "open-channel".to_string(),
                                        recipient: Some(address_clone.clone()),
                                        message: err_msg.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Error opening channel to {}: {}", address_clone, err_msg)),
                                });
                                Ok((Err(err_msg),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "open_channel_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'open-channel' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async open-channel function: {}", e)
            })?;

        // Record successful 'open-channel' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "open-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'open-channel' function wrapper".to_string()),
        });

        // Add send-on-channel function
        let theater_tx = self.theater_tx.clone();

        // Record attempt to setup 'send-on-channel' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (channel_id, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let channel_id_clone = channel_id.clone();
                    
                    // Get the message string for display, only display the first 100 characters
                    let msg_str = if msg.len() > 100 {
                        format!("{}...", String::from_utf8_lossy(&msg[0..100]))
                    } else {
                        String::from_utf8_lossy(&msg).to_string()
                    };
                    
                    // Record the channel message call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/send-on-channel".to_string(),
                        data: EventData::Message(MessageEventData::ChannelMessageCall {
                            channel_id: channel_id.clone(),
                            msg: msg.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Sending message on channel [{}]: {}", channel_id, msg_str)),
                    });
                    
                    // Parse channel ID
                    let channel_id_parsed = ChannelId(channel_id.clone());
                    
                    // Create the command
                    let command = TheaterCommand::ChannelMessage {
                        channel_id: channel_id_parsed,
                        message: msg.clone(),
                        sender_id: ChannelParticipant::Actor(ctx.data().id.clone()),
                    };
                    
                    let theater_tx = theater_tx.clone();
                    
                    Box::new(async move {
                        match theater_tx.send(command).await {
                            Ok(_) => {
                                // Record successful message send on channel
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel".to_string(),
                                    data: EventData::Message(MessageEventData::ChannelMessageResult {
                                        channel_id: channel_id_clone.clone(),
                                        success: true,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Successfully sent message on channel {}", channel_id_clone)),
                                });
                                Ok((Ok(()),))
                            },
                            Err(e) => {
                                let err_msg = format!("Failed to send message on channel: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/send-on-channel".to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "send-on-channel".to_string(),
                                        recipient: None,
                                        message: err_msg.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Error sending on channel {}: {}", channel_id_clone, err_msg)),
                                });
                                Ok((Err(err_msg),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "send_on_channel_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'send-on-channel' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async send-on-channel function: {}", e)
            })?;

        // Record successful 'send-on-channel' function setup
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupSuccess {
                function_name: "send-on-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Successfully set up 'send-on-channel' function wrapper".to_string()),
        });

        // Add close-channel function
        let theater_tx = self.theater_tx.clone();

        // Record attempt to setup 'close-channel' function
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "message-server-setup".to_string(),
            data: EventData::Message(MessageEventData::FunctionSetupStart {
                function_name: "close-channel".to_string(),
            }),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Setting up 'close-channel' function wrapper".to_string()),
        });

        // Record attempt to setup 'close-channel' function
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
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (channel_id,): (String,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let channel_id_clone = channel_id.clone();
                    
                    // Record the channel close call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/message-server-host/close-channel".to_string(),
                        data: EventData::Message(MessageEventData::CloseChannelCall {
                            channel_id: channel_id.clone(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Closing channel {}", channel_id)),
                    });
                    
                    // Parse channel ID
                    let channel_id_parsed = ChannelId(channel_id.clone());
                    
                    // Create the command
                    let command = TheaterCommand::ChannelClose {
                        channel_id: channel_id_parsed,
                    };
                    
                    let theater_tx = theater_tx.clone();
                    
                    Box::new(async move {
                        match theater_tx.send(command).await {
                            Ok(_) => {
                                // Record successful channel close
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel".to_string(),
                                    data: EventData::Message(MessageEventData::CloseChannelResult {
                                        channel_id: channel_id_clone.clone(),
                                        success: true,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Successfully closed channel {}", channel_id_clone)),
                                });
                                Ok((Ok(()),))
                            },
                            Err(e) => {
                                let err_msg = format!("Failed to close channel: {}", e);
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "theater:simple/message-server-host/close-channel".to_string(),
                                    data: EventData::Message(MessageEventData::Error {
                                        operation: "close-channel".to_string(),
                                        recipient: None,
                                        message: err_msg.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Error closing channel {}: {}", channel_id_clone, err_msg)),
                                });
                                Ok((Err(err_msg),))
                            }
                        }
                    })
                },
            )
            .map_err(|e| {
                // Record the specific function setup error
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "message-server-setup".to_string(),
                    data: EventData::Message(MessageEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "close_channel_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to set up 'close-channel' function wrapper: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap async close-channel function: {}", e)
            })?;

        // Record successful 'close-channel' function setup
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
            description: Some("Message server host functions setup completed successfully".to_string()),
        });

        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance
            .register_function_no_result::<(Vec<u8>,)>(
                "theater:simple/message-server-client",
                "handle-send",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-send function: {}", e))?;
        actor_instance
            .register_function::<(String, Vec<u8>), (Option<Vec<u8>>,)>(
                "theater:simple/message-server-client",
                "handle-request",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-request function: {}", e))?;

        // Register channel functions
        actor_instance
            .register_function::<(String, Vec<u8>), (ChannelAccept,)>(
                "theater:simple/message-server-client",
                "handle-channel-open",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-channel-open function: {}", e))?;
        actor_instance
            .register_function_no_result::<(String, Vec<u8>)>(
                "theater:simple/message-server-client",
                "handle-channel-message",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-channel-message function: {}", e))?;
        actor_instance
            .register_function_no_result::<(String,)>(
                "theater:simple/message-server-client",
                "handle-channel-close",
            )
            .map_err(|e| anyhow::anyhow!("Failed to register handle-channel-close function: {}", e))?;

        Ok(())
    }

    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting message server");
        loop {
            tokio::select! {
                // Monitor shutdown channel
                _ = &mut shutdown_receiver.receiver => {
                    info!("Message server received shutdown signal");
                    debug!("Message server shutting down");
                    break;
                }
                msg = self.mailbox_rx.recv() => {
                    match msg {
                        Some(message) => {
                            let _ = self.process_message(message, actor_handle.clone()).await;
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
    }

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
                let response = actor_handle
                    .call_function::<(String, Vec<u8>), (Option<Vec<u8>>,)>(
                        "theater:simple/message-server-client.handle-request".to_string(),
                        (request_id.clone(), data),
                    )
                    .await?;
                info!(
                    "Got request response: id={}, data size={}",
                    request_id,
                    response.0.as_ref().map_or(0, |v| v.len())
                );
                if response.0.is_some() {
                    let _ = response_tx.send(response.0.unwrap());
                } else {
                    self.outstanding_requests
                        .lock()
                        .unwrap()
                        .insert(request_id, response_tx);
                }
            }
            ActorMessage::ChannelOpen(ActorChannelOpen {
                channel_id,
                response_tx,
                data,
            }) => {
                info!(
                    "Got channel open request: channel={:?}, data size={}",
                    channel_id,
                    data.len()
                );

                let result = actor_handle
                    .call_function::<(String, Vec<u8>), (ChannelAccept,)>(
                        "theater:simple/message-server-client.handle-channel-open".to_string(),
                        (channel_id.to_string(), data),
                    )
                    .await?;

                let accepted = result.0.accepted;

                if accepted {
                    // Store channel in active channels
                    self.active_channels
                        .insert(channel_id.clone(), ChannelState { is_open: true });
                }

                let _ = response_tx.send(Ok(accepted));
            }
            ActorMessage::ChannelMessage(ActorChannelMessage { channel_id, data }) => {
                // Find the channel
                if let Some(channel) = self.active_channels.get(&channel_id) {
                    if channel.is_open {
                        info!(
                            "Got channel message: channel={:?}, data size={}",
                            channel_id,
                            data.len()
                        );

                        actor_handle
                            .call_function::<(String, Vec<u8>), ()>(
                                "theater:simple/message-server-client.handle-channel-message"
                                    .to_string(),
                                (channel_id.to_string(), data),
                            )
                            .await?;
                    } else {
                        warn!("Received message for closed channel: {}", channel_id);
                    }
                } else {
                    warn!("Received message for unknown channel: {}", channel_id);
                }
            }
            ActorMessage::ChannelClose(ActorChannelClose { channel_id }) => {
                info!("Got channel close: channel={:?}", channel_id);

                // Find and close the channel
                if let Some(channel) = self.active_channels.get_mut(&channel_id) {
                    channel.is_open = false;

                    actor_handle
                        .call_function::<(String,), ()>(
                            "theater:simple/message-server-client.handle-channel-close".to_string(),
                            (channel_id.to_string(),),
                        )
                        .await?;
                } else {
                    warn!("Received close for unknown channel: {}", channel_id);
                }
            }
            ActorMessage::ChannelInitiated(ActorChannelInitiated {
                channel_id,
                target_id,
                initial_msg,
            }) => {
                info!(
                    "Channel initiated: channel={:?}, target={:?}, initial_msg size={}",
                    channel_id,
                    target_id,
                    initial_msg.len()
                );

                // Store channel in active channels
                self.active_channels
                    .insert(channel_id.clone(), ChannelState { is_open: true });
            }
        }
        Ok(())
    }
}
