//! # Actor Types
//!
//! This module defines the core data types and error types used throughout the actor system.
//! These include operation types, error definitions, and other shared types needed for
//! the actor system to function.

use crate::pack_bridge::{InterfaceHash, Value};
use crate::ChainEvent;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::time::Duration;

/// Default timeout for actor operations (50 minutes)
pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(3000);

/// # ActorError
///
/// Represents errors that can occur during actor execution.
///
/// This enum provides detailed error information for various failure modes that
/// might occur when interacting with an actor. These errors are propagated back
/// to callers to help diagnose and handle problems.
#[derive(Error, Debug, Clone, Serialize, Deserialize, PartialEq, Hash, Eq)]
pub enum ActorError {
    /// Operation exceeded the maximum allowed execution time
    #[error("Operation timed out after {0:?}")]
    OperationTimeout(u64),

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
    Internal(ChainEvent),

    /// Unexpected error
    #[error("Unexpected error: {0}")]
    UnexpectedError(String),

    /// Failed to serialize or deserialize data
    #[error("Serialization error")]
    SerializationError,

    /// Actor is paused
    #[error("Actor is paused")]
    Paused,

    /// Actor is not paused
    #[error("Actor is not paused")]
    NotPaused,

    #[error("Handler error: {0}")]
    HandlerError(String),
}

/// # ActorOperation
///
/// Represents the different types of operations that can be performed on an actor.
///
/// This enum defines the message types that can be sent to an `ActorRuntime` via
/// its operation channel. Each variant includes the necessary data for the operation
/// and a oneshot channel sender for returning the result.
#[derive(Debug)]
pub enum ActorOperation {
    /// Call a WebAssembly function using Pack-native Value encoding.
    ///
    /// Params and result are Pack ABI encoded bytes (encode_value/decode_value).
    /// Preserves structured type information through the call boundary.
    CallFunctionPack {
        /// Name of the function to call
        name: String,
        /// Pack ABI encoded params (encode_value(Value) bytes)
        params: Vec<u8>,
        /// Channel to send the result back to the caller
        response_tx: oneshot::Sender<Result<Vec<u8>, ActorError>>,
    },
}

#[derive(Debug)]
pub enum ActorControl {
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
    /// Initiate actor shutdown
    Shutdown {
        /// Channel to confirm shutdown completion
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
    /// Forcefully terminate the actor, aborting any ongoing operations
    Terminate {
        /// Channel to confirm termination completion
        response_tx: oneshot::Sender<Result<(), ActorError>>,
    },
}

#[derive(Debug)]
pub enum ActorInfo {
    /// Retrieve the actor's current state
    GetState {
        /// Channel to send state back to the caller
        response_tx: oneshot::Sender<Result<Value, ActorError>>,
    },
    /// Retrieve the actor's current processing state
    GetStatus {
        /// Channel to send processing state back to the caller
        response_tx: oneshot::Sender<Result<String, ActorError>>,
    },
    /// Retrieve the actor's exported interface hashes
    GetExportHashes {
        /// Channel to send export hashes back to the caller
        response_tx: oneshot::Sender<Result<Vec<InterfaceHash>, ActorError>>,
    },
}
