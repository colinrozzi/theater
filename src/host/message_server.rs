use crate::actor_handle::ActorHandle;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
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
                      -> Box<dyn Future<Output = Result<(Vec<u8>,)>> + Send> {
                    // make a channel that will carry the byte array of the resposne
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let actor_message = TheaterCommand::SendMessage {
                        actor_id: TheaterId::parse(&address).expect("Failed to parse actor ID"),
                        actor_message: ActorMessage {
                            data: msg,
                            response_tx,
                        },
                    };
                    let theater_tx = theater_tx.clone();
                    Box::new(async move {
                        theater_tx.send(actor_message).await.map_err(|e| {
                            MessageServerError::WasmError {
                                context: "send",
                                message: e.to_string(),
                            }
                        })?;
                        // wait for the response from the actor
                        let response =
                            response_rx
                                .await
                                .map_err(|e| MessageServerError::WasmError {
                                    context: "send",
                                    message: e.to_string(),
                                })?;
                        Ok((response,))
                    })
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
        let mut actor = self.actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();
        match actor
            .call_func::<(Json, ActorState), (Json, ActorState)>("handle", (msg.data, actor_state))
            .await
        {
            Ok((resp, new_state)) => {
                actor.actor_state = new_state;
                let _ = msg.response_tx.send(resp);
            }
            Err(e) => info!("Error processing message: {}", e),
        }
    }
}
