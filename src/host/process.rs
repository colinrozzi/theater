
use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::ProcessHostConfig;
use crate::events::process::ProcessEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

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
pub enum OutputMode {
    Raw,
    LineByLine,
    Json,
    Chunked(usize),
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
#[derive(Debug, Clone)]
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

impl ProcessHost {
    /// Create a new ProcessHost with the given configuration
    pub fn new(config: ProcessHostConfig) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            next_process_id: Arc::new(Mutex::new(1)),
            actor_handle: None,
        }
    }

    /// Start the process host
    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting ProcessHost");
        self.actor_handle = Some(actor_handle);
        Ok(())
    }

    /// Set up WebAssembly host functions for the process interface
    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up host functions for process handling");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/process")
            .expect("Could not instantiate ntwk:theater/process");

        // Implementation for os-spawn
        let processes = self.processes.clone();
        let next_process_id = self.next_process_id.clone();
        let config = self.config.clone();
        interface.func_wrap_async(
            "os-spawn",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (process_config,)|  // Process configuration from WebAssembly
                  -> Box<dyn Future<Output = Result<(Result<u64, String>,)>> + Send> {
                let processes = processes.clone();
                let next_process_id = next_process_id.clone();
                let config = config.clone();
                
                Box::new(async move {
                    let program = process_config.program;
                    let args = process_config.args;
                    let cwd = process_config.cwd;
                    let env_vars = process_config.env;
                    
                    // Validate program path
                    if let Some(allowed_programs) = &config.allowed_programs {
                        if !allowed_programs.contains(&program) {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: None,
                                    operation: "spawn".to_string(),
                                    message: format!("Program not allowed: {}", program),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to spawn process: Program not allowed: {}", program)),
                            });
                            
                            return Ok((Err(format!("Program not allowed: {}", program)),));
                        }
                    }
                    
                    // Validate working directory
                    if let Some(cwd_path) = &cwd {
                        if let Some(allowed_paths) = &config.allowed_paths {
                            if !allowed_paths.iter().any(|path| cwd_path.starts_with(path)) {
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/spawn".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: None,
                                        operation: "spawn".to_string(),
                                        message: format!("Working directory not allowed: {}", cwd_path),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to spawn process: Working directory not allowed: {}", cwd_path)),
                                });
                                
                                return Ok((Err(format!("Working directory not allowed: {}", cwd_path)),));
                            }
                        }
                    }
                    
                    // Check if we've reached the max number of processes
                    {
                        let processes_lock = processes.lock().unwrap();
                        if processes_lock.len() >= config.max_processes {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/spawn".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: None,
                                    operation: "spawn".to_string(),
                                    message: "Too many processes".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some("Failed to spawn process: Too many processes running".to_string()),
                            });
                            
                            return Ok((Err("Too many processes running".to_string()),));
                        }
                    }
                    
                    // Get a new process ID
                    let process_id = {
                        let mut id_lock = next_process_id.lock().unwrap();
                        let id = *id_lock;
                        *id_lock += 1;
                        id
                    };
                    
                    // Parse output modes
                    let stdout_mode = match process_config.stdout_mode {
                        0 => OutputMode::Raw,
                        1 => OutputMode::LineByLine,
                        2 => OutputMode::Json,
                        3 => {
                            if let Some(chunk_size) = process_config.chunk_size {
                                OutputMode::Chunked(chunk_size as usize)
                            } else {
                                OutputMode::Chunked(4096) // Default chunk size
                            }
                        },
                        _ => OutputMode::Raw,
                    };
                    
                    let stderr_mode = match process_config.stderr_mode {
                        0 => OutputMode::Raw,
                        1 => OutputMode::LineByLine,
                        2 => OutputMode::Json,
                        3 => {
                            if let Some(chunk_size) = process_config.chunk_size {
                                OutputMode::Chunked(chunk_size as usize)
                            } else {
                                OutputMode::Chunked(4096) // Default chunk size
                            }
                        },
                        _ => OutputMode::Raw,
                    };
                    
                    // Create the process configuration
                    let process_config = ProcessConfig {
                        program: program.clone(),
                        args: args.clone(),
                        cwd: cwd.clone(),
                        env: env_vars,
                        buffer_size: process_config.buffer_size,
                        stdout_mode,
                        stderr_mode,
                        chunk_size: process_config.chunk_size,
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
                            let os_pid = child.id();
                            
                            let actor_id = ctx.data().id.clone();
                            let theater_tx = ctx.data().theater_tx.clone();
                            
                            // Set up stdin channel
                            let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(32);
                            let mut stdin = child.stdin.take().expect("Failed to open stdin");
                            
                            // Handle stdin writes in a separate task
                            let stdin_writer = tokio::spawn(async move {
                                while let Some(data) = stdin_rx.recv().await {
                                    if let Err(e) = stdin.write_all(&data).await {
                                        error!("Failed to write to process stdin: {}", e);
                                        break;
                                    }
                                    if let Err(e) = stdin.flush().await {
                                        error!("Failed to flush process stdin: {}", e);
                                        break;
                                    }
                                }
                            });
                            
                            // Set up stdout reader
                            let stdout_handle = if let Some(stdout) = child.stdout.take() {
                                let stdout_mode = stdout_mode;
                                let buffer_size = process_config.buffer_size as usize;
                                let process_id_clone = process_id;
                                let actor_id_clone = actor_id.clone();
                                let theater_tx_clone = theater_tx.clone();
                                
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stdout,
                                        stdout_mode,
                                        buffer_size,
                                        process_id_clone,
                                        actor_id_clone,
                                        theater_tx_clone,
                                        true, // is stdout
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
                                
                                Some(tokio::spawn(async move {
                                    Self::process_output(
                                        stderr,
                                        stderr_mode,
                                        buffer_size,
                                        process_id_clone,
                                        actor_id_clone,
                                        theater_tx_clone,
                                        false, // not stdout (stderr)
                                    ).await;
                                }))
                            } else {
                                None
                            };
                            
                            // Spawn a task to wait for the process to exit
                            let process_id_clone = process_id;
                            let actor_id_clone = actor_id.clone();
                            let theater_tx_clone = theater_tx.clone();
                            let processes_clone = processes.clone();
                            
                            tokio::spawn(async move {
                                match child.wait().await {
                                    Ok(status) => {
                                        let exit_code = status.code().unwrap_or(-1);
                                        
                                        // Send exit event to the actor
                                        let _ = theater_tx_clone.send(crate::messages::TheaterCommand::SendEvent {
                                            actor_id: actor_id_clone,
                                            event: ChainEventData {
                                                event_type: "process/exit".to_string(),
                                                data: EventData::Process(ProcessEventData::ProcessExit {
                                                    process_id: process_id_clone,
                                                    exit_code,
                                                }),
                                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                                description: Some(format!("Process {} exited with code {}", process_id_clone, exit_code)),
                                            },
                                        }).await;
                                        
                                        // Update process status
                                        let mut processes = processes_clone.lock().unwrap();
                                        if let Some(process) = processes.get_mut(&process_id_clone) {
                                            process.exit_code = Some(exit_code);
                                            process.child = None;
                                        }
                                    },
                                    Err(e) => {
                                        error!("Failed to wait for process: {}", e);
                                    }
                                }
                            });
                            
                            // Create the managed process
                            let managed_process = ManagedProcess {
                                id: process_id,
                                child: Some(child),
                                os_pid,
                                config: process_config.clone(),
                                start_time: SystemTime::now(),
                                stdin_writer: Some(stdin_writer),
                                stdin_tx: Some(stdin_tx),
                                stdout_reader: stdout_handle,
                                stderr_reader: stderr_handle,
                                exit_code: None,
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
                                    os_pid,
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
                
                Box::new(async move {
                    let stdin_tx = {
                        let processes = processes.lock().unwrap();
                        
                        if let Some(process) = processes.get(&pid) {
                            if let Some(tx) = &process.stdin_tx {
                                tx.clone()
                            } else {
                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "process/write-stdin".to_string(),
                                    data: EventData::Process(ProcessEventData::Error {
                                        process_id: Some(pid),
                                        operation: "write-stdin".to_string(),
                                        message: "Process stdin not available".to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Failed to write to process {}: stdin not available", pid)),
                                });
                                
                                return Ok((Err("Process stdin not available".to_string()),));
                            }
                        } else {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/write-stdin".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(pid),
                                    operation: "write-stdin".to_string(),
                                    message: format!("Process not found: {}", pid),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to write to process {}: process not found", pid)),
                            });
                            
                            return Ok((Err(format!("Process not found: {}", pid)),));
                        }
                    };
                    
                    // Send data to the stdin writer
                    let bytes_written = data.len() as u32;
                    match stdin_tx.send(data).await {
                        Ok(_) => {
                            // Record stdin write event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/write-stdin".to_string(),
                                data: EventData::Process(ProcessEventData::StdinWrite {
                                    process_id: pid,
                                    bytes_written,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Wrote {} bytes to process {} stdin", bytes_written, pid)),
                            });
                            
                            Ok((Ok(bytes_written),))
                        },
                        Err(e) => {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/write-stdin".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(pid),
                                    operation: "write-stdin".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to write to process {} stdin: {}", pid, e)),
                            });
                            
                            Ok((Err(format!("Failed to write to stdin: {}", e)),))
                        }
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
                    let processes = processes.lock().unwrap();
                    
                    if let Some(process) = processes.get(&pid) {
                        let start_time = match process.start_time.duration_since(std::time::UNIX_EPOCH) {
                            Ok(duration) => duration.as_millis() as u64,
                            Err(_) => 0,
                        };
                        
                        let status = ProcessStatus {
                            pid,
                            running: process.child.is_some(),
                            exit_code: process.exit_code,
                            start_time,
                            cpu_usage: 0.0, // Not implemented yet
                            memory_usage: 0, // Not implemented yet
                        };
                        
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

        // Implementation for os-signal
        let processes = self.processes.clone();
        interface.func_wrap_async(
            "os-signal",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (pid, signal): (u64, u32)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let processes = processes.clone();
                
                Box::new(async move {
                    let mut processes = processes.lock().unwrap();
                    
                    if let Some(process) = processes.get_mut(&pid) {
                        if let Some(child) = &mut process.child {
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                
                                if let Some(os_pid) = child.id() {
                                    // Map signal numbers to Unix signals
                                    let sig = match signal {
                                        1 => libc::SIGHUP,
                                        2 => libc::SIGINT,
                                        15 => libc::SIGTERM,
                                        _ => signal as i32
                                    };
                                    
                                    let os_pid = os_pid as i32;
                                    unsafe {
                                        libc::kill(os_pid, sig);
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
                                    
                                    return Ok((Ok(()),));
                                }
                            }
                            
                            #[cfg(windows)]
                            {
                                // On Windows, the only thing we can do is terminate the process
                                if signal == 15 { // SIGTERM equivalent
                                    let _ = child.kill().await;
                                    
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
                                    
                                    return Ok((Ok(()),));
                                } else {
                                    return Ok((Err(format!("Signal {} not supported on Windows", signal)),));
                                }
                            }
                            
                            // If we get here, something went wrong
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/signal".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(pid),
                                    operation: "signal".to_string(),
                                    message: "Failed to send signal".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to send signal to process {}", pid)),
                            });
                            
                            Ok((Err("Failed to send signal".to_string()),))
                        } else {
                            // Record error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "process/signal".to_string(),
                                data: EventData::Process(ProcessEventData::Error {
                                    process_id: Some(pid),
                                    operation: "signal".to_string(),
                                    message: "Process not running".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Failed to send signal to process {}: process not running", pid)),
                            });
                            
                            Ok((Err("Process not running".to_string()),))
                        }
                    } else {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/signal".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "signal".to_string(),
                                message: format!("Process not found: {}", pid),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to send signal to process {}: process not found", pid)),
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
                    let mut processes = processes.lock().unwrap();
                    
                    if let Some(process) = processes.get_mut(&pid) {
                        if let Some(child) = &mut process.child {
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
                                    
                                    Ok((Ok(()),))
                                },
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
                                        description: Some(format!("Failed to kill process {}: {}", pid, e)),
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
                    } else {
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "process/kill".to_string(),
                            data: EventData::Process(ProcessEventData::Error {
                                process_id: Some(pid),
                                operation: "kill".to_string(),
                                message: format!("Process not found: {}", pid),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Failed to kill process {}: process not found", pid)),
                        });
                        
                        Ok((Err(format!("Process not found: {}", pid)),))
                    }
                })
            },
        )?;

        Ok(())
    }

    /// Add export functions to the actor instance
    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for process handling");

        // Register the process handler export functions
        let exports = actor_instance.exports.clone();
        let actor_store = actor_instance.store.data_mut();
        
        // Get the handle-stdout export
        if let Some(handle_stdout) = exports.get_async_function("handle-stdout") {
            info!("Registered handle-stdout export function");
            actor_store.register_export("handle-stdout".to_string(), handle_stdout);
        } else {
            warn!("No handle-stdout export function found in actor");
        }
        
        // Get the handle-stderr export
        if let Some(handle_stderr) = exports.get_async_function("handle-stderr") {
            info!("Registered handle-stderr export function");
            actor_store.register_export("handle-stderr".to_string(), handle_stderr);
        } else {
            warn!("No handle-stderr export function found in actor");
        }
        
        // Get the handle-exit export
        if let Some(handle_exit) = exports.get_async_function("handle-exit") {
            info!("Registered handle-exit export function");
            actor_store.register_export("handle-exit".to_string(), handle_exit);
        } else {
            warn!("No handle-exit export function found in actor");
        }

        Ok(())
    }

    /// Process output from a child process
    async fn process_output<R>(
        mut reader: R,
        mode: OutputMode,
        buffer_size: usize,
        process_id: u64,
        actor_id: crate::id::TheaterId,
        theater_tx: tokio::sync::mpsc::Sender<crate::messages::TheaterCommand>,
        is_stdout: bool,
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
                            // Create event data
                            let event_type = if is_stdout { "process/stdout" } else { "process/stderr" };
                            let event_data = if is_stdout {
                                EventData::Process(ProcessEventData::StdoutOutput {
                                    process_id,
                                    bytes: n,
                                })
                            } else {
                                EventData::Process(ProcessEventData::StderrOutput {
                                    process_id,
                                    bytes: n,
                                })
                            };
                            
                            // Send the output to the actor
                            let data = buffer[0..n].to_vec();
                            let _ = theater_tx
                                .send(crate::messages::TheaterCommand::CallExport {
                                    actor_id: actor_id.clone(),
                                    export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                    args: (process_id, data),
                                })
                                .await;
                        },
                        Ok(_) => break, // EOF
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            },
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
                                    // Create event data
                                    let event_type = if is_stdout { "process/stdout" } else { "process/stderr" };
                                    let event_data = if is_stdout {
                                        EventData::Process(ProcessEventData::StdoutOutput {
                                            process_id,
                                            bytes: line.len(),
                                        })
                                    } else {
                                        EventData::Process(ProcessEventData::StderrOutput {
                                            process_id,
                                            bytes: line.len(),
                                        })
                                    };
                                    
                                    // Send the line to the actor
                                    let data = line.clone();
                                    let _ = theater_tx
                                        .send(crate::messages::TheaterCommand::CallExport {
                                            actor_id: actor_id.clone(),
                                            export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                            args: (process_id, data),
                                        })
                                        .await;
                                    
                                    line.clear();
                                }
                            } else {
                                line.push(buffer[0]);
                                
                                // Check if line is too long
                                if line.len() >= buffer_size {
                                    // Create event data
                                    let event_type = if is_stdout { "process/stdout" } else { "process/stderr" };
                                    let event_data = if is_stdout {
                                        EventData::Process(ProcessEventData::StdoutOutput {
                                            process_id,
                                            bytes: line.len(),
                                        })
                                    } else {
                                        EventData::Process(ProcessEventData::StderrOutput {
                                            process_id,
                                            bytes: line.len(),
                                        })
                                    };
                                    
                                    // Send the partial line to the actor
                                    let data = line.clone();
                                    let _ = theater_tx
                                        .send(crate::messages::TheaterCommand::CallExport {
                                            actor_id: actor_id.clone(),
                                            export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                            args: (process_id, data),
                                        })
                                        .await;
                                    
                                    line.clear();
                                }
                            }
                        },
                        Ok(_) => {
                            // EOF - send any remaining data
                            if !line.is_empty() {
                                // Create event data
                                let event_type = if is_stdout { "process/stdout" } else { "process/stderr" };
                                let event_data = if is_stdout {
                                    EventData::Process(ProcessEventData::StdoutOutput {
                                        process_id,
                                        bytes: line.len(),
                                    })
                                } else {
                                    EventData::Process(ProcessEventData::StderrOutput {
                                        process_id,
                                        bytes: line.len(),
                                    })
                                };
                                
                                // Send the line to the actor
                                let data = line.clone();
                                let _ = theater_tx
                                    .send(crate::messages::TheaterCommand::CallExport {
                                        actor_id: actor_id.clone(),
                                        export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                        args: (process_id, data),
                                    })
                                    .await;
                            }
                            break;
                        },
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            },
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
                                let line = buffer[0..pos].trim();
                                buffer = buffer[pos+1..].to_string();
                                
                                if !line.is_empty() {
                                    // Validate JSON
                                    if let Ok(_) = serde_json::from_str::<serde_json::Value>(line) {
                                        // Valid JSON, send it
                                        let data = line.as_bytes().to_vec();
                                        let _ = theater_tx
                                            .send(crate::messages::TheaterCommand::CallExport {
                                                actor_id: actor_id.clone(),
                                                export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                                args: (process_id, data),
                                            })
                                            .await;
                                    }
                                }
                            }
                            
                            // Check if buffer is too large
                            if buffer.len() > buffer_size {
                                // Buffer too large, flush it as raw data
                                let data = buffer.as_bytes().to_vec();
                                let _ = theater_tx
                                    .send(crate::messages::TheaterCommand::CallExport {
                                        actor_id: actor_id.clone(),
                                        export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                        args: (process_id, data),
                                    })
                                    .await;
                                
                                buffer.clear();
                            }
                        },
                        Ok(_) => {
                            // EOF - send any remaining data
                            if !buffer.is_empty() {
                                let data = buffer.as_bytes().to_vec();
                                let _ = theater_tx
                                    .send(crate::messages::TheaterCommand::CallExport {
                                        actor_id: actor_id.clone(),
                                        export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                        args: (process_id, data),
                                    })
                                    .await;
                            }
                            break;
                        },
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            },
            OutputMode::Chunked(chunk_size) => {
                // Read in fixed-size chunks
                let mut buffer = vec![0; chunk_size];
                
                loop {
                    match reader.read_exact(&mut buffer).await {
                        Ok(_) => {
                            // Create event data
                            let event_type = if is_stdout { "process/stdout" } else { "process/stderr" };
                            let event_data = if is_stdout {
                                EventData::Process(ProcessEventData::StdoutOutput {
                                    process_id,
                                    bytes: chunk_size,
                                })
                            } else {
                                EventData::Process(ProcessEventData::StderrOutput {
                                    process_id,
                                    bytes: chunk_size,
                                })
                            };
                            
                            // Send the chunk to the actor
                            let data = buffer.clone();
                            let _ = theater_tx
                                .send(crate::messages::TheaterCommand::CallExport {
                                    actor_id: actor_id.clone(),
                                    export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                    args: (process_id, data),
                                })
                                .await;
                        },
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            // Partial chunk - read what's available
                            let mut partial_buffer = vec![0; chunk_size];
                            if let Ok(n) = reader.read(&mut partial_buffer).await {
                                if n > 0 {
                                    // Create event data
                                    let event_type = if is_stdout { "process/stdout" } else { "process/stderr" };
                                    let event_data = if is_stdout {
                                        EventData::Process(ProcessEventData::StdoutOutput {
                                            process_id,
                                            bytes: n,
                                        })
                                    } else {
                                        EventData::Process(ProcessEventData::StderrOutput {
                                            process_id,
                                            bytes: n,
                                        })
                                    };
                                    
                                    // Send the partial chunk to the actor
                                    let data = partial_buffer[0..n].to_vec();
                                    let _ = theater_tx
                                        .send(crate::messages::TheaterCommand::CallExport {
                                            actor_id: actor_id.clone(),
                                            export_name: if is_stdout { "handle-stdout" } else { "handle-stderr" },
                                            args: (process_id, data),
                                        })
                                        .await;
                                }
                            }
                            break;
                        },
                        Err(e) => {
                            error!("Error reading process output: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    }
}
