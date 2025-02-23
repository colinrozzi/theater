use anyhow::Result;
use serde::Serialize;
use std::any::Any;
use std::fmt::Debug;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info};
use wasmtime::component::ComponentType;
use wasmtime::component::{ComponentNamedList, Lift, Lower};

use crate::metrics::{ActorMetrics, MetricsCollector};
use crate::wasm::WasmActor;
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

// Represents a function call to the WebAssembly component
pub struct HandlerFunctionCall<P, R>
where
    P: ComponentType,
    R: ComponentType,
{
    pub function_name: String,
    pub parameters: P,
    _phantom: PhantomData<R>,
}

impl<P, R> HandlerFunctionCall<P, R>
where
    P: ComponentType,
    R: ComponentType,
{
    pub fn new(function_name: String, parameters: P) -> Self {
        Self {
            function_name,
            parameters,
            _phantom: PhantomData,
        }
    }
}

// Different types of operations the executor can handle
pub enum ActorOperation {
    // Handler function calls are type-safe
    CallFunction {
        call: Box<dyn std::any::Any + Send>,
        response_tx: oneshot::Sender<Result<Box<dyn std::any::Any + Send>, ActorError>>,
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

    // Execute a type-safe function call
    async fn execute_call<P, R>(&mut self, call: HandlerFunctionCall<P, R>) -> Result<R, ActorError>
    where
        P: ComponentType
            + ComponentNamedList
            + Lift
            + Lower
            + Clone
            + Debug
            + Serialize
            + Sync
            + Send
            + 'static,
        R: ComponentType
            + ComponentNamedList
            + Lift
            + Lower
            + Clone
            + Debug
            + Serialize
            + Sync
            + Send
            + 'static,
    {
        // Validate the function exists
        if !self.actor.has_function(&call.function_name) {
            return Err(ActorError::FunctionNotFound(call.function_name));
        }

        let start = Instant::now();

        // Execute the call
        let result = self
            .actor
            .call_func(&call.function_name, call.parameters)
            .await
            .map_err(ActorError::Internal)?;

        // Record metrics
        let duration = start.elapsed();
        self.metrics.record_operation(duration, true).await;

        Ok(result)
    }

    async fn update_resource_metrics(&self) {
        let memory_size = self.actor.get_memory_size();
        let queue_size = self.operation_rx.capacity();
        self.metrics
            .update_resource_usage(memory_size, queue_size)
            .await;
    }

    pub async fn run(&mut self) {
        info!("Actor executor starting");

        // Initialize the actor
        if let Err(e) = self.actor.init().await {
            error!("Failed to initialize actor: {}", e);
            return;
        }

        let mut metrics_interval = tokio::time::interval(METRICS_UPDATE_INTERVAL);

        loop {
            tokio::select! {
                _ = metrics_interval.tick() => {
                    self.update_resource_metrics().await;
                }

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
                        }
                        continue;
                    }

                    match op {
                        ActorOperation::CallFunction { call, response_tx } => {
                            // Downcast the boxed call to its concrete type
                            let result = if let Ok(call) = call.downcast::<HandlerFunctionCall<_, _>>() {
                                match self.execute_call(*call).await {
                                    Ok(result) => {
                                        // Box the result for sending back
                                        Ok(Box::new(result) as Box<dyn std::any::Any + Send>)
                                    }
                                    Err(e) => Err(e),
                                }
                            } else {
                                Err(ActorError::TypeMismatch("Invalid function call type".to_string()))
                            };

                            if let Err(e) = response_tx.send(result) {
                                error!("Failed to send function call response: {:?}", e);
                            }
                        }

                        ActorOperation::GetMetrics { response_tx } => {
                            let metrics = self.metrics.get_metrics().await;
                            if let Err(e) = response_tx.send(Ok(metrics)) {
                                error!("Failed to send metrics: {:?}", e);
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

// Example handler implementation
#[cfg(test)]
mod tests {
    use super::*;

    // Example of how a handler would use this
    struct ExampleHandler {
        executor_tx: mpsc::Sender<ActorOperation>,
    }

    impl ExampleHandler {
        async fn call_function<P, R>(
            &self,
            call: HandlerFunctionCall<P, R>,
        ) -> Result<R, ActorError>
        where
            P: ComponentType + Send + 'static,
            R: ComponentType + Send + 'static,
        {
            let (tx, rx) = oneshot::channel();

            let boxed_call = Box::new(call);

            self.executor_tx
                .send(ActorOperation::CallFunction {
                    call: boxed_call,
                    response_tx: tx,
                })
                .await
                .map_err(|_| ActorError::ChannelClosed)?;

            let result = rx.await.map_err(|_| ActorError::ChannelClosed)??;

            result
                .downcast::<R>()
                .map(|b| *b)
                .map_err(|_| ActorError::TypeMismatch("Unexpected return type".to_string()))
        }
    }
}
