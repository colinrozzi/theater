use crate::actor::ActorError;
use crate::id::TheaterId;
use thiserror::Error;

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
