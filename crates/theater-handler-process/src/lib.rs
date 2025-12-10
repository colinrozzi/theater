//! # Process Handler
//!
//! Provides OS process spawning and management capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to spawn processes, manage their I/O, and monitor their lifecycle with
//! permission-based access control.
//!
//! ## Features
//!
//! - Process spawning with full configuration
//! - Process I/O management (stdin/stdout/stderr)
//! - Multiple output modes (raw, line-by-line, JSON, chunked)
//! - Process lifecycle monitoring
//! - Execution timeouts
//! - Permission-based access control
//! - Complete event chain recording for auditability
//!
//! ## Operations
//!
//! - `os-spawn` - Spawn a new OS process
//! - `os-write-stdin` - Write data to process stdin
//! - `os-status` - Get process status
//! - `os-kill` - Kill a process
//! - `os-signal` - Send signal to a process
//!
//! ## Usage
//!
//! ```rust
//! use theater_handler_process::ProcessHandler;
//! use theater::config::actor_manifest::ProcessHostConfig;
//! use theater::actor::handle::ActorHandle;
//!
//! # fn example() {
//! let config = ProcessHostConfig {
//!     max_processes: 10,
//!     max_output_buffer: 1024,
//!     allowed_programs: None,
//!     allowed_paths: None,
//! };
//! let (operation_tx, _) = tokio::sync::mpsc::channel(100);
//! let (info_tx, _) = tokio::sync::mpsc::channel(100);
//! let (control_tx, _) = tokio::sync::mpsc::channel(100);
//! let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);
//! let handler = ProcessHandler::new(config, actor_handle, None);
//! # }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::component::{ComponentType, Lift, Lower};
use wasmtime::StoreContextMut;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::ProcessHostConfig;
use theater::config::enforcement::PermissionChecker;
use theater::events::process::ProcessEventData;
use theater::events::{ChainEventData, EventData};
use theater::handler::Handler;
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

/// Errors that can occur in the ProcessHandler
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
#[derive(Debug, Clone, Copy, PartialEq, ComponentType, Lift, Lower, Deserialize, Serialize)]
#[component(variant)]
pub enum OutputMode {
    #[component(name = "raw")]
    Raw,
    #[component(name = "line-by-line")]
    LineByLine,
    #[component(name = "json")]
    Json,
    #[component(name = "chunked")]
    Chunked,
}

/// Configuration for a process
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
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
    #[component(name = "buffer-size")]
    pub buffer_size: u32,
    /// How to process stdout
    #[component(name = "stdout-mode")]
    pub stdout_mode: OutputMode,
    /// How to process stderr
    #[component(name = "stderr-mode")]
    pub stderr_mode: OutputMode,
    /// Chunk size for chunked mode
    #[component(name = "chunk-size")]
    pub chunk_size: Option<u32>,
    /// Execution timeout in seconds (None = no timeout)
    #[component(name = "execution-timeout")]
    pub execution_timeout: Option<u64>,
}

/// Status of a running process
#[derive(Debug, Clone, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ProcessStatus {
    /// Process ID (within Theater)
    pub pid: u64,
    /// Whether the process is running
    pub running: bool,
    /// Exit code if not running
    #[component(name = "exit-code")]
    pub exit_code: Option<i32>,
    /// Start time in milliseconds since epoch
    #[component(name = "start-time")]
    pub start_time: u64,
}

/// Represents an OS process managed by Theater
#[allow(dead_code)]
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
    /// Timeout monitoring task
    timeout_handle: Option<JoinHandle<()>>,
    /// Flag to indicate if process was terminated due to timeout
    timeout_terminated: bool,
}

/// Handler for providing OS process spawning and management to WebAssembly actors
#[derive(Clone)]
pub struct ProcessHandler {
    /// Configuration for the process handler
    config: ProcessHostConfig,
    /// Map of process IDs to managed processes
    processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    /// Next process ID to assign
    next_process_id: Arc<Mutex<u64>>,
    /// Actor handle for sending events
    actor_handle: ActorHandle,
    /// Permission configuration
    permissions: Option<theater::config::permissions::ProcessPermissions>,
}

