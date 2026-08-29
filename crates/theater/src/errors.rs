use crate::actor::runtime::ActorRuntimeError;
use crate::actor::ActorError;
use crate::id::TheaterId;
use thiserror::Error;

/// Why an actor failed to spawn. Carries the structured cause from the phase
/// that failed so callers (the supervisor handler) can react to the specific
/// reason instead of substring-matching a flattened string. Sent back over
/// `SpawnActor`/`SetupActor`'s `response_tx`.
#[derive(Debug, Error)]
pub enum SpawnError {
    /// Building the actor's handler registry from its manifest failed.
    #[error("failed to build handler registry: {0}")]
    HandlerRegistry(String),

    /// The actor failed to set up — wasm instantiation, interface-hash
    /// verification, missing `__pack_types` metadata, etc. (see the variant).
    #[error(transparent)]
    Setup(#[from] ActorRuntimeError),

    /// The setup task ended without reporting a result (it panicked or was
    /// dropped before signalling success or failure).
    #[error("actor setup task ended without reporting a result")]
    SetupChannelClosed,

    /// The actor's `init` export returned an error or trapped.
    #[error("actor.init failed: {0}")]
    Init(#[from] ActorError),
}

/// # Theater Runtime Error
///
/// Represents specific error conditions that can occur in the Theater runtime system.
/// These structured errors allow for better error handling and provide more context
/// about what went wrong.
#[derive(Debug, Clone, Error)]
pub enum TheaterRuntimeError {
    /// Actor not found in the runtime
    #[error("Actor not found: {0}")]
    ActorNotFound(TheaterId),

    /// Actor already exists with the given ID
    #[error("Actor already exists: {0}")]
    ActorAlreadyExists(TheaterId),

    /// Actor exists but is not in running state
    #[error("Actor not running: {0}")]
    ActorNotRunning(TheaterId),

    /// Actor operation failed
    #[error("Actor operation failed: {0}")]
    ActorOperationFailed(String),

    /// Error from within an actor
    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    /// Error with communication channels
    #[error("Channel error: {0}")]
    ChannelError(String),

    /// Channel not found
    #[error("Channel not found: {0}")]
    ChannelNotFound(String),

    /// Channel rejected by target
    #[error("Channel rejected by target")]
    ChannelRejected,

    /// Error with serialization/deserialization
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Error during actor initialization
    #[error("Actor initialization error: {0}")]
    ActorInitializationError(String),

    /// Internal runtime error
    #[error("Internal error: {0}")]
    InternalError(String),
}

// Helper method to convert from other errors
impl TheaterRuntimeError {
    pub fn from_error<E: std::error::Error + 'static>(error: E) -> Self {
        Self::InternalError(error.to_string())
    }
}
