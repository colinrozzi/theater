use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

use crate::chain::ChainEvent;
use crate::wasm::{Event, WasmActor};

#[derive(Debug)]
pub enum ActorOperation {
    HandleEvent {
        event: Event,
        response_tx: oneshot::Sender<Result<()>>,
    },
    GetState {
        response_tx: oneshot::Sender<Result<Vec<u8>>>,
    },
    GetChain {
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>>>,
    },
}

pub struct ActorExecutor {
    actor: WasmActor,
    operation_rx: mpsc::Receiver<ActorOperation>,
}

impl ActorExecutor {
    pub fn new(actor: WasmActor, operation_rx: mpsc::Receiver<ActorOperation>) -> Self {
        Self {
            actor,
            operation_rx,
        }
    }

    pub async fn run(&mut self) {
        info!("Actor executor starting");
        
        // Initialize the actor
        self.actor.init().await;
        
        while let Some(op) = self.operation_rx.recv().await {
            debug!("Processing actor operation");
            
            match op {
                ActorOperation::HandleEvent { event, response_tx } => {
                    debug!("Handling event: {:?}", event);
                    let result = self.actor.handle_event(event).await;
                    if let Err(e) = &result {
                        error!("Error handling event: {}", e);
                    }
                    let _ = response_tx.send(result);
                },
                ActorOperation::GetState { response_tx } => {
                    debug!("Getting actor state");
                    let _ = response_tx.send(Ok(self.actor.actor_state.clone()));
                },
                ActorOperation::GetChain { response_tx } => {
                    debug!("Getting actor chain");
                    let chain = self.actor.actor_store.get_chain();
                    let _ = response_tx.send(Ok(chain));
                },
            }
        }
        
        info!("Actor executor shutting down");
    }
}