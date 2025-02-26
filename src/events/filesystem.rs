use super::ChainEventData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilesystemEvent {
    FileRead {
        path: String,
        bytes_read: usize,
        success: bool,
    },
    FileWrite {
        path: String,
        bytes_written: usize,
        success: bool,
    },
    DirectoryCreated {
        path: String,
        success: bool,
    },
    DirectoryListed {
        path: String,
        entries_count: usize,
        success: bool,
    },
    Error {
        operation: String,
        path: String,
        message: String,
    },
}

impl ChainEventData for FilesystemEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::FileRead { .. } => "filesystem.file_read",
            Self::FileWrite { .. } => "filesystem.file_write",
            Self::DirectoryCreated { .. } => "filesystem.dir_created",
            Self::DirectoryListed { .. } => "filesystem.dir_listed",
            Self::Error { .. } => "filesystem.error",
        }
    }
    
    fn description(&self) -> String {
        match self {
            Self::FileRead { path, bytes_read, success } => {
                if *success {
                    format!("Read {} bytes from file {}", bytes_read, path)
                } else {
                    format!("Failed to read from file {}", path)
                }
            },
            Self::FileWrite { path, bytes_written, success } => {
                if *success {
                    format!("Wrote {} bytes to file {}", bytes_written, path)
                } else {
                    format!("Failed to write to file {}", path)
                }
            },
            Self::DirectoryCreated { path, success } => {
                if *success {
                    format!("Created directory {}", path)
                } else {
                    format!("Failed to create directory {}", path)
                }
            },
            Self::DirectoryListed { path, entries_count, success } => {
                if *success {
                    format!("Listed directory {} with {} entries", path, entries_count)
                } else {
                    format!("Failed to list directory {}", path)
                }
            },
            Self::Error { operation, path, message } => {
                format!("Filesystem error during {}: {} (path: {})", operation, message, path)
            },
        }
    }
}
