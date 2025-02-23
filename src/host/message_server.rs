use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::actor_runtime::WrappedActor;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::messages::{ActorMessage, ActorRequest, ActorSend, TheaterCommand};
use crate::store::ActorStore;
use crate::wasm::Event;
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
    actor_handle: ActorHandle,
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
    pub fn new(
        mailbox_rx: Receiver<ActorMessage>,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
    ) -> Self {
        Self {
            mailbox_rx,
            theater_tx,
            actor_handle,
        }
    }

    pub async fn setup_host_functions(&self, wrapped_actor: WrappedActor) -> Result<()> {
        info!("Setting up message server host functions");

        let mut actor = wrapped_actor.inner().lock().unwrap();
        let mut interface = actor
            .linker
            .instance("ntwk:theater/message-server-host")
            .expect("Could not instantiate ntwk:theater/message-server-host");

        let boundary = HostFunctionBoundary::new("ntwk:theater/message-server-host", "send");
        let theater_tx = self.theater_tx.clone();

        interface
            .func_wrap_async(
                "send",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // make a channel that will carry the byte array of the resposne
                    info!("Sending message to actor: {}", address);
                    let actor_message = TheaterCommand::SendMessage {
                        actor_id: TheaterId::parse(&address).expect("Failed to parse actor ID"),
                        actor_message: ActorMessage::Send(ActorSend {
                            data: msg.clone(),
                        }),
                    };
                    let theater_tx = theater_tx.clone();
                    let boundary = boundary.clone();
                    Box::new(async move {
                        if let Err(e) = boundary.wrap(&mut ctx, (address.clone(), msg.clone()), |_| Ok(())) {
                            return Ok((Err(e.to_string()),));
                        }

                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                match boundary.wrap(&mut ctx, (address.clone(), "success"), |_| Ok(())) {
                                    Ok(_) => Ok((Ok(()),)),
                                    Err(e) => Ok((Err(e.to_string()),)),
                                }
                            }
                            Err(e) => {
                                let err = e.to_string();
                                match boundary.wrap(&mut ctx, (address.clone(), err.clone()), |_| Ok(())) {
                                    Ok(_) => Ok((Err(err),)),
                                    Err(e) => Ok((Err(e.to_string()),)),
                                }
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap async send function");

        let theater_tx = self.theater_tx.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/message-server-host", "request");

        interface
            .func_wrap_async(
                "request",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    let theater_tx = theater_tx.clone();
                    let boundary = boundary.clone();
                    let msg_clone = msg.clone();

                    Box::new(async move {
                        // Record the outbound request
                        if let Err(e) = boundary.wrap(&mut ctx, (address.clone(), msg_clone), |_| Ok(())) {
                            return Ok((Err(e.to_string()),));
                        }

                        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                        let actor_message = TheaterCommand::SendMessage {
                            actor_id: TheaterId::parse(&address).expect("Failed to parse actor ID"),
                            actor_message: ActorMessage::Request(ActorRequest {
                                data: msg,
                                response_tx,
                            }),
                        };

                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                match response_rx.await {
                                    Ok(response) => {
                                        match boundary.wrap(&mut ctx, response.clone(), |_| Ok(())) {
                                            Ok(_) => Ok((Ok(response),)),
                                            Err(e) => Ok((Err(e.to_string()),))
                                        }
                                    }
                                    Err(e) => {
                                        let err = e.to_string();
                                        match boundary.wrap(&mut ctx, err.clone(), |_| Ok(())) {
                                            Ok(_) => Ok((Err(err),)),
                                            Err(e) => Ok((Err(e.to_string()),))
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let err = e.to_string();
                                match boundary.wrap(&mut ctx, err.clone(), |_| Ok(())) {
                                    Ok(_) => Ok((Err(err),)),
                                    Err(e) => Ok((Err(e.to_string()),))
                                }
                            }
                        }
                    })
                },
            )
            .expect("Failed to wrap async request function");

        Ok(())
    }

    pub async fn add_exports(&self, wrapped_actor: WrappedActor) -> Result<()> {
        info!("Adding exports to message-server");
        let mut actor = wrapped_actor.inner().lock().unwrap();
        let handle_send = actor
            .find_export("ntwk:theater/message-server-client", "handle-send")
            .expect("Failed to find export ntwk:theater/message-server-client/handle-send");
        actor.exports.insert("handle-send".to_string(), handle_send);
        let handle_request = actor
            .find_export("ntwk:theater/message-server-client", "handle-request")
            .expect("Failed to find export ntwk:theater/message-server-client/handle-request");
        actor
            .exports
            .insert("handle-request".to_string(), handle_request);
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting message server");
        while let Some(msg) = self.mailbox_rx.recv().await {
            let _ = self.process_message(msg).await;
        }
        Ok(())
    }

    async fn process_message(&self, msg: ActorMessage) -> Result<(), MessageServerError> {
        match msg {
            ActorMessage::Send(ActorSend { data }) => {
                let event = Event {
                    event_type: "handle-send".to_string(),
                    parent: None,
                    data,
                };

                self.actor_handle.handle_event(event).await?;
            }
            ActorMessage::Request(ActorRequest { response_tx, data }) => {
                let event = Event {
                    event_type: "handle-request".to_string(),
                    parent: None,
                    data,
                };

                self.actor_handle.handle_event(event).await?;

                // Get the response from the state
                let response = self.actor_handle.get_state().await?;
                let _ = response_tx.send(response);
            }
        }
        Ok(())
    }
}
