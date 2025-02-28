use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpEventData {
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

pub struct HttpEvent {
    pub data: HttpEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
