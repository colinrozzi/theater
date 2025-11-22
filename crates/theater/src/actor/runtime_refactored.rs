//! # Refactored Actor Runtime with Explicit State Machine
//!
//! This is a proposed refactoring of the actor runtime that uses an explicit
//! state machine to manage the actor lifecycle, making state transitions clear
//! and reducing cognitive complexity.

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::{ActorControl, ActorError, ActorInfo, ActorOperation};
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::metrics::MetricsCollector;
use crate::shutdown::{ShutdownController, ShutdownReceiver, ShutdownType};
use crate::wasm::{ActorComponent, ActorInstance};
use crate::ManifestConfig;
use crate::Result;
use crate::StateChain;

use std::sync::{Arc, RwLock as SyncRwLock};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

// ============================================================================
// STATE MACHINE DEFINITION
// ============================================================================

/// The explicit state machine for an actor's lifecycle.
/// Each state contains only the data relevant to that state.
enum ActorState {
    /// Actor is initializing - loading WASM, setting up handlers, etc.
    Starting {
        setup_task: JoinHandle<Result<SetupComplete, ActorError>>,
        status_rx: Receiver<String>,
        current_status: String,
        /// If shutdown is requested during startup, store the response channel
        pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
    },

    /// Actor is ready and idle, waiting for operations
    Idle { resources: ActorResources },

    /// Actor is currently processing an operation
    Processing {
        resources: ActorResources,
        current_operation: JoinHandle<Result<Vec<u8>, ActorError>>,
        operation_name: String,
        /// If shutdown is requested during operation, store the response channel
        pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
    },

    /// Actor is paused and won't accept new operations
    Paused { resources: ActorResources },

    /// Actor is shutting down - terminal state
    ShuttingDown,
}

/// Resources that exist once the actor is fully initialized
struct ActorResources {
    instance: Arc<RwLock<ActorInstance>>,
    metrics: Arc<RwLock<MetricsCollector>>,
    handler_tasks: Vec<JoinHandle<()>>,
    shutdown_controller: ShutdownController,
}

/// Everything needed from the successful setup phase
struct SetupComplete {
    instance: ActorInstance,
    handler_tasks: Vec<JoinHandle<()>>,
    metrics: MetricsCollector,
}

// ============================================================================
// ACTOR RUNTIME
// ============================================================================

pub struct ActorRuntime {
    pub actor_id: TheaterId,
}

impl ActorRuntime {
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        initial_state: Option<serde_json::Value>,
        theater_tx: Sender<TheaterCommand>,
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        info_rx: Receiver<ActorInfo>,
        info_tx: Sender<ActorInfo>,
        control_rx: Receiver<ActorControl>,
        control_tx: Sender<ActorControl>,
        init: bool,
        parent_shutdown_receiver: ShutdownReceiver,
        engine: wasmtime::Engine,
        parent_permissions: crate::config::permissions::HandlerPermission,
        chain: Arc<SyncRwLock<StateChain>>,
        handler_registry: crate::handler::HandlerRegistry,
    ) {
        let mut machine = StateMachine::new(
            id.clone(),
            config.clone(),
            initial_state,
            theater_tx,
            actor_sender,
            actor_mailbox,
            operation_rx,
            operation_tx,
            info_rx,
            info_tx,
            control_rx,
            control_tx,
            init,
            parent_shutdown_receiver,
            engine,
            parent_permissions,
            chain,
            handler_registry,
        );

        machine.run().await;
    }
}

// ============================================================================
// STATE MACHINE IMPLEMENTATION
// ============================================================================

struct StateMachine {
    id: TheaterId,
    state: ActorState,

    // Channels
    theater_tx: Sender<TheaterCommand>,
    operation_rx: Receiver<ActorOperation>,
    info_rx: Receiver<ActorInfo>,
    control_rx: Receiver<ActorControl>,
    parent_shutdown_receiver: ShutdownReceiver,

    // Configuration for startup
    config: ManifestConfig,
    initial_state: Option<serde_json::Value>,
    init: bool,
    engine: wasmtime::Engine,
    parent_permissions: crate::config::permissions::HandlerPermission,
    chain: Arc<SyncRwLock<StateChain>>,
    handler_registry: crate::handler::HandlerRegistry,

    // Actor handle for operations
    actor_handle: ActorHandle,
}

