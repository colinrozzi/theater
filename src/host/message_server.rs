use crate::actor_handle::ActorHandle;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, ActorRequest, ActorSend, TheaterCommand};
use crate::wasm::Json;
use crate::wasm::{ActorState, WasmActor};
use crate::ActorStore;
use crate::host::host_wrapper::HostFunctionBoundary;
use anyhow::Result;
use std::future::Future;
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;

pub struct MessageServerHost {
    mailbox_rx: Receiver<ActorMessage>,
    theater_tx: Sender<TheaterCommand>,
    actor_handle: ActorHandle,
}

#[derive(Error, Debug)]
pub enum MessageServerError {
    #[error("Calling WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },
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

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for message-server-host");
        let mut actor = self.actor_handle.inner().lock().await;
        let mut interface = actor
            .linker
            .instance("ntwk:theater/message-server-host")
            .expect("could not instantiate ntwk:theater/message-server-host");

        let theater_tx = self.theater_tx.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/message-server-host", "send");

        interface
            .func_wrap_async(
                "send",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let theater_tx = theater_tx.clone();
                    let boundary = boundary.clone();
                    let msg_clone = msg.clone();
                    
                    Box::new(async move {
                        // Record the outbound message
                        if let Err(e) = boundary.wrap(&mut ctx, (address.clone(), msg_clone), |_| Ok(())) {
                            return Ok((Err(e.to_string()),));
                        }

                        let actor_message = TheaterCommand::SendMessage {
                            actor_id: TheaterId::parse(&address).expect("Failed to parse actor ID"),
                            actor_message: ActorMessage::Send(ActorSend { data: msg }),
                        };

                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                match boundary.wrap(&mut ctx, "success", |_| Ok(())) {
                                    Ok(_) => Ok((Ok(()),)),
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

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports for message-server-client");
        let _ = self
            .actor_handle
            .with_actor_mut(|actor: &mut WasmActor| -> Result<()> {
                let handle_export = actor
                    .find_export("ntwk:theater/message-server-client", "handle-send")
                    .expect("Failed to find handle-send export");
                actor
                    .exports
                    .insert("handle-send".to_string(), handle_export);
                let handle_export = actor
                    .find_export("ntwk:theater/message-server-client", "handle-request")
                    .expect("Failed to find handle-request export");
                actor
                    .exports
                    .insert("handle-request".to_string(), handle_export);
                Ok(())
            })
            .await;
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting message server host");
        while let Some(msg) = self.mailbox_rx.recv().await {
            self.process_message(msg).await
        }
        Ok(())
    }

    async fn process_message(&self, msg: ActorMessage) -> () {
        match msg {
            ActorMessage::Send(ActorSend { data }) => {
                self.handle_send(data).await;
            }
            ActorMessage::Request(ActorRequest { response_tx, data }) => {
                self.handle_request(data, response_tx).await;
            }
        }
    }

    async fn handle_send(&self, data: Vec<u8>) -> () {
        let boundary = HostFunctionBoundary::new("ntwk:theater/message-server-client", "handle-send");
        let mut actor = self.actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();

        match actor
            .call_func::<(Json, ActorState), (ActorState,)>(
                "handle-send",
                (Json::from(data), actor_state),
            )
            .await
        {
            Ok((new_state,)) => {
                actor.actor_state = new_state;
            }
            Err(e) => info!("Error processing message: {}", e),
        }
    }

    async fn handle_request(
        &self,
        data: Vec<u8>,
        response_tx: tokio::sync::oneshot::Sender<Vec<u8>>,
    ) -> () {
        let boundary = HostFunctionBoundary::new("ntwk:theater/message-server-client", "handle-request");
        let mut actor = self.actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();

        match actor
            .call_func::<(Json, ActorState), ((Json, ActorState),)>(
                "handle-request",
                (Json::from(data), actor_state),
            )
            .await
        {
            Ok(((resp, new_state),)) => {
                actor.actor_state = new_state;
                let _ = response_tx.send(resp.into());
            }
            Err(e) => info!("Error processing message: {}", e),
        }
    }
}