use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::actor_executor::{ActorOperation, ActorError, DEFAULT_OPERATION_TIMEOUT};
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

    pub async fn handle_event(&self, event: Event) -> Result<(), ActorError> {
        let (tx, rx) = oneshot::channel();
        
        self.operation_tx
            .send(ActorOperation::HandleEvent {
                event,
                response_tx: tx,
            })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|_| ActorError::ChannelClosed)?,
            Err(_) => Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT)),
        }
    }

    pub async fn get_state(&self) -> Result<Vec<u8>, ActorError> {
        let (tx, rx) = oneshot::channel();
        
        self.operation_tx
            .send(ActorOperation::GetState { response_tx: tx })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|_| ActorError::ChannelClosed)?,
            Err(_) => Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT)),
        }
    }

    pub async fn get_chain(&self) -> Result<Vec<ChainEvent>, ActorError> {
        let (tx, rx) = oneshot::channel();
        
        self.operation_tx
            .send(ActorOperation::GetChain { response_tx: tx })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|_| ActorError::ChannelClosed)?,
            Err(_) => Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT)),
        }
    }

    pub async fn shutdown(&self) -> Result<(), ActorError> {
        let (tx, rx) = oneshot::channel();
        
        self.operation_tx
            .send(ActorOperation::Shutdown { response_tx: tx })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|_| ActorError::ChannelClosed)?,
            Err(_) => Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT)),
        }
    }
}