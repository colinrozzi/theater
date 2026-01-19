//! HTTP client handler event types

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpEventData {
    // Client request events
    HttpClientRequestCall {
        method: String,
        url: String,
        headers_count: usize,
        body: Option<String>,
    },
    HttpClientRequestResult {
        status: u16,
        headers_count: usize,
        success: bool,
        body: Option<String>,
    },

    // Error events
    Error {
        operation: String,
        path: String,
        message: String,
    },

    // Permission events
    PermissionDenied {
        operation: String,
        method: String,
        url: String,
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

pub struct HttpEvent {
    pub data: HttpEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
