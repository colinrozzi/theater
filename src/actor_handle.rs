use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::error;

use crate::actor_executor::{ActorError, ActorOperation, DEFAULT_OPERATION_TIMEOUT};
use crate::chain::ChainEvent;
use crate::metrics::ActorMetrics;

#[derive(Clone)]
pub struct ActorHandle {
    operation_tx: mpsc::Sender<ActorOperation>,
}

impl ActorHandle {
    pub fn new(operation_tx: mpsc::Sender<ActorOperation>) -> Self {
        Self { operation_tx }
    }

    pub async fn call_function(
        &self,
        name: String,
        params: Vec<u8>,
    ) -> Result<Vec<u8>, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::CallFunction {
                name,
                params,
                response_tx: tx,
            })
            .await
            .map_err(|e| {
                error!("Failed to send operation: {}", e);
                ActorError::ChannelClosed
            })?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|e| {
                error!("Channel closed while waiting for response: {:?}", e);
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!("Operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            },
        }
    }

    pub async fn get_state(&self) -> Result<Option<Vec<u8>>, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::GetState { response_tx: tx })
            .await
            .map_err(|e| {
                error!("Failed to send GetState operation: {}", e);
                ActorError::ChannelClosed
            })?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|e| {
                error!("Channel closed while waiting for GetState response: {:?}", e);
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!("GetState operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            },
        }
    }

    pub async fn get_chain(&self) -> Result<Vec<ChainEvent>, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::GetChain { response_tx: tx })
            .await
            .map_err(|e| {
                error!("Failed to send GetChain operation: {}", e);
                ActorError::ChannelClosed
            })?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|e| {
                error!("Channel closed while waiting for GetChain response: {:?}", e);
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!("GetChain operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            },
        }
    }

    pub async fn get_metrics(&self) -> Result<ActorMetrics, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::GetMetrics { response_tx: tx })
            .await
            .map_err(|e| {
                error!("Failed to send GetMetrics operation: {}", e);
                ActorError::ChannelClosed
            })?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|e| {
                error!("Channel closed while waiting for GetMetrics response: {:?}", e);
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!("GetMetrics operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            },
        }
    }

    pub async fn shutdown(&self) -> Result<(), ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::Shutdown { response_tx: tx })
            .await
            .map_err(|e| {
                error!("Failed to send Shutdown operation: {}", e);
                ActorError::ChannelClosed
            })?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|e| {
                error!("Channel closed while waiting for Shutdown response: {:?}", e);
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!("Shutdown operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            },
        }
    }
}
