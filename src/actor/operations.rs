//! # Actor Operations
//!
//! This module implements the execution of operations for the actor system.
//! It contains the core logic for processing function calls, state queries,
//! chain events, and other operations that can be performed on an actor.

use crate::actor::types::ActorError;
use crate::actor::types::ActorOperation;
use crate::events::{ChainEventData, EventData};
use crate::events::wasm::WasmEventData;
use crate::messages::TheaterCommand;
use crate::metrics::MetricsCollector;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::ActorInstance;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

/// # Operations Processor
///
/// This module contains functions for processing operations on an actor instance.
pub struct OperationsProcessor;

impl OperationsProcessor {
    /// # Run the actor operations processor
    ///
    /// Main loop for processing actor operations and handling shutdown signals.
    /// This method runs continuously until a shutdown is signaled or the operation
    /// channel is closed.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - The WebAssembly actor instance
    /// * `operation_rx` - Channel for receiving operations to perform
    /// * `metrics` - Collector for performance metrics
    /// * `shutdown_receiver` - Receiver for shutdown signals
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `shutdown_controller` - Controller for graceful shutdown
    /// * `handler_tasks` - Tasks for handling actor operations
    pub async fn run(
        mut actor_instance: ActorInstance,
        mut operation_rx: mpsc::Receiver<ActorOperation>,
        metrics: MetricsCollector,
        mut shutdown_receiver: ShutdownReceiver,
        theater_tx: mpsc::Sender<TheaterCommand>,
        shutdown_controller: crate::shutdown::ShutdownController,
        handler_tasks: Vec<JoinHandle<()>>,
    ) {
        info!("Actor runtime starting operation processing loop");
        let mut shutdown_initiated = false;

        loop {
            tokio::select! {
                // Monitor shutdown channel
                _ = shutdown_receiver.wait_for_shutdown() => {
                    info!("Actor runtime received shutdown signal");
                    debug!("Actor runtime starting shutdown sequence");
                    shutdown_initiated = true;
                    debug!("Actor runtime marked as shutting down, will reject new operations");
                    break;
                }
                Some(op) = operation_rx.recv() => {
                    if shutdown_initiated {
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
                            ActorOperation::UpdateComponent { response_tx, .. } => {
                                let _ = response_tx.send(Err(ActorError::ShuttingDown));
                            }
                        }
                        continue;
                    }
                    debug!("Processing actor operation");

                    match op {
                        ActorOperation::CallFunction { name, params, response_tx } => {
                            match Self::execute_call(&mut actor_instance, &name, params, &theater_tx, &metrics).await {
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
                            let metrics = metrics.get_metrics().await;
                            if let Err(e) = response_tx.send(Ok(metrics)) {
                                error!("Failed to send metrics response: {:?}", e);
                            }
                            debug!("GetMetrics operation completed");
                        }

                        ActorOperation::GetChain { response_tx } => {
                            debug!("Processing GetChain operation");
                            let chain = actor_instance.store.data().get_chain();
                            debug!("Retrieved chain with {} events", chain.len());
                            if let Err(e) = response_tx.send(Ok(chain)) {
                                error!("Failed to send chain response: {:?}", e);
                            }
                            debug!("GetChain operation completed");
                        }

                        ActorOperation::GetState { response_tx } => {
                            debug!("Processing GetState operation");
                            let state = actor_instance.store.data().get_state();
                            debug!("Retrieved state with size: {:?}", state.as_ref().map(|s| s.len()).unwrap_or(0));
                            if let Err(e) = response_tx.send(Ok(state)) {
                                error!("Failed to send state response: {:?}", e);
                            }
                            debug!("GetState operation completed");
                        }

                        ActorOperation::Shutdown { response_tx } => {
                            info!("Processing shutdown request");
                            shutdown_initiated = true;
                            if let Err(e) = response_tx.send(Ok(())) {
                                error!("Failed to send shutdown confirmation: {:?}", e);
                            } else {
                                info!("Shutdown confirmation sent successfully");
                            }
                            info!("Breaking from operation loop to begin shutdown process");
                            break;
                        }

                        ActorOperation::UpdateComponent { component_address, response_tx } => {
                            debug!("Processing UpdateComponent operation");
                            // TODO: Implement update_component method on ActorInstance
                            match Ok::<(), ActorError>(()) {
                                Ok(_) => {
                                    if let Err(e) = response_tx.send(Ok(())) {
                                        error!("Failed to send update component response: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("UpdateComponent operation failed: {:?}", e);
                                    if let Err(send_err) = response_tx.send(Err(ActorError::UpdateComponentError(e.to_string()))) {
                                        error!("Failed to send update component error response: {:?}", send_err);
                                    }
                                }
                            }
                        }
                    }
                }

                else => {
                    info!("Operation channel closed, shutting down");
                    break;
                }
            }
        }

        info!("Actor runtime operation loop shutting down");
        Self::perform_cleanup(&shutdown_controller, handler_tasks, &metrics).await;
    }

    /// # Execute a function call in the WebAssembly actor
    ///
    /// Calls a function in the WebAssembly actor with the given parameters,
    /// updates the actor's state based on the result, and records the
    /// operation in the actor's chain.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - The WebAssembly actor instance
    /// * `name` - Name of the function to call
    /// * `params` - Serialized parameters for the function
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `metrics` - Collector for performance metrics
    ///
    /// ## Returns
    ///
    /// * `Ok(Vec<u8>)` - Serialized result of the function call
    /// * `Err(ActorError)` - Error that occurred during execution
    async fn execute_call(
        actor_instance: &mut ActorInstance,
        name: &String,
        params: Vec<u8>,
        theater_tx: &mpsc::Sender<TheaterCommand>,
        metrics: &MetricsCollector,
    ) -> Result<Vec<u8>, ActorError> {
        // Validate the function exists
        if !actor_instance.has_function(&name) {
            error!("Function '{}' not found in actor", name);
            return Err(ActorError::FunctionNotFound(name.to_string()));
        }

        let start = Instant::now();

        let state = actor_instance.store.data().get_state();
        debug!(
            "Executing call to function '{}' with state size: {:?}",
            name,
            state.as_ref().map(|s| s.len()).unwrap_or(0)
        );

        actor_instance
            .store
            .data_mut()
            .record_event(ChainEventData {
                event_type: "wasm".to_string(),
                timestamp: start.elapsed().as_secs(),
                description: Some(format!("Wasm call to function '{}'", name)),
                data: EventData::Wasm(
                    WasmEventData::WasmCall {
                        function_name: name.clone(),
                        params: params.clone(),
                    },
                ),
            });

        // Execute the call
        let (new_state, results) = match actor_instance
            .call_function(&name, state, params)
            .await
        {
            Ok(result) => {
                actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: start.elapsed().as_secs(),
                        description: Some(format!("Wasm call to function '{}' completed", name)),
                        data: EventData::Wasm(
                            WasmEventData::WasmResult {
                                function_name: name.clone(),
                                result: result.clone(),
                            },
                        ),
                    });
                result
            }
            Err(e) => {
                let event = actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: start.elapsed().as_secs(),
                        description: Some(format!("Wasm call to function '{}' failed", name)),
                        data: EventData::Wasm(
                            WasmEventData::WasmError {
                                function_name: name.clone(),
                                message: e.to_string(),
                            },
                        ),
                    });

                // Notify the theater runtime the actor has failed
                let _ = theater_tx
                    .send(TheaterCommand::ActorError {
                        actor_id: actor_instance.id().clone(),
                        event,
                    })
                    .await;

                error!("Failed to execute function '{}': {}", name, e);
                return Err(ActorError::Internal(e));
            }
        };

        debug!(
            "Call to '{}' completed, new state size: {:?}",
            name,
            new_state.as_ref().map(|s| s.len()).unwrap_or(0)
        );
        actor_instance.store.data_mut().set_state(new_state);

        // Record metrics
        let duration = start.elapsed();
        metrics.record_operation(duration, true).await;

        Ok(results)
    }

    /// # Perform final cleanup when shutting down
    ///
    /// This method is called during shutdown to release resources and log
    /// final metrics before the actor terminates.
    async fn perform_cleanup(
        shutdown_controller: &crate::shutdown::ShutdownController,
        handler_tasks: Vec<JoinHandle<()>>,
        metrics: &MetricsCollector,
    ) {
        info!("Performing final cleanup");

        // Signal shutdown to all handler components
        info!("Signaling shutdown to all handler components");
        shutdown_controller.signal_shutdown();

        // Wait briefly for handlers to shut down gracefully
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // If any handlers are still running, abort them
        for task in handler_tasks {
            if !task.is_finished() {
                debug!("Aborting handler task that didn't shut down gracefully");
                task.abort();
            }
        }

        // Log final metrics
        let final_metrics = metrics.get_metrics().await;
        info!("Final metrics at shutdown: {:?}", final_metrics);
        
        info!("Actor runtime cleanup complete");
    }
}
