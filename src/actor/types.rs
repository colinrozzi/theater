//! # Actor Types
//!
//! This module defines the core data types and error types used throughout the actor system.
//! These include operation types, error definitions, and other shared types needed for
//! the actor system to function.

use crate::metrics::ActorMetrics;
use crate::ChainEvent;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::time::Duration;
use wasmtime::component::{ComponentType, Lift, Lower};

/// Default timeout for actor operations (50 minutes)
pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(3000);

/// # ActorError
///
/// Represents errors that can occur during actor execution.
///
/// This enum provides detailed error information for various failure modes that
/// might occur when interacting with an actor. These errors are propagated back
/// to callers to help diagnose and handle problems.
#[derive(Error, Debug, Clone, ComponentType, Lift, Lower, Serialize, Deserialize)]
#[component(variant)]
pub enum ActorError {
    /// Operation exceeded the maximum allowed execution time
    #[error("Operation timed out after {0:?}")]
    #[component(name = "operation-timeout")]
    OperationTimeout(u64),

    /// Communication channel to the actor was closed unexpectedly
    #[error("Operation channel closed")]
    #[component(name = "channel-closed")]
    ChannelClosed,

    /// Actor is in the process of shutting down and cannot accept new operations
    #[error("Actor is shutting down")]
    #[component(name = "shutting-down")]
    ShuttingDown,

    /// The requested WebAssembly function was not found in the actor
    #[error("Function not found: {0}")]
    #[component(name = "function-not-found")]
    FunctionNotFound(String),

    /// Parameter or return types did not match the WebAssembly function signature
    #[error("Type mismatch for function {0}")]
    #[component(name = "type-mismatch")]
    TypeMismatch(String),

    /// An internal error occurred during execution
    #[error("Internal error: {0}")]
    #[component(name = "internal-error")]
    Internal(ChainEvent),

    /// Failed to serialize or deserialize data
    #[error("Serialization error")]
    #[component(name = "serialization-error")]
    SerializationError,

    /// Failed to update the actor's WebAssembly component
    #[error("Failed to update component: {0}")]
    #[component(name = "update-component-error")]
    UpdateComponentError(String),

    /// Actor is paused
    #[error("Actor is paused")]
    #[component(name = "actor-paused")]
    Paused,
}

/// # ActorOperation
///
/// Represents the different types of operations that can be performed on an actor.
///
/// This enum defines the message types that can be sent to an `ActorRuntime` via
/// its operation channel. Each variant includes the necessary data for the operation
/// and a oneshot channel sender for returning the result.
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
        response_tx: oneshot::Sender<Result<Vec<crate::ChainEvent>, ActorError>>,
    },
    /// Retrieve the actor's current state
    GetState {
        /// Channel to send state back to the caller
        response_tx: oneshot::Sender<Result<Option<Vec<u8>>, ActorError>>,
    },
    /// Update a WebAssembly component in the actor
    UpdateComponent {
        /// Address of the component to update
        component_address: String,
        /// Channel to send the result back to the caller
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
    /// Pause the actor
    Pause {
        /// Channel to confirm pause completion
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
    /// Resume the actor
    Resume {
        /// Channel to confirm resume completion
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
}
