//! # Actor Executor
//!
//! The Actor Executor is responsible for managing the execution of individual WebAssembly actors
//! within the Theater system. It handles operations like function calls, state management,
//! and metrics collection for a specific actor instance.
//!
//! ## Purpose
//!
//! The Actor Executor serves as the runtime environment for a single actor, providing:
//!
//! - Execution of WebAssembly functions with proper state management
//! - Isolation between actors through independent execution contexts
//! - Recording of operations in the actor's chain (audit log)
//! - Collection of performance metrics
//! - Graceful shutdown handling
//!
//! Each actor in the Theater system has its own dedicated executor running in a separate
//! Tokio task, allowing for concurrent execution and isolation between actors.

use crate::shutdown::ShutdownReceiver;
use anyhow::Result;
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info};

use crate::events::ChainEventData;
use crate::messages::TheaterCommand;
use crate::metrics::{ActorMetrics, MetricsCollector};
use crate::wasm::ActorInstance;
use crate::ChainEvent;

/// Default timeout for actor operations (50 minutes)
pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(3000);
#[allow(dead_code)]
/// Interval for updating metrics (1 second)
const METRICS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

/// # ActorError
///
/// Represents errors that can occur during actor execution.
///
/// ## Purpose
///
/// This enum provides detailed error information for various failure modes that
/// might occur when interacting with an actor. These errors are propagated back
/// to callers to help diagnose and handle problems.
///
/// ## Example
///
/// ```rust
/// use theater::actor_executor::ActorError;
///
/// fn handle_actor_error(error: ActorError) {
///     match error {
///         ActorError::OperationTimeout(duration) => {
///             println!("Operation timed out after {:?}", duration);
///             // Implement retry logic
///         },
///         ActorError::ShuttingDown => {
///             println!("Cannot perform operation as actor is shutting down");
///             // Abort further operations
///         },
///         // Handle other error types...
///         _ => println!("Unexpected actor error: {:?}", error),
///     }
/// }
/// ```
#[derive(Error, Debug)]
pub enum ActorError {
    /// Operation exceeded the maximum allowed execution time
    #[error("Operation timed out after {0:?}")]
    OperationTimeout(Duration),

    /// Communication channel to the actor was closed unexpectedly
    #[error("Operation channel closed")]
    ChannelClosed,

    /// Actor is in the process of shutting down and cannot accept new operations
    #[error("Actor is shutting down")]
    ShuttingDown,

    /// The requested WebAssembly function was not found in the actor
    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    /// Parameter or return types did not match the WebAssembly function signature
    #[error("Type mismatch for function {0}")]
    TypeMismatch(String),

    /// An internal error occurred during execution
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),

    /// Failed to serialize or deserialize data
    #[error("Serialization error")]
    SerializationError,

    #[error("Failed to update component: {0}")]
    UpdateComponentError(String),
}

