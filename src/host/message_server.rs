use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::actor_store::ActorStore;
use crate::events::{ChainEventData, EventData, message::MessageEventData};
use crate::messages::{ActorMessage, ActorRequest, ActorSend, TheaterCommand};
use crate::wasm::{ActorComponent, ActorInstance};
use crate::TheaterId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, info};

pub struct MessageServerHost {
    mailbox_rx: Receiver<ActorMessage>,
    theater_tx: Sender<TheaterCommand>,
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

#[derive(Debug, Serialize, Deserialize)]
struct MessageEvent {
    message_type: String,
    data: Vec<u8>,
}

impl MessageServerHost {
    pub fn new(mailbox_rx: Receiver<ActorMessage>, theater_tx: Sender<TheaterCommand>) -> Self {
        Self {
            mailbox_rx,
            theater_tx,
        }
    }

     pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up message server host functions");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/message-server-host")
            .expect("Could not instantiate ntwk:theater/message-server-host");

        let theater_tx = self.theater_tx.clone();

        interface
            .func_wrap_async(
                "send",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // Record the message send call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/message-server-host/send".to_string(),
                        data: EventData::Message(MessageEventData::SendMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(), // Could be enhanced to detect type
                            size: msg.len(),
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
                                    event_type: "ntwk:theater/message-server-host/send".to_string(),
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
                    
                    Box::new(async move {
                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                // Record successful message send result
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/message-server-host/send".to_string(),
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
                                    event_type: "ntwk:theater/message-server-host/send".to_string(),
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
            .expect("Failed to wrap async send function");

        let theater_tx = self.theater_tx.clone();

        interface
            .func_wrap_async(
                "request",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    // Record the message request call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/message-server-host/request".to_string(),
                        data: EventData::Message(MessageEventData::RequestMessageCall {
                            recipient: address.clone(),
                            message_type: "binary".to_string(), // Could be enhanced to detect type
                            size: msg.len(),
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
                                        event_type: "ntwk:theater/message-server-host/request".to_string(),
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
                                            event_type: "ntwk:theater/message-server-host/request".to_string(),
                                            data: EventData::Message(MessageEventData::RequestMessageResult {
                                                recipient: address_clone.clone(),
                                                response_size: response.len(),
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
                                            event_type: "ntwk:theater/message-server-host/request".to_string(),
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
                                    event_type: "ntwk:theater/message-server-host/request".to_string(),
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
            .expect("Failed to wrap async request function");

        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance
            .register_function_no_result::<(Vec<u8>,)>(
                "ntwk:theater/message-server-client",
                "handle-send",
            )
            .expect("Failed to register handle-send function");
        actor_instance
            .register_function::<(Vec<u8>,), (Vec<u8>,)>(
                "ntwk:theater/message-server-client",
                "handle-request",
            )
            .expect("Failed to register handle-request function");
        Ok(())
    }

    pub async fn start(&mut self, actor_handle: ActorHandle) -> Result<()> {
        info!("Starting message server");
        while let Some(msg) = self.mailbox_rx.recv().await {
            let _ = self.process_message(msg, actor_handle.clone()).await;
        }
        Ok(())
    }

    async fn process_message(
        &self,
        msg: ActorMessage,
        actor_handle: ActorHandle,
    ) -> Result<(), MessageServerError> {
        match msg {
            ActorMessage::Send(ActorSend { data }) => {
                actor_handle
                    .call_function::<(Vec<u8>,), ()>(
                        "ntwk:theater/message-server-client.handle-send".to_string(),
                        (data,),
                    )
                    .await?;
            }
            ActorMessage::Request(ActorRequest { response_tx, data }) => {
                info!("Got request: {:?}", data);
                let response = actor_handle
                    .call_function::<(Vec<u8>,), (Vec<u8>,)>(
                        "ntwk:theater/message-server-client.handle-request".to_string(),
                        (data,),
                    )
                    .await?;
                info!("Got response: {:?}", response);
                let _ = response_tx.send(response.0);
            }
        }
        Ok(())
    }
}

