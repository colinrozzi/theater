use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilesystemEventData {
    FileReadCall {
        path: String,
    },
    FileReadResult {
        bytes_read: usize,
        success: bool,
    },
    FileWriteCall {
        path: String,
        data_size: usize,
    },
    FileWriteResult {
        path: String,
        bytes_written: usize,
        success: bool,
    },
    DirectoryCreatedCall {
        path: String,
    },
    DirectoryCreatedResult {
        success: bool,
    },
    DirectoryListedCall {
        path: String,
    },
    DirectoryListResult {
        entries: Vec<String>,
        success: bool,
    },
    Error {
        operation: String,
        path: String,
        message: String,
    },
}

pub struct FilesystemEvent {
    pub data: FilesystemEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
