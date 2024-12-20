use crate::actor::Actor;
use crate::actor::Event;
use crate::actor_runtime::ChainRequest;
use crate::actor_runtime::ChainRequestType;
use crate::actor_runtime::ChainResponse;
use crate::chain::ChainEntry;
use crate::chain::HashChain;
use crate::state::ActorState;
use crate::Result;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::info;

pub struct ActorProcess {
    mailbox_rx: mpsc::Receiver<ActorMessage>,
    chain_tx: mpsc::Sender<ChainRequest>,
    state: ActorState,
    actor: Box<dyn Actor>,
    #[allow(dead_code)]
    name: String,
}

pub struct ActorMessage {
    pub event: Event,
    pub response_channel: Option<mpsc::Sender<Event>>,
}

impl ActorProcess {
    pub fn new(
        name: &String,
        actor: Box<dyn Actor>,
        mailbox_rx: mpsc::Receiver<ActorMessage>,
        chain_tx: mpsc::Sender<ChainRequest>,
    ) -> Result<Self> {
        let mut chain = HashChain::new();

        // Initialize actor state
        let initial_state = actor.init()?;
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
            let evt = msg.event;

            info!("Recording event in chain");
            // Record the event in chain
            self.add_event(evt.clone()).await?;

            // Process input using current state
            let current_state = self.state.get_state();
            let (new_state, response_event) = self
                .actor
                .handle_event(current_state.clone(), evt.clone())?;

            self.add_event(Event {
                type_: "state".to_string(),
                data: json!(new_state),
            })
            .await?;

            self.add_event(response_event.clone()).await?;

            self.state.update_state(new_state.clone());

            // Send response if metadata contains response channel
            if let Some(response_channel) = msg.response_channel {
                info!("Response channel found, sending response event");
                let a = response_channel.send(response_event).await;
                info!("Response sent: {:?}", a);
            }
        }

        Ok(())
    }

    pub async fn get_chain(&self) -> Vec<(String, ChainEntry)> {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        self.chain_tx
            .send(ChainRequest {
                request_type: ChainRequestType::GetChain,
                response_tx: tx,
            })
            .await
            .expect("Failed to get chain");
        let chain_response = rx.try_recv().expect("Failed to get chain");
        if let ChainResponse::FullChain(chain) = chain_response {
            chain
        } else {
            panic!("Failed to get chain");
        }
    }
}
