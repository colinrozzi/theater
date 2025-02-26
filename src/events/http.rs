use super::ChainEventData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpEvent {
    RequestReceived {
        method: String,
        path: String,
        headers_count: usize,
        body_size: usize,
    },
    ResponseSent {
        status: u16,
        headers_count: usize,
        body_size: usize,
    },
    Error {
        message: String,
        code: Option<u16>,
    },
}

impl ChainEventData for HttpEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::RequestReceived { .. } => "http.request_received",
            Self::ResponseSent { .. } => "http.response_sent",
            Self::Error { .. } => "http.error",
        }
    }
    
    fn description(&self) -> String {
        match self {
            Self::RequestReceived { method, path, headers_count, body_size } => {
                format!("HTTP {} request to {} ({} headers, {} bytes)", 
                    method, path, headers_count, body_size)
            },
            Self::ResponseSent { status, headers_count, body_size } => {
                format!("HTTP {} response ({} headers, {} bytes)", 
                    status, headers_count, body_size)
            },
            Self::Error { message, code } => {
                if let Some(code) = code {
                    format!("HTTP error {}: {}", code, message)
                } else {
                    format!("HTTP error: {}", message)
                }
            },
        }
    }
}