impl StateMachine {
    fn new(
        id: TheaterId,
        config: ManifestConfig,
        initial_state: Option<serde_json::Value>,
        theater_tx: Sender<TheaterCommand>,
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        info_rx: Receiver<ActorInfo>,
        info_tx: Sender<ActorInfo>,
        control_rx: Receiver<ActorControl>,
        control_tx: Sender<ActorControl>,
        init: bool,
        parent_shutdown_receiver: ShutdownReceiver,
        engine: wasmtime::Engine,
        parent_permissions: crate::config::permissions::HandlerPermission,
        chain: Arc<SyncRwLock<StateChain>>,
        handler_registry: crate::handler::HandlerRegistry,
    ) -> Self {
        let actor_handle =
            ActorHandle::new(operation_tx.clone(), info_tx.clone(), control_tx.clone());

        // Start the setup task
        let (status_tx, status_rx) = tokio::sync::mpsc::channel(10);
        let setup_task = tokio::spawn(Self::build_actor_resources(
            id.clone(),
            config.clone(),
            initial_state.clone(),
            theater_tx.clone(),
            actor_sender,
            actor_mailbox,
            operation_tx.clone(),
            info_tx.clone(),
            control_tx.clone(),
            init,
            engine.clone(),
            parent_permissions.clone(),
            status_tx,
            chain.clone(),
            handler_registry.clone(),
        ));

        Self {
            id,
            state: ActorState::Starting {
                setup_task,
                status_rx,
                current_status: "Initializing".to_string(),
                pending_shutdown: None,
            },
            theater_tx,
            operation_rx,
            info_rx,
            control_rx,
            parent_shutdown_receiver,
            config,
            initial_state,
            init,
            engine,
            parent_permissions,
            chain,
            handler_registry,
            actor_handle,
        }
    }

    /// Main state machine loop
    async fn run(&mut self) {
        info!("Actor {} state machine starting", self.id);

        loop {
            // Transition to next state
            let next_state = match &mut self.state {
                ActorState::Starting { .. } => self.handle_starting_state().await,
                ActorState::Idle { .. } => self.handle_idle_state().await,
                ActorState::Processing { .. } => self.handle_processing_state().await,
                ActorState::Paused { .. } => self.handle_paused_state().await,
                ActorState::ShuttingDown => break,
            };

            match next_state {
                StateTransition::Continue(new_state) => {
                    self.state = new_state;
                }
                StateTransition::Shutdown => {
                    self.transition_to_shutdown().await;
                    break;
                }
                StateTransition::Error(error) => {
                    error!("Actor {} encountered fatal error: {:?}", self.id, error);
                    self.notify_error(error).await;
                    self.transition_to_shutdown().await;
                    break;
                }
            }
        }

        info!("Actor {} state machine exiting", self.id);
    }

    // ========================================================================
    // STATE HANDLERS
    // ========================================================================

