//! # Actor Handle
//!
//! This module provides the `ActorHandle` type, which serves as the primary interface
//! for interacting with actors in the Theater system.

use anyhow::Result;
use wasmtime::component::{ComponentNamedList, ComponentType, Lift, Lower};

use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::error;

use crate::actor::types::{ActorError, ActorOperation, WasiHttpResponse, DEFAULT_OPERATION_TIMEOUT};
use crate::chain::ChainEvent;
use crate::metrics::ActorMetrics;

use super::types::{ActorControl, ActorInfo};

/// # ActorHandle
///
/// A handle to an actor in the Theater system, providing methods to interact with the actor.
///
/// ## Purpose
///
/// ActorHandle provides a high-level interface for communicating with actors, managing their
/// lifecycle, and accessing their state and events. It encapsulates the details of message
/// passing and synchronization between the caller and the actor's execution environment.
#[derive(Clone, Debug)]
pub struct ActorHandle {
    operation_tx: mpsc::Sender<ActorOperation>,
    info_tx: mpsc::Sender<ActorInfo>,
    control_tx: mpsc::Sender<ActorControl>,
}

impl ActorHandle {
    /// Creates a new ActorHandle with the given operation channel.
    ///
    /// ## Parameters
    ///
    /// * `operation_tx` - The sender side of a channel used to send operations to the actor.
    ///
    /// ## Returns
    ///
    /// A new ActorHandle instance.
    pub fn new(
        operation_tx: mpsc::Sender<ActorOperation>,
        info_tx: mpsc::Sender<ActorInfo>,
        control_tx: mpsc::Sender<ActorControl>,
    ) -> Self {
        Self {
            operation_tx,
            info_tx,
            control_tx,
        }
    }

    /// Calls a function on the actor with the given name and parameters.
    ///
    /// ## Purpose
    ///
    /// This method allows calling exported functions on the WebAssembly actor with
    /// type-safe parameters and return values.
    ///
    /// ## Parameters
    ///
    /// * `name` - The name of the function to call on the actor.
    /// * `params` - The parameters to pass to the function, must be compatible with the
    ///   function's signature and serializable.
    ///
    /// ## Returns
    ///
    /// * `Ok(R)` - The return value from the function call, deserialized to the expected type.
    /// * `Err(ActorError)` - An error occurred during the function call.
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
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }

    /// Call a function on the actor without expecting a return value.
    ///
    /// This is useful for Composite runtime where results are in Graph ABI format,
    /// not JSON. The function call succeeds if no error is returned.
    pub async fn call_function_void(&self, name: String, params: Vec<u8>) -> Result<(), ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::CallFunction {
                name: name.clone(),
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
                Ok(inner_result) => match inner_result {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        error!("Function call '{}' failed: {:?}", name, e);
                        Err(e)
                    }
                },
                Err(_) => {
                    error!("Channel closed while waiting for function call '{}'", name);
                    Err(ActorError::ChannelClosed)
                }
            },
            Err(_) => {
                error!("Operation timed out after {:?}", DEFAULT_OPERATION_TIMEOUT);
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }

    /// Handle a WASI HTTP incoming request.
    ///
    /// ## Purpose
    ///
    /// This method handles an incoming HTTP request by creating WASI HTTP resources
    /// in the actor's store and calling the actor's exported `wasi:http/incoming-handler.handle`
    /// function.
    ///
    /// ## Parameters
    ///
    /// * `method` - HTTP method (GET, POST, etc.)
    /// * `scheme` - URL scheme (http, https, etc.)
    /// * `authority` - Authority component (host:port)
    /// * `path_with_query` - Path with optional query string
    /// * `headers` - Request headers as (name, value) pairs
    /// * `body` - Request body bytes
    ///
    /// ## Returns
    ///
    /// * `Ok(WasiHttpResponse)` - The HTTP response from the actor
    /// * `Err(ActorError)` - An error occurred during request handling
    pub async fn handle_wasi_http_request(
        &self,
        method: String,
        scheme: Option<String>,
        authority: Option<String>,
        path_with_query: Option<String>,
        headers: Vec<(String, Vec<u8>)>,
        body: Vec<u8>,
    ) -> Result<WasiHttpResponse, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.operation_tx
            .send(ActorOperation::HandleWasiHttpRequest {
                method,
                scheme,
                authority,
                path_with_query,
                headers,
                body,
                response_tx: tx,
            })
            .await
            .map_err(|e| {
                error!("Failed to send HandleWasiHttpRequest operation: {}", e);
                ActorError::ChannelClosed
            })?;

        match timeout(DEFAULT_OPERATION_TIMEOUT, rx).await {
            Ok(result) => match result {
                Ok(result) => result,
                Err(e) => {
                    error!("Channel closed while waiting for HTTP response: {:?}", e);
                    Err(ActorError::ChannelClosed)
                }
            },
            Err(_) => {
                error!(
                    "HTTP request timed out after {:?}",
                    DEFAULT_OPERATION_TIMEOUT
                );
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }

    /// Retrieves the current state of the actor.
    ///
    /// ## Purpose
    ///
    /// This method allows access to the actor's current state, which is useful for
    /// inspecting the actor's internal data or for backup purposes.
    ///
    /// ## Returns
    ///
    /// * `Ok(Some(Vec<u8>))` - The actor's current state as a byte array, if it has state.
    /// * `Ok(None)` - The actor does not have any state.
    /// * `Err(ActorError)` - An error occurred while retrieving the state.
    pub async fn get_state(&self) -> Result<Option<Vec<u8>>, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.info_tx
            .send(ActorInfo::GetState { response_tx: tx })
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
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }

    /// Retrieves the event chain for the actor.
    ///
    /// ## Purpose
    ///
    /// This method returns the history of state changes for the actor,
    /// which is useful for auditing, debugging, or reconstructing the actor's state evolution.
    ///
    /// ## Returns
    ///
    /// * `Ok(Vec<ChainEvent>)` - The event chain containing the history of state changes.
    /// * `Err(ActorError)` - An error occurred while retrieving the chain.
    pub async fn get_chain(&self) -> Result<Vec<ChainEvent>, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.info_tx
            .send(ActorInfo::GetChain { response_tx: tx })
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
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }

    /// Retrieves performance metrics for the actor.
    ///
    /// ## Purpose
    ///
    /// This method provides access to performance metrics for the actor, which is useful
    /// for monitoring, debugging, and performance analysis.
    ///
    /// ## Returns
    ///
    /// * `Ok(ActorMetrics)` - The current metrics for the actor.
    /// * `Err(ActorError)` - An error occurred while retrieving the metrics.
    pub async fn get_metrics(&self) -> Result<ActorMetrics, ActorError> {
        let (tx, rx) = oneshot::channel();

        self.info_tx
            .send(ActorInfo::GetMetrics { response_tx: tx })
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
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }

    /// Initiates an orderly shutdown of the actor.
    ///
    /// ## Purpose
    ///
    /// This method requests that the actor shut down gracefully, allowing it to
    /// complete any in-progress operations and perform any necessary cleanup.
    ///
    /// ## Returns
    ///
    /// * `Ok(())` - The actor was successfully shut down.
    /// * `Err(ActorError)` - An error occurred during the shutdown process.
    pub async fn shutdown(&self) -> Result<(), ActorError> {
        let (tx, rx) = oneshot::channel();

        self.control_tx
            .send(ActorControl::Shutdown { response_tx: tx })
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
                Err(ActorError::OperationTimeout(
                    DEFAULT_OPERATION_TIMEOUT.as_secs(),
                ))
            }
        }
    }
}
