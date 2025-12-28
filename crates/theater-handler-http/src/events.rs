//! Event types for WASI HTTP operations
//!
//! These events are logged to Theater's event chain to track all HTTP operations
//! for replay and verification purposes.

use serde::{Deserialize, Serialize};

/// Event data for WASI HTTP operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HttpEventData {
    // === Outgoing Request Events ===
    /// outgoing-handler.handle called
    OutgoingRequestCall {
        method: String,
        uri: String,
    },

    /// outgoing-handler.handle result
    OutgoingRequestResult {
        status_code: u16,
        success: bool,
    },

    // === Incoming Request Events ===
    /// Server received an incoming HTTP request
    IncomingRequestReceived {
        method: String,
        path: String,
    },

    /// incoming-handler.handle called
    IncomingRequestCall {
        method: String,
        path: String,
    },

    /// incoming-handler.handle result
    IncomingRequestResult {
        status_code: u16,
        success: bool,
    },

    // === Header Operations ===
    /// headers.get called
    HeadersGetCall {
        name: String,
    },

    /// headers.get result
    HeadersGetResult {
        found: bool,
        value: Option<String>,
    },

    /// headers.set called
    HeadersSetCall {
        name: String,
        value_len: usize,
    },

    /// headers.set result
    HeadersSetResult {
        success: bool,
    },

    // === Body Stream Events ===
    /// incoming-body.stream called
    IncomingBodyStreamCall,

    /// incoming-body.stream result
    IncomingBodyStreamResult {
        success: bool,
    },

    /// outgoing-body.write called
    OutgoingBodyWriteCall {
        len: usize,
    },

    /// outgoing-body.write result
    OutgoingBodyWriteResult {
        success: bool,
    },
}
