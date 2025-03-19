use anyhow::Result;
use wasmtime::component::{ComponentNamedList, ComponentType, Lift, Lower};

use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::{error, info};

use crate::actor_executor::{ActorError, ActorOperation, DEFAULT_OPERATION_TIMEOUT};
use crate::chain::ChainEvent;
use crate::events::ChainEventData;
use crate::messages::TheaterCommand;
use crate::metrics::ActorMetrics;
use crate::TheaterId;

#[derive(Clone)]
pub struct ActorHandle {
    operation_tx: mpsc::Sender<ActorOperation>,
    actor_id: TheaterId,
    theater_tx: mpsc::Sender<TheaterCommand>,
}

impl ActorHandle {
    pub fn new(operation_tx: mpsc::Sender<ActorOperation>, actor_id: TheaterId, theater_tx: mpsc::Sender<TheaterCommand>) -> Self {
        Self { operation_tx, actor_id, theater_tx }
    }
    
    pub fn actor_id(&self) -> &TheaterId {
        &self.actor_id
    }
    
    pub fn record_event(&self, event: ChainEventData) {
        let event = ChainEvent {
            data: event,
            // These fields will be filled in by the actor runtime
            hash: 0,
            parent_hash: None,
        };
        
        // Send the event to the theater runtime
        let theater_tx = self.theater_tx.clone();
        let actor_id = self.actor_id.clone();
        tokio::spawn(async move {
            if let Err(e) = theater_tx.send(TheaterCommand::NewEvent { actor_id, event }).await {
                error!("Failed to send event to theater runtime: {}", e);
            }
        });
    }

    pub async fn call_function<P, R>(&self, name: String, params: P) -> Result<R, ActorError>
    where
        P: ComponentType + Lower + ComponentNamedList + Send + Sync + 'static + serde::Serialize,
        R: ComponentType
            + Lift
            + ComponentNamedList
            + Send
            + Sync
            + 'static
            + serde::de::DeserializeOwned,
    {
        let (tx, rx) = oneshot::channel();

        let params = serde_json::to_vec(&params).map_err(|e| {
            error!("Failed to serialize params: {}", e);
            ActorError::SerializationError
        })?;

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
            Ok(result) => match result {
                Ok(result) => {
                    let res = serde_json::from_slice::<R>(&result.unwrap()).map_err(|e| {
                        error!("Failed to deserialize response: {}", e);
                        ActorError::SerializationError
                    })?;
                    Ok(res)
                }
                Err(e) => {
                    error!("Channel closed while waiting for response: {:?}", e);
                    return Err(ActorError::ChannelClosed);
                }
            },
            Err(_) => {
                error!("Operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            }
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
                error!(
                    "Channel closed while waiting for GetState response: {:?}",
                    e
                );
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!(
                    "GetState operation timed out after {:?}",
                    DEFAULT_OPERATION_TIMEOUT
                );
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            }
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
                error!(
                    "Channel closed while waiting for GetChain response: {:?}",
                    e
                );
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!(
                    "GetChain operation timed out after {:?}",
                    DEFAULT_OPERATION_TIMEOUT
                );
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            }
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
                error!(
                    "Channel closed while waiting for GetMetrics response: {:?}",
                    e
                );
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!(
                    "GetMetrics operation timed out after {:?}",
                    DEFAULT_OPERATION_TIMEOUT
                );
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            }
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
                error!(
                    "Channel closed while waiting for Shutdown response: {:?}",
                    e
                );
                ActorError::ChannelClosed
            })?,
            Err(_) => {
                error!(
                    "Shutdown operation timed out after {:?}",
                    DEFAULT_OPERATION_TIMEOUT
                );
                Err(ActorError::OperationTimeout(DEFAULT_OPERATION_TIMEOUT))
            }
        }
    }
}
