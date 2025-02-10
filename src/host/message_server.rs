use crate::actor_handle::ActorHandle;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, ActorRequest, ActorSend, TheaterCommand};
use crate::wasm::Json;
use crate::wasm::{ActorState, WasmActor};
use crate::ActorStore;
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
        interface
            .func_wrap_async(
                "send",
                move |_ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    // make a channel that will carry the byte array of the resposne
                    info!("Sending message to actor: {}", address);
                    let actor_message = TheaterCommand::SendMessage {
                        actor_id: TheaterId::parse(&address).expect("Failed to parse actor ID"),
                        actor_message: ActorMessage::Send(ActorSend {
                            data: msg,
                        }),
                    };
                    let theater_tx = theater_tx.clone();
                    Box::new(async move {
                        match theater_tx.send(actor_message).await {
                            Ok(_) => Ok((Ok(()),)),
                            Err(e) => Ok((Err(e.to_string()),)),
                        }
                    })
                },
            )
            .expect("Failed to wrap async send function");

        let theater_tx = self.theater_tx.clone();
        interface
            .func_wrap_async(
                "request",
                move |_ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (address, msg): (String, Vec<u8>)|
                      -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                    // make a channel that will carry the byte array of the resposne
                    info!("Sending message to actor: {}", address);
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let actor_message = TheaterCommand::SendMessage {
                        actor_id: TheaterId::parse(&address).expect("Failed to parse actor ID"),
                        actor_message: ActorMessage::Request(ActorRequest {
                            data: msg,
                            response_tx,
                        }),
                    };
                    let theater_tx = theater_tx.clone();
                    Box::new(async move {
                        match theater_tx.send(actor_message).await {
                            Ok(_) => {
                                // wait for the response from the actor
                                match response_rx.await {
                                    Ok(response) => Ok((Ok(response),)),
                                    Err(e) => Ok((Err(e.to_string()),)),
                                }
                            },
                            Err(e) => Ok((Err(e.to_string()),)),
                    }})
                },
            )
            .expect("Failed to wrap async send function");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports for message-server-client");
        let _ = self
            .actor_handle
            .with_actor_mut(|actor: &mut WasmActor| -> Result<()> {
                let handle_export = actor
                    .find_export("ntwk:theater/message-server-client", "handle")
                    .expect("Failed to find handle export");
                actor.exports.insert("handle".to_string(), handle_export);
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
        let mut actor = self.actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();
        match actor
            .call_func::<(Json, ActorState), ((ActorState,),)>(
                "handle-send",
                (Json::from(data), actor_state),
            )
            .await
        {
            Ok(((new_state,),)) => {
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
