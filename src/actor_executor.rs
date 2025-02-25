use anyhow::Result;
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info};

use crate::metrics::{ActorMetrics, MetricsCollector};
use crate::wasm::ActorInstance;
use crate::ChainEvent;

pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const METRICS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Error, Debug)]
pub enum ActorError {
    #[error("Operation timed out after {0:?}")]
    OperationTimeout(Duration),

    #[error("Operation channel closed")]
    ChannelClosed,

    #[error("Actor is shutting down")]
    ShuttingDown,

    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    #[error("Type mismatch for function {0}")]
    TypeMismatch(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

// Different types of operations the executor can handle
pub enum ActorOperation {
    CallFunction {
        name: String,
        params: Vec<u8>,
        response_tx: oneshot::Sender<Result<FunctionResponse, ActorError>>,
    },
    GetMetrics {
        response_tx: oneshot::Sender<Result<ActorMetrics, ActorError>>,
    },
    Shutdown {
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
    GetChain {
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>, ActorError>>,
    },
    GetState {
        response_tx: oneshot::Sender<Result<Option<Vec<u8>>, ActorError>>,
    },
}

pub struct FunctionResponse {
    pub result: Result<Vec<u8>, ActorError>,
    pub state_update_tx: mpsc::Sender<Option<Option<Vec<u8>>>>,
}

pub struct ActorExecutor {
    actor_instance: ActorInstance,
    operation_rx: mpsc::Receiver<ActorOperation>,
    metrics: MetricsCollector,
    shutdown_initiated: bool,
}

impl ActorExecutor {
    pub fn new(
        actor_instance: ActorInstance,
        operation_rx: mpsc::Receiver<ActorOperation>,
    ) -> Self {
        Self {
            actor_instance,
            operation_rx,
            metrics: MetricsCollector::new(),
            shutdown_initiated: false,
        }
    }

    // Execute a function call with serialized params/results
    async fn execute_call(&mut self, name: String, params: Vec<u8>) -> Result<Vec<u8>, ActorError> {
        // Validate the function exists
        if !self.actor_instance.has_function(&name) {
            return Err(ActorError::FunctionNotFound(name));
        }

        let start = Instant::now();

        // Execute the call
        let results = self
            .actor_instance
            .call_function(&name, params)
            .await
            .map_err(ActorError::Internal)?;

        // Record metrics
        let duration = start.elapsed();
        self.metrics.record_operation(duration, true).await;

        Ok(results)
    }

    // Call a function on the actor instance
    async fn call_function(
        &mut self,
        name: String,
        params: Vec<u8>,
        response_tx: oneshot::Sender<Result<FunctionResponse, ActorError>>,
    ) {
        let result = self.execute_call(name, params).await;

        // Send the result back
        let (state_update_tx, mut state_update_rx) = mpsc::channel(1);
        let response = FunctionResponse {
            result,
            state_update_tx,
        };
        let _ = response_tx.send(Ok(response));

        // Wait for state updates
        let state_update = state_update_rx.recv().await;
        if let Some(state) = state_update {
            self.actor_instance
                .store
                .data_mut()
                .set_state(state.unwrap());
        }
    }

    pub async fn run(&mut self) {
        info!("Actor executor starting");

        loop {
            tokio::select! {
                Some(op) = self.operation_rx.recv() => {
                    debug!("Processing actor operation");

                    if self.shutdown_initiated {
                        match op {
                            ActorOperation::Shutdown { response_tx } => {
                                let _ = response_tx.send(Ok(()));
                                break;
                            }
                            ActorOperation::CallFunction { response_tx, .. } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::GetMetrics { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::GetChain { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                            ActorOperation::GetState { response_tx } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                        }
                        continue;
                    }

                    match op {
                        ActorOperation::CallFunction { name, params, response_tx } => {
                            self.call_function(name, params, response_tx).await;
                        }

                        ActorOperation::GetMetrics { response_tx } => {
                            let metrics = self.metrics.get_metrics().await;
                            if let Err(e) = response_tx.send(Ok(metrics)) {
                                error!("Failed to send metrics: {:?}", e);
                            }
                        }

                        ActorOperation::GetChain { response_tx } => {
                            let chain = self.actor_instance.store.data().get_chain();
                            if let Err(e) = response_tx.send(Ok(chain)) {
                                error!("Failed to send chain: {:?}", e);
                            }
                        }

                        ActorOperation::GetState { response_tx } => {
                            let state = self.actor_instance.store.data().get_state();
                            if let Err(e) = response_tx.send(Ok(state)) {
                                error!("Failed to send state: {:?}", e);
                            }
                        }


                        ActorOperation::Shutdown { response_tx } => {
                            info!("Processing shutdown request");
                            self.shutdown_initiated = true;
                            let _ = response_tx.send(Ok(()));
                            break;
                        }
                    }
                }

                else => {
                    info!("Operation channel closed, shutting down");
                    break;
                }
            }
        }

        info!("Actor executor shutting down");
        self.cleanup().await;
    }

    async fn cleanup(&mut self) {
        info!("Performing final cleanup");

        // Log final metrics
        let final_metrics = self.metrics.get_metrics().await;
        info!("Final metrics at shutdown: {:?}", final_metrics);
    }
}