/// # ActorOperation
///
/// Represents the different types of operations that can be performed on an actor.
///
/// ## Purpose
///
/// This enum defines the message types that can be sent to an `ActorExecutor` via
/// its operation channel. Each variant includes the necessary data for the operation
/// and a oneshot channel sender for returning the result.
///
/// ## Example
///
/// ```rust
/// use theater::actor_executor::ActorOperation;
/// use tokio::sync::oneshot;
///
/// async fn call_actor_function(
///     operation_tx: tokio::sync::mpsc::Sender<ActorOperation>,
///     function_name: String,
///     params: Vec<u8>,
/// ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
///     // Create a channel for receiving the response
///     let (response_tx, response_rx) = oneshot::channel();
///     
///     // Send the operation to the actor
///     operation_tx.send(ActorOperation::CallFunction {
///         name: function_name,
///         params,
///         response_tx,
///     }).await?;
///     
///     // Wait for and return the response
///     Ok(response_rx.await??)
/// }
/// ```
///
/// ## Security
///
/// The operation channel is the primary interface for interacting with actors.
/// Access to this channel should be carefully controlled as it allows executing
/// arbitrary functions within the actor.
pub enum ActorOperation {
    /// Call a WebAssembly function in the actor
    CallFunction {
        /// Name of the function to call
        name: String,
        /// Serialized parameters for the function
        params: Vec<u8>,
        /// Channel to send the result back to the caller
        response_tx: oneshot::Sender<Result<Vec<u8>, ActorError>>,
    },
    /// Retrieve current metrics for the actor
    GetMetrics {
        /// Channel to send metrics back to the caller
        response_tx: oneshot::Sender<Result<ActorMetrics, ActorError>>,
    },
    /// Initiate actor shutdown
    Shutdown {
        /// Channel to confirm shutdown completion
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
    /// Retrieve the actor's event chain (audit log)
    GetChain {
        /// Channel to send chain events back to the caller
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>, ActorError>>,
    },
    /// Retrieve the actor's current state
    GetState {
        /// Channel to send state back to the caller
        response_tx: oneshot::Sender<Result<Option<Vec<u8>>, ActorError>>,
    },
    UpdateComponent {
        /// Address of the component to update
        component_address: String,
        /// Channel to send the result back to the caller
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
}

/// # ActorExecutor
///
/// The runtime environment for executing a single WebAssembly actor in the Theater system.
///
/// ## Purpose
///
/// `ActorExecutor` manages the lifecycle and execution of a WebAssembly actor.
/// It receives operations via a channel, processes them by interacting with the
/// WebAssembly instance, and sends back results. The executor maintains the actor's
/// state between operations and records events to the actor's chain.
///
/// ## Example
///
/// ```rust
/// use theater::actor_executor::ActorExecutor;
/// use theater::wasm::ActorInstance;
/// use theater::shutdown::ShutdownReceiver;
/// use tokio::sync::mpsc;
///
/// async fn run_actor(
///     actor_instance: ActorInstance,
///     shutdown_receiver: ShutdownReceiver,
///     theater_tx: mpsc::Sender<TheaterCommand>,
/// ) {
///     // Create channels for communicating with the executor
///     let (operation_tx, operation_rx) = mpsc::channel(32);
///     
///     // Create the executor
///     let mut executor = ActorExecutor::new(
///         actor_instance,
///         operation_rx,
///         shutdown_receiver,
///         theater_tx,
///     );
///     
///     // Spawn a task to run the executor
///     tokio::spawn(async move {
///         executor.run().await;
///     });
///     
///     // Use operation_tx to send operations to the executor
///     // ...
/// }
/// ```
///
/// ## Safety
///
/// While `ActorExecutor` itself is safe to use, it interacts with WebAssembly code
/// which is inherently unsafe. The executor ensures that the WebAssembly code operates
/// within its sandbox and cannot directly access host resources.
///
/// ## Security
///
/// The executor enforces the isolation boundary for the actor by:
/// - Carefully managing what data is passed to and from the WebAssembly instance
/// - Recording all operations to the actor's chain for auditing
/// - Handling errors from WebAssembly execution and preventing them from affecting other actors
///
/// ## Implementation Notes
///
/// The executor runs in a dedicated Tokio task and uses `tokio::select!` to concurrently
/// handle incoming operations and shutdown signals. When a shutdown is initiated, it
/// completes any in-progress operations and then performs cleanup.
pub struct ActorExecutor {
    /// The WebAssembly actor instance being executed
    actor_instance: ActorInstance,
    /// Channel for receiving operations to perform
    operation_rx: mpsc::Receiver<ActorOperation>,
    /// Collector for performance metrics
    metrics: MetricsCollector,
    /// Flag indicating whether shutdown has been initiated
    shutdown_initiated: bool,
    /// Receiver for system-wide shutdown signals
    shutdown_receiver: ShutdownReceiver,
    /// Channel for sending commands back to the Theater runtime
    theater_tx: mpsc::Sender<TheaterCommand>,
}

impl ActorExecutor {
    /// # Create a new actor executor
    ///
    /// Initializes a new executor for running a WebAssembly actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - The WebAssembly actor instance to execute
    /// * `operation_rx` - Receiver channel for incoming operations
    /// * `shutdown_receiver` - Receiver for system shutdown signals
    /// * `theater_tx` - Sender channel for communicating with the Theater runtime
    ///
    /// ## Returns
    ///
    /// A new `ActorExecutor` instance ready to run the actor.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::actor_executor::ActorExecutor;
    /// use theater::wasm::ActorInstance;
    /// use theater::shutdown::ShutdownReceiver;
    /// use tokio::sync::mpsc;
    ///
    /// async fn create_executor(
    ///     actor_instance: ActorInstance,
    ///     shutdown_receiver: ShutdownReceiver,
    ///     theater_tx: mpsc::Sender<TheaterCommand>,
    /// ) -> ActorExecutor {
    ///     let (_, operation_rx) = mpsc::channel(32);
    ///     
    ///     ActorExecutor::new(
    ///         actor_instance,
    ///         operation_rx,
    ///         shutdown_receiver,
    ///         theater_tx,
    ///     )
    /// }
    /// ```
    pub fn new(
        actor_instance: ActorInstance,
        operation_rx: mpsc::Receiver<ActorOperation>,
        shutdown_receiver: ShutdownReceiver,
        theater_tx: mpsc::Sender<TheaterCommand>,
    ) -> Self {
        Self {
            actor_instance,
            operation_rx,
            metrics: MetricsCollector::new(),
            shutdown_initiated: false,
            shutdown_receiver,
            theater_tx,
        }
    }

    /// # Execute a function call in the WebAssembly actor
    ///
    /// Calls a function in the WebAssembly actor with the given parameters,
    /// updates the actor's state based on the result, and records the
    /// operation in the actor's chain.
    ///
    /// ## Parameters
    ///
    /// * `name` - Name of the function to call
    /// * `params` - Serialized parameters for the function
    ///
    /// ## Returns
    ///
    /// * `Ok(Vec<u8>)` - Serialized result of the function call
    /// * `Err(ActorError)` - Error that occurred during execution
    ///
    /// ## Implementation Notes
    ///
    /// This method:
    /// 1. Validates that the function exists in the actor
    /// 2. Records the call in the actor's chain
    /// 3. Executes the function with the current actor state
    /// 4. Updates the actor state with the new state returned by the function
    /// 5. Records metrics about the execution
    /// 6. Returns the function's result
    ///
    /// If the function call fails, it records the error in the chain and
    /// notifies the Theater runtime about the failure.
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
                description: Some(format!("Wasm call to function '{}'", name)),
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
                        description: Some(format!("Wasm call to function '{}' completed", name)),
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
                let event = self
                    .actor_instance
                    .store
                    .data_mut()
                    .record_event(ChainEventData {
                        event_type: "wasm".to_string(),
                        timestamp: start.elapsed().as_secs(),
                        description: Some(format!("Wasm call to function '{}' failed", name)),
                        data: crate::events::EventData::Wasm(
                            crate::events::wasm::WasmEventData::WasmError {
                                function_name: name.clone(),
                                message: e.to_string(),
                            },
                        ),
                    });

                // Notify the theater runtime the actor has failed
                let _ = self
                    .theater_tx
                    .send(TheaterCommand::ActorError {
                        actor_id: self.actor_instance.id().clone(),
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
        self.actor_instance.store.data_mut().set_state(new_state);

        // Record metrics
        let duration = start.elapsed();
        self.metrics.record_operation(duration, true).await;

        Ok(results)
    }

    /// # Run the actor executor
    ///
    /// Starts the main execution loop for the actor. This method continuously
    /// processes incoming operations until a shutdown is requested or the
    /// operation channel is closed.
    ///
    /// ## Example
    ///
    /// ```rust
    /// async fn start_actor(mut executor: ActorExecutor) {
    ///     // This will block until the actor shuts down
    ///     executor.run().await;
    ///     println!("Actor has been shut down");
    /// }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// The executor uses `tokio::select!` to concurrently handle:
    /// - Shutdown signals (which trigger graceful shutdown)
    /// - Incoming operations (which are processed based on their type)
    ///
    /// During shutdown, the executor rejects new operations (except Shutdown)
    /// and performs cleanup before terminating.
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
                            ActorOperation::UpdateComponent { response_tx, .. } => {
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
                            ActorOperation::UpdateComponent { response_tx, .. } => {
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

                        ActorOperation::UpdateComponent { component_address, response_tx } => {
                            debug!("Processing UpdateComponent operation");
                            match self.actor_instance.update_component(&component_address).await {
                                Ok(_) => {
                                    if let Err(e) = response_tx.send(Ok(())) {
                                        error!("Failed to send update component response: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("UpdateComponent operation failed: {:?}", e);
                                    if let Err(send_err) = response_tx.send(Err(e.to_string())) {
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

        info!("Actor executor shutting down");
        self.cleanup().await;
    }

    /// # Perform final cleanup when shutting down
    ///
    /// This method is called during shutdown to release resources and log
    /// final metrics before the executor terminates.
    ///
    /// ## Implementation Notes
    ///
    /// Currently, this method logs final metrics but could be extended to
    /// perform additional cleanup such as releasing external resources or
    /// notifying other components about the shutdown.
    async fn cleanup(&mut self) {
        info!("Performing final cleanup");

        // Log final metrics
        let final_metrics = self.metrics.get_metrics().await;
        info!("Final metrics at shutdown: {:?}", final_metrics);
    }
}
