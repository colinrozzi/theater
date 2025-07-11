//! # Random Number Generator Events
//!
//! Event data structures for tracking random number generation operations
//! within the Theater actor system. These events are recorded in actor
//! event chains for auditing and debugging purposes.

use serde::{Deserialize, Serialize};

/// Event data for random number generation operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RandomEventData {
    /// Random bytes generation was called
    #[serde(rename = "random_bytes_call")]
    RandomBytesCall {
        /// Number of bytes requested
        requested_size: usize,
    },

    /// Random bytes generation completed
    #[serde(rename = "random_bytes_result")]
    RandomBytesResult {
        /// Number of bytes actually generated
        generated_size: usize,
        /// Whether the operation was successful
        success: bool,
    },

    /// Random range generation was called
    #[serde(rename = "random_range_call")]
    RandomRangeCall {
        /// Minimum value of the range (inclusive)
        min: u64,
        /// Maximum value of the range (exclusive)
        max: u64,
    },

    /// Random range generation completed
    #[serde(rename = "random_range_result")]
    RandomRangeResult {
        /// Minimum value of the range that was requested
        min: u64,
        /// Maximum value of the range that was requested
        max: u64,
        /// The generated value
        value: u64,
        /// Whether the operation was successful
        success: bool,
    },

    /// Random float generation was called
    #[serde(rename = "random_float_call")]
    RandomFloatCall,

    /// Random float generation completed
    #[serde(rename = "random_float_result")]
    RandomFloatResult {
        /// The generated float value
        value: f64,
        /// Whether the operation was successful
        success: bool,
    },

    /// UUID generation was called
    #[serde(rename = "generate_uuid_call")]
    GenerateUuidCall,

    /// UUID generation completed
    #[serde(rename = "generate_uuid_result")]
    GenerateUuidResult {
        /// The generated UUID
        uuid: String,
        /// Whether the operation was successful
        success: bool,
    },

    /// An error occurred during random number generation
    #[serde(rename = "error")]
    Error {
        /// The operation that failed
        operation: String,
        /// Error message describing what went wrong
        message: String,
    },

    /// Permission was denied for random operation
    #[serde(rename = "permission_denied")]
    PermissionDenied {
        /// The operation that was denied
        operation: String,
        /// Reason for denial
        reason: String,
    },

    // Handler setup events
    HandlerSetupStart,
    HandlerSetupSuccess,
    HandlerSetupError {
        error: String,
        step: String,
    },
    LinkerInstanceSuccess,
    FunctionSetupStart {
        function_name: String,
    },
    FunctionSetupSuccess {
        function_name: String,
    },
}
