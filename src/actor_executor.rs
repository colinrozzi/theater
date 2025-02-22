use anyhow::Result;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::chain::ChainEvent;
use crate::metrics::MetricsCollector;
use crate::wasm::{Event, WasmActor};

const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const METRICS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Error, Debug)]
pub enum ActorError {
    #[error("Operation timed out after {0:?}")]
    OperationTimeout(Duration),
    
    #[error("Operation channel closed")]
    ChannelClosed,
    
    #[error("Actor is shutting down")]
    ShuttingDown,
    
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug)]
pub enum ActorOperation {
    HandleEvent {
        event: Event,
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
    GetState {
        response_tx: oneshot::Sender<Result<Vec<u8>, ActorError>>,
    },
    GetChain {
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>, ActorError>>,
    },
    GetMetrics {
        response_tx: oneshot::Sender<Result<crate::metrics::ActorMetrics, ActorError>>,
    },
    Shutdown {
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
}

pub struct ActorExecutor {
    actor: WasmActor,
    operation_rx: mpsc::Receiver<ActorOperation>,
    metrics: MetricsCollector,
    shutdown_initiated: bool,
}

impl ActorExecutor {
    pub fn new(actor: WasmActor, operation_rx: mpsc::Receiver<ActorOperation>) -> Self {
        Self {
            actor,
            operation_rx,
            metrics: MetricsCollector::new(),
            shutdown_initiated: false,
        }
    }

    async fn update_resource_metrics(&self) {
        // Get memory usage from wasm instance
        let memory_size = self.actor.get_memory_size();
        
        // Get operation queue size
        let queue_size = self.operation_rx.capacity().unwrap_or(0);
        
        self.metrics.update_resource_usage(memory_size, queue_size).await;
    }

    pub async fn run(&mut self) {
        info!("Actor executor starting");
        
        // Initialize the actor
        if let Err(e) = self.actor.init().await {
            error!("Failed to initialize actor: {}", e);
            return;
        }

        // Set up metrics update interval
        let mut metrics_interval = tokio::time::interval(METRICS_UPDATE_INTERVAL);
        
        loop {
            tokio::select! {
                _ = metrics_interval.tick() => {
                    self.update_resource_metrics().await;
                }
                
                Some(op) = self.operation_rx.recv() => {
                    debug!("Processing actor operation");
                    
                    // If shutdown was initiated, only process Shutdown operations
                    if self.shutdown_initiated {
                        if !matches!(op, ActorOperation::Shutdown { .. }) {
                            if let Some(tx) = self.get_response_tx(op) {
                                let _ = tx.send(Err(ActorError::ShuttingDown));
                            }
                            continue;
                        }
                    }
                    
                    let start_time = Instant::now();
                    let result = match &op {
                        ActorOperation::HandleEvent { event, .. } => {
                            debug!("Handling event: {:?}", event);
                            self.actor.handle_event(event.clone()).await
                                .map_err(|e| ActorError::Internal(e))
                        },
                        ActorOperation::GetState { .. } => {
                            debug!("Getting actor state");
                            Ok(())
                        },
                        ActorOperation::GetChain { .. } => {
                            debug!("Getting actor chain");
                            Ok(())
                        },
                        ActorOperation::GetMetrics { response_tx } => {
                            debug!("Getting metrics");
                            let metrics = self.metrics.get_metrics().await;
                            let _ = response_tx.send(Ok(metrics));
                            Ok(())
                        },
                        ActorOperation::Shutdown { response_tx } => {
                            info!("Processing shutdown request");
                            self.shutdown_initiated = true;
                            let _ = response_tx.send(Ok(()));
                            break;
                        }
                    };

                    // Record operation metrics
                    let duration = start_time.elapsed();
                    self.metrics.record_operation(duration, result.is_ok()).await;

                    // Send response
                    match op {
                        ActorOperation::HandleEvent { response_tx, .. } => {
                            let _ = response_tx.send(result);
                        },
                        ActorOperation::GetState { response_tx } => {
                            let _ = response_tx.send(Ok(self.actor.actor_state.clone()));
                        },
                        ActorOperation::GetChain { response_tx } => {
                            let chain = self.actor.actor_store.get_chain();
                            let _ = response_tx.send(Ok(chain));
                        },
                        _ => {}
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
        // Add any necessary cleanup code here
        // For example:
        // - Save final state
        // - Close connections
        // - Final metrics snapshot
        let final_metrics = self.metrics.get_metrics().await;
        info!("Final metrics at shutdown: {:?}", final_metrics);
    }
    
    fn get_response_tx(&self, op: ActorOperation) -> Option<oneshot::Sender<Result<(), ActorError>>> {
        match op {
            ActorOperation::HandleEvent { response_tx, .. } => Some(response_tx),
            ActorOperation::Shutdown { response_tx } => Some(response_tx),
            _ => None,
        }
    }
}