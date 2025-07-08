use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::actor_manifest::ProcessHostConfig;
use crate::config::enforcement::PermissionChecker;
use crate::events::process::ProcessEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use wasmtime::component::{ComponentType, Lift, Lower};

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
#[derive(
    Debug,
    Clone,
    wasmtime::component::ComponentType,
    wasmtime::component::Lift,
    wasmtime::component::Lower,
)]
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

// Parse process config function no longer needed since we're using structs directly

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
    actor_handle: ActorHandle,
    /// Permission configuration
    permissions: Option<crate::config::permissions::ProcessPermissions>,
}

impl ProcessHost {
    /// Create a new ProcessHost with the given configuration
    pub fn new(config: ProcessHostConfig, actor_handle: ActorHandle, permissions: Option<crate::config::permissions::ProcessPermissions>) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            next_process_id: Arc::new(Mutex::new(1)),
            actor_handle,
            permissions,
        }
    }

    /// Start the process host
    pub async fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting ProcessHost");
        Ok(())
    }

    /// Add export functions to the actor instance
    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for process handling");

        // Register the process handler export functions
        actor_instance
            .register_function_no_result::<(u64, Vec<u8>)>(
                "theater:simple/process-handlers",
                "handle-stdout",
            )
            .expect("Failed to register handle-stdout function");

        actor_instance
            .register_function_no_result::<(u64, Vec<u8>)>(
                "theater:simple/process-handlers",
                "handle-stderr",
            )
            .expect("Failed to register handle-stderr function");

        actor_instance
            .register_function_no_result::<(u64, i32)>(
                "theater:simple/process-handlers",
                "handle-exit",
            )
            .expect("Failed to register handle-exit function");

        Ok(())
    }

    /// Process output from a child process
    async fn process_output<R>(
        mut reader: R,
        mode: OutputMode,
        buffer_size: usize,
        process_id: u64,
        _actor_id: crate::id::TheaterId,
        _theater_tx: tokio::sync::mpsc::Sender<crate::messages::TheaterCommand>,
        actor_handle: ActorHandle,
        handler: String,
    ) where
        R: AsyncReadExt + Unpin + Send + 'static,
    {
        match mode {
            OutputMode::Raw => {
                // Read in raw mode without any special processing
                let mut buffer = vec![0; buffer_size];
                loop {
                    match reader.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            // Send the output to the actor
                            let data = buffer[0..n].to_vec();
                            actor_handle
                                .call_function::<(u64, Vec<u8>), ()>(
                                    handler.clone(),
                                    (process_id, data),
                                )
                                .await
                                .expect("Failed to send chunk to actor");
                        }
                        Ok(_) => break, // EOF
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
            OutputMode::LineByLine => {
                // Process output line by line
                let mut line = vec![];
                let mut buffer = vec![0; 1]; // Read one byte at a time for line processing

                loop {
                    match reader.read(&mut buffer).await {
                        Ok(n) if n > 0 => {
                            if buffer[0] == b'\n' {
                                // Line complete, send it
                                if !line.is_empty() {
                                    // Send the line to the actor
                                    let data = line.clone();
                                    actor_handle
                                        .call_function::<(u64, Vec<u8>), ()>(
                                            handler.clone(),
                                            (process_id, data),
                                        )
                                        .await
                                        .expect("Failed to send chunk to actor");

                                    line.clear();
                                }
                            } else {
                                line.push(buffer[0]);

                                // Check if line is too long
                                if line.len() >= buffer_size {
                                    // Send the partial line to the actor
                                    let data = line.clone();
                                    actor_handle
                                        .call_function::<(u64, Vec<u8>), ()>(
                                            handler.clone(),
                                            (process_id, data),
                                        )
                                        .await
                                        .expect("Failed to send chunk to actor");

                                    line.clear();
                                }
                            }
                        }
                        Ok(_) => {
                            // EOF - send any remaining data
                            if !line.is_empty() {
                                // Send the line to the actor
                                let data = line.clone();
                                actor_handle
                                    .call_function::<(u64, Vec<u8>), ()>(
                                        handler.clone(),
                                        (process_id, data),
                                    )
                                    .await
                                    .expect("Failed to send chunk to actor");
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
                // Process output as JSON objects (newline-delimited)
                let mut buffer = String::new();
                let mut temp_buffer = vec![0; 1024]; // Read in chunks

                loop {
                    match reader.read(&mut temp_buffer).await {
                        Ok(n) if n > 0 => {
                            let chunk = String::from_utf8_lossy(&temp_buffer[0..n]);
                            buffer.push_str(&chunk);

                            // Process complete JSON objects
                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[0..pos].trim().to_string();
                                let remaining = buffer[pos + 1..].to_string();
                                buffer = remaining;

                                if !line.is_empty() {
                                    // Validate JSON
                                    if let Ok(_) = serde_json::from_str::<serde_json::Value>(&line)
                                    {
                                        // Valid JSON, send it
                                        let data = line.as_bytes().to_vec();
                                        actor_handle
                                            .call_function::<(u64, Vec<u8>), ()>(
                                                handler.clone(),
                                                (process_id, data),
                                            )
                                            .await
                                            .expect("Failed to send chunk to actor");
                                    }
                                }
                            }

                            // Check if buffer is too large
                            if buffer.len() > buffer_size {
                                // Buffer too large, flush it as raw data
                                let data = buffer.as_bytes().to_vec();
                                actor_handle
                                    .call_function::<(u64, Vec<u8>), ()>(
                                        handler.clone(),
                                        (process_id, data),
                                    )
                                    .await
                                    .expect("Failed to send chunk to actor");

                                buffer.clear();
                            }
                        }
                        Ok(_) => {
                            // EOF - send any remaining data
                            if !buffer.is_empty() {
                                let data = buffer.as_bytes().to_vec();
                                actor_handle
                                    .call_function::<(u64, Vec<u8>), ()>(
                                        handler.clone(),
                                        (process_id, data),
                                    )
                                    .await
                                    .expect("Failed to send chunk to actor");
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
                // Read in fixed-size chunks
                let chunk_size = match mode {
                    OutputMode::Chunked => buffer_size,
                    _ => unreachable!(), // We're in the Chunked match arm
                };

                let mut buffer = vec![0; chunk_size];

                loop {
                    match reader.read_exact(&mut buffer).await {
                        Ok(_) => {
                            // Send the chunk to the actor
                            let data = buffer.clone();
                            actor_handle
                                .call_function::<(u64, Vec<u8>), ()>(
                                    handler.clone(),
                                    (process_id, data),
                                )
                                .await
                                .expect("Failed to send chunk to actor");
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            // Partial chunk - read what's available
                            let mut partial_buffer = vec![0; chunk_size];
                            if let Ok(n) = reader.read(&mut partial_buffer).await {
                                if n > 0 {
                                    // Send the partial chunk to the actor
                                    let data = partial_buffer[0..n].to_vec();
                                    actor_handle
                                        .call_function::<(u64, Vec<u8>), ()>(
                                            handler.clone(),
                                            (process_id, data),
                                        )
                                        .await
                                        .expect("Failed to send chunk to actor");
                                }
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
        }
    }

    /// Set up WebAssembly host functions for the process interface
    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up host functions for process handling");

        let mut interface = actor_component
            .linker
            .instance("theater:simple/process")
            .expect("Could not instantiate theater:simple/process");

        // Implementation for os-spawn
        let processes = self.processes.clone();
        let next_process_id = self.next_process_id.clone();
        let config = self.config.clone();
        let actor_handle = self.actor_handle.clone();
        let permissions = self.permissions.clone();
        interface.func_wrap_async(
            "os-spawn",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (process_config,): (ProcessConfig,)|  // Process configuration from WebAssembly as struct
                  -> Box<dyn Future<Output = Result<(Result<u64, String>,)>> + Send> {
                let processes = processes.clone();
                let next_process_id = next_process_id.clone();
                let _config = config.clone();
                let actor_handle = actor_handle.clone();
                let permissions = permissions.clone();
                
                Box::new(async move {
                    // Convert u8 modes to OutputMode enum
                    let stdout_mode = process_config.stdout_mode;
                    let stderr_mode = process_config.stderr_mode;
                    
                    let program = process_config.program.clone();
                    let args = process_config.args.clone();
                    let cwd = process_config.cwd.clone();
                    
                    // PERMISSION CHECK BEFORE OPERATION
                    let current_process_count = {
                        let processes_lock = processes.lock().unwrap();
                        processes_lock.len()
                    };
                    
                    if let Err(e) = PermissionChecker::check_process_operation(
                        &permissions,
                        &program,
                        current_process_count,
                    ) {
                        // Record permission denied event
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

                    
                    // Get a new process ID
                    let process_id = {
                        let mut id_lock = next_process_id.lock().unwrap();
                        let id = *id_lock;
                        *id_lock += 1;
                        id
                    };
                    
                    // Start the process
                    let mut command = Command::new(&program);
                    command.args(&args);
                    
                    if let Some(cwd_path) = &cwd {
                        command.current_dir(cwd_path);
                    }
                    
                    for (key, value) in &process_config.env {
                        command.env(key, value);
                    }
                    
                    // Set up pipes for stdin, stdout, stderr
                    command.stdin(std::process::Stdio::piped());
                    command.stdout(std::process::Stdio::piped());
                    command.stderr(std::process::Stdio::piped());
                    
                    // Spawn the process
                    match command.spawn() {
                        Ok(mut child) => {
                            let _os_pid = child.id(); // We capture this for debugging purposes
                            
                            let actor_id = ctx.data().id.clone();
                            let theater_tx = ctx.data().theater_tx.clone();
                            
                            // Set up stdin channel
                            let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(32);
                            let mut stdin = child.stdin.take().expect("Failed to open stdin");
                            
                            // Handle stdin writes in a separate task
                            let stdin_writer = tokio::spawn(async move {
                                debug!("Starting stdin writer task for process {}", process_id);
                                while let Some(data) = stdin_rx.recv().await {
                                    debug!("Stdin writer received {} bytes for process {}", data.len(), process_id);
                                    if let Err(e) = stdin.write_all(&data).await {
                                        error!("Failed to write to process stdin: {}", e);
                                        break;
                                    }
                                    if let Err(e) = stdin.flush().await {
                                        error!("Failed to flush process stdin: {}", e);
                                        break;
                                    }
                                    debug!("Stdin writer Wrote {} bytes to stdin of process {}", data.len(), process_id);
                                }
                            });
                            
                            // Set up stdout reader
                            let stdout_handle = if let Some(stdout) = child.stdout.take() {
                                let stdout_mode = stdout_mode;
                                let buffer_size = process_config.buffer_size as usize;
                                let process_id_clone = process_id;
                                let actor_id_clone = actor_id.clone();
                                let theater_tx_clone = theater_tx.clone();
                                let actor_handle = actor_handle.clone();
                                
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stdout,
                                        stdout_mode,
                                        buffer_size,
                                        process_id_clone,
                                        actor_id_clone,
                                        theater_tx_clone,
                                        actor_handle.clone(),
                                        "theater:simple/process-handlers.handle-stdout".to_string(),
                                    ).await;
                                }))
                            } else {
                                None
                            };
                            
                            // Set up stderr reader
                            let stderr_handle = if let Some(stderr) = child.stderr.take() {
                                let stderr_mode = stderr_mode;
                                let buffer_size = process_config.buffer_size as usize;
                                let process_id_clone = process_id;
                                let actor_id_clone = actor_id.clone();
                                let theater_tx_clone = theater_tx.clone();
                                let actor_handle = actor_handle.clone();
                                
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stderr,
                                        stderr_mode,
                                        buffer_size,
                                        process_id_clone,
                                        actor_id_clone,
                                        theater_tx_clone,
                                        actor_handle.clone(),
                                        "theater:simple/process-handlers.handle-stderr".to_string(),
                                    ).await;
                                }))
                            } else {
                                None
                            };
                            
                            // Create separate task for monitoring
                            let processes_clone = processes.clone();
                            let process_id_clone = process_id;
                            let actor_id_clone = actor_id.clone();
                            let theater_tx_clone = theater_tx.clone();
                            let actor_handle_clone = actor_handle.clone(); // Clone actor_handle for use in the monitoring task
                            
                            // Create a mutable copy of the child for waiting (we'll take ownership later)
                            // But first store properties we need to keep in the ManagedProcess
                            let os_pid = child.id();
                            
                            // Spawn a task to wait for the process to exit
                            tokio::spawn(async move { // actor_handle_clone is available in this scope
                                // Move ownership of the child into this task
                                if let Ok(status) = child.wait().await {
                                    let exit_code = status.code().unwrap_or(-10);
                                    
                                    // IMPORTANT: Wait for stdout/stderr readers to complete before calling handle_exit
                                    // This prevents race conditions where the actor shuts down before all output is processed
                                    
                                    // Get handles to the reader tasks
                                    let (stdout_handle, stderr_handle) = {
                                        let mut processes = processes_clone.lock().unwrap();
                                        if let Some(process) = processes.get_mut(&process_id_clone) {
                                            (process.stdout_reader.take(), process.stderr_reader.take())
                                        } else {
                                            (None, None)
                                        }
                                    };
                                    
                                    // Wait for stdout reader to complete
                                    if let Some(handle) = stdout_handle {
                                        if let Err(e) = handle.await {
                                            error!("Error waiting for stdout reader to complete: {}", e);
                                        }
                                    }
                                    
                                    // Wait for stderr reader to complete
                                    if let Some(handle) = stderr_handle {
                                        if let Err(e) = handle.await {
                                            error!("Error waiting for stderr reader to complete: {}", e);
                                        }
                                    }
                                    
                                    // Give a small additional delay to ensure all async calls complete
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    
                                    // Cancel timeout if it exists
                                    if let Some(timeout_handle) = {
                                        let mut processes = processes_clone.lock().unwrap();
                                        if let Some(process) = processes.get_mut(&process_id_clone) {
                                            process.timeout_handle.take()
                                        } else {
                                            None
                                        }
                                    } {
                                        timeout_handle.abort();
                                    }
                                    
                                    // Check if process was terminated due to timeout
                                    let was_timeout_terminated = {
                                        let processes = processes_clone.lock().unwrap();
                                        if let Some(process) = processes.get(&process_id_clone) {
                                            process.timeout_terminated
                                        } else {
                                            false
                                        }
                                    };
                                    
                                    // Update the exit description
                                    let exit_description = if was_timeout_terminated {
                                        format!("Process {} exited with code {} (terminated due to timeout)", process_id_clone, exit_code)
                                    } else {
                                        format!("Process {} exited with code {}", process_id_clone, exit_code)
                                    };
                                    
                                    // Create the event data
                                    let event_data = ChainEventData {
                                        event_type: "process/exit".to_string(),
                                        data: EventData::Process(ProcessEventData::ProcessExit {
                                            process_id: process_id_clone,
                                            exit_code,
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(exit_description),
                                    };
                                    
                                    // Send exit event to the actor
                                    match theater_tx_clone.send(crate::messages::TheaterCommand::NewEvent {
                                        actor_id: actor_id_clone.clone(),
                                        event: event_data.to_chain_event(None),
                                    }).await {
                                        Ok(_) => {
                                            info!("Successfully sent exit event for process {} with code {}", process_id_clone, exit_code);
                                            
                                            // Now also explicitly call the actor's handle_exit function directly
                                            // This provides redundancy in case the event system fails
                                            match actor_handle_clone.call_function::<(u64, i32), ()>(
                                                "theater:simple/process-handlers.handle-exit".to_string(),
                                                (process_id_clone, exit_code)
                                            ).await {
                                                Ok(_) => info!("Successfully called handle_exit directly for process {}", process_id_clone),
                                                Err(e) => error!("Failed to call handle_exit directly for process {}: {}", process_id_clone, e),
                                            };
                                        },
                                        Err(e) => error!("Failed to send exit event for process {}: {}", process_id_clone, e),
                                    };
                                    
                                    // Update process status
                                    let mut processes = processes_clone.lock().unwrap();
                                    if let Some(process) = processes.get_mut(&process_id_clone) {
                                        process.exit_code = Some(exit_code);
                                        process.child = None;
                                    }
                                }
                            });
                            
                            // Set up execution timeout if configured
                            let timeout_handle = if let Some(timeout_secs) = process_config.execution_timeout {
                                if timeout_secs > 0 {
                                    let processes_for_timeout = processes.clone();
                                    let actor_handle_for_timeout = actor_handle.clone();
                                    let theater_tx_for_timeout = theater_tx.clone();
                                    let actor_id_for_timeout = actor_id.clone();
                                    
                                    Some(tokio::spawn(async move {
                                        // Wait for the timeout duration
                                        tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
                                        
                                        // Check if process still exists and is running
                                        let should_terminate = {
                                            let processes_lock = processes_for_timeout.lock().unwrap();
                                            if let Some(process) = processes_lock.get(&process_id) {
                                                // Only terminate if process is still running
                                                process.child.is_some() && process.exit_code.is_none()
                                            } else {
                                                false
                                            }
                                        };
                                        
                                        if should_terminate {
                                            info!("Process {} execution timeout after {} seconds", process_id, timeout_secs);
                                            
                                            // Mark process as timeout terminated
                                            {
                                                let mut processes_lock = processes_for_timeout.lock().unwrap();
                                                if let Some(process) = processes_lock.get_mut(&process_id) {
                                                    process.timeout_terminated = true;
                                                }
                                            }
                                            
                                            // Record timeout event
                                            let timeout_event = ChainEventData {
                                                event_type: "process/timeout".to_string(),
                                                data: EventData::Process(ProcessEventData::TimeoutTriggered {
                                                    process_id,
                                                    timeout_seconds: timeout_secs,
                                                    action: "SIGTERM".to_string(),
                                                }),
                                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                                description: Some(format!(
                                                    "Process {} execution timeout after {} seconds, initiating graceful termination",
                                                    process_id, timeout_secs
                                                )),
                                            };
                                            
                                            // Send timeout event
                                            if let Err(e) = theater_tx_for_timeout.send(crate::messages::TheaterCommand::NewEvent {
                                                actor_id: actor_id_for_timeout.clone(),
                                                event: timeout_event.to_chain_event(None),
                                            }).await {
                                                error!("Failed to send timeout event: {}", e);
                                            }
                                            
                                            // Attempt graceful termination
                                            Self::graceful_terminate_process(process_id, processes_for_timeout, actor_handle_for_timeout, theater_tx_for_timeout, actor_id_for_timeout).await;
                                        }
                                    }))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            
                            // Create the managed process with the child's details but not the child itself
                            // Since we'll move the child to the monitoring task
                            let managed_process = ManagedProcess {
                                id: process_id,
                                child: None, // We'll move the actual child to the monitoring task
                                os_pid: os_pid,
                                config: ProcessConfig {
                                    stdout_mode: process_config.stdout_mode,
                                    stderr_mode: process_config.stderr_mode,
                                    execution_timeout: process_config.execution_timeout,
                                    ..process_config.clone()
                                },
                                start_time: SystemTime::now(),
                                stdin_writer: Some(stdin_writer),
                                stdin_tx: Some(stdin_tx),
                                stdout_reader: stdout_handle,
                                stderr_reader: stderr_handle,
                                exit_code: None,
                                timeout_handle,
                                timeout_terminated: false,
                            };
                            
                            // Store the managed process
                            {
                                let mut processes = processes.lock().unwrap();
                                processes.insert(process_id, managed_process);
                            }
                            
                            // Record spawn event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::ProcessSpawn {
                                    process_id,
                                    program: program.clone(),
                                    args: args.clone(),
                                    os_pid: os_pid,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Spawned process {} (OS PID: {:?}): {}", process_id, os_pid, program)),
                            });
                            
                            Ok((Ok(process_id),))
                        },
                        Err(e) => {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
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

        // Implementation for os-write-stdin
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-write-stdin",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid, data): (u64, Vec<u8>)|
                  -> Box<dyn Future<Output = Result<(Result<u32, String>,)>> + Send> {
                let processes = processes.clone();
                debug!("Writing to stdin of process {}", pid);

                Box::new(async move {
                    let stdin_tx = {
                        // We need to drop the mutex guard before the async operation
                        let processes = processes.lock().unwrap();

                        if let Some(process) = processes.get(&pid) {
                            if let Some(tx) = &process.stdin_tx {
                                debug!("Found stdin channel for process {}", pid);
                                Some(tx.clone())
                            } else {
                                debug!("No stdin channel for process {}", pid);
                                None
                            }
                        } else {
                            debug!("Process {} not found", pid);
                            None
                        }
                    };

                    if let Some(stdin_tx) = stdin_tx {
                        // Send data to the stdin writer
                        let bytes_written = data.len() as u32;
                        match stdin_tx.send(data).await {
                            Ok(_) => {
                                debug!("Wrote {} bytes to stdin of process {}", bytes_written, pid);
                                // Record stdin write event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/write-stdin".to_string(),
                                    data: EventData::Process(ProcessEventData::StdinWrite {
                                        process_id: pid,
                                        bytes_written,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Wrote {} bytes to process {} stdin",
                                        bytes_written, pid
                                    )),
                                });

                                Ok((Ok(bytes_written),))
                            }
                            Err(e) => {
                                error!("Failed to write to stdin of process {}: {}", pid, e);
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/write-stdin".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "write-stdin".to_string(),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to write to process {} stdin: {}",
                                        pid, e
                                    )),
                                });

                                Ok((Err(format!("Failed to write to stdin: {}", e)),))
                            }
                        }
                    } else {
                        error!("Process {} not found or stdin not available", pid);
                        // Process not found or stdin not available
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/write-stdin".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "write-stdin".to_string(),
                                message: "Process stdin not available".to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Failed to write to process {}: stdin not available",
                                pid
                            )),
                        });

                        Ok((Err("Process stdin not available".to_string()),))
                    }
                })
            },
        )?;

        // Implementation for os-status
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-status",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<ProcessStatus, String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    let process_status = {
                        let processes = processes.lock().unwrap();
                        
                        if let Some(process) = processes.get(&pid) {
                            let start_time = match process.start_time.duration_since(std::time::UNIX_EPOCH) {
                                Ok(duration) => duration.as_millis() as u64,
                                Err(_) => 0,
                            };
                            
                            Some(ProcessStatus {
                                pid,
                                running: process.child.is_some(),
                                exit_code: process.exit_code,
                                start_time,
                            })
                        } else {
                            None
                        }
                    };
                    
                    if let Some(status) = process_status {
                        Ok((Ok(status),))
                    } else {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/status".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "status".to_string(),
                                message: format!("Process not found: {}", pid),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to get status for process {}: process not found", pid)),
                        });
                        
                        Ok((Err(format!("Process not found: {}", pid)),))
                    }
                })
            },
        )?;

        // Implementation for os-kill
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-kill",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();

                Box::new(async move {
                    // Get a clone of the child process to kill
                    let child_opt = {
                        let mut processes = processes.lock().unwrap();
                        if let Some(process) = processes.get_mut(&pid) {
                            process.child.take()
                        } else {
                            None
                        }
                    };

                    // Kill the process if we have a handle
                    if let Some(mut child) = child_opt {
                        match child.kill().await {
                            Ok(_) => {
                                // Record kill event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/kill".to_string(),
                                    data: EventData::Process(ProcessEventData::KillRequest {
                                        process_id: pid,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Killed process {}", pid)),
                                });

                                // Update the process status to reflect it's been killed
                                {
                                    let mut processes = processes.lock().unwrap();
                                    if let Some(process) = processes.get_mut(&pid) {
                                        process.exit_code = Some(-1); // Killed
                                    }
                                }

                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/kill".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "kill".to_string(),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to kill process {}: {}",
                                        pid, e
                                    )),
                                });

                                Ok((Err(format!("Failed to kill process: {}", e)),))
                            }
                        }
                    } else {
                        // Process is already dead, so consider this a success
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/kill".to_string(),
                            data: EventData::Process(ProcessEventData::KillRequest {
                                process_id: pid,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Process {} is already terminated", pid)),
                        });

                        Ok((Ok(()),))
                    }
                })
            },
        )?;

        // Implementation for os-signal
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-signal",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid, signal): (u64, u32)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    // Get the process OS PID
                    let os_pid = {
                        let processes = processes.lock().unwrap();
                        if let Some(process) = processes.get(&pid) {
                            if let Some(child) = &process.child {
                                child.id()
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    
                    if let Some(os_pid) = os_pid {
                        #[cfg(unix)]
                        {
                            // Map signal numbers to Unix signals
                            let sig = match signal {
                                1 => 1,  // SIGHUP
                                2 => 2,  // SIGINT
                                15 => 15, // SIGTERM
                                _ => signal as i32
                            };
                            
                            // Send the signal
                            unsafe {
                                libc::kill(os_pid as i32, sig);
                            }
                            
                            // Record signal event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/signal".to_string(),
                                data: EventData::Process(ProcessEventData::SignalSent {
                                    process_id: pid,
                                    signal,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Sent signal {} to process {}", signal, pid)),
                            });
                            
                            Ok((Ok(()),))
                        }
                        
                        #[cfg(not(unix))]
                        {
                            // On non-Unix platforms, we can only kill the process
                            if signal == 15 { // SIGTERM equivalent
                                let mut processes = processes.lock().unwrap();
                                if let Some(process) = processes.get_mut(&pid) {
                                    if let Some(child) = &mut process.child {
                                        let _ = child.kill();
                                    }
                                }
                                
                                // Record signal event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/signal".to_string(),
                                    data: EventData::Process(ProcessEventData::SignalSent {
                                        process_id: pid,
                                        signal,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Sent kill signal to process {}", pid)),
                                });
                                
                                Ok((Ok(()),))
                            } else {
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/signal".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "signal".to_string(),
                                        message: format!("Signal {} not supported on this platform", signal),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Signal {} not supported on this platform", signal)),
                                });
                                
                                Ok((Err(format!("Signal {} not supported on this platform", signal)),))
                            }
                        }
                    } else {
                        // Process not found or not running
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/signal".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "signal".to_string(),
                                message: "Process not found or not running".to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to send signal to process {}: process not found or not running", pid)),
                        });
                        
                        Ok((Err("Process not found or not running".to_string()),))
                    }
                })
            },
        )?;

        Ok(())
    }

    /// Gracefully terminate a process (SIGTERM -> wait -> SIGKILL)
    async fn graceful_terminate_process(
        process_id: u64,
        processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
        _actor_handle: ActorHandle,
        theater_tx: tokio::sync::mpsc::Sender<crate::messages::TheaterCommand>,
        actor_id: crate::id::TheaterId,
    ) {
        const GRACE_PERIOD_SECS: u64 = 5;
        
        // First, try SIGTERM
        let sigterm_result = Self::send_signal_to_process(process_id, 15, &processes).await; // SIGTERM = 15
        
        if sigterm_result.is_ok() {
            info!("Sent SIGTERM to process {}, waiting {} seconds for graceful exit", process_id, GRACE_PERIOD_SECS);
            
            // Wait for grace period
            tokio::time::sleep(Duration::from_secs(GRACE_PERIOD_SECS)).await;
            
            // Check if process is still running
            let still_running = {
                let processes_lock = processes.lock().unwrap();
                if let Some(process) = processes_lock.get(&process_id) {
                    process.child.is_some() && process.exit_code.is_none()
                } else {
                    false
                }
            };
            
            if still_running {
                info!("Process {} did not exit gracefully, sending SIGKILL", process_id);
                
                // Record escalation event
                let escalation_event = ChainEventData {
                    event_type: "process/timeout".to_string(),
                    data: EventData::Process(ProcessEventData::TimeoutTriggered {
                        process_id,
                        timeout_seconds: GRACE_PERIOD_SECS,
                        action: "SIGKILL".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Process {} did not exit gracefully, escalating to SIGKILL",
                        process_id
                    )),
                };
                
                // Send escalation event
                if let Err(e) = theater_tx.send(crate::messages::TheaterCommand::NewEvent {
                    actor_id: actor_id.clone(),
                    event: escalation_event.to_chain_event(None),
                }).await {
                    error!("Failed to send timeout escalation event: {}", e);
                }
                
                // Send SIGKILL
                let _sigkill_result = Self::send_signal_to_process(process_id, 9, &processes).await; // SIGKILL = 9
            }
        } else {
            // If SIGTERM failed, try direct kill
            info!("SIGTERM failed for process {}, attempting direct kill", process_id);
            Self::kill_process_directly(process_id, &processes).await;
        }
    }
    
    /// Send signal to process (helper function)
    async fn send_signal_to_process(
        process_id: u64,
        signal: i32,
        processes: &Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    ) -> Result<(), String> {
        let os_pid = {
            let processes_lock = processes.lock().unwrap();
            if let Some(process) = processes_lock.get(&process_id) {
                process.os_pid
            } else {
                return Err("Process not found".to_string());
            }
        };
        
        if let Some(os_pid) = os_pid {
            #[cfg(unix)]
            {
                unsafe {
                    if libc::kill(os_pid as i32, signal) == 0 {
                        Ok(())
                    } else {
                        Err(format!("Failed to send signal {}: {}", signal, std::io::Error::last_os_error()))
                    }
                }
            }
            
            #[cfg(not(unix))]
            {
                // On non-Unix platforms, we can only kill
                if signal == 9 || signal == 15 {
                    Self::kill_process_directly(process_id, processes).await;
                    Ok(())
                } else {
                    Err(format!("Signal {} not supported on this platform", signal))
                }
            }
        } else {
            Err("Process OS PID not available".to_string())
        }
    }
    
    /// Kill process directly using Child::kill()
    async fn kill_process_directly(
        process_id: u64,
        processes: &Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    ) {
        let child_opt = {
            let mut processes_lock = processes.lock().unwrap();
            if let Some(process) = processes_lock.get_mut(&process_id) {
                process.child.take()
            } else {
                None
            }
        };
        
        if let Some(mut child) = child_opt {
            if let Err(e) = child.kill().await {
                error!("Failed to kill process {}: {}", process_id, e);
            } else {
                info!("Successfully killed process {} directly", process_id);
            }
        }
    }
}
