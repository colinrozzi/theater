//! Type definitions for filesystem handler

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileSystemError {
    #[error("Path error: {0}")]
    PathError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) enum FileSystemCommand {
    ReadFile { path: String },
    WriteFile { path: String, contents: String },
    ListFiles { path: String },
    DeleteFile { path: String },
    CreateDir { path: String },
    DeleteDir { path: String },
    PathExists { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) enum FileSystemResponse {
    ReadFile(Result<Vec<u8>, String>),
    WriteFile(Result<(), String>),
    ListFiles(Result<Vec<String>, String>),
    DeleteFile(Result<(), String>),
    CreateDir(Result<(), String>),
    DeleteDir(Result<(), String>),
    PathExists(Result<bool, String>),
}
