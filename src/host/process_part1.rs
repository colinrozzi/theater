use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::ProcessHostConfig;
use crate::events::process::ProcessEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Errors that can occur in the ProcessHost
#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Process output error: {0}")]
    OutputError(String),

    #[error("Process not found: {0}")]
    ProcessNotFound(u64),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Path not allowed: {0}")]
    PathNotAllowed(String),

    #[error("Program not allowed: {0}")]
    ProgramNotAllowed(String),

    #[error("Too many processes")]
    TooManyProcesses,

    #[error("OS error: {0}")]
    OsError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Output processing mode for process stdout/stderr
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum OutputMode {
    Raw = 0,
    LineByLine = 1,
    Json = 2,
    Chunked = 3,
}

/// Represents an OS process managed by Theater
struct ManagedProcess {
    /// Unique ID for this process (within Theater)
    id: u64,
    /// Child process handle
    child: Option<Child>,
    /// OS process ID
    os_pid: Option<u32>,
    /// Process configuration
    config: ProcessConfig,
    /// When the process was started
    start_time: SystemTime,
    /// Handle to the stdin writer task
    stdin_writer: Option<JoinHandle<()>>,
    /// Channel to send data to the stdin writer
    stdin_tx: Option<mpsc::Sender<Vec<u8>>>,
    /// Handle to the stdout reader task
    stdout_reader: Option<JoinHandle<()>>,
    /// Handle to the stderr reader task
    stderr_reader: Option<JoinHandle<()>>,
    /// Last known exit code
    exit_code: Option<i32>,
}

/// Configuration for a process
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    /// Executable path
    pub program: String,
    /// Command line arguments
    pub args: Vec<String>,
    /// Working directory
    pub cwd: Option<String>,
    /// Environment variables
    pub env: Vec<(String, String)>,
    /// Buffer size for stdout/stderr
    pub buffer_size: u32,
    /// How to process stdout
    pub stdout_mode: OutputMode,
    /// How to process stderr
    pub stderr_mode: OutputMode,
    /// Chunk size for chunked mode
    pub chunk_size: Option<u32>,
}

/// Status of a running process
#[derive(Debug, Clone, wasmtime::component::ComponentType, wasmtime::component::Lift, wasmtime::component::Lower)]
#[component(record)]
pub struct ProcessStatus {
    /// Process ID (within Theater)
    pub pid: u64,
    /// Whether the process is running
    pub running: bool,
    /// Exit code if not running
    pub exit_code: Option<i32>,
    /// Start time in milliseconds since epoch
    pub start_time: u64,
    /// CPU usage percentage (not implemented yet)
    pub cpu_usage: f32,
    /// Memory usage in bytes (not implemented yet)
    pub memory_usage: u64,
}

/// Parse process configuration from WIT components
fn parse_process_config(config_wit: serde_json::Value) -> ProcessConfig {
    let program = config_wit["program"].as_str().unwrap_or("").to_string();
    
    let args = if let Some(args_array) = config_wit["args"].as_array() {
        args_array.iter()
            .filter_map(|arg| arg.as_str())
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    
    let cwd = config_wit["cwd"].as_str().map(|s| s.to_string());
    
    let env = if let Some(env_array) = config_wit["env"].as_array() {
        env_array.iter()
            .filter_map(|pair| {
                if let Some(pair_array) = pair.as_array() {
                    if pair_array.len() == 2 {
                        let key = pair_array[0].as_str()?.to_string();
                        let value = pair_array[1].as_str()?.to_string();
                        Some((key, value))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    
    let buffer_size = config_wit["buffer-size"].as_u64().unwrap_or(4096) as u32;
    
    let stdout_mode = match config_wit["stdout-mode"].as_u64().unwrap_or(0) {
        0 => OutputMode::Raw,
        1 => OutputMode::LineByLine,
        2 => OutputMode::Json,
        3 => OutputMode::Chunked,
        _ => OutputMode::Raw,
    };
    
    let stderr_mode = match config_wit["stderr-mode"].as_u64().unwrap_or(0) {
        0 => OutputMode::Raw,
        1 => OutputMode::LineByLine,
        2 => OutputMode::Json,
        3 => OutputMode::Chunked,
        _ => OutputMode::Raw,
    };
    
    let chunk_size = config_wit["chunk-size"].as_u64().map(|v| v as u32);
    
    ProcessConfig {
        program,
        args,
        cwd,
        env,
        buffer_size,
        stdout_mode,
        stderr_mode,
        chunk_size,
    }
}

/// Host handler for spawning and managing OS processes
#[derive(Clone)]
pub struct ProcessHost {
    /// Configuration for the process host
    config: ProcessHostConfig,
    /// Map of process IDs to managed processes
    processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    /// Next process ID to assign
    next_process_id: Arc<Mutex<u64>>,
    /// Actor handle for sending events
    actor_handle: Option<ActorHandle>,
}
