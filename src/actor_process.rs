use crate::actor_runtime::ChainRequest;
use crate::actor_runtime::ChainRequestType;
use crate::actor_runtime::ChainResponse;
use crate::chain::ChainEntry;
use crate::chain::HashChain;
use crate::state::ActorState;
use crate::wasm::{Event, WasmActor};
use crate::Result;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct ActorProcess {
    mailbox_rx: mpsc::Receiver<ActorMessage>,
    chain_tx: mpsc::Sender<ChainRequest>,
    state: ActorState,
    actor: WasmActor,
    #[allow(dead_code)]
    name: String,
}

pub struct ActorMessage {
    pub event: Event,
    pub response_channel: Option<mpsc::Sender<Event>>,
}

impl ActorProcess {
    pub async fn new(
        name: &String,
        actor: WasmActor,
        mailbox_rx: mpsc::Receiver<ActorMessage>,
        chain_tx: mpsc::Sender<ChainRequest>,
    ) -> Result<Self> {
        let mut chain = HashChain::new();

        // Initialize actor state
        let initial_state = actor.init().await?;
        let state = ActorState::new(initial_state.clone());

        // Record initialization in chain
        chain.add_event("init".to_string(), json!(initial_state));

        Ok(Self {
            mailbox_rx,
            chain_tx,
            state,
            actor,
            name: name.to_string(),
        })
    }

    pub async fn add_event(&mut self, event: Event) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.chain_tx
            .send(ChainRequest {
                request_type: ChainRequestType::AddEvent { event },
                response_tx: tx,
            })
            .await
            .expect("Failed to record message in chain");
        rx.await?;

        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Some(msg) = self.mailbox_rx.recv().await {
            info!("Processing message with event type: {}", msg.event.type_);

            match self.add_event(msg.event.clone()).await {
                Ok(_) => info!("Event recorded in chain"),
                Err(e) => {
                    error!("Failed to record event in chain: {:?}", e);
                }
            }

            let current_state = self.state.get_state();
            match self
                .actor
                .handle_event(current_state.clone(), msg.event.clone())
                .await
            {
                Ok((new_state, response_event)) => {
                    self.add_event(Event {
                        type_: "state".to_string(),
                        data: json!(new_state),
                    })
                    .await?;

                    self.state.update_state(new_state.clone());

                    if let Some(response_channel) = msg.response_channel {
                        self.add_event(response_event.clone()).await?;
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

        Ok(())
    }

    pub async fn get_chain(&self) -> Vec<(String, ChainEntry)> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.chain_tx
            .send(ChainRequest {
                request_type: ChainRequestType::GetChain,
                response_tx: tx,
            })
            .await
            .expect("Failed to get chain");
        match rx.await.expect("Failed to get chain") {
            ChainResponse::FullChain(chain) => chain,
            _ => panic!("Failed to get chain"),
        }
    }
}

