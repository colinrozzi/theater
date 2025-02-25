use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::actor_executor::FunctionResponse;
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

    // Generic typed function call that handles serialization
    pub async fn call_function<P, R>(
        &self,
        name: String,
        params: P,
    ) -> Result<(R, Sender<Option<Option<Vec<u8>>>>), ActorError>
    where
        P: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        // Serialize the params to send through the channel
        let serialized_params =
            serde_json::to_vec(&params).map_err(|e| ActorError::Internal(e.into()))?;

        // Call the raw function
        let function_result = self.call_function_raw(name, serialized_params).await?;

        // Deserialize the result
        let result: R = serde_json::from_slice(&function_result.result.expect("missing result"))
            .map_err(|e| ActorError::Internal(e.into()))?;

        Ok((result, function_result.state_update_tx))
    }

    // Base implementation that works with raw bytes
    pub async fn call_function_raw(
        &self,
        name: String,
        params: Vec<u8>,
    ) -> Result<FunctionResponse, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::CallFunction {
                name,
                params,
                response_tx: tx,
            })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|_| ActorError::ChannelClosed)?,
            Err(_) => return Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT)),
        }
    }

    pub async fn get_state(&self) -> Result<Option<Vec<u8>>, ActorError> {
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

    pub async fn get_metrics(&self) -> Result<ActorMetrics, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::GetMetrics { response_tx: tx })
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