    async fn handle_starting_state(&mut self) -> StateTransition {
        let (setup_task, status_rx, current_status, pending_shutdown) =
            match std::mem::replace(&mut self.state, ActorState::ShuttingDown) {
                ActorState::Starting {
                    setup_task,
                    status_rx,
                    current_status,
                    pending_shutdown,
                } => (setup_task, status_rx, current_status, pending_shutdown),
                _ => unreachable!(),
            };

        tokio::select! {
            // Setup completed
            result = setup_task => {
                match result {
                    Ok(Ok(setup)) => {
                        info!("Actor {} setup completed successfully", self.id);

                        let resources = ActorResources {
                            instance: Arc::new(RwLock::new(setup.instance)),
                            metrics: Arc::new(RwLock::new(setup.metrics)),
                            handler_tasks: setup.handler_tasks,
                            shutdown_controller: ShutdownController::new(),
                        };

                        // Call init if needed
                        if self.init {
                            self.call_init_function(&resources).await;
                        }

                        // If shutdown was requested during startup, handle it
                        if let Some(response_tx) = pending_shutdown {
                            let _ = response_tx.send(Ok(()));
                            return StateTransition::Shutdown;
                        }

                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                    Ok(Err(error)) => {
                        error!("Actor {} setup failed: {:?}", self.id, error);

                        if let Some(response_tx) = pending_shutdown {
                            let _ = response_tx.send(Err(error.clone()));
                        }

                        StateTransition::Error(error)
                    }
                    Err(e) => {
                        error!("Actor {} setup task panicked: {:?}", self.id, e);
                        StateTransition::Error(ActorError::UnexpectedError(e.to_string()))
                    }
                }
            }

            // Status updates
            Some(new_status) = status_rx.recv() => {
                debug!("Actor {} status: {}", self.id, new_status);
                StateTransition::Continue(ActorState::Starting {
                    setup_task,
                    status_rx,
                    current_status: new_status,
                    pending_shutdown,
                })
            }

            // Info requests (can be handled even during startup)
            Some(info) = self.info_rx.recv() => {
                self.handle_info_request_during_startup(info, &current_status).await;
                StateTransition::Continue(ActorState::Starting {
                    setup_task,
                    status_rx,
                    current_status,
                    pending_shutdown,
                })
            }

            // Control commands
            Some(control) = self.control_rx.recv() => {
                match control {
                    ActorControl::Shutdown { response_tx } => {
                        info!("Shutdown requested during startup");
                        // Wait for setup to complete, then shutdown
                        StateTransition::Continue(ActorState::Starting {
                            setup_task,
                            status_rx,
                            current_status,
                            pending_shutdown: Some(response_tx),
                        })
                    }
                    ActorControl::Terminate { response_tx } => {
                        info!("Terminate requested during startup");
                        setup_task.abort();
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Pause { response_tx } => {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Cannot pause during startup".to_string()
                        )));
                        StateTransition::Continue(ActorState::Starting {
                            setup_task,
                            status_rx,
                            current_status,
                            pending_shutdown,
                        })
                    }
                    ActorControl::Resume { response_tx } => {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Cannot resume during startup".to_string()
                        )));
                        StateTransition::Continue(ActorState::Starting {
                            setup_task,
                            status_rx,
                            current_status,
                            pending_shutdown,
                        })
                    }
                }
            }

            // Parent shutdown
            shutdown_signal = &mut self.parent_shutdown_receiver.receiver => {
                match shutdown_signal {
                    Ok(signal) => {
                        info!("Parent shutdown signal received during startup: {:?}", signal.shutdown_type);
                        match signal.shutdown_type {
                            ShutdownType::Graceful => {
                                // Wait for setup to complete
                                StateTransition::Continue(ActorState::Starting {
                                    setup_task,
                                    status_rx,
                                    current_status,
                                    pending_shutdown,
                                })
                            }
                            ShutdownType::Force => {
                                setup_task.abort();
                                StateTransition::Shutdown
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to receive shutdown signal: {:?}", e);
                        StateTransition::Shutdown
                    }
                }
            }
        }
    }

    async fn handle_idle_state(&mut self) -> StateTransition {
        let resources = match std::mem::replace(&mut self.state, ActorState::ShuttingDown) {
            ActorState::Idle { resources } => resources,
            _ => unreachable!(),
        };

        tokio::select! {
            // New operation
            Some(op) = self.operation_rx.recv() => {
                match op {
                    ActorOperation::CallFunction { name, params, response_tx } => {
                        info!("Starting operation: {}", name);

                        let operation_task = self.spawn_operation(
                            &resources,
                            name.clone(),
                            params,
                            response_tx,
                        );

                        StateTransition::Continue(ActorState::Processing {
                            resources,
                            current_operation: operation_task,
                            operation_name: name,
                            pending_shutdown: None,
                        })
                    }
                    ActorOperation::UpdateComponent { component_address: _, response_tx } => {
                        let _ = response_tx.send(Err(ActorError::UpdateComponentError(
                            "Not implemented".to_string()
                        )));
                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                }
            }

            // Info requests
            Some(info) = self.info_rx.recv() => {
                self.handle_info_request(&resources, "Idle").await;
                StateTransition::Continue(ActorState::Idle { resources })
            }

            // Control commands
            Some(control) = self.control_rx.recv() => {
                match control {
                    ActorControl::Shutdown { response_tx } => {
                        info!("Shutdown requested while idle");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Terminate { response_tx } => {
                        info!("Terminate requested while idle");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Pause { response_tx } => {
                        info!("Pausing actor");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Continue(ActorState::Paused { resources })
                    }
                    ActorControl::Resume { response_tx } => {
                        let _ = response_tx.send(Err(ActorError::NotPaused));
                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                }
            }

            // Parent shutdown
            shutdown_signal = &mut self.parent_shutdown_receiver.receiver => {
                match shutdown_signal {
                    Ok(signal) => {
                        info!("Parent shutdown signal: {:?}", signal.shutdown_type);
                        StateTransition::Shutdown
                    }
                    Err(e) => {
                        error!("Shutdown signal error: {:?}", e);
                        StateTransition::Shutdown
                    }
                }
            }
        }
    }

    async fn handle_processing_state(&mut self) -> StateTransition {
        let (resources, current_operation, operation_name, pending_shutdown) =
            match std::mem::replace(&mut self.state, ActorState::ShuttingDown) {
                ActorState::Processing {
                    resources,
                    current_operation,
                    operation_name,
                    pending_shutdown,
                } => (
                    resources,
                    current_operation,
                    operation_name,
                    pending_shutdown,
                ),
                _ => unreachable!(),
            };

        tokio::select! {
            // Operation completed
            result = current_operation => {
                info!("Operation '{}' completed", operation_name);

                // If shutdown was pending, do it now
                if let Some(response_tx) = pending_shutdown {
                    let _ = response_tx.send(Ok(()));
                    return StateTransition::Shutdown;
                }

                StateTransition::Continue(ActorState::Idle { resources })
            }

            // Info requests
            Some(info) = self.info_rx.recv() => {
                self.handle_info_request(&resources, "Processing").await;
                StateTransition::Continue(ActorState::Processing {
                    resources,
                    current_operation,
                    operation_name,
                    pending_shutdown,
                })
            }

            // Control commands
            Some(control) = self.control_rx.recv() => {
                match control {
                    ActorControl::Shutdown { response_tx } => {
                        info!("Shutdown requested during operation - will complete after operation");
                        StateTransition::Continue(ActorState::Processing {
                            resources,
                            current_operation,
                            operation_name,
                            pending_shutdown: Some(response_tx),
                        })
                    }
                    ActorControl::Terminate { response_tx } => {
                        info!("Terminate requested - aborting operation");
                        current_operation.abort();
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Pause { response_tx } => {
                        let _ = response_tx.send(Err(ActorError::UnexpectedError(
                            "Cannot pause during operation".to_string()
                        )));
                        StateTransition::Continue(ActorState::Processing {
                            resources,
                            current_operation,
                            operation_name,
                            pending_shutdown,
                        })
                    }
                    ActorControl::Resume { response_tx } => {
                        let _ = response_tx.send(Err(ActorError::NotPaused));
                        StateTransition::Continue(ActorState::Processing {
                            resources,
                            current_operation,
                            operation_name,
                            pending_shutdown,
                        })
                    }
                }
            }

            // Parent shutdown
            shutdown_signal = &mut self.parent_shutdown_receiver.receiver => {
                match shutdown_signal {
                    Ok(signal) => {
                        match signal.shutdown_type {
                            ShutdownType::Graceful => {
                                info!("Graceful shutdown - waiting for operation to complete");
                                StateTransition::Continue(ActorState::Processing {
                                    resources,
                                    current_operation,
                                    operation_name,
                                    pending_shutdown,
                                })
                            }
                            ShutdownType::Force => {
                                info!("Forced shutdown - aborting operation");
                                current_operation.abort();
                                StateTransition::Shutdown
                            }
                        }
                    }
                    Err(e) => {
                        error!("Shutdown signal error: {:?}", e);
                        StateTransition::Shutdown
                    }
                }
            }
        }
    }

    async fn handle_paused_state(&mut self) -> StateTransition {
        let resources = match std::mem::replace(&mut self.state, ActorState::ShuttingDown) {
            ActorState::Paused { resources } => resources,
            _ => unreachable!(),
        };

        tokio::select! {
            // Operations are not accepted while paused
            Some(_op) = self.operation_rx.recv() => {
                error!("Operation received while paused - this shouldn't happen");
                StateTransition::Continue(ActorState::Paused { resources })
            }

            // Info requests
            Some(info) = self.info_rx.recv() => {
                self.handle_info_request(&resources, "Paused").await;
                StateTransition::Continue(ActorState::Paused { resources })
            }

            // Control commands
            Some(control) = self.control_rx.recv() => {
                match control {
                    ActorControl::Shutdown { response_tx } => {
                        info!("Shutdown requested while paused");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Terminate { response_tx } => {
                        info!("Terminate requested while paused");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Shutdown
                    }
                    ActorControl::Pause { response_tx } => {
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Continue(ActorState::Paused { resources })
                    }
                    ActorControl::Resume { response_tx } => {
                        info!("Resuming actor");
                        let _ = response_tx.send(Ok(()));
                        StateTransition::Continue(ActorState::Idle { resources })
                    }
                }
            }

            // Parent shutdown
            shutdown_signal = &mut self.parent_shutdown_receiver.receiver => {
                match shutdown_signal {
                    Ok(_) => StateTransition::Shutdown,
                    Err(e) => {
                        error!("Shutdown signal error: {:?}", e);
                        StateTransition::Shutdown
                    }
                }
            }
        }
    }

    // ========================================================================
    // HELPER METHODS
    // ========================================================================

    async fn call_init_function(&self, resources: &ActorResources) {
        let actor_handle = self.actor_handle.clone();
        let id = self.id.clone();

        tokio::spawn(async move {
            match actor_handle
                .call_function::<(String,), ()>(
                    "theater:simple/actor.init".to_string(),
                    (id.to_string(),),
                )
                .await
            {
                Ok(_) => debug!("Init function completed for actor: {:?}", id),
                Err(e) => error!("Failed to call init function: {}", e),
            }
        });
    }

    fn spawn_operation(
        &self,
        resources: &ActorResources,
        name: String,
        params: Vec<u8>,
        response_tx: oneshot::Sender<Result<Vec<u8>, ActorError>>,
    ) -> JoinHandle<Result<Vec<u8>, ActorError>> {
        let instance = resources.instance.clone();
        let metrics = resources.metrics.clone();
        let theater_tx = self.theater_tx.clone();
        let actor_id = self.id.clone();

        tokio::spawn(async move {
            let mut instance = instance.write().await;
            let metrics = metrics.read().await;

            match Self::execute_call(&mut instance, &name, params, &theater_tx, &metrics).await {
                Ok(result) => {
                    let _ = response_tx.send(Ok(result.clone()));
                    Ok(result)
                }
                Err(error) => {
                    // Notify theater of error
                    let _ = theater_tx
                        .send(TheaterCommand::ActorError {
                            actor_id,
                            error: error.clone(),
                        })
                        .await;

                    let _ = response_tx.send(Err(error.clone()));
                    Err(error)
                }
            }
        })
    }

    async fn handle_info_request(&self, resources: &ActorResources, status: &str) {
        // Implementation similar to original, but cleaner since we have resources
        // This is a placeholder - you'd implement the actual info handling here
    }

    async fn handle_info_request_during_startup(&self, info: ActorInfo, status: &str) {
        // Handle info requests that can work even during startup
        match info {
            ActorInfo::GetStatus { response_tx } => {
                let _ = response_tx.send(Ok(status.to_string()));
            }
            _ => {
                // Other info requests need the actor to be ready
            }
        }
    }

    async fn notify_error(&self, error: ActorError) {
        let _ = self
            .theater_tx
            .send(TheaterCommand::ActorError {
                actor_id: self.id.clone(),
                error,
            })
            .await;
    }

    async fn transition_to_shutdown(&mut self) {
        info!("Actor {} transitioning to shutdown", self.id);

        // Extract resources if we have them
        let resources = match std::mem::replace(&mut self.state, ActorState::ShuttingDown) {
            ActorState::Idle { resources } | ActorState::Paused { resources } => Some(resources),
            ActorState::Processing {
                resources,
                current_operation,
                ..
            } => {
                current_operation.abort();
                Some(resources)
            }
            _ => None,
        };

        if let Some(resources) = resources {
            Self::perform_cleanup(resources).await;
        }
    }

    async fn perform_cleanup(resources: ActorResources) {
        info!("Performing cleanup");

        // Signal shutdown to handlers
        resources
            .shutdown_controller
            .signal_shutdown(ShutdownType::Graceful)
            .await;

        // Abort any handlers that don't shut down gracefully
        for task in resources.handler_tasks {
            if !task.is_finished() {
                task.abort();
            }
        }

        // Log final metrics
        let metrics = resources.metrics.read().await;
        let final_metrics = metrics.get_metrics().await;
        info!("Final metrics: {:?}", final_metrics);
    }

    // ========================================================================
    // COPIED FROM ORIGINAL (these would stay mostly the same)
    // ========================================================================

    async fn build_actor_resources(
        id: TheaterId,
        config: ManifestConfig,
        initial_state: Option<serde_json::Value>,
        theater_tx: Sender<TheaterCommand>,
        actor_sender: Sender<ActorMessage>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_tx: Sender<ActorOperation>,
        info_tx: Sender<ActorInfo>,
        control_tx: Sender<ActorControl>,
        init: bool,
        engine: wasmtime::Engine,
        parent_permissions: crate::config::permissions::HandlerPermission,
        status_tx: Sender<String>,
        chain: Arc<SyncRwLock<StateChain>>,
        handler_registry: crate::handler::HandlerRegistry,
    ) -> Result<SetupComplete, ActorError> {
        // This would be mostly the same as your current build_actor_resources
        // Just returns SetupComplete instead of a tuple
        todo!("Copy implementation from original")
    }

    async fn execute_call(
        actor_instance: &mut ActorInstance,
        name: &String,
        params: Vec<u8>,
        theater_tx: &Sender<TheaterCommand>,
        metrics: &MetricsCollector,
    ) -> Result<Vec<u8>, ActorError> {
        // Copy from original - this is fine as-is
        todo!("Copy implementation from original")
    }
}

// ============================================================================
// STATE TRANSITION ENUM
// ============================================================================

enum StateTransition {
    /// Continue with a new state
    Continue(ActorState),
    /// Shut down the actor
    Shutdown,
    /// Fatal error occurred
    Error(ActorError),
}
