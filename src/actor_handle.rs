use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use crate::actor_executor::ActorOperation;
use crate::chain::ChainEvent;
use crate::wasm::Event;

#[derive(Clone)]
pub struct ActorHandle {
    operation_tx: mpsc::Sender<ActorOperation>,
}

impl ActorHandle {
    pub fn new(operation_tx: mpsc::Sender<ActorOperation>) -> Self {
        Self { operation_tx }
    }

    pub async fn handle_event(&self, event: Event) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.operation_tx
            .send(ActorOperation::HandleEvent {
                event,
                response_tx: tx,
            })
            .await?;
        rx.await?
    }

    pub async fn get_state(&self) -> Result<Vec<u8>> {
        let (tx, rx) = oneshot::channel();
        self.operation_tx
            .send(ActorOperation::GetState { response_tx: tx })
            .await?;
        rx.await?
    }

    pub async fn get_chain(&self) -> Result<Vec<ChainEvent>> {
        let (tx, rx) = oneshot::channel();
        self.operation_tx
            .send(ActorOperation::GetChain { response_tx: tx })
            .await?;
        rx.await?
    }
}