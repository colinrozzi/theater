use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpEventData {
    // Request handling events
    HttpRequestCall {
        method: String,
        path: String,
        headers_count: usize,
        body_size: usize,
    },
    HttpRequestResult {
        status: u16,
        headers_count: usize,
        body_size: usize,
        success: bool,
    },

    // Client request events
    HttpClientRequestCall {
        method: String,
        url: String,
        headers_count: usize,
        body_size: usize,
    },
    HttpClientRequestResult {
        status: u16,
        headers_count: usize,
        body_size: usize,
        success: bool,
    },

    // Error events
    Error {
        operation: String,
        path: String,
        message: String,
    },
}

pub struct HttpEvent {
    pub data: HttpEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
