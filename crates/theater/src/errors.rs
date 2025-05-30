use crate::id::TheaterId;
use crate::actor::ActorError;
use std::fmt;

/// # Theater Runtime Error
///
/// Represents specific error conditions that can occur in the Theater runtime system.
/// These structured errors allow for better error handling and provide more context
/// about what went wrong.
#[derive(Debug, Clone)]
pub enum TheaterRuntimeError {
    /// Actor not found in the runtime
    ActorNotFound(TheaterId),
    
    /// Actor already exists with the given ID
    ActorAlreadyExists(TheaterId),
    
    /// Actor exists but is not in running state
    ActorNotRunning(TheaterId),
    
    /// Actor operation failed
    ActorOperationFailed(String),
    
    /// Error from within an actor
    ActorError(ActorError),
    
    /// Error with communication channels
    ChannelError(String),
    
    /// Channel not found
    ChannelNotFound(String),
    
    /// Channel rejected by target
    ChannelRejected,
    
    /// Error with serialization/deserialization
    SerializationError(String),
    
    /// Internal runtime error
    InternalError(String),
}

impl fmt::Display for TheaterRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActorNotFound(id) => write!(f, "Actor not found: {}", id),
            Self::ActorAlreadyExists(id) => write!(f, "Actor already exists: {}", id),
            Self::ActorNotRunning(id) => write!(f, "Actor not running: {}", id),
            Self::ActorOperationFailed(msg) => write!(f, "Actor operation failed: {}", msg),
            Self::ActorError(e) => write!(f, "Actor error: {}", e),
            Self::ChannelError(msg) => write!(f, "Channel error: {}", msg),
            Self::ChannelNotFound(id) => write!(f, "Channel not found: {}", id),
            Self::ChannelRejected => write!(f, "Channel rejected by target"),
            Self::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for TheaterRuntimeError {}

// Allow converting from ActorError to TheaterRuntimeError
impl From<ActorError> for TheaterRuntimeError {
    fn from(error: ActorError) -> Self {
        Self::ActorError(error)
    }
}

// Helper method to convert from other errors
impl TheaterRuntimeError {
    pub fn from_error<E: std::error::Error + 'static>(error: E) -> Self {
        Self::InternalError(error.to_string())
    }
}