impl ProcessHandler {
    /// Create a new ProcessHandler with the given configuration
    pub fn new(
        config: ProcessHostConfig,
        actor_handle: ActorHandle,
        permissions: Option<theater::config::permissions::ProcessPermissions>,
    ) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            next_process_id: Arc::new(Mutex::new(1)),
            actor_handle,
            permissions,
        }
    }

    /// Process output from a child process
    async fn process_output<R>(
        mut reader: R,
        mode: OutputMode,
        buffer_size: usize,
        process_id: u64,
        _actor_id: theater::id::TheaterId,
        _theater_tx: tokio::sync::mpsc::Sender<theater::messages::TheaterCommand>,
        actor_handle: ActorHandle,
        handler: String,
    ) where
        R: AsyncReadExt + Unpin + Send + 'static,
    {
        match mode {
            OutputMode::Raw => {
                let mut buffer = vec![0; buffer_size];
                loop {
                    match reader.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            let data = buffer[0..n].to_vec();
                            let _ = actor_handle
                                .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                .await;
                        }
                        Ok(_) => break,
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
            OutputMode::LineByLine => {
                let mut line = vec![];
                let mut buffer = vec![0; 1];

                loop {
                    match reader.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            if buffer[0] == b'\n' {
                                if !line.is_empty() {
                                    let data = line.clone();
                                    let _ = actor_handle
                                        .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                        .await;
                                    line.clear();
                                }
                            } else {
                                line.push(buffer[0]);
                                if line.len() >= buffer_size {
                                    let data = line.clone();
                                    let _ = actor_handle
                                        .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                        .await;
                                    line.clear();
                                }
                            }
                        }
                        Ok(_) => {
                            if !line.is_empty() {
                                let data = line.clone();
                                let _ = actor_handle
                                    .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                    .await;
                            }
                            break;
                        }
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
            OutputMode::Json => {
                let mut buffer = String::new();
                let mut temp_buffer = vec![0; 1024];

                loop {
                    match reader.read(&mut temp_buffer).await {
                        Ok(n) if n > 0 => {
                            let chunk = String::from_utf8_lossy(&temp_buffer[0..n]);
                            buffer.push_str(&chunk);

                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[0..pos].trim().to_string();
                                let remaining = buffer[pos + 1..].to_string();
                                buffer = remaining;

                                if !line.is_empty() {
                                    if serde_json::from_str::<serde_json::Value>(&line).is_ok() {
                                        let data = line.as_bytes().to_vec();
                                        let _ = actor_handle
                                            .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                            .await;
                                    }
                                }
                            }

                            if buffer.len() > buffer_size {
                                let data = buffer.as_bytes().to_vec();
                                let _ = actor_handle
                                    .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                    .await;
                                buffer.clear();
                            }
                        }
                        Ok(_) => {
                            if !buffer.is_empty() {
                                let data = buffer.as_bytes().to_vec();
                                let _ = actor_handle
                                    .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                    .await;
                            }
                            break;
                        }
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
            OutputMode::Chunked => {
                let chunk_size = buffer_size;
                let mut buffer = vec![0; chunk_size];

                loop {
                    match reader.read_exact(&mut buffer).await {
                        Ok(_) => {
                            let data = buffer.clone();
                            let _ = actor_handle
                                .call_function::<(u64, Vec<u8>), ()>(handler.clone(), (process_id, data))
                                .await;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            break;
                        }
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Kill a process directly
    async fn kill_process_directly(
        processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
        process_id: u64,
    ) -> Result<(), ProcessError> {
        // Take the child out of the process struct while holding the lock
        let mut child_opt = {
            let mut processes_lock = processes.lock().unwrap();
            if let Some(process) = processes_lock.get_mut(&process_id) {
                process.child.take()
            } else {
                return Err(ProcessError::ProcessNotFound(process_id));
            }
        };

        // Kill the child without holding the lock
        if let Some(ref mut child) = child_opt {
            child.kill().await?;
        }

        Ok(())
    }
}

impl Handler for ProcessHandler {
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting process handler");

        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("Process handler received shutdown signal");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> anyhow::Result<()> {
        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "process-setup".to_string(),
            data: EventData::Process(ProcessEventData::HandlerSetupStart),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Starting process host function setup".to_string()),
        });

        info!("Setting up host functions for process handling");

        let mut interface = match actor_component.linker.instance("theater:simple/process") {
            Ok(interface) => {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "process-setup".to_string(),
                    data: EventData::Process(ProcessEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "process-setup".to_string(),
                    data: EventData::Process(ProcessEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/process: {}",
                    e
                ));
            }
        };

        // Setup: os-spawn - Spawn a new OS process
        let processes = self.processes.clone();
        let next_process_id = self.next_process_id.clone();
        let config = self.config.clone();
        let actor_handle = self.actor_handle.clone();
        let permissions = self.permissions.clone();

        interface.func_wrap_async(
            "os-spawn",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (process_config,): (ProcessConfig,)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<u64, String>,)>> + Send> {
                let processes = processes.clone();
                let next_process_id = next_process_id.clone();
                let _config = config.clone();
                let actor_handle = actor_handle.clone();
                let permissions = permissions.clone();

                Box::new(async move {
                    let stdout_mode = process_config.stdout_mode;
                    let stderr_mode = process_config.stderr_mode;
                    let program = process_config.program.clone();
                    let args = process_config.args.clone();
                    let cwd = process_config.cwd.clone();

                    // Permission check
                    let current_process_count = {
                        let processes_lock = processes.lock().unwrap();
                        processes_lock.len()
                    };

                    if let Err(e) = PermissionChecker::check_process_operation(
                        &permissions,
                        &program,
                        current_process_count,
                    ) {
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/permission-denied".to_string(),
                            data: EventData::Process(ProcessEventData::PermissionDenied {
                                operation: "spawn".to_string(),
                                program: program.clone(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Permission denied for process spawn: {}", e)),
                        });

                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }

                    // Get new process ID
                    let process_id = {
                        let mut id_lock = next_process_id.lock().unwrap();
                        let id = *id_lock;
                        *id_lock += 1;
                        id
                    };

                    // Record spawn attempt
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "process/spawn".to_string(),
                        data: EventData::Process(ProcessEventData::ProcessSpawn {
                            process_id,
                            program: program.clone(),
                            args: args.clone(),
                            os_pid: None,
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Attempting to spawn process: {}", program)),
                    });

                    // Build command
                    let mut command = Command::new(&program);
                    command.args(&args);
                    command.stdin(std::process::Stdio::piped());
                    command.stdout(std::process::Stdio::piped());
                    command.stderr(std::process::Stdio::piped());

                    if let Some(cwd_path) = cwd {
                        command.current_dir(&cwd_path);
                    }

                    for (key, value) in &process_config.env {
                        command.env(key, value);
                    }

                    // Spawn process
                    match command.spawn() {
                        Ok(mut child) => {
                            let os_pid = child.id();
                            let start_time = SystemTime::now();

                            // Set up stdin writer
                            let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(100);
                            let stdin_writer = if let Some(mut stdin) = child.stdin.take() {
                                Some(tokio::spawn(async move {
                                    while let Some(data) = stdin_rx.recv().await {
                                        if let Err(e) = stdin.write_all(&data).await {
                                            error!("Error writing to stdin: {}", e);
                                            break;
                                        }
                                    }
                                }))
                            } else {
                                None
                            };

                            // Set up stdout reader
                            let stdout_reader = if let Some(stdout) = child.stdout.take() {
                                let actor_id = ctx.data().id;
                                let theater_tx = ctx.data().theater_tx.clone();
                                let actor_handle_clone = actor_handle.clone();
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stdout,
                                        stdout_mode,
                                        process_config.buffer_size as usize,
                                        process_id,
                                        actor_id,
                                        theater_tx,
                                        actor_handle_clone,
                                        "theater:simple/process-handlers/handle-stdout".to_string(),
                                    )
                                    .await;
                                }))
                            } else {
                                None
                            };

                            // Set up stderr reader
                            let stderr_reader = if let Some(stderr) = child.stderr.take() {
                                let actor_id = ctx.data().id;
                                let theater_tx = ctx.data().theater_tx.clone();
                                let actor_handle_clone = actor_handle.clone();
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stderr,
                                        stderr_mode,
                                        process_config.buffer_size as usize,
                                        process_id,
                                        actor_id,
                                        theater_tx,
                                        actor_handle_clone,
                                        "theater:simple/process-handlers/handle-stderr".to_string(),
                                    )
                                    .await;
                                }))
                            } else {
                                None
                            };

                            // Set up timeout monitoring
                            let timeout_handle = if let Some(timeout_secs) = process_config.execution_timeout {
                                let processes_clone = processes.clone();
                                let pid = process_id;
                                Some(tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
                                    let _ = Self::kill_process_directly(processes_clone, pid).await;
                                }))
                            } else {
                                None
                            };

                            // Store managed process
                            let managed_process = ManagedProcess {
                                id: process_id,
                                child: Some(child),
                                os_pid,
                                config: process_config.clone(),
                                start_time,
                                stdin_writer,
                                stdin_tx: Some(stdin_tx),
                                stdout_reader,
                                stderr_reader,
                                exit_code: None,
                                timeout_handle,
                                timeout_terminated: false,
                            };

                            {
                                let mut processes_lock = processes.lock().unwrap();
                                processes_lock.insert(process_id, managed_process);
                            }

                            // Monitor process exit
                            let processes_monitor = processes.clone();
                            let actor_handle_exit = actor_handle.clone();
                            tokio::spawn(async move {
                                // Take the child out of the process struct
                                let mut child_opt = {
                                    let mut processes_lock = processes_monitor.lock().unwrap();
                                    if let Some(process) = processes_lock.get_mut(&process_id) {
                                        process.child.take()
                                    } else {
                                        None
                                    }
                                };

                                // Wait for process to exit without holding lock
                                if let Some(ref mut child) = child_opt {
                                    if let Ok(status) = child.wait().await {
                                        if let Some(code) = status.code() {
                                            // Record exit code in a separate block to ensure lock is dropped
                                            {
                                                let mut processes_lock = processes_monitor.lock().unwrap();
                                                if let Some(process) = processes_lock.get_mut(&process_id) {
                                                    process.exit_code = Some(code);
                                                }
                                            } // Lock is dropped here

                                            // Now safe to await
                                            let _ = actor_handle_exit
                                                .call_function::<(u64, i32), ()>(
                                                    "theater:simple/process-handlers/handle-exit".to_string(),
                                                    (process_id, code),
                                                )
                                                .await;
                                        }
                                    }
                                }
                            });

                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn-success".to_string(),
                                data: EventData::Process(ProcessEventData::ProcessSpawn {
                                    process_id,
                                    program: program.clone(),
                                    args: args.clone(),
                                    os_pid,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Successfully spawned process {} with OS PID {:?}",
                                    process_id, os_pid
                                )),
                            });

                            Ok((Ok(process_id),))
                        }
                        Err(e) => {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn-error".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: None,
                                    operation: "spawn".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to spawn process: {}", e)),
                            });

                            Ok((Err(format!("Failed to spawn process: {}", e)),))
                        }
                    }
                })
            },
        )?;

        // Setup: os-write-stdin - Write to process stdin
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-write-stdin",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (process_id, data): (u64, Vec<u8>)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();

                Box::new(async move {
                    // Clone the stdin sender to avoid holding the lock across await
                    let stdin_tx_opt = {
                        let processes_lock = processes.lock().unwrap();
                        if let Some(process) = processes_lock.get(&process_id) {
                            process.stdin_tx.clone()
                        } else {
                            None
                        }
                    };

                    let result = if let Some(stdin_tx) = stdin_tx_opt {
                        stdin_tx.send(data.clone()).await
                            .map_err(|e| format!("Failed to send to stdin: {}", e))
                    } else {
                        Err(format!("Process {} not found or has no stdin", process_id))
                    };

                    match result {
                        Ok(_) => {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/write-stdin".to_string(),
                                data: EventData::Process(ProcessEventData::StdinWrite {
                                    process_id,
                                    bytes_written: data.len() as u32,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Wrote {} bytes to process {} stdin", data.len(), process_id)),
                            });
                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/write-stdin-error".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(process_id),
                                    operation: "write-stdin".to_string(),
                                    message: e.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error writing to stdin: {}", e)),
                            });
                            Ok((Err(e),))
                        }
                    }
                })
            },
        )?;

        // Setup: os-status - Get process status
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-status",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (process_id,): (u64,)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<ProcessStatus, String>,)>> + Send> {
                let processes = processes.clone();

                Box::new(async move {
                    let result = {
                        let processes_lock = processes.lock().unwrap();
                        if let Some(process) = processes_lock.get(&process_id) {
                            let running = process.child.is_some();
                            let start_time = process.start_time
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;

                            Ok(ProcessStatus {
                                pid: process_id,
                                running,
                                exit_code: process.exit_code,
                                start_time,
                            })
                        } else {
                            Err(format!("Process {} not found", process_id))
                        }
                    };

                    match &result {
                        Ok(_status) => {
                            // Status check successful - event recorded via result
                        }
                        Err(e) => {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/status-error".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(process_id),
                                    operation: "status".to_string(),
                                    message: e.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error checking status: {}", e)),
                            });
                        }
                    }

                    Ok((result,))
                })
            },
        )?;

        // Setup: os-kill - Kill a process
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-kill",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (process_id,): (u64,)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();

                Box::new(async move {
                    let result = Self::kill_process_directly(processes, process_id)
                        .await
                        .map_err(|e| e.to_string());

                    match &result {
                        Ok(_) => {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/kill".to_string(),
                                data: EventData::Process(ProcessEventData::KillRequest { process_id }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Killed process {}", process_id)),
                            });
                        }
                        Err(e) => {
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/kill-error".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(process_id),
                                    operation: "kill".to_string(),
                                    message: e.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error killing process: {}", e)),
                            });
                        }
                    }

                    Ok((result,))
                })
            },
        )?;

        // Setup: os-signal - Send signal to process (Unix only, stub for cross-platform)
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-signal",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (process_id, _signal): (u64, i32)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<(), String>,)>> + Send> {
                let _processes = processes.clone();

                Box::new(async move {
                    // Signal sending is platform-specific and not implemented in this version
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "process/signal-not-implemented".to_string(),
                        data: EventData::Process(ProcessEventData::Error {
                            process_id: None,
                            operation: "signal".to_string(),
                            message: "Signal sending not implemented".to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Signal sending not implemented for process {}", process_id)),
                    });

                    Ok((Err("Signal sending not implemented".to_string()),))
                })
            },
        )?;

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "process-setup".to_string(),
            data: EventData::Process(ProcessEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Process host functions setup completed successfully".to_string()),
        });

        info!("Process host functions set up successfully");

        Ok(())
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> anyhow::Result<()> {
        info!("Adding export functions for process handling");

        // Register handle-stdout
        actor_instance.register_function_no_result::<(u64, Vec<u8>)>(
            "theater:simple/process-handlers",
            "handle-stdout",
        )?;

        // Register handle-stderr
        actor_instance.register_function_no_result::<(u64, Vec<u8>)>(
            "theater:simple/process-handlers",
            "handle-stderr",
        )?;

        // Register handle-exit
        actor_instance.register_function_no_result::<(u64, i32)>(
            "theater:simple/process-handlers",
            "handle-exit",
        )?;

        info!("Successfully registered all process handler export functions");

        Ok(())
    }

    fn name(&self) -> &str {
        "process"
    }

    fn imports(&self) -> Option<String> {
        Some("theater:simple/process".to_string())
    }

    fn exports(&self) -> Option<String> {
        Some("theater:simple/process-handlers".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_handle() -> ActorHandle {
        let (operation_tx, _) = tokio::sync::mpsc::channel(100);
        let (info_tx, _) = tokio::sync::mpsc::channel(100);
        let (control_tx, _) = tokio::sync::mpsc::channel(100);
        ActorHandle::new(operation_tx, info_tx, control_tx)
    }

    #[test]
    fn test_process_handler_creation() {
        let config = ProcessHostConfig {
            max_processes: 10,
            max_output_buffer: 1024,
            allowed_programs: None,
            allowed_paths: None,
        };
        let actor_handle = create_test_handle();

        let handler = ProcessHandler::new(config, actor_handle, None);

        assert_eq!(handler.name(), "process");
        assert_eq!(handler.imports(), Some("theater:simple/process".to_string()));
        assert_eq!(handler.exports(), Some("theater:simple/process-handlers".to_string()));
    }

    #[test]
    fn test_process_handler_clone() {
        let config = ProcessHostConfig {
            max_processes: 10,
            max_output_buffer: 1024,
            allowed_programs: None,
            allowed_paths: None,
        };
        let actor_handle = create_test_handle();

        let handler = ProcessHandler::new(config, actor_handle, None);
        let cloned = handler.create_instance();

        assert_eq!(cloned.name(), "process");
    }

    #[test]
    fn test_output_mode_serialization() {
        let mode = OutputMode::LineByLine;
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: OutputMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }
}
