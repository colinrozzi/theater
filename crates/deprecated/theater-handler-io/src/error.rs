//! WASI I/O Error resource implementation
//!
//! Provides the `wasi:io/error` resource type for representing I/O errors
//! and the `stream-error` variant type for stream operations.

use std::fmt;
use wasmtime::component::{ComponentType, Lift, Lower, Resource};

/// Error resource for WASI I/O operations
///
/// This represents an error that occurred during stream operations.
/// The only method is `to-debug-string` which returns a human-readable description.
#[derive(Debug, Clone)]
pub struct IoError {
    /// The error message
    pub message: String,

    /// Optional error code or category
    pub kind: Option<String>,
}

impl IoError {
    /// Create a new I/O error with a message
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: None,
        }
    }

    /// Create an I/O error with a message and kind
    pub fn with_kind(message: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: Some(kind.into()),
        }
    }

    /// Get a debug string representation of this error
    ///
    /// This is the implementation of the `to-debug-string` method
    /// from the WIT interface.
    pub fn to_debug_string(&self) -> String {
        if let Some(kind) = &self.kind {
            format!("{}: {}", kind, self.message)
        } else {
            self.message.clone()
        }
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_debug_string())
    }
}

impl std::error::Error for IoError {}

impl From<std::io::Error> for IoError {
    fn from(err: std::io::Error) -> Self {
        Self::with_kind(err.to_string(), format!("{:?}", err.kind()))
    }
}

impl From<String> for IoError {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for IoError {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// WASI stream-error variant type
///
/// Matches the WIT definition:
/// ```wit
/// variant stream-error {
///     last-operation-failed(error),
///     closed
/// }
/// ```
#[derive(Debug, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum StreamError {
    /// The last operation failed with this error
    #[component(name = "last-operation-failed")]
    LastOperationFailed(Resource<IoError>),
    /// The stream is closed
    #[component(name = "closed")]
    Closed,
}
