use crate::shutdown::ShutdownReceiver;
use anyhow::Result;
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info};

use crate::events::ChainEventData;
use crate::metrics::{ActorMetrics, MetricsCollector};
use crate::wasm::ActorInstance;
use crate::ChainEvent;

pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(300);
#[allow(dead_code)]
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

    #[error("Serialization error")]
    SerializationError,
}

// Different types of operations the executor can handle
pub enum ActorOperation {
    CallFunction {
        name: String,
        params: Vec<u8>,
        response_tx: oneshot::Sender<Result<Vec<u8>, ActorError>>,
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

pub struct ActorExecutor {
    actor_instance: ActorInstance,
    operation_rx: mpsc::Receiver<ActorOperation>,
    metrics: MetricsCollector,
    shutdown_initiated: bool,
    shutdown_receiver: ShutdownReceiver,
}

impl ActorExecutor {
    pub fn new(
        actor_instance: ActorInstance,
        operation_rx: mpsc::Receiver<ActorOperation>,
        shutdown_receiver: ShutdownReceiver,
    ) -> Self {
        Self {
            actor_instance,
            operation_rx,
            metrics: MetricsCollector::new(),
            shutdown_initiated: false,
            shutdown_receiver,
        }
    }

    // Execute a type-safe function call
    async fn execute_call(
        &mut self,
        name: &String,
        params: Vec<u8>,
    ) -> Result<Vec<u8>, ActorError> {
        // Validate the function exists
        if !self.actor_instance.has_function(&name) {
            error!("Function '{}' not found in actor", name);
            return Err(ActorError::FunctionNotFound(name.to_string()));
        }

        let start = Instant::now();

        let state = self.actor_instance.store.data().get_state();
        debug!(
            "Executing call to function '{}' with state size: {:?}",
            name,
            state.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        self.actor_instance
            .store
            .data_mut()
            .record_event(ChainEventData {
                event_type: "wasm".to_string(),
                timestamp: start.elapsed().as_secs(),
                description: None,
                data: crate::events::EventData::Wasm(
                    crate::events::wasm::WasmEventData::WasmCall {
                        function_name: name.clone(),
                        params: params.clone(),
                    },
                ),
            });

        // Execute the call
        let (new_state, results) = match self
            .actor_instance
            .call_function(&name, state, params)
            .await
        {
            Ok(result) => {
                self.actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: start.elapsed().as_secs(),
                        description: None,
                        data: crate::events::EventData::Wasm(
                            crate::events::wasm::WasmEventData::WasmResult {
                                function_name: name.clone(),
                                result: result.clone(),
                            },
                        ),
                    });
                result
            }
            Err(e) => {
                self.actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: start.elapsed().as_secs(),
                        description: None,
                        data: crate::events::EventData::Wasm(
                            crate::events::wasm::WasmEventData::WasmError {
                                function_name: name.clone(),
                                message: e.to_string(),
                            },
                        ),
                    });

                error!("Failed to execute function '{}': {}", name, e);
                return Err(ActorError::Internal(e));
            }
        };

        debug!(
            "Call to '{}' completed, new state size: {:?}",
            name,
            new_state.as_ref().map(|s| s.len()).unwrap_or(0)
        );
        self.actor_instance.store.data_mut().set_state(new_state);

        // Record metrics
        let duration = start.elapsed();
        self.metrics.record_operation(duration, true).await;

        Ok(results)
    }

    pub async fn run(&mut self) {
        info!("Actor executor starting");

        loop {
            tokio::select! {
                // Monitor shutdown channel
                _ = self.shutdown_receiver.wait_for_shutdown() => {
                    info!("Actor executor received shutdown signal");
                    debug!("Executor for actor instance starting shutdown sequence");
                    self.shutdown_initiated = true;
                    debug!("Executor marked as shutting down, will reject new operations");
                    break;
                }
                Some(op) = self.operation_rx.recv() => {
                    if self.shutdown_initiated {
                        // Reject operations during shutdown
                        debug!("Rejecting operation during shutdown");
                        match op {
                            ActorOperation::Shutdown { response_tx } => {
                                let _ = response_tx.send(Ok(()));
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
                            match self.execute_call(&name, params).await {
                                Ok(result) => {
                                    if let Err(e) = response_tx.send(Ok(result)) {
                                        error!("Failed to send function call response for operation '{}': {:?}", name, e);
                                    }
                                }
                                Err(e) => {
                                    error!("Operation '{}' failed with error: {:?}", name, e);
                                    if let Err(send_err) = response_tx.send(Err(e)) {
                                        error!("Failed to send function call error response for operation '{}': {:?}", name, send_err);
                                    }
                                }
                }
                        }

                        ActorOperation::GetMetrics { response_tx } => {
                            debug!("Processing GetMetrics operation");
                            let metrics = self.metrics.get_metrics().await;
                            if let Err(e) = response_tx.send(Ok(metrics)) {
                                error!("Failed to send metrics response: {:?}", e);
                            }
                            debug!("GetMetrics operation completed");
                        }

                        ActorOperation::GetChain { response_tx } => {
                            debug!("Processing GetChain operation");
                            let chain = self.actor_instance.store.data().get_chain();
                            debug!("Retrieved chain with {} events", chain.len());
                            if let Err(e) = response_tx.send(Ok(chain)) {
                                error!("Failed to send chain response: {:?}", e);
                            }
                            debug!("GetChain operation completed");
                        }

                        ActorOperation::GetState { response_tx } => {
                            debug!("Processing GetState operation");
                            let state = self.actor_instance.store.data().get_state();
                            debug!("Retrieved state with size: {:?}", state.as_ref().map(|s| s.len()).unwrap_or(0));
                            if let Err(e) = response_tx.send(Ok(state)) {
                                error!("Failed to send state response: {:?}", e);
                            }
                            debug!("GetState operation completed");
                        }


                        ActorOperation::Shutdown { response_tx } => {
                            info!("Processing shutdown request");
                            self.shutdown_initiated = true;
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send shutdown confirmation: {:?}", e);
                            } else {
                                info!("Shutdown confirmation sent successfully");
                            }
                            info!("Breaking from operation loop to begin shutdown process");
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
