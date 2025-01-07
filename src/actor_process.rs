use crate::state::ActorState;
use crate::wasm::{Event, WasmActor};
use crate::Result;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct ActorProcess {
    mailbox_rx: mpsc::Receiver<ProcessMessage>,
    state: ActorState,
    actor: WasmActor,
    #[allow(dead_code)]
    name: String,
}

#[derive(Debug)]
pub struct ActorMessage {
    pub event: Event,
    pub response_channel: Option<mpsc::Sender<Event>>,
}

#[derive(Debug)]
pub enum ProcessMessage {
    ActorMessage(ActorMessage),
    ProcessCommand(ProcessCommand),
}

#[derive(Debug)]
pub enum ProcessCommand {
    StopActor {
        response_tx: mpsc::Sender<Result<()>>,
    },
    CurrentState {
        response_tx: mpsc::Sender<Result<serde_json::Value>>,
    },
}

impl ActorProcess {
    pub async fn new(
        name: &String,
        actor: WasmActor,
        mailbox_rx: mpsc::Receiver<ProcessMessage>,
    ) -> Result<Self> {
        // Initialize actor state
        let initial_state = actor.init().await?;
        let state = ActorState::new(initial_state.clone());

        Ok(Self {
            mailbox_rx,
            state,
            actor,
            name: name.to_string(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Some(process_message) = self.mailbox_rx.recv().await {
            match process_message {
                ProcessMessage::ActorMessage(msg) => {
                    info!("Processing message with event type: {}", msg.event.type_);

                    let current_state = self.state.get_state();
                    match self
                        .actor
                        .handle_event(current_state.clone(), msg.event.clone())
                        .await
                    {
                        Ok((new_state, response_event)) => {
                            self.state.update_state(new_state.clone());

                            if let Some(response_channel) = msg.response_channel {
                                info!("Response channel found, sending response event");
                                let a = response_channel.send(response_event).await;
                                info!("Response sent: {:?}", a);
                            }
                        }
                        Err(e) => {
                            error!("Failed to handle event: {:?}", e);
                        }
                    }
                }
                ProcessMessage::ProcessCommand(cmd) => match cmd {
                    ProcessCommand::StopActor { response_tx } => {
                        info!("Stopping actor");
                        let _ = response_tx.send(Ok(()));
                        break;
                    }
                    ProcessCommand::CurrentState { response_tx } => {
                        let state = self.state.get_state();
                        let _ = response_tx.send(Ok(json!(state)));
                    }
                },
            }
        }
        Ok(())
    }
}
