use serde::{Deserialize, Serialize};
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilesystemEventData {
    // Read file events
    FileReadCall {
        path: String,
    },
    FileReadResult {
        success: bool,
        contents: Vec<u8>,
    },

    // Write file events
    FileWriteCall {
        path: String,
        contents: Vec<u8>,
    },
    FileWriteResult {
        path: String,
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
        path: String,
        success: bool,
    },

    DirectoryDeletedCall {
        path: String,
    },
    DirectoryDeletedResult {
        path: String,
        success: bool,
    },

    // Directory listing events
    DirectoryListedCall {
        path: String,
    },
    DirectoryListResult {
        path: String,
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

    // Command events
    CommandExecuted {
        directory: String,
        command: String,
        args: Vec<String>,
    },
    NixCommandExecuted {
        directory: String,
        command: String,
    },
    CommandCompleted {
        result: CommandResult,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum CommandResult {
    #[component(name = "success")]
    Success(CommandSuccess),
    #[component(name = "error")]
    Error(CommandError),
}

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct CommandSuccess {
    pub stdout: String,
    pub stderr: String,
    #[component(name = "exit-code")]
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct CommandError {
    pub message: String,
}

pub struct FilesystemEvent {
    pub data: FilesystemEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
