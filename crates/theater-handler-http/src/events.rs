//! Event types for WASI HTTP operations
//!
//! These events are logged to Theater's event chain to track all HTTP operations
//! for replay and verification purposes.

use serde::{Deserialize, Serialize};

/// Complete HTTP request data for replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestData {
    /// HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: String,
    /// URL scheme (http or https)
    pub scheme: Option<String>,
    /// Authority (hostname:port)
    pub authority: Option<String>,
    /// Path with query string
    pub path_with_query: Option<String>,
    /// All request headers as (name, value) pairs
    /// Values are base64-encoded to handle binary data
    pub headers: Vec<(String, String)>,
    /// Request body as base64-encoded bytes
    pub body: Option<String>,
}

/// Complete HTTP response data for replay verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponseData {
    /// HTTP status code
    pub status_code: u16,
    /// All response headers as (name, value) pairs
    /// Values are base64-encoded to handle binary data
    pub headers: Vec<(String, String)>,
    /// Response body as base64-encoded bytes
    pub body: String,
}

/// Event data for WASI HTTP operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HttpEventData {
    // === Complete Incoming Request/Response (for replay) ===
    /// Complete incoming HTTP request and response for replay
    IncomingHttpCall {
        /// The complete HTTP request
        request: HttpRequestData,
        /// The complete HTTP response
        response: HttpResponseData,
    },

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

    // === Incoming Request Events (legacy, less detailed) ===
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
