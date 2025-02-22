use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::MessageServerConfig;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, ActorRequest, ActorSend, TheaterCommand};
use crate::wasm::Event;
use anyhow::Result;
use serde::{Deserialize, Serialize};
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

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for message-server-host");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports for message-server-client");
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting message server host");
        while let Some(msg) = self.mailbox_rx.recv().await {
            if let Err(e) = self.process_message(msg).await {
                error!("Error processing message: {}", e);
            }
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

