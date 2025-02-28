use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilesystemEventData {
    // Read file events
    FileReadCall {
        path: String,
    },
    FileReadResult {
        bytes_read: usize,
        success: bool,
    },

    // Write file events
    FileWriteCall {
        path: String,
        data_size: usize,
    },
    FileWriteResult {
        path: String,
        bytes_written: usize,
        success: bool,
    },

    // Delete file events
    FileDeleteCall {
        path: String,
    },
    FileDeleteResult {
        path: String,
        success: bool,
    },

    // Directory events
    DirectoryCreatedCall {
        path: String,
    },
    DirectoryCreatedResult {
        success: bool,
    },

    DirectoryDeletedCall {
        path: String,
    },
    DirectoryDeletedResult {
        success: bool,
    },

    // Directory listing events
    DirectoryListedCall {
        path: String,
    },
    DirectoryListResult {
        entries: Vec<String>,
        success: bool,
    },

    // Path exists events
    PathExistsCall {
        path: String,
    },
    PathExistsResult {
        path: String,
        exists: bool,
        success: bool,
    },

    // Error events
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
